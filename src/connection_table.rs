use std::{collections::HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
use futures::FutureExt;
use tokio::time::sleep;


use crate::match_context::{EventBus, PlayerHandle, RequestError, RequestMessage};
use crate::msg_stream::{msg_stream, MsgStreamHandle};


pub struct ConnectionTable {
    connections: HashMap<Token, ConnectionData>,
}

struct ConnectionData {
    messages: MsgStreamHandle<Vec<u8>>,
    event_bus: Arc<Mutex<EventBus>>,
    player_id: u32,
}

// TODO: put this in a central location
pub type Token = [u8; 32];

impl ConnectionTable {
    pub fn new() -> ConnectionTableHandle {
        let t = ConnectionTable {
            connections: HashMap::new(),
        };
        return ConnectionTableHandle {
            connection_table: Arc::new(Mutex::new(t))
        };
    }
}

#[derive(Clone)]
pub struct ConnectionTableHandle {
    connection_table: Arc<Mutex<ConnectionTable>>,
}

impl ConnectionTableHandle {
    fn lock<'a>(&'a mut self) -> MutexGuard<'a, ConnectionTable> {
        self.connection_table.lock().unwrap()
    }

    pub fn receive(&mut self, token: Token, data: Vec<u8>) {
        // TODO: properly handle errors
        let table = self.lock();
        let conn_data = match table.connections.get(&token) {
            Some(conn_data) => conn_data,
            None => {
                eprintln!("got data for invalid connection");
                return
            }
        };
        let req: PlayerResponse = match rmp_serde::from_read_ref(&data) {
            Ok(req ) => req,
            Err(err) => return eprintln!("{}", err),
        };
        let req_id = (conn_data.player_id, req.request_id);
        conn_data.event_bus.lock().unwrap().resolve_request(req_id, Ok(req.content));
    }

    pub fn open_connection(&mut self,
        token: Token,
        player_id: u32,
        event_bus: Arc<Mutex<EventBus>>,
    ) -> RemotePlayerHandle
    {
        let messages = msg_stream();
        let conn_data = ConnectionData {
            player_id,
            event_bus: event_bus.clone(),
            messages: messages.clone(),
        };
        self.lock().connections.insert(token, conn_data);
        RemotePlayerHandle {
            player_id,
            token,
            conn_table: self.clone(),
            messages,
            event_bus
        }
    }

    pub fn messages_for(&mut self, token: &Token) -> Option<MsgStreamHandle<Vec<u8>>> {
        self.lock().connections.get(token).map(|data| data.messages.clone())
    }
}

pub struct RemotePlayerHandle {
    token: Token,
    player_id: u32,
    conn_table: ConnectionTableHandle,
    messages: MsgStreamHandle<Vec<u8>>,
    event_bus: Arc<Mutex<EventBus>>,
}

impl PlayerHandle for RemotePlayerHandle {
    fn send_request(&mut self, r: RequestMessage) {
        let player_req = PlayerRequest { request_id: r.request_id, content: r.content };
        let data = rmp_serde::to_vec(&ServerMessage::Request(player_req)).unwrap();
        self.messages.write(data);

        let req_id = (self.player_id, r.request_id);
        let bus = self.event_bus.clone();
        tokio::spawn(sleep(r.timeout).map(move |_| {
            bus.lock().unwrap().resolve_request(req_id, Err(RequestError::Timeout))
        }));
    }

    fn send_info(&mut self, msg: String) {
        let data = rmp_serde::to_vec(&ServerMessage::Info(msg)).unwrap();
        self.messages.write(data);
    }
}

impl Drop for RemotePlayerHandle {
    fn drop(&mut self) {
        self.conn_table.lock().connections.remove(&self.token);
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ServerMessage {
    Request(PlayerRequest),
    Info(String),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PlayerRequest {
    pub request_id: u32,
    pub content: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PlayerResponse {
    pub content: Vec<u8>,
    pub request_id: u32,
}
