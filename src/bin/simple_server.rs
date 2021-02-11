extern crate tokio;
extern crate rand;

extern crate futures;
extern crate bincode;
#[macro_use]
extern crate serde;
extern crate serde_json;
extern crate hex;

use mozaic_core::{EventBus, match_context::PlayerHandle, msg_stream::msg_stream};
use std::{fs::File, sync::{Arc, Mutex}};
use tokio::time::Duration;
use rand::Rng;
use futures::stream::futures_unordered::FuturesUnordered;
use futures::{FutureExt, stream, StreamExt};

use mozaic_core::{Token, MatchCtx, GameServer};
use hex::FromHex;

#[derive(Serialize, Deserialize, Debug)]
struct MatchConfig {
    client_tokens: Vec<String>,
}

#[tokio::main]
async fn main() {
    let serv = GameServer::new();
    tokio::spawn(serv.run_ws_server("127.0.0.1:8080".to_string()));

    let file = File::open("match_config.json").unwrap();
    let match_config = serde_json::from_reader(file).unwrap();
    run_lobby(serv, match_config).await
}


async fn run_lobby(
    serv: GameServer,
    match_config: MatchConfig,
) {
    let mut clients = match_config.client_tokens.iter().map(|token_hex| {
        let token = Token::from_hex(&token_hex).unwrap();
        serv.get_client(&token)
    }).collect::<Vec<_>>();

    let event_bus = Arc::new(Mutex::new(EventBus::new()));
    let players = stream::iter(clients.iter_mut().enumerate())
        .then(|(client_num, client)| {
            let player_id = (client_num + 1) as u32;
            let player_token: Token = rand::thread_rng().gen();
            let player = serv.conn_table().open_connection(player_token, player_id, event_bus.clone());
            client.run_player(player_token).map(move |_| (player_id, Box::new(player) as Box<dyn PlayerHandle>))
        }).collect().await;
    let ctx = MatchCtx::new(event_bus, players, msg_stream());
    guessing_game(ctx).await
}

async fn guessing_game(mut match_ctx: MatchCtx) {    
    let the_number: usize = rand::thread_rng().gen_range(1, 11);
    println!("the number is {}", the_number);

    let players = match_ctx.players();

    for turn_num in 1..=10usize  {
        match_ctx.emit(format!("round {}", turn_num));
        let guesses = 
            // for every player:
            players.iter()
            // prompt them for their guess
            .map(|&player_id| {
                match_ctx.request(
                    player_id,
                    Vec::new(), // no data
                    Duration::from_millis(1000),
                ).map(move |resp| {
                    (player_id, resp)
                })
            })
            // collect these futures into a FuturesUnordered
            // for concurrency
            .collect::<FuturesUnordered<_>>()
            // Collect the resulting guesses into a vector
            .collect::<Vec<_>>()
            // await the result
            .await;

        for (player_id, resp) in guesses {
            if let Ok(bytes) = resp {
                let text = String::from_utf8(bytes)
                    .expect("received invalid string");
                let guess = text.parse::<usize>()
                    .expect("failed to parse guess");
                    
                match_ctx.emit(format!("received guess from {}: {}", player_id, guess));
                if guess == the_number {
                    match_ctx.emit(format!("{} won the game", player_id));
                    return;
                }
            } else {
                match_ctx.emit(format!("{} timed out", player_id));
                match_ctx.send_info(player_id, format!("timed out for turn {}", turn_num));
            }
        }
    }
}
