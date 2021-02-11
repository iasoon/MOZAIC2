extern crate bytes;
extern crate tokio;
#[macro_use]
extern crate serde;
#[macro_use]
extern crate futures;
extern crate bincode;
extern crate tokio_tungstenite;

pub mod connection_table;
pub mod match_context;
pub mod websocket;
pub mod msg_stream;
pub mod client_manager;
pub mod client;
pub mod utils;

use std::sync::{Arc, Mutex};

// re-exports
pub use crate::msg_stream::MsgStreamHandle;
pub use connection_table::Token;
pub use match_context::{MatchCtx, EventBus};

use connection_table::{
    ConnectionTable,
    ConnectionTableHandle,
};


use websocket::websocket_server;
use client_manager::{ClientHandle, ClientMgrHandle};
use futures::Future;

#[derive(Clone)]
pub struct GameServer {
    conn_table: ConnectionTableHandle,
    client_manager: ClientMgrHandle,
}

impl GameServer {
    pub fn new() -> Self {
        let conn_table = ConnectionTable::new();
        let client_manager = ClientMgrHandle::new();
        GameServer { conn_table, client_manager }
    }

    pub fn run_ws_server(&self, addr: String) -> impl Future<Output=()>
    {
        websocket_server(
            self.conn_table.clone(),
            self.client_manager.clone(),
            addr
        )
    }
    
    pub fn conn_table(&self) -> ConnectionTableHandle {
        self.conn_table.clone()
    }

    pub fn client_manager(&self) -> &ClientMgrHandle {
        &self.client_manager
    }

    pub fn client_manager_mut(&mut self) -> &mut ClientMgrHandle {
        &mut self.client_manager
    }

    pub fn get_client(&self, token: &Token) -> ClientHandle {
        self.client_manager.get_client(token)
    }

    // TODO: this part should just go.
    // the 'gameserver' part entirely handles player/client connections now,
    // it could totally do time-outs as well.
    pub fn register_player(
        &mut self,
        player_token: Token,
        player_id: u32,
        event_bus: Arc<Mutex<EventBus>>,
    ) {
        self.conn_table.open_connection(player_token, player_id, event_bus);
    }
}
