use std::sync::{Arc, Mutex};
use tokio::{sync::mpsc, time::timeout};

use crate::{EventBus, match_context::{PlayerHandle, RequestError, RequestMessage}};

use super::runner::Bot;

pub struct LocalBotHandle {
    tx: mpsc::UnboundedSender<RequestMessage>,
}

impl PlayerHandle for LocalBotHandle {
    fn send_request(&mut self, r: RequestMessage) {
        self.tx.send(r)
            .expect("failed to send message to local bot");
    }

    fn send_info(&mut self, _msg: String) {
        // TODO: log this somewhere
        // drop info message
    }
}


pub fn run_local_bot(player_id: u32, event_bus: Arc<Mutex<EventBus>>, bot: Bot) -> LocalBotHandle {
    let (tx, rx) = mpsc::unbounded_channel();

    let runner = LocalBotRunner {
        event_bus,
        rx,
        player_id,
        bot,
    };
    tokio::spawn(runner.run());

    return LocalBotHandle {
        tx,
    };
}

pub struct LocalBotRunner {
    event_bus: Arc<Mutex<EventBus>>,
    rx: mpsc::UnboundedReceiver<RequestMessage>,
    player_id: u32,
    bot: Bot,
}

impl LocalBotRunner {
    pub async fn run(mut self) {
        let mut process = self.bot.spawn_process();

        while let Some(request) = self.rx.recv().await {
            let resp_fut = process.communicate(&request.content);
            let result = timeout(request.timeout, resp_fut).await;
            let result = match result {
                Ok(line) => Ok(line.into_bytes()),
                Err(_elapsed) => Err(RequestError::Timeout),
            };
            let request_id = (self.player_id, request.request_id);

            self.event_bus.lock()
                .unwrap()
                .resolve_request(request_id, result);
        }
    }
}
