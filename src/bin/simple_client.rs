extern crate serde;
extern crate serde_json;

use std::env;
use std::fs::File;
use mozaic_core::client::simple_client::{ClientParams, simple_client};

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let filename = &args[1];

    let f = File::open(filename).unwrap();
    let params: ClientParams = serde_json::from_reader(f).unwrap();
    simple_client(params).await;
}