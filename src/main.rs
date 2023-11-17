#![deny(clippy::all)]
#![warn(clippy::pedantic)]
//#![warn(clippy::restriction)]
//#![warn(clippy::nursery)]
#![warn(clippy::cargo)]

use std::{sync::Arc, time::Duration};

use color_eyre::Result;
use tokio::sync::Mutex;
mod config;
mod dbus;
mod event;
mod logging;
mod player;
mod sources;
mod ui;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    logging::init()?;
    let ui = Arc::new(Mutex::new(ui::Ui::init().await?));
    let ui_clone = ui.clone();
    tokio::spawn(async move { dbus::start(ui_clone).await.unwrap() });
    ui.lock().await.load();
    loop {
        if crossterm::event::poll(Duration::from_millis(100))? {
            let e = crossterm::event::read()?;
            if let crossterm::event::Event::Key(key) = e {
                ui.lock().await.handle_key(key).await;
                if ui.lock().await.quit {
                    break;
                }
            };
        }
        ui.lock().await.draw().await?;
    }
    ui.lock().await.exit().await?;
    Ok(())
}
