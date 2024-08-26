use protocol::Action;

#[tokio::main]
async fn main() {
    let (ui_tx, mut ui_rx) = tokio::sync::mpsc::channel(100);
    let mut yt = youtube::Source::new(ui_tx).await;
    let task = tokio::task::spawn(async move { yt.get_all_playlists().await });
    while let Some(Action { command, response }) = ui_rx.recv().await {
        match command {
            protocol::Command::Refresh => todo!(),
            protocol::Command::Restart => todo!(),
            protocol::Command::Playlist(_) => todo!(),
            protocol::Command::Playback(_) => todo!(),
            protocol::Command::UI(cmd) => match cmd {
                protocol::UICommand::PromptUser(s) => println!("{s}"),
                protocol::UICommand::InformUser(s) => println!("{s}"),
                protocol::UICommand::OpenUrl(s) => println!("{s}"),
                protocol::UICommand::CloseNotification(_) => todo!(),
            },
        }
        response.send(protocol::DataType::Unit);
    }
    task.await;
}
