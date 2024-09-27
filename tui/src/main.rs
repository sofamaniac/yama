use std::sync::Arc;

use color_eyre::Result;
use tokio_util::sync::CancellationToken;

mod dbus;
mod player_widget;
mod source;
mod ui;

#[derive(Debug)]
struct Notification(pub String);

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();
    // let app_result = App::new().await.run(terminal).await;
    let cancel_token = CancellationToken::new();
    let (mut app, ui_tx) = ui::UI::new(cancel_token.clone(), Arc::new([]), terminal);
    let (youtube, yt_tx) = youtube::Handler::new(ui_tx, cancel_token.child_token()).await;
    app.add_source(ui::Source {
        name: "Youtube".to_string(),
        out_channel: yt_tx.clone(),
    })
    .await;
    tokio::task::spawn(async move { youtube.run().await });
    tokio::task::spawn(async move { dbus::start(yt_tx, cancel_token).await });
    let app_task = tokio::task::spawn(async move { app.run().await });
    app_task.await?;
    ratatui::restore();
    Ok(())
}
