use crate::match_context::{GameEvent, PlayerResponse as PlayerResp, Timeout as TimeoutErr};
use crate::msg_stream::{MsgStreamHandle, MsgStreamReader};
use futures::{Stream, StreamExt, FutureExt};

use serde::{Serialize, Deserialize};
use futures::stream::{FusedStream};

use super::connection_table::{
    ConnectionTableHandle,
    Token,
};

use std::collections::{BinaryHeap, HashSet};
use tokio::time::{Delay, Instant, Duration, delay_until};
use std::cmp::{Ord, Ordering};
use std::pin::Pin;
use futures::task::{Context, Poll};
use crate::match_context::Connection;

#[derive(Serialize, Deserialize, Debug)]
pub struct RequestMessage {
    pub request_id: u32,
    pub timeout: Duration,
    pub content: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum SupervisorMsg {
    Response {
        request_id: u32,
        content: Vec<u8>
    },
    Timeout { request_id: u32 },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PlayerRequest {
    pub request_id: u32,
    pub content: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PlayerResponse {
    pub request_id: u32,
    pub content: Vec<u8>,
}


pub struct PlayerSupervisor {
    player_id: u32,
    event_bus: MsgStreamHandle<GameEvent>,
    game_conn: MsgStreamReader<RequestMessage>,

    player_conn: Connection,

    timeouts: TimeoutHeap<u32>,
    open_requests: HashSet<u32>,
}


impl PlayerSupervisor {
    pub fn create(
        mut connection_table: ConnectionTableHandle,
        event_bus: MsgStreamHandle<GameEvent>,
        game_conn: MsgStreamReader<RequestMessage>,
        player_id: u32,
        player_token: Token
    ) -> Self
    {

        let player_conn = connection_table.open_connection(player_token);

        return PlayerSupervisor {
            player_id,
            event_bus,
            game_conn,
            player_conn,
            timeouts: TimeoutHeap::new(),
            open_requests: HashSet::new(),
        }
    }

    pub fn send_player_request(&mut self, request: &RequestMessage) {
        self.open_requests.insert(request.request_id);
        let deadline = Instant::now() + request.timeout;
        self.timeouts.enqueue(request.request_id, deadline);
        self.player_conn.emit(PlayerRequest {
            request_id: request.request_id,
            // TODO: don't clone I guess
            content: request.content.clone(),
        });
    }

    pub fn handle_response(&mut self, response: PlayerResponse) {
        if !self.open_requests.remove(&response.request_id) {
            return;
        }
        self.event_bus.write(GameEvent::PlayerResponse(PlayerResp {
            player_id: self.player_id,
            request_id: response.request_id,
            response: Ok(response.content),
        }));
    }

    pub fn handle_timeout(&mut self, request_id: u32) {
        if !self.open_requests.remove(&request_id) {
            return;
        }
        self.event_bus.write(GameEvent::PlayerResponse(PlayerResp {
            player_id: self.player_id,
            request_id,
            response: Err(TimeoutErr),
        }));
    }

    pub fn run(mut self) {
        let task = async move {
            loop {
                select!(
                    req = self.game_conn.recv().fuse() => {
                        self.send_player_request(&req);
                    }
                    resp = self.player_conn.recv::<PlayerResponse>() => {
                        self.handle_response(resp);
                    }
                    item = self.timeouts.next() => {
                        // TODO: this stream never terminates, maybe improve
                        // the API.
                        let request_id = item.unwrap();
                        self.handle_timeout(request_id);
                    }
                );
            }
        };
        tokio::spawn(task);
    }
}


// B O I L E R P L A T E
struct Timeout<T> {
    instant: Instant,
    item: T,
}

impl<T> PartialEq for Timeout<T> {
    fn eq(&self, other: &Timeout<T>) -> bool {
        self.instant == other.instant
    }
}

impl<T> Eq for Timeout<T> {}

impl<T> PartialOrd for Timeout<T> {
    fn partial_cmp(&self, other: &Timeout<T>) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Timeout<T> {
    fn cmp(&self, other: &Timeout<T>) -> Ordering {
        // Reverse these, so that the timeout that will happen
        // next is the maximum.
        // We can then use collections::BinaryHeap as a min-heap.
        self.instant.cmp(&other.instant).reverse()
    }
}

struct TimeoutHeap<T> {
    sleep: Delay,
    heap: BinaryHeap<Timeout<T>>,
}

impl<T> TimeoutHeap<T> {
    fn new() -> Self {
        TimeoutHeap {
            sleep: delay_until(Instant::now()),
            heap: BinaryHeap::new(),
        }
    }

    fn enqueue(&mut self, item: T, instant: Instant) {
        self.heap.push(Timeout { item, instant });
    }
}

impl<T: Unpin> Stream for TimeoutHeap<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>>
    {
        let s = self.get_mut();

        if s.heap.is_empty() {
            return Poll::Pending;
        }

        // set the timer
        if let Some(timeout) = s.heap.peek() {
            s.sleep.reset(timeout.instant);
        }
        
        // check for timer completion
        ready!(s.sleep.poll_unpin(cx));
        let timeout = s.heap.pop().unwrap();
        return Poll::Ready(Some(timeout.item));
    }
}

impl<T: Unpin> FusedStream for TimeoutHeap<T> {
    fn is_terminated(&self) -> bool {
        false
    }
}
