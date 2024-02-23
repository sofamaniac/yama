mod client;

use anyhow::Result;
use orchestrator::OrchestratorBuilder;
use tokio::{sync::mpsc, task::JoinSet};
use tui::Tui;
mod config;
#[cfg(feature = "mpris")]
mod dbus;
mod logging;
mod orchestrator;
mod tui;

#[tokio::main]
async fn main() -> Result<()> {
    logging::init()?;
    initialize_panic_handler();
    let mut orchestrator_build = OrchestratorBuilder::new();
    let mut tasks_set = JoinSet::new();
    // Creating TUI
    let event_tx = orchestrator_build.get_event_tx();
    let cancel_token = orchestrator_build.get_cancel_token().child_token();
    let mut tui = Tui::new(event_tx.clone(), cancel_token.clone())?;
    orchestrator_build.set_tui(tui.event_tx.clone());
    tasks_set.spawn(async move {
        tui.enter()?;
        tui.run().await;
        Ok(())
    });

    // Creating Dbus session
    #[cfg(feature = "mpris")]
    {
        let (dbus_sender, mut dbus_receiver) = mpsc::channel(2);
        orchestrator_build.set_dbus(dbus_sender);
        tasks_set.spawn(async move { crate::dbus::start(event_tx.clone(), &mut dbus_receiver).await });
    }

    // Creating local client
    #[cfg(feature = "local")]
    {
        let (request_tx, request_rx) = mpsc::channel(32);
        let (answer_tx, answer_rx) = mpsc::channel(32);
        let cancel_token = orchestrator_build.get_cancel_token();
        let mut loc_client = client::local::Client::create(request_rx, answer_tx, cancel_token);
        orchestrator_build.add_client("local".to_string(), request_tx, answer_rx);
        tasks_set.spawn(async move { loc_client.main_loop().await });
    };

    // Creating Youtube client
    #[cfg(feature = "youtube")]
    {
        let (request_tx, request_rx) = mpsc::channel(32);
        let (answer_tx, answer_rx) = mpsc::channel(32);
        let cancel_token = orchestrator_build.get_cancel_token();
        let mut yt_client = client::youtube::Client::create(request_rx, answer_tx, cancel_token.clone());
        orchestrator_build.add_client("youtube".to_string(), request_tx, answer_rx);
        tasks_set.spawn(async move { yt_client.main_loop().await });
    }

    // Creating Spotify client
    #[cfg(feature = "spotify")]
    {
        let (request_tx, request_rx) = mpsc::channel(32);
        let (answer_tx, answer_rx) = mpsc::channel(32);
        let cancel_token = orchestrator_build.get_cancel_token();
        let mut spot_client = client::spotify::Client::create(request_rx, answer_tx, cancel_token.clone());
        orchestrator_build.add_client("spotify".to_string(), request_tx, answer_rx);
        tasks_set.spawn(async move { spot_client.main_loop().await });
    }

    // Starting tasks
    let mut orchestrator = orchestrator_build.build();
    tasks_set.spawn(async move { orchestrator.run().await });
    while tasks_set.join_next().await.is_some() {}
    Ok(())
}

pub fn initialize_panic_handler() {
    // hook to ensure that terminal settings are reset on panic
    // add any extra configuration you need to the hook builder
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        crossterm::execute!(std::io::stderr(), crossterm::terminal::LeaveAlternateScreen).unwrap();
        crossterm::terminal::disable_raw_mode().unwrap();
        original_hook(panic_info);
    }));
}
