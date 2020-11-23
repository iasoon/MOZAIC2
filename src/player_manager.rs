use std::collections::HashMap;
use futures::future::{select_all, Future, FutureExt, FusedFuture};
use std::pin::Pin;
use futures::task::{Context, Poll};
use tokio::time::Duration;
use std::sync::Arc;
use serde::{Serialize, Deserialize};

use crate::connection_table::{Token, ConnectionTableHandle};
use crate::player_supervisor::{PlayerSupervisor, SupervisorMsg, SupervisorRequest};


pub struct PlayerManager {
    conn_mgr: ConnectionManager,
    players: HashMap<u32, PlayerData>,
    connection_table: ConnectionTableHandle,
}

impl PlayerManager {
    pub fn new(connection_table: ConnectionTableHandle) -> Self {
        PlayerManager {
            conn_mgr: ConnectionManager::new(),
            players: HashMap::new(),

            connection_table,
        }
    }
    pub fn create_player(&mut self, player_id: u32, token: Token) {
        let remote = self.conn_mgr.create_connection(player_id);
        PlayerSupervisor::create(
            self.connection_table.clone(),
            remote,
            token
        ).run();

        self.players.insert(player_id, PlayerData {
            msg_ctr: 0,
        });
    }

    pub fn request(&mut self,
                   player_id: u32,
                   content: Vec<u8>,
                   timeout: Duration)
                   -> Request
    {
        let player = self.players.get_mut(&player_id).unwrap();
        let request_id = player.msg_ctr;
        player.msg_ctr += 1;

        let req = SupervisorRequest {
            request_id,
            content,
            timeout
        };

        self.conn_mgr.send(player_id, &req);
        let reader = self.conn_mgr.connections[&player_id].rx.clone();
        return Request {
            player_id,
            request_id,
            reader,
        };
    }

    pub fn players(&self) -> Vec<u32> {
        self.players.keys().cloned().collect()
    }
}

struct PlayerData {
    msg_ctr: u32,
}


pub struct Request {
    player_id: u32,
    request_id: u32,
    reader: MsgStreamReader,
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
            let data = ready!(self.reader.recv().poll_unpin(cx));
            let resp: SupervisorMsg = bincode::deserialize(&data).unwrap();
            let (request_id, value) = match resp {
                SupervisorMsg::Response { request_id, content } => {
                    (request_id, Ok(content))
                }
                SupervisorMsg::Timeout { request_id } => {
                    (request_id, Err(RequestError::Timeout))
                }
            };
            if request_id == self.request_id {
                return Poll::Ready(value);
            }
        }
    }
}

pub enum RequestError {
    Timeout
}

pub type RequestResult<T> = Result<T, RequestError>;

use crate::msg_stream::{msg_stream, MsgStreamReader, MsgStreamHandle};

pub struct Connection {
    pub tx: MsgStreamHandle,
    pub rx: MsgStreamReader,
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

pub struct ConnectionManager {
    connections: HashMap<u32, Connection>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        ConnectionManager { connections: HashMap::new() }
    }

    pub fn create_connection(&mut self, key: u32) -> Connection {
        let (local, remote) = Connection::create();
        self.connections.insert(key, local);
        return remote;
    }

    pub fn send<T>(&mut self, player_id: u32, message: &T)
        where T: Serialize
    {
        let conn = self.connections.get_mut(&player_id).unwrap();
        let data = bincode::serialize(message).unwrap();
        conn.tx.write(data);
    }

    pub async fn recv(&mut self) -> (u32, Arc<Vec<u8>>) {
        let i = self.connections.iter_mut().map(|(&id, conn)| {
            conn.rx.recv().map(move |data| (id, data))
        });
        let (value, _, _) = select_all(i).await;
        return value;
    }
}
