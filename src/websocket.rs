use crate::msg_stream::{MsgStreamHandle, MsgStreamReader};
use futures::Future;
use std::collections::HashMap;
use futures::stream::FusedStream;
use futures::{Stream, SinkExt, StreamExt, FutureExt};
use futures::stream::{StreamFuture, FuturesUnordered};
use futures::task::{Poll, Context};
use tokio::sync::mpsc;
use tokio::net::{TcpListener, TcpStream};
use crate::connection_table::{Token, ConnectionTableHandle};
use crate::player_manager::Connection;
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
    addr: &str)
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
    let mut ws_stream = tokio_tungstenite::accept_async(stream).await
        .expect("Error during the websocket handshake occurred")
        .fuse();

    let mut stream_set: StreamSet<Token, MsgStreamReader> = StreamSet::new();
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



pub async fn connect_client<F, T>(url: &str, client_token: Token, mut run_player: F)
    where F: Send + 'static + FnMut(Token, Connection) -> T,
          T: Future<Output=()> + Send + 'static
{
    let (ws_stream, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("Failed to connect");
    let mut ws_stream = ws_stream.fuse();


    // subscribe to connection
    let t_msg = ClientMessage::IdentifyClient { client_token };
    let ws_msg = WsMessage::from(bincode::serialize(&t_msg).unwrap());
    ws_stream.send(ws_msg).await.unwrap();

    let mut stream_set: StreamSet<Token, MsgStreamReader> = StreamSet::new();
    let mut writers: HashMap<Token, MsgStreamHandle> = HashMap::new();

    tokio::spawn(async move {
        loop {
            select!(
                ws_msg = ws_stream.next() => {
                    let msg = bincode::deserialize(
                        &ws_msg.unwrap().unwrap().into_data()
                    ).unwrap();
                    
                    match msg {
                        ServerMessage::PlayerMessage { player_token, data } => {
                            if let Some(tx) = writers.get_mut(&player_token) {
                                tx.write(data);
                            } else {
                                eprintln!("got message for unregistered player {:x?}", player_token);
                            }
                        }
                        ServerMessage::RunPlayer { player_token } => {
                            let (up, down) = Connection::create();
                            stream_set.push(player_token, up.rx);
                            writers.insert(player_token, up.tx);
                            
                            // run player in background
                            tokio::spawn(run_player(player_token, down));
                            let msg = ClientMessage::ConnectPlayer { player_token };
                            let ws_msg = WsMessage::from(bincode::serialize(&msg).unwrap());
                            ws_stream.send(ws_msg).await.unwrap();
                        } 
                    }
                }
                item = stream_set.next() => {
                    let (player_token, data) = item.unwrap();
                    
                    // Forward message to server
                    let msg = ClientMessage::PlayerMessage {
                        player_token,
                        data: data.as_ref().clone(),
                    };
                    let ws_msg = WsMessage::from(bincode::serialize(&msg).unwrap());
                    ws_stream.send(ws_msg).await.unwrap();
                }
            );
        }
    });
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