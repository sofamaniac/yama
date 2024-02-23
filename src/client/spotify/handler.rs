use std::time::Duration;

use anyhow::Result;
use tokio::sync::broadcast::Sender as BroadSender;
use tokio::sync::mpsc::{self, Receiver as MpscReceiver, Sender as MpscSender};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::client::interface::{Answer, Request};

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
    tasks: JoinSet<()>,
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
            tasks: JoinSet::new(),
        }
    }
    pub async fn main_loop(&mut self) -> Result<()> {
        let (answer_tx, mut answer_rx) = mpsc::channel(32);
        let mut backend = Backend::init(
            self.request_tx.subscribe(),
            answer_tx.clone(),
            self.cancel_token_backend.clone(),
        )
        .await?;
        self.tasks.spawn(async move { backend.main_loop().await });
        loop {
            tokio::select! {
                _ = self.cancel_token_frontend.cancelled() => {self.quit().await; break},
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
        Ok(())
    }

    async fn quit(&mut self) {
        self.cancel_token_backend.cancel();
        // wait for task to terminate
        std::thread::sleep(Duration::from_millis(100));
        if !self.tasks.is_empty() {
            // forcefully shutdown any task remaining
            log::error!("Some tasks failed to abort in 100 milliseconds");
            self.tasks.shutdown().await;
        }
    }
}
