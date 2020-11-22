use std::collections::HashMap;
use tokio::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};

use futures::future::{FutureExt, FusedFuture};
use futures::stream::{StreamExt};
use serde::{Serialize, Deserialize};
use bincode;


pub struct ConnectionTable {
    connections: HashMap<Token, Connection>,
}

pub type Token = [u8; 32];

pub struct Connection {
    rx: mpsc::UnboundedSender<Message>,
    subscribers: Vec<mpsc::UnboundedSender<Message>>,
}

impl ConnectionTable {
    pub fn new() -> ConnectionTableHandle {
        let t = ConnectionTable {
            connections: HashMap::new(),
        };
        return ConnectionTableHandle {
            connection_table: Arc::new(Mutex::new(t))
        };
    }

    fn connection<'a>(&'a mut self, token: &Token) -> &'a mut Connection {
        if let Some(conn) = self.connections.get_mut(token) {
            conn
        } else {
            panic!("unknown connection {:x?}", token);
        }
    }
}

#[derive(Clone)]
pub struct Message {
    pub conn_token: Token,
    pub payload: Vec<u8>,
}

#[derive(Clone)]
pub struct ConnectionTableHandle {
    connection_table: Arc<Mutex<ConnectionTable>>,
}

impl ConnectionTableHandle {
    fn lock<'a>(&'a mut self) -> MutexGuard<'a, ConnectionTable> {
        self.connection_table.lock().unwrap()
    }

    fn emit(&mut self, message: Message) {
        let mut lock = self.lock();
        let conn = lock.connection(&message.conn_token);
        let mut i = 0;
        while i < conn.subscribers.len() {
            match conn.subscribers[i].send(message.clone()) {
                Ok(_) => { i += 1; }
                Err(_) => { conn.subscribers.swap_remove(i); }
            }
        }

    }
    
    pub fn receive(&mut self, message: Message) {
        self.lock().connection(&message.conn_token).rx.send(message)
            .unwrap_or_else(|_| panic!("writing to channel failed"));
    }

    pub fn create_connection(
        &mut self,
        token: Token,
        ) -> ConnectionHandle
    {
        let (tx, rx) = mpsc::unbounded_channel();
        let conn = Connection {
            subscribers: Vec::new(),
            // TODO: fix, this is too confusing
            rx: tx,
        };
        self.lock().connections.insert(token.clone(), conn);

        return ConnectionHandle {
            rx,
            token,
            table_handle: self.clone(),
        };
    }

    pub fn connect(&mut self, token: Token) -> Client {
        let (tx, rx) = mpsc::unbounded_channel();
        self.register_subscriber(&token, tx);
        let table_handle = self.clone();
        return Client { rx, token, table_handle };
    }

    pub fn register_subscriber(
        &mut self,
        token: &Token,
        sink: mpsc::UnboundedSender<Message>)
    {
        self.lock().connection(&token).subscribers.push(sink);
    }
}

// TODO: terminate connections
pub struct ConnectionHandle {
    rx: mpsc::UnboundedReceiver<Message>,
    token: Token,
    table_handle: ConnectionTableHandle,
}

impl ConnectionHandle {
    pub fn emit<T>(&mut self, t: T)
        where T: Serialize
    {
        let serialized = bincode::serialize(&t).unwrap();
        let message = Message {
            conn_token: self.token,
            payload: serialized,
        };
        self.table_handle.emit(message);
    }

    pub fn recv<'a, T>(&'a mut self) -> impl FusedFuture<Output=T> + 'a
        where T: for<'b> Deserialize<'b>
    {
        // TODO: handle errors
        self.rx.next().map(|item| {
            let msg = item.unwrap();
            return bincode::deserialize(&msg.payload[..]).unwrap();
        })
    }
}


pub struct Client {
    rx: mpsc::UnboundedReceiver<Message>,
    token: Token,
    table_handle: ConnectionTableHandle,
}

impl Client {
    pub fn emit<T>(&mut self, t: T)
        where T: Serialize
    {
        let serialized = bincode::serialize(&t).unwrap();
        let message = Message {
            conn_token: self.token,
            payload: serialized,
        };
        self.table_handle.receive(message);
    }

    pub fn recv<'a, T>(&'a mut self) -> impl FusedFuture<Output=T> + 'a
        where T: for<'b> Deserialize<'b>
    {
        // TODO: handle errors
        self.rx.next().map(|item| {
            let msg = item.unwrap();
            return bincode::deserialize(&msg.payload[..]).unwrap();
        })
    }
}

pub struct ConnectionManager {
    rx: mpsc::UnboundedReceiver<Message>,
    tx: mpsc::UnboundedSender<Message>,
    table_handle: ConnectionTableHandle,
}

impl ConnectionManager {
    pub fn new(table_handle: ConnectionTableHandle) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        return ConnectionManager { rx, tx, table_handle };
    }

    pub fn create_connection(&mut self, token: Token) {
        self.table_handle.lock().connections.insert(token, Connection {
            subscribers: Vec::new(),
            rx: self.tx.clone(),
        });
    }

    pub fn emit<T>(&mut self, token: Token, payload: &T)
        where T: Serialize
    {
        self.table_handle.emit(Message {
            conn_token: token,
            payload: bincode::serialize(payload).unwrap(),
        });
    }

    pub async fn recv(&mut self) -> Message {
        return self.rx.recv().await.unwrap()
    }
}
