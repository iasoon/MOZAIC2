use futures::{SinkExt, StreamExt, future};
use futures::future::{FusedFuture, FutureExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use crate::connection_table::{Message, Token, ConnectionTableHandle};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum TransportMsg {
    Subscribe { token: Token },
    Publish { token: Token, data: Vec<u8> },
}

pub async fn websocket_server(
    connection_table: ConnectionTableHandle)
{
    let addr = "127.0.0.1:8080".to_string();
    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("Failed to bind");

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(accept_connection(stream, connection_table.clone()));
    }
}

async fn accept_connection(
    stream: TcpStream,
    mut connection_table: ConnectionTableHandle)
{
    let ws_stream = tokio_tungstenite::accept_async(stream).await
        .expect("Error during the websocket handshake occurred");

    let (tx, rx) = mpsc::unbounded_channel();
    
    let (ws_writer, ws_reader) = ws_stream.split();

    let writer = rx.map(|msg: Message| {
        let t_msg = TransportMsg::Publish {
            token: msg.conn_token,
            data: msg.payload,
        };
        Ok(WsMessage::from(bincode::serialize(&t_msg).unwrap()))
    }).forward(ws_writer);

    let reader = ws_reader.for_each(|ws_msg| {
        let t_msg = bincode::deserialize(
            &ws_msg.unwrap().into_data()
        ).unwrap();
        match t_msg {
            TransportMsg::Subscribe { token } => {
                connection_table.register_subscriber(
                    &token,
                    tx.clone());
            }
            TransportMsg::Publish { token, data } => {
                connection_table.receive(Message {
                    conn_token: token,
                    payload: data,
                });
            }
        }
        return future::ready(());
    });

    join!(reader, writer);
}

pub async fn ws_client(token: Token) -> Client {
    let url = "ws://127.0.0.1:8080".to_string();
    let (mut ws_stream, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("Failed to connect");

    // subscribe to connection
    let t_msg = TransportMsg::Subscribe { token };
    let ws_msg = WsMessage::from(bincode::serialize(&t_msg).unwrap());
    ws_stream.send(ws_msg).await.unwrap();

    let (reader_tx, reader_rx) = mpsc::unbounded_channel();
    let (writer_tx, writer_rx) = mpsc::unbounded_channel();
    let (ws_writer, ws_reader) = ws_stream.split();

    tokio::spawn(async move {
        let receive = ws_reader.for_each(|ws_msg| {
            let t_msg = bincode::deserialize(
                &ws_msg.unwrap().into_data()
            ).unwrap();
            match t_msg {
                TransportMsg::Subscribe { .. } => {
                    panic!("unexpected subscribe");
                }
                TransportMsg::Publish { token, data } => {
                    reader_tx.send(Message {
                        conn_token: token,
                        payload: data,
                    }).unwrap_or_else(|_| panic!("write failed"));
                }
            }
            future::ready(())
        });

        let publish = writer_rx.map(|msg: Message| {
            let t_msg = TransportMsg::Publish {
                token: msg.conn_token,
                data: msg.payload,
            };
            Ok(WsMessage::from(bincode::serialize(&t_msg).unwrap()))
        }).forward(ws_writer);

        join!(receive, publish);
    });

    return Client {
        rx: reader_rx,
        tx: writer_tx,
        token,
    };
}


pub struct Client {
    rx: mpsc::UnboundedReceiver<Message>,
    tx: mpsc::UnboundedSender<Message>,
    token: Token,
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
        self.tx.send(message).unwrap_or_else(|_| panic!("send failed"));
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
