// https://doc.rust-lang.org/book/ch02-00-guessing-game-tutorial.html
extern crate tokio;
extern crate rand;

#[macro_use]
extern crate futures;
extern crate bincode;

use tokio::time::{Duration, sleep};
use rand::Rng;
use futures::stream::futures_unordered::FuturesUnordered;
use futures::{FutureExt, StreamExt};
use futures::task::{Poll};

use std::cell::RefCell;
use std::pin::Pin;

use std::rc::Rc;

use mozaic_core::player_supervisor::{
    PlayerRequest,
    PlayerResponse,
};

use mozaic_core::connection_table::{
    Token,
    ConnectionTable,
};

use mozaic_core::player_manager::PlayerManager;
use mozaic_core::websocket::{websocket_server, ws_client};

#[tokio::main]
async fn main() {
    run_game().await;
}

fn simulate_player(
    player_id: u32,
    player_token: Token)
{        
    tokio::spawn(async move {
        let mut client = ws_client(player_token).await;

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

async fn run_game() {
    let conn_table = ConnectionTable::new();
    tokio::spawn(websocket_server(conn_table.clone()));
    let handler = Rc::new(RefCell::new(PlayerManager::new(conn_table)));

    let game_ = guessing_game(handler.clone());

    pin_mut!(game_);
    let mut game: Pin<&mut _> = game_;

    // register players - this should be done by a lobby or something
    for &player_id in &[1, 2] {
        let player_token: Token = rand::thread_rng().gen();
        handler.borrow_mut().create_player(player_id, player_token);
        simulate_player(
            player_id,
            player_token
        );
    }

    loop {
        if let Poll::Ready(outcome) =  poll!(game.as_mut()) {
            return outcome;
        }
        handler.borrow_mut().step().await;
    }
}

async fn guessing_game(player_handler: Rc<RefCell<PlayerManager>>) {
    let the_number: u8 = rand::thread_rng().gen_range(1, 11);
    println!("the number is {}", the_number);

    let players = player_handler.borrow().players();

    for turn_num in 1..=10  {
        println!("round {}", turn_num);
        let guesses = 
            // for every player:
            players.iter()
            // prompt them for their guess
            .map(|&player_id| {
                player_handler.borrow_mut().request(
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
