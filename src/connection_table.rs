use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};


use crate::match_context::Connection;
use crate::msg_stream::{msg_stream, MsgStreamHandle};

// TODO: this code sucks. Please help.
pub struct ConnectionStub {
    // sending half
    outgoing: MsgStreamHandle<Vec<u8>>,
    // receiving half
    incoming: MsgStreamHandle<Vec<u8>>,
}

pub struct ConnectionTable {
    connections: HashMap<Token, ConnectionStub>,
}

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

    fn conn<'a>(&'a mut self, token: &Token) -> &'a mut ConnectionStub {
        if let Some(conn) = self.connections.get_mut(token) {
            conn
        } else {
            panic!("unknown connection {:x?}", token);
        }
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
        self.lock().conn(&token).incoming.write(data);
    }

    pub fn open_connection(&mut self, token: Token) -> Connection {
        let incoming = msg_stream();
        let outgoing = msg_stream();

        let local_conn = Connection {
            tx: outgoing.clone(),
            rx: incoming.reader(),
        };

        let stub = ConnectionStub { incoming, outgoing };
        self.lock().connections.insert(token, stub);
        return local_conn;
    }

    pub fn connect(&mut self, token: &Token) -> Option<Connection> {
        self.lock().connections.get(token).map(|conn_stub| {
            Connection {
                tx: conn_stub.incoming.clone(),
                rx: conn_stub.outgoing.reader(),
            }
        })
    }

    pub fn outgoing(&mut self, token: &Token) -> MsgStreamHandle<Vec<u8>> {
        self.lock().connections.get(token).unwrap().outgoing.clone()
    }
}
