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

use tokio::sync::{mpsc};

use std::rc::Rc;

use mozaic_core::player_supervisor::{
    PlayerRequest,
    PlayerResponse,
};

use mozaic_core::connection_table::{
    Token,
    ConnectionTable,
    ConnectionTableHandle,
    Message,
};

use mozaic_core::player_manager::PlayerManager;

#[tokio::main]
async fn main() {
    run_game().await;
}

fn simulate_player(
    mut connection_table: ConnectionTableHandle,
    player_id: u32,
    player_token: Token)
{
    let (tx, rx) = mpsc::unbounded_channel();
    connection_table.subscribe(&player_token, tx);

    let player = rx.for_each(move |msg| {        
        simulate_palyer_response(player_id, msg, connection_table.clone())
    });
    tokio::spawn(player);
}

async fn simulate_palyer_response(
    player_id: u32,
    msg: Message,
    mut connection_table: ConnectionTableHandle)
{
    // decode
    let req: PlayerRequest = bincode::deserialize(&msg.payload).unwrap();
    // think for a bit
    let think_millis = rand::thread_rng().gen_range(0, 1200);
    println!("{} needs to think for {} ms", player_id, think_millis);

    sleep(Duration::from_millis(think_millis)).await;


    let guess: u8 = rand::thread_rng().gen_range(1, 11);
    println!("{} is done thinking and guesses {}", player_id, guess);

    let player_msg = PlayerResponse {
        request_id: req.request_id,
        content: vec![guess]
    };
    let resp = Message {
        conn_token: msg.conn_token,
        payload: bincode::serialize(&player_msg).unwrap(),
    };
    
    connection_table.receive(resp);
}

async fn run_game() {
    let conn_table = ConnectionTable::new();
    let handler = Rc::new(RefCell::new(PlayerManager::new(conn_table)));

    let game_ = guessing_game(handler.clone());

    pin_mut!(game_);
    let mut game: Pin<&mut _> = game_;

    loop {
        if let Poll::Ready(outcome) =  poll!(game.as_mut()) {
            return outcome;
        }
        handler.borrow_mut().step().await;
    }
}

async fn guessing_game(player_handler: Rc<RefCell<PlayerManager>>) {

    let mut winner = None;
    let the_number: u8 = rand::thread_rng().gen_range(1, 11);
    println!("the number is {}", the_number);

    // list of player ids
    let players: Vec<u32> = vec![1, 2];
    
    // register players
    for &player_id in players.iter() {
        let player_token: Token = rand::thread_rng().gen();
        player_handler.borrow_mut().create_player(player_id, player_token);
        simulate_player(
            player_handler.borrow().connection_table.clone(),
            player_id,
            player_token
        );
    }

    let mut turn_num: usize = 0;
    while winner.is_none() {
        turn_num += 1;
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
                    winner = Some(player_id);
                    break;
                }
            } else {
                println!("{} timed out", player_id);
            }
        }
    }
    println!("{} won the game", winner.unwrap());
}
