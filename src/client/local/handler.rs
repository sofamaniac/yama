use anyhow::Result;
use tokio::sync::broadcast::Sender as BroadSender;
use tokio::sync::mpsc::{self, Receiver as MpscReceiver, Sender as MpscSender};
use tokio_util::sync::CancellationToken;

use crate::client::interface::{Answer, Request};

use super::super::mpv::PlayerHandler;
use super::backend::Backend;

pub struct Client {
    /// channel on which request are received
    receiver: MpscReceiver<Request>,
    /// channel on which to send back answers
    sender: MpscSender<Answer>,
    /// channel used to send [Request] to [Backend] and [PlayerHandler]
    request_tx: BroadSender<Request>,
    /// cancel token shared with frontend
    cancel_token_frontend: CancellationToken,
    /// cancel token shared with [Backend] and [PlayerHandler]
    /// is automatically cancel when [Self::cancel_token_frontend] is cancelled
    cancel_token_backend: CancellationToken,
}
impl Client {
    pub fn create(
        receiver: MpscReceiver<Request>,
        sender: MpscSender<Answer>,
        cancel_token_frontend: CancellationToken,
    ) -> Self {
        let (request_tx, _) = tokio::sync::broadcast::channel(10);
        let cancel_token_backend = cancel_token_frontend.child_token();
        Client {
            receiver,
            sender,
            request_tx,
            cancel_token_frontend,
            cancel_token_backend,
        }
    }
    pub async fn main_loop(&mut self) -> Result<()> {
        let (answer_tx, mut answer_rx) = mpsc::channel(32);
        let mut backend = Backend::init(
            self.request_tx.subscribe(),
            answer_tx.clone(),
            self.cancel_token_backend.clone(),
        );
        let mut player = PlayerHandler::new(
            self.request_tx.subscribe(),
            answer_tx.clone(),
            self.cancel_token_backend.clone(),
        );
        let task_backend = tokio::spawn(async move { backend.main_loop().await });
        let task_player = tokio::spawn(async move { player.main_loop().await });
        loop {
            tokio::select! {
                _ = self.cancel_token_frontend.cancelled() => {self.quit(); break},
                maybe_request = self.receiver.recv() => {
                    if let Some(request) = maybe_request {
                        if self.request_tx.send(request).is_err() {
                            // everyone is dead :(
                            break;
                        };
                    } else {
                        // the channel was closed
                        break;
                        // TODO: send quit message to backend and player;
                    }
                },
                maybe_answer = answer_rx.recv() => {
                    if let Some(answer) = maybe_answer {
                        if self.sender.send(answer).await.is_err() {
                            // the connection was drop
                            break;
                        }
                    } else {
                        // TODO
                        continue;
                    }
                }
            }
        }
        let _ = task_backend.await;
        let _ = task_player.await;
        Ok(())
    }

    fn quit(&self) {
        self.cancel_token_backend.cancel()
    }
}
