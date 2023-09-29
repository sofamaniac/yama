#![deny(clippy::all)]
#![warn(clippy::pedantic)]
//#![warn(clippy::restriction)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]

use std::sync::Arc;

use color_eyre::Result;
use log::*;
use tokio::sync::Mutex;
mod config;
mod dbus;
mod player;
mod sources;
mod ui;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    debug!("ok");
    let ui = Arc::new(Mutex::new(ui::Ui::init()?));
    let ui_clone = ui.clone();
    tokio::spawn(async move { dbus::start_dbus(ui_clone).await.unwrap() });
    ui.lock().await.load();
    ui.lock().await.event_loop().await?;
    ui.lock().await.exit().await?;
    Ok(())
}
