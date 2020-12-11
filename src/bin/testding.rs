// https://doc.rust-lang.org/book/ch02-00-guessing-game-tutorial.html
extern crate tokio;
extern crate rand;

extern crate futures;
extern crate bincode;

use mozaic_core::client::client::run_client;
use mozaic_core::match_context::Connection;
use tokio::time::{Duration, sleep};
use rand::Rng;
use futures::stream::futures_unordered::FuturesUnordered;
use futures::{FutureExt, StreamExt};

use mozaic_core::player_supervisor::{
    PlayerRequest,
    PlayerResponse,
};

use mozaic_core::connection_table::{
    Token,
    ConnectionTable,
    ConnectionTableHandle,
};

use mozaic_core::match_context::MatchCtx;
use mozaic_core::websocket::websocket_server;
use mozaic_core::client_manager::ClientMgrHandle;
use mozaic_core::client::runner::Bot;

#[tokio::main]
async fn main() {
    let conn_table = ConnectionTable::new();
    let client_manager = ClientMgrHandle::new();
    tokio::spawn(
        websocket_server(
            conn_table.clone(),
            client_manager.clone(),
            "127.0.0.1:8080"
        ));
    run_lobby(client_manager, conn_table).await
}


async fn run_lobby(
    client_mgr: ClientMgrHandle,
    conn_table: ConnectionTableHandle,
) {
    let client_tokens: [Token; 2] = rand::thread_rng().gen();

    let mut clients = client_tokens.iter().map(|token| {
        let url = "ws://127.0.0.1:8080";
        let bot = Bot {
            name: "testbot".to_string(),
            argv: vec![
                "python3".to_string(),
                "testbot.py".to_string(),
            ]
        };
        

        tokio::spawn(run_client(url, token.clone(), bot));

        client_mgr.get_client(token)
    }).collect::<Vec<_>>();



    for match_num in 1..=1 {
        println!("match {}", match_num);
        let mut player_mgr = MatchCtx::new(conn_table.clone());

        for (player_num, client) in clients.iter_mut().enumerate() {
            let player_token: Token = rand::thread_rng().gen();
            player_mgr.create_player((player_num + 1) as u32, player_token);
            client.run_player(player_token).await;
        }
    
        guessing_game(player_mgr).await
    }
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
            }
        }
    }
}
