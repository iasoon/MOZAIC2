mod server {
    #[derive(Serialize, Deserialize, Debug)]
    pub struct PlayerMessage {
        pub player_token: Token,
        pub data: Vec<u8>,
    }
    
    #[derive(Serialize, Deserialize, Debug)]
    pub struct StartPlayer {
        pub player_token: Token,
        pub bot_name: String,
    }
}

mod client {
    #[derive(Serialize, Deserialize, Debug)]
    pub struct PlayerMessage {
        pub player_token: Token,
        pub data: Vec<u8>,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct IdentifyClient {
        pub client_token: Token,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct ConnectPlayer {
        pub player_token: Token,
        pub seq: u64,
    }
}
