// https://doc.rust-lang.org/book/ch02-00-guessing-game-tutorial.html
extern crate tokio;
extern crate rand;

extern crate futures;
extern crate bincode;

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
};

use mozaic_core::player_manager::PlayerManager;
use mozaic_core::websocket::{websocket_server, ws_connection};

#[tokio::main]
async fn main() {
    let conn_table = ConnectionTable::new();
    tokio::spawn(websocket_server(conn_table.clone(), "127.0.0.1:8080"));
    let player_mgr = PlayerManager::new(conn_table);
    guessing_game(player_mgr).await;

}

fn simulate_player (
    player_id: u32,
    player_token: Token)
{
    tokio::spawn(async move {
        let url = "ws://127.0.0.1:8080";
        let mut client = ws_connection(url, player_token).await;

        loop {
            let req: PlayerRequest = client.recv().await;

            let think_millis = rand::thread_rng().gen_range(0, 1200);
            println!("{} needs to think for {} ms", player_id, think_millis);
        
            sleep(Duration::from_millis(think_millis)).await;
        
        
            let guess: u8 = rand::thread_rng().gen_range(1, 11);
            println!("{} is done thinking and guesses {}", player_id, guess);
        
            let response = PlayerResponse {
                request_id: req.request_id,
                content: vec![guess]
            };

            client.emit(response);
        }
    });
}


async fn guessing_game(mut player_handler: PlayerManager) {
    // register players - this should be done by a lobby or something
    for &player_id in &[1, 2] {
        let player_token: Token = rand::thread_rng().gen();
        player_handler.create_player(player_id, player_token);
        simulate_player(
            player_id,
            player_token
        );
    }
    
    let the_number: u8 = rand::thread_rng().gen_range(1, 11);
    println!("the number is {}", the_number);

    let players = player_handler.players();

    for turn_num in 1..=10  {
        println!("round {}", turn_num);
        let guesses = 
            // for every player:
            players.iter()
            // prompt them for their guess
            .map(|&player_id| {
                player_handler.request(
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
                let guess = bytes[0];
                println!("received guess from {}: {}", player_id, guess);
                if guess == the_number {
                    println!("{} won the game", player_id);
                    return;
                }
            } else {
                println!("{} timed out", player_id);
            }
        }
    }
}
