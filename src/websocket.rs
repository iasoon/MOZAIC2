use futures::{SinkExt, StreamExt, FutureExt};
use tokio::net::{TcpListener, TcpStream};
use crate::connection_table::{Token, ConnectionTableHandle};
use crate::player_manager::Connection;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
struct Connect {
    token: Token,
}

#[derive(Serialize, Deserialize, Debug)]
struct Send {
    data: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum TransportMsg {
    Subscribe { token: Token },
    Publish { token: Token, data: Vec<u8> },
}

pub async fn websocket_server(
    connection_table: ConnectionTableHandle,
    addr: &str)
{
    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("Failed to bind");

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(accept_connection(stream, connection_table.clone()));
    }
}

async fn accept_connection(
    stream: TcpStream,
    mut conn_table: ConnectionTableHandle)
{
    let mut ws_stream = tokio_tungstenite::accept_async(stream).await
        .expect("Error during the websocket handshake occurred")
        .fuse();

    let data = match ws_stream.next().await {
        Some(Ok(data)) => data.into_data(),
        _ => panic!("what did you just send me???"),
    };
    let connect_msg: Connect = bincode::deserialize(&data).unwrap();
    let mut conn = match conn_table.connect(&connect_msg.token) {
        Some(conn) => conn,
        None => panic!("no such connection"),
    };

    loop {
        select!(
            data = conn.rx.recv().fuse() => {
                let msg = Send { data: data.as_ref().clone() };
                let ws_msg = WsMessage::from(bincode::serialize(&msg).unwrap());
                ws_stream.send(ws_msg).await.unwrap();
            }
            ws_msg = ws_stream.next() => {
                match ws_msg {
                    Some(Ok(msg)) => {
                        let send: Send = bincode::deserialize(&msg.into_data()).unwrap();
                        conn.tx.write(send.data);
                    }
                    _ => return,
                }
            }
        )
    }
}

pub async fn ws_connection(url: &str, token: Token) -> Connection {
    let (ws_stream, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("Failed to connect");
    let mut ws_stream = ws_stream.fuse();

    let (mut upstream, downstream) = Connection::create();

    // subscribe to connection
    let t_msg = Connect { token };
    let ws_msg = WsMessage::from(bincode::serialize(&t_msg).unwrap());
    ws_stream.send(ws_msg).await.unwrap();

    tokio::spawn(async move {
        loop {
            select!(
                ws_msg = ws_stream.next() => {
                    let send: Send = bincode::deserialize(
                        &ws_msg.unwrap().unwrap().into_data()
                    ).unwrap();

                    upstream.tx.write(send.data);
                }
                data = upstream.rx.recv().fuse() => {
                    let msg = Send { data: data.as_ref().clone() };
                    let ws_msg = WsMessage::from(bincode::serialize(&msg).unwrap());
                    ws_stream.send(ws_msg).await.unwrap();
                }
            );
        }
    });

    return downstream;
}
