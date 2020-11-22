extern crate bytes;
extern crate tokio;
extern crate serde;
#[macro_use]
extern crate futures;
extern crate bincode;
extern crate tokio_tungstenite;

pub mod player_supervisor;
pub mod connection_table;
pub mod player_manager;
pub mod websocket;