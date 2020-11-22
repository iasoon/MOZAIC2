//! # Higher-lower
//!
//! This example demonstrates a simple higher-lower number guessing game.
//! The goal is to guess the number the server has in mind, if you are wrong
//! it will give you a hint: whether the number was actually higher or lower
//! than your guess.
//!
//! Ofcourse, the goal is to guess the number as fast a possible!

extern crate rand;
extern crate tokio;

#[macro_use]
extern crate futures;
extern crate bincode;

use futures::stream::futures_unordered::FuturesUnordered;
use futures::task::Poll;
use futures::{FutureExt, StreamExt};
use rand::Rng;
use tokio::time::{sleep, Duration};

use std::pin::Pin;
use std::{cell::RefCell, collections::HashMap};

use std::rc::Rc;

use mozaic_core::player_supervisor::{PlayerRequest, PlayerResponse};

use mozaic_core::connection_table::{ConnectionTable, Token};

use mozaic_core::player_manager::PlayerManager;
use mozaic_core::websocket::{websocket_server, ws_client};

#[tokio::main]
async fn main() {
    run_game().await;
}

async fn run_game() {
    let conn_table = ConnectionTable::new();
    tokio::spawn(websocket_server(conn_table.clone()));
    let handler = Rc::new(RefCell::new(PlayerManager::new(conn_table)));

    let game_ = higher_lower(handler.clone());

    pin_mut!(game_);
    let mut game: Pin<&mut _> = game_;

    loop {
        if let Poll::Ready(outcome) = poll!(game.as_mut()) {
            return outcome;
        }
        handler.borrow_mut().step().await;
    }
}

async fn higher_lower(player_handler: Rc<RefCell<PlayerManager>>) {
    let mut winner: Option<u32> = None;
    let the_number: u8 = rand::thread_rng().gen_range(1, 11);
    println!("the number is {}", the_number);

    // list of player ids
    let players: Vec<u32> = vec![1, 2];

    // register players
    for &player_id in players.iter() {
        let player_token: Token = rand::thread_rng().gen();
        player_handler
            .borrow_mut()
            .create_player(player_id, player_token);
        
        // Use a simulated player for demo purposes
        // Player 1 gets to be smarter!
        if player_id == 1 {
            simulate_smarter_player(player_id, player_token)
        } else {
            simulate_player(player_id, player_token);
        }
    }

    // Initialise state
    let mut turn_num: usize = 0;
    let mut previous_guesses: HashMap<u32, Option<u8>> = HashMap::new();
    for player_id in players.iter() {
        previous_guesses.insert(*player_id, None);
    }

    // Game loop
    while winner.is_none() {
        turn_num += 1;
        println!(" round {}", turn_num);

        let guesses =
            // for every player:
            players.iter()

            // prompt them for their guess
            .map(|&player_id| {

                // Provide a hint if the players already made a guess
                // in the last round (== not first round && no time out).
                let answer = match previous_guesses.get(&player_id).unwrap() {
                    Some(guess) => {
                        if *guess < the_number {
                            "higher"
                        } else {
                            "lower"
                        }
                    }
                    None => "just guess bby"
                };

                // Send the answer + request for next guess
                player_handler.borrow_mut().request(
                    player_id,
                    answer.as_bytes().to_owned(), // no data
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

        // Check for winner & and update state
        for (player_id, resp) in guesses {
            if let Ok(bytes) = resp {
                let guess = bytes[0];
                previous_guesses.insert(player_id, Some(guess));
                println!("received guess from {}: {}", player_id, guess);
                if guess == the_number {
                    winner = Some(player_id);
                    break;
                }
            } else {
                previous_guesses.insert(player_id, None);
                println!("{} timed out", player_id);
            }
        }
    }

    println!("{} won the game", winner.unwrap());
}

/// Simulate a player for demo purposes
fn simulate_player(player_id: u32, player_token: Token) {
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
                content: vec![guess],
            };

            client.emit(response);
        }
    });
}

/// Simulate a smarter (!) player for demo purposes
fn simulate_smarter_player(player_id: u32, player_token: Token) {
    tokio::spawn(async move {
        let mut client = ws_client(player_token).await;

        // Initialise state
        let mut upper_bound: u8 = 11;
        let mut lower_bound: u8 = 1;
        let mut last_guess: Option<u8> = None;

        loop {
            let req: PlayerRequest = client.recv().await;
            
            // Some calculation time for demo purposes
            let think_millis = rand::thread_rng().gen_range(0, 1200);
            println!("{} needs to think for {} ms", player_id, think_millis);
            sleep(Duration::from_millis(think_millis)).await;

            // Update bounds based on the server's message
            let hint = String::from_utf8(req.content).unwrap_or_else(|_| panic!("{}'s hint was foul language!", player_id));
            match hint.as_ref() {
                "higher" => {
                    lower_bound = last_guess.unwrap();
                },
                "lower" => {
                    upper_bound = last_guess.unwrap();
                },
                _ => {}
            };

            // Make a calculated guess, giving us most information
            let guess: u8 = (upper_bound + lower_bound) / 2;
            last_guess = Some(guess);
            println!("{} is done thinking and guesses {}", player_id, guess);

            let response = PlayerResponse {
                request_id: req.request_id,
                content: vec![guess],
            };

            client.emit(response);
        }
    });
}