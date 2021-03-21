use std::collections::HashMap;
use crate::connection_table::Token;
use tokio::sync::{mpsc, oneshot};
use std::sync::{Arc, Mutex};

type Callback = Box<dyn 'static + Send + FnMut(&Token) -> ()>;

struct ClientMgrState {
    clients: HashMap<Token, ClientEntry>,
    connect_handlers: Vec<Callback>,
    disconnect_handlers: Vec<Callback>,
}

enum ClientEntry {
    Connected { tx: mpsc::Sender<ClientCtrlMsg> },
    Disconnected { waiting: Vec<oneshot::Sender<mpsc::Sender<ClientCtrlMsg>>> },
}

#[derive(Clone)]
pub struct ClientMgrHandle {
    inner: Arc<Mutex<ClientMgrState>>,
}

impl ClientMgrHandle {
    pub fn new() -> Self {
        ClientMgrHandle { 
            inner: Arc::new(Mutex::new(
                ClientMgrState {
                    clients: HashMap::new(),
                    connect_handlers: Vec::new(),
                    disconnect_handlers: Vec::new(),
                }
            ))
        }
    }

    pub fn register_client(&mut self, token: Token, tx: mpsc::Sender<ClientCtrlMsg>) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(ClientEntry::Disconnected { waiting }) = inner.clients.remove(&token)
        {
            for waiter in waiting {
                // don't care about errors here
                let _ = waiter.send(tx.clone());
            }
        }
        inner.clients.insert(token, ClientEntry::Connected { tx });
        for callback in inner.connect_handlers.iter_mut() {
            callback(&token);
        }
    }

    pub fn unregister_client(&mut self, token: Token) {
        let mut inner = self.inner.lock().unwrap();
        inner.clients.remove(&token);
        for callback in inner.disconnect_handlers.iter_mut() {
            callback(&token);
        }
    }

    pub fn on_connect(&mut self, cb: Callback) {
        let mut inner = self.inner.lock().unwrap();
        inner.connect_handlers.push(cb);
    }

    pub fn on_disconnect(&mut self, cb: Callback) {
        let mut inner = self.inner.lock().unwrap();
        inner.disconnect_handlers.push(cb);
    }

    pub fn is_connected(&self, token: &Token) -> bool {
        let inner = self.inner.lock().unwrap();
        return inner.clients.contains_key(token);
    }

    pub fn get_client(&self, token: &Token) -> ClientHandle {
        ClientHandle {
            token: token.clone(),
            client_mgr: self.clone(),
        }
    }

    fn request_tx(&mut self, token: &Token)
        -> oneshot::Receiver<mpsc::Sender<ClientCtrlMsg>>
    {
        let (tx, rx) = oneshot::channel();
        let mut inner = self.inner.lock().unwrap();
        let entry = inner.clients.entry(token.clone()).or_insert_with(|| {
            ClientEntry::Disconnected { waiting: Vec::new() }
        });
        match entry {
            ClientEntry::Connected { tx: client_tx } => {
                // no error here, we have the receiver in scope
                tx.send(client_tx.clone()).unwrap();
            }
            ClientEntry::Disconnected { waiting } => {
                waiting.push(tx);
            }
        }
        return rx;
    }

}

pub enum ClientCtrlMsg {
    StartPlayer { player_token: Token },
}

pub struct ClientHandle {
    token: Token,
    client_mgr: ClientMgrHandle,
}

impl ClientHandle {
    async fn request_tx(&mut self) -> mpsc::Sender<ClientCtrlMsg> {
        self.client_mgr.request_tx(&self.token).await
            .unwrap_or_else(|_| panic!("sender should never be dropped"))
    }

    pub async fn run_player(mut self, player_token: Token) {
        let tx = self.request_tx().await;
        tx.send(ClientCtrlMsg::StartPlayer { player_token }).await
            .unwrap_or_else(|_| panic!("client channel broke"))
    }
}