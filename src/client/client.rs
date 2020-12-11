use crate::player_supervisor::PlayerResponse;
use crate::player_supervisor::PlayerRequest;
use crate::msg_stream::MsgStreamHandle;
use crate::websocket::{StreamSet, ServerMessage, ClientMessage};
use std::collections::HashMap;
use crate::msg_stream::MsgStreamReader;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use futures::{Future, StreamExt, SinkExt};
use crate::match_context::Connection;
use crate::connection_table::Token;
use super::runner::{Bot, run_bot};

pub async fn run_client(url: &str, client_token: Token, bot: Bot) {
    connect_client(url, client_token, move |_token, conn| {
        run_bot(bot.clone(), conn)
    }).await;
}

// yeeted from websocket code for now
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

    let mut stream_set: StreamSet<Token, MsgStreamReader<_>> = StreamSet::new();
    let mut writers: HashMap<Token, MsgStreamHandle<_>> = HashMap::new();

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