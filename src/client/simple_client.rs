use crate::client::runner::{Bot, run_bot};
use super::client::run_client;
use crate::connection_table::Token;

// TODO

// Run a single bot
#[derive(Deserialize)]
pub struct ClientParams {
    // TODO; maybe this should not be a string
    pub server: String,
    #[serde(with="hex")]
    pub token: Token,
    pub argv: Vec<String>,
}

pub async fn simple_client(params: ClientParams) {
    let bot = Bot {
        name: String::from(""),
        argv: params.argv,
    };

    run_client(&params.server, params.token, move |_token, conn| {
        run_bot(bot.clone(), conn)
    }).await;
}
