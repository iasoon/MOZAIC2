use crate::utils::StreamSet;
use crate::msg_stream::MsgStreamReader;
use futures::{ SinkExt, StreamExt, FutureExt};
use tokio::sync::mpsc;
use tokio::net::{TcpListener, TcpStream};
use crate::connection_table::{Token, ConnectionTableHandle};
use crate::client_manager::{ClientMgrHandle, ClientCtrlMsg};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum ServerMessage {
    RunPlayer { player_token: Token },
    PlayerMessage { player_token: Token, data: Vec<u8> },
    TerminatePlayer { player_token: Token },
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
    let listener = try_socket.expect("Failed to bind");

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(accept_connection(stream, client_mgr.clone(), connection_table.clone()));
    }
}

async fn accept_connection(
    stream: TcpStream,
    mut client_mgr: ClientMgrHandle,
    mut conn_table: ConnectionTableHandle)
{
    let mut ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws_stream) => ws_stream.fuse(),
        Err(error) => {
            eprint!("error during websocket handshake: {}", error);
            return;
        }
    };

    let mut stream_set: StreamSet<Token, MsgStreamReader<Vec<u8>>> = StreamSet::new();
    let (ctrl_tx, mut ctrl_rx) = mpsc::channel::<ClientCtrlMsg>(10);

    let mut client_tokens = Vec::new();
    'handle_messages: loop {
        select!(
            item = stream_set.next() => {
                let (player_token, stream_item) = item.unwrap();
                let msg = match stream_item {
                    Some(data) => {
                        ServerMessage::PlayerMessage {
                            player_token,
                            data: data.as_ref().clone()
                        }
                    }
                    None => {
                        ServerMessage::TerminatePlayer { player_token }
                    }
                };
                let ws_msg = WsMessage::from(rmp_serde::to_vec(&msg).unwrap());
                ws_stream.send(ws_msg).await.unwrap();
            }
            ws_msg = ws_stream.next() => {
                match ws_msg {
                    Some(Ok(msg)) => {
                        let client_msg: ClientMessage = rmp_serde::from_read_ref(&msg.into_data()).unwrap();
                        match client_msg {
                            ClientMessage::ConnectPlayer { player_token } => {
                                if let Some(msg_stream) = conn_table.messages_for(&player_token) {
                                    stream_set.push(player_token, msg_stream.reader());
                                }
                            }
                            ClientMessage::PlayerMessage { player_token, data } => {
                                conn_table.receive(player_token, data);
                            }
                            ClientMessage::IdentifyClient { client_token } => {
                                client_mgr.register_client(client_token, ctrl_tx.clone());
                                client_tokens.push(client_token);
                            }
                        }
                    }
                    _ => break 'handle_messages,
                }
            }
            item = ctrl_rx.recv().fuse() => {
                let ctrl_msg = item.unwrap_or_else(
                    || panic!("cannot happen, we hold a sender"));
                match ctrl_msg {
                    ClientCtrlMsg::StartPlayer { player_token } => {
                        let msg = ServerMessage::RunPlayer { player_token };
                        let ws_msg = WsMessage::from(rmp_serde::to_vec(&msg).unwrap());
                        ws_stream.send(ws_msg).await.unwrap();
                    }
                }
            }
        )
    }

    for token in client_tokens.into_iter() {
        // TODO: this is not correct when there are multiple connections for a client
        client_mgr.unregister_client(token);
    }
}
