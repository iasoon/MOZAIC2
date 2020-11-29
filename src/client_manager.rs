use std::collections::HashMap;
use crate::connection_table::Token;
use tokio::sync::mpsc;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct ClientMgrHandle {
    inner: Arc<Mutex<HashMap<Token, mpsc::Sender<ClientCtrlMsg>>>>,
}

impl ClientMgrHandle {
    pub fn new() -> Self {
        ClientMgrHandle { inner: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn register_client(&mut self, token: Token, tx: mpsc::Sender<ClientCtrlMsg>) {
        let mut inner = self.inner.lock().unwrap();
        inner.insert(token, tx);
    }

    pub fn get_client(&mut self, token: &Token) -> Option<ClientHandle> {
        self.inner.lock().unwrap().get(token).map(|tx| {
            ClientHandle { tx: tx.clone() }
        })
    }
}

pub enum ClientCtrlMsg {
    StartPlayer { player_token: Token },
}

pub struct ClientHandle {
    tx: mpsc::Sender<ClientCtrlMsg>,
}

impl ClientHandle {
    pub async fn run_player(&mut self, player_token: Token) {
        self.tx.send(ClientCtrlMsg::StartPlayer { player_token }).await
            .unwrap_or_else(|_| panic!("client channel broke"))
    }
}