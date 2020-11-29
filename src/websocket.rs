use crate::msg_stream::MsgStreamReader;
use std::collections::HashMap;
use futures::stream::FusedStream;
use futures::{Stream, SinkExt, StreamExt, FutureExt};
use futures::stream::{StreamFuture, FuturesUnordered};
use futures::task::{Poll, Context};
use tokio::net::{TcpListener, TcpStream};
use crate::connection_table::{Token, ConnectionTableHandle};
use crate::player_manager::Connection;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use serde::{Serialize, Deserialize};
use std::pin::Pin;

#[derive(Serialize, Deserialize, Debug)]
struct Send {
    player_token: Token,
    data: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ClientMessage {
    ConnectPlayer { token: Token },
    PlayerMessage { token: Token, data: Vec<u8> },
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

    let mut stream_set: StreamSet<Token, MsgStreamReader> = StreamSet::new();
    let mut writers = HashMap::new();


    loop {
        select!(
            item = stream_set.next() => {
                let (player_token, data) = item.unwrap();
                let msg = Send {
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
                            ClientMessage::ConnectPlayer { token } => {
                                let c = conn_table.connect(&token);

                                // TODO: don't panic
                                // AND BREAK THIS FUNCTION UP JESUS
                                let Connection { tx, rx } = match c {
                                    Some(conn) => conn,
                                    None => panic!("no such connection"),
                                };

                                stream_set.push(token, rx);
                                writers.insert(token, tx);

                            }
                            ClientMessage::PlayerMessage { token, data } => {
                                if let Some(tx) = writers.get_mut(&token) {
                                    tx.write(data);
                                } else {
                                    eprintln!("got message for unregistered player {:x?}", token);
                                }
                            }
                        }
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
    let t_msg = ClientMessage::ConnectPlayer { token };
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
                    let msg = ClientMessage::PlayerMessage {
                        token: token,
                        data: data.as_ref().clone(),
                    };
                    let ws_msg = WsMessage::from(bincode::serialize(&msg).unwrap());
                    ws_stream.send(ws_msg).await.unwrap();
                }
            );
        }
    });

    return downstream;
}

struct StreamSet<K, S> {
    inner: FuturesUnordered<StreamFuture<StreamSetEntry<K, S>>>,
}

impl<K, S> StreamSet<K, S>
    where S: Stream + Unpin,
          K: Unpin,
{
    fn new() -> Self {
        StreamSet { inner: FuturesUnordered::new() }
    }

    fn push(&mut self, key: K, stream: S) {
        self.inner.push(StreamSetEntry{ key, stream }.into_future());
    }
}

impl<K, S> Stream for StreamSet<K, S>
    where S: Stream + Unpin,
          K: Clone + Unpin,
{
    type Item = (K, S::Item);

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>)
        -> Poll<Option<Self::Item>>
    {
        let inner = &mut self.get_mut().inner;
        let res = ready!(inner.poll_next_unpin(cx));
        match res {
            None => return Poll::Pending,
            Some((None, _stream)) => return Poll::Pending,
            Some((Some(item), stream)) => {
                let key = stream.key.clone();
                inner.push(stream.into_future());
                return Poll::Ready(Some((key, item)));
            }
        }
    }
}

impl<K, S> FusedStream for StreamSet<K, S>
    where S: Stream + Unpin,
          K: Clone + Unpin,
{
    fn is_terminated(&self) -> bool {
        return false;
    }
}

struct StreamSetEntry<K, S> {
    key: K,
    stream: S,
}

impl<K, S> Stream for StreamSetEntry<K, S>
    where S: Stream + Unpin,
          K: Unpin,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>)
        -> Poll<Option<Self::Item>>
    {
        self.get_mut().stream.poll_next_unpin(cx)
    }
}