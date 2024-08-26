use playback::{PlayerInfo, Queue, Repeat, Volume, VolumeSetter};
use playlist::{Playlist, PlaylistId, Song, SongId};
use protocol_derive::Protocol;
use std::marker::PhantomData;
pub use std::time::Duration;
pub use tokio::sync::oneshot::{Receiver, Sender};
pub mod playback;
pub mod playlist;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum DataType {
    NotificationId(NotificationId),
    PlaylistId(PlaylistId),
    Playlist(Playlist),
    Playlists(Vec<PlaylistId>),
    Volume(Volume),
    PlayerInfo(PlayerInfo),
    Repeat(Repeat),
    Queue(Queue),
    Bool(bool),
    Err(Error),
    Unit,
}

#[derive(Debug)]
pub struct Action {
    pub command: Command,
    pub response: Sender<DataType>,
}
impl Action {
    pub(crate) fn new(cmd: impl Into<Command>) -> (Action, Receiver<DataType>) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        (
            Action {
                command: cmd.into(),
                response: sender,
            },
            receiver,
        )
    }
}
#[derive(Debug)]
pub enum Command {
    Refresh,
    Restart,
    Playlist(playlist::Command),
    Playback(playback::Command),
    UI(UICommand),
}
#[derive(Debug)]
pub enum Error {}

#[derive(Debug, Protocol)]
pub enum UICommand {
    #[protocol(output = Result<NotificationId>, args_name = [ prompt ])]
    PromptUser(String),
    #[protocol(output = Result<NotificationId>, args_name = [ message ])]
    InformUser(String),
    #[protocol(output = Result<NotificationId>, args_name = [ url ])]
    OpenUrl(String),
    #[protocol(output = Result<()>, args_name = [ notification_id ])]
    CloseNotification(NotificationId),
}
impl From<UICommand> for Command {
    fn from(cmd: UICommand) -> Self {
        Self::UI(cmd)
    }
}

#[derive(Debug)]
pub struct NotificationId(String);
impl From<NotificationId> for DataType {
    fn from(value: NotificationId) -> Self {
        Self::NotificationId(value)
    }
}

impl From<()> for DataType {
    fn from(_: ()) -> Self {
        Self::Unit
    }
}

impl<T> From<Result<T>> for DataType
where
    T: Into<DataType>,
{
    fn from(result: Result<T>) -> Self {
        match result {
            Ok(val) => val.into(),
            Err(err) => DataType::Err(err),
        }
    }
}
