use crate::utils::StreamSet;
use crate::msg_stream::MsgStreamReader;
use std::collections::HashMap;
use futures::stream::FusedStream;
use futures::{Stream, SinkExt, StreamExt, FutureExt};
use futures::stream::{StreamFuture, FuturesUnordered};
use futures::task::{Poll, Context};
use tokio::sync::mpsc;
use tokio::net::{TcpListener, TcpStream};
use crate::connection_table::{Token, ConnectionTableHandle};
use crate::match_context::Connection;
use crate::client_manager::{ClientMgrHandle, ClientCtrlMsg};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use serde::{Serialize, Deserialize};
use std::pin::Pin;

#[derive(Serialize, Deserialize, Debug)]
pub enum ServerMessage {
    PlayerMessage { player_token: Token, data: Vec<u8> },
    RunPlayer { player_token: Token },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ClientMessage {
    ConnectPlayer  { player_token: Token },
    IdentifyClient { client_token: Token },
    PlayerMessage  { player_token: Token, data: Vec<u8> },
}

pub async fn websocket_server(
    connection_table: ConnectionTableHandle,
    client_mgr: ClientMgrHandle,
    addr: String)
{
    let try_socket = TcpListener::bind(&addr).await;
    let mut listener = try_socket.expect("Failed to bind");

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(accept_connection(stream, client_mgr.clone(), connection_table.clone()));
    }
}

async fn accept_connection(
    stream: TcpStream,
    mut client_mgr: ClientMgrHandle,
    mut conn_table: ConnectionTableHandle)
{
    let mut ws_stream = tokio_tungstenite::accept_async(stream).await
        .expect("Error during the websocket handshake occurred")
        .fuse();

    let mut stream_set: StreamSet<Token, MsgStreamReader<Vec<u8>>> = StreamSet::new();
    let mut writers = HashMap::new();

    let (ctrl_tx, mut ctrl_rx) = mpsc::channel::<ClientCtrlMsg>(10);

    loop {
        select!(
            item = stream_set.next() => {
                let (player_token, data) = item.unwrap();
                let msg = ServerMessage::PlayerMessage {
                    player_token,
                    data: data.as_ref().clone()
                };
                let ws_msg = WsMessage::from(bincode::serialize(&msg).unwrap());
                ws_stream.send(ws_msg).await.unwrap();
            }
            ws_msg = ws_stream.next() => {
                match ws_msg {
                    Some(Ok(msg)) => {
                        let client_msg: ClientMessage = bincode::deserialize(&msg.into_data()).unwrap();
                        match client_msg {
                            ClientMessage::ConnectPlayer { player_token } => {
                                let c = conn_table.connect(&player_token);

                                // TODO: don't panic
                                // AND BREAK THIS FUNCTION UP JESUS
                                let Connection { tx, rx } = match c {
                                    Some(conn) => conn,
                                    None => panic!("no such connection"),
                                };

                                stream_set.push(player_token, rx);
                                writers.insert(player_token, tx);

                            }
                            ClientMessage::PlayerMessage { player_token, data } => {
                                if let Some(tx) = writers.get_mut(&player_token) {
                                    tx.write(data);
                                } else {
                                    eprintln!("got message for unregistered player {:x?}", player_token);
                                }
                            }
                            ClientMessage::IdentifyClient { client_token } => {
                                client_mgr.register_client(client_token, ctrl_tx.clone());
                            }
                        }
                    }
                    _ => return,
                }
            }
            item = ctrl_rx.recv().fuse() => {
                let ctrl_msg = item.unwrap_or_else(
                    || panic!("cannot happen, we hold a sender"));
                match ctrl_msg {
                    ClientCtrlMsg::StartPlayer { player_token } => {
                        let msg = ServerMessage::RunPlayer { player_token };
                        let ws_msg = WsMessage::from(bincode::serialize(&msg).unwrap());
                        ws_stream.send(ws_msg).await.unwrap();
                    }
                }
            }
        )
    }
}
