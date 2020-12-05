// https://doc.rust-lang.org/book/ch02-00-guessing-game-tutorial.html
extern crate tokio;
extern crate rand;

extern crate futures;
extern crate bincode;

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
use mozaic_core::websocket::{websocket_server, connect_client};
use mozaic_core::client_manager::ClientMgrHandle;

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

fn simulate_client(client_token: Token) {
    // run client in background
    tokio::spawn(async move {
        let url = "ws://127.0.0.1:8080";
        let delay_millis = rand::thread_rng().gen_range(0, 3000);
        println!("delaying client {:02x?} for {} ms", &client_token[..4], delay_millis);
    
        sleep(Duration::from_millis(delay_millis)).await;

        connect_client(url, client_token, simulate_player).await;
    });
}

async fn simulate_player(player_token: Token, mut conn: Connection) {
    loop {
        let req: PlayerRequest = conn.recv().await;

        let think_millis = rand::thread_rng().gen_range(0, 1200);
        println!("{:02x?} needs to think for {} ms", &player_token[..4], think_millis);
    
        sleep(Duration::from_millis(think_millis)).await;
    
    
        let guess: u8 = rand::thread_rng().gen_range(1, 11);
        println!("{:02x?} is done thinking and guesses {}", &player_token[..4], guess);
    
        let response = PlayerResponse {
            request_id: req.request_id,
            content: vec![guess]
        };

        conn.emit(response);
    }
}

async fn run_lobby(
    client_mgr: ClientMgrHandle,
    conn_table: ConnectionTableHandle,
) {
    let client_tokens: [Token; 2] = rand::thread_rng().gen();

    let mut clients = client_tokens.iter().map(|token| {
        simulate_client(token.clone());
        client_mgr.get_client(token)
    }).collect::<Vec<_>>();



    for match_num in 1..=3 {
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
    let the_number: u8 = rand::thread_rng().gen_range(1, 11);
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
                let guess = bytes[0];
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
