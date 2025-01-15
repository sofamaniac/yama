use std::{ops::Deref, sync::Arc};

use anyhow::{bail, Result};

use protocol::Action;
use tokio::sync::{mpsc::Sender, Mutex};
use tokio_util::sync::CancellationToken;

mod dbus;
mod player_widget;
mod ui;

#[derive(Debug)]
struct Notification(pub String);

#[derive(Clone)]
struct Source {
    pub name: String,
    pub sender: Sender<Action>,
}

#[derive(Clone, Default)]
struct Sources {
    pub sources: Vec<Source>,
    active: Option<usize>,
}

impl Sources {
    const fn new() -> Self {
        Self {
            sources: Vec::new(),
            active: None,
        }
    }
    fn get_active(&self) -> Option<&Source> {
        if let Some(index) = self.active {
            self.sources.get(index)
        } else {
            None
        }
    }
}

#[derive(Clone, Default)]
struct FullSource(Arc<Mutex<Sources>>);
impl Deref for FullSource {
    type Target = Arc<Mutex<Sources>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl FullSource {
    async fn add(&mut self, name: String, sender: Sender<Action>) {
        self.lock().await.sources.push(Source { name, sender })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let Ok(_) = color_eyre::install() else {
        bail!("Could not initialize color_eyre")
    };
    let terminal = ratatui::init();
    let cancel_token = CancellationToken::new();
    let mut sources = FullSource(Arc::new(Mutex::new(Sources::new())));
    let (mut app, ui_tx) = ui::UI::new(cancel_token.clone(), sources.clone(), terminal);
    let (youtube, yt_tx) = youtube::Handler::new(ui_tx, cancel_token.child_token()).await;
    sources.add(String::from("Youtube"), yt_tx).await;
    tokio::task::spawn(async move { youtube.run().await });
    tokio::task::spawn(async move { dbus::start(sources.clone(), cancel_token).await });
    let app_task = tokio::task::spawn(async move { app.run().await });
    let res = app_task.await;
    ratatui::restore();
    res?;
    Ok(())
}
