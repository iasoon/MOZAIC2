extern crate bytes;
extern crate tokio;
#[macro_use]
extern crate serde;
#[macro_use]
extern crate futures;
extern crate bincode;
extern crate tokio_tungstenite;

pub mod player_supervisor;
pub mod connection_table;
pub mod match_context;
pub mod websocket;
pub mod msg_stream;
pub mod client_manager;
pub mod client;
pub mod utils;

// re-exports
use crate::player_supervisor::RequestMessage;
use crate::msg_stream::MsgStreamHandle;
use crate::player_supervisor::PlayerSupervisor;
use crate::msg_stream::msg_stream;
use crate::match_context::EventBus;
pub use connection_table::Token;
pub use match_context::MatchCtx;

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
        player_id: u32,
        player_token: Token,
        event_bus: &EventBus,
    ) -> MsgStreamHandle<RequestMessage>
    {
        let player_stream = msg_stream();
        PlayerSupervisor::create(
            self.conn_table.clone(),
            event_bus.clone(),
            player_stream.reader(),
            player_id,
            player_token,
        ).run();
        return player_stream;
    }
}
