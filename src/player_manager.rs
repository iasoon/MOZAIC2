use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};
use rand::Rng;
use futures::{Future, FutureExt};
use std::pin::Pin;
use futures::task::{Context, Poll};
use tokio::time::Duration;

use crate::connection_table::{Token, Message, ConnectionTableHandle};
use crate::player_supervisor::{PlayerSupervisor, SupervisorMsg, SupervisorRequest};

pub struct PlayerManager {
    rx: mpsc::UnboundedReceiver<Message>,
    tx: mpsc::UnboundedSender<Message>,

    request_table: HashMap<(u32, u32), RequestHandle>,
    players: HashMap<u32, PlayerData>,
    conn_player: HashMap<Token, u32>,

    // todo: remove pub
    pub connection_table: ConnectionTableHandle,
}

impl PlayerManager {
    pub fn new(connection_table: ConnectionTableHandle) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        return PlayerManager {
            rx,
            tx,

            request_table: HashMap::new(),
            players: HashMap::new(),
            conn_player: HashMap::new(),

            connection_table,
        };
    }
    pub fn create_player(&mut self, player_id: u32, token: Token) {
        let supervisor_token = rand::thread_rng().gen();
        PlayerSupervisor::create(
            self.connection_table.clone(),
            supervisor_token,
            token
        ).run();


        self.players.insert(player_id, PlayerData {
            supervisor_token,
            msg_ctr: 0,
        });
        self.conn_player.insert(supervisor_token, player_id);

        self.connection_table.subscribe(&supervisor_token, self.tx.clone());
    }

    pub fn request(&mut self,
                   player_id: u32,
                   content: Vec<u8>,
                   timeout: Duration)
                   -> Request
    {
        let (tx, rx) = oneshot::channel();
        let player = self.players.get_mut(&player_id).unwrap();
        self.request_table.insert((player_id, player.msg_ctr), tx);
        let req = SupervisorRequest {
            request_id: player.msg_ctr,
            content,
            timeout
        };
        self.connection_table.receive(Message {
            conn_token: player.supervisor_token,
            payload: bincode::serialize(&req).unwrap(),
        });
        player.msg_ctr += 1;
        return Request { rx };
    }

    fn receive(&mut self, msg: Message) {
        let &player_id = self.conn_player.get(&msg.conn_token).unwrap();
        let resp: SupervisorMsg = bincode::deserialize(&msg.payload).unwrap();
        
        let (request_id, value) = match resp {
            SupervisorMsg::Response { request_id, content } => {
                (request_id, Ok(content))
            }
            SupervisorMsg::Timeout { request_id } => {
                (request_id, Err(RequestError::Timeout))
            }
        };

        if let Some(tx) = self.request_table.remove(&(player_id, request_id)) {
            tx.send(value).unwrap_or_else(|_| panic!("send failed"));
        }
    }

    pub async fn step(&mut self) {
        let msg = self.rx.recv().await.unwrap();
        self.receive(msg);
    }
}

struct PlayerData {
    supervisor_token: Token,
    msg_ctr: u32,
}

type RequestHandle = oneshot::Sender<RequestResult<Vec<u8>>>;

pub struct Request {
    rx: oneshot::Receiver<RequestResult<Vec<u8>>>,
}

impl Future for Request {
    type Output = RequestResult<Vec<u8>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>)
        -> Poll<Self::Output>
    {
        // errors should not happen
        self.rx.poll_unpin(cx).map(|e| e.unwrap())
    }
}

pub enum RequestError {
    Timeout
}

pub type RequestResult<T> = Result<T, RequestError>;