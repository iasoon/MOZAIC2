use crate::msg_stream::MsgStreamHandle;
use std::collections::HashMap;
use futures::future::{Future, FutureExt, FusedFuture};
use std::pin::Pin;
use futures::task::{Context, Poll};
use serde::{Serialize, Deserialize};
use std::time::{Duration};

use crate::player_supervisor::RequestMessage;

pub enum GameEvent {
    PlayerResponse(PlayerResponse),
}

pub struct PlayerResponse {
    pub player_id: u32,
    pub request_id: u32,
    pub response: Result<Vec<u8>, Timeout>,
}

pub struct Timeout;


// TODO: replace with a specialized struct
pub type EventBus = MsgStreamHandle<GameEvent>;

pub struct MatchCtx {
    event_bus: EventBus,
    players: HashMap<u32, PlayerHandle>,
    output: MsgStreamHandle<String>,
}

impl MatchCtx {
    pub fn new(
        event_bus: EventBus,
        players: HashMap<u32, MsgStreamHandle<RequestMessage>>,
        log: MsgStreamHandle<String>,
    ) -> Self
    {
        MatchCtx {
            event_bus,
            players: players.into_iter().map(|(id, handle)| {
                let player_handle = PlayerHandle {
                    msg_ctr: 0,
                    stream_handle: handle,
                };
                (id, player_handle)
            }).collect(),
            output: log,
        }
    }

    // TODO: implement a clean way to handle the player not existing
    pub fn request(&mut self,
                   player_id: u32,
                   content: Vec<u8>,
                   timeout: Duration)
                   -> Request
    {
        let player = self.players.get_mut(&player_id).unwrap();
        let request_id = player.msg_ctr;
        player.msg_ctr += 1;

        player.stream_handle.write(RequestMessage {
            request_id,
            content,
            timeout
        });

        return Request {
            player_id,
            request_id,
            event_bus: self.event_bus.reader(),
        };
    }

    pub fn players(&self) -> Vec<u32> {
        self.players.keys().cloned().collect()
    }

    // this method should be used to emit log states etc.
    // this should place them in chronological relation
    // to the events that happened.
    pub fn emit(&mut self, message: String) {
        self.output.write(message);
    }

    pub fn output_stream<'a>(&'a self) -> &'a MsgStreamHandle<String> {
        &self.output
    }
}

struct PlayerHandle {
    msg_ctr: u32,
    stream_handle: MsgStreamHandle<RequestMessage>,
}


pub struct Request {
    player_id: u32,
    request_id: u32,
    event_bus: MsgStreamReader<GameEvent>,
}

impl Request {
    pub fn player_id(&self) -> u32 {
        self.player_id
    }
}

impl Future for Request {
    type Output = RequestResult<Vec<u8>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>)
        -> Poll<Self::Output>
    {
        loop {
            let event = ready!(self.event_bus.recv().poll_unpin(cx));
            match *event {
                GameEvent::PlayerResponse(ref resp) => {
                    if resp.player_id == self.player_id && resp.request_id == self.request_id
                    {
                        let value = resp.response.as_ref()
                            .map(|data| data.clone())
                            .map_err(|_| RequestError::Timeout);
                        return Poll::Ready(value);
                    }

                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum RequestError {
    Timeout
}

pub type RequestResult<T> = Result<T, RequestError>;

use crate::msg_stream::{msg_stream, MsgStreamReader};

pub struct Connection {
    pub tx: MsgStreamHandle<Vec<u8>>,
    pub rx: MsgStreamReader<Vec<u8>>,
}

impl Connection {
    pub fn emit<T>(&mut self, message: T)
        where T: Serialize
    {
        let encoded = bincode::serialize(&message).unwrap();
        self.tx.write(encoded);
    }

    pub fn recv<'a, T>(&'a mut self) -> impl FusedFuture<Output=T> + 'a
        where T: for<'b> Deserialize<'b>
    {
        self.rx.recv().map(|data| {
            bincode::deserialize(&data).unwrap()
        })
    }
}

impl Connection {
    pub fn create() -> (Self, Self) {
        let tx1 = msg_stream();
        let rx1 = tx1.reader();
        let tx2 = msg_stream();
        let rx2 = tx2.reader();
        let conn1 = Connection { rx: rx1, tx: tx2};
        let conn2 = Connection { rx: rx2, tx: tx1};
        return (conn1, conn2);
    }
}
