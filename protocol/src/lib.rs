pub use iso8601;
pub use playback::{PlayerInfo, Queue, Repeat, Volume};
pub use playlist::{FullPlaylist, Playlist, PlaylistId, Song, SongId};
use protocol_derive::Protocol;
use std::{fmt::Display, marker::PhantomData, sync::Arc};
use thiserror::Error;
pub use tokio::sync::oneshot::{Receiver, Sender};
use uuid::Uuid;
pub mod playback;
pub mod playlist;

pub type Duration = iso8601::Duration;

pub type Result<T> = std::result::Result<T, Error>;
pub type ResponseSender = tokio::sync::oneshot::Sender<Result<DataType>>;

#[derive(Debug)]
pub enum DataType {
    NotificationId(NotificationId),
    PlaylistId(PlaylistId),
    Playlist(Arc<FullPlaylist>),
    Playlists(Arc<[Playlist]>),
    Volume(Volume),
    PlayerInfo(PlayerInfo),
    Repeat(Repeat),
    Queue(Queue),
    Bool(bool),
    Unit(()),
}

pub struct TypedAction<T> {
    command: Command,
    _ret_typ: PhantomData<T>,
}
impl<T> TypedAction<T> {
    pub fn from_command(cmd: impl Into<Command>) -> Self {
        Self {
            command: cmd.into(),
            _ret_typ: PhantomData,
        }
    }
    pub async fn send(self, channel: &tokio::sync::mpsc::Sender<Action>) -> TypedResult<T> {
        let (action, receiver) = Action::new(self.command);
        channel.send(action).await;
        TypedResult::new(receiver)
    }
    pub fn try_send(self, channel: &tokio::sync::mpsc::Sender<Action>) -> TypedResult<T> {
        let (action, receiver) = Action::new(self.command);
        channel.try_send(action).expect("Channel full");
        TypedResult::new(receiver)
    }
}
pub struct TypedResult<T> {
    receiver: Receiver<Result<DataType>>,
    _ret_typ: PhantomData<T>,
}
impl<T> TypedResult<T> {
    pub(crate) fn new(receiver: Receiver<Result<DataType>>) -> Self {
        Self {
            receiver,
            _ret_typ: PhantomData,
        }
    }
}

#[macro_export]
macro_rules! to_from_datatype {
    ($ty:ty, $ident:ident) => {
        $crate::to_datatype!($ty, $ident);
        $crate::from_datatype!($ty, $ident);
    };
    ($ident:ident) => {
        $crate::to_datatype!($ident, $ident);
        $crate::from_datatype!($ident, $ident);
    };
}

#[macro_export]
macro_rules! to_datatype {
    ($type: tt, $ident: tt) => {
        impl From<$type> for DataType {
            fn from(value: $type) -> Self {
                Self::$ident(value)
            }
        }
    };
}
#[macro_export]
macro_rules! from_datatype {
    ($type: tt, $ident: tt) => {
        impl TypedResult<Result<$type>> {
            pub async fn recv(self) -> Result<$type> {
                let res = self.receiver.await?;
                match res? {
                    DataType::$ident(val) => Ok(val),
                    _ => Err($crate::Error::WrongType),
                }
            }
        }
    };
}

#[derive(Debug)]
pub struct Action {
    pub command: Command,
    pub response: ResponseSender,
}
impl Action {
    pub(crate) fn new(cmd: impl Into<Command>) -> (Action, Receiver<Result<DataType>>) {
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
    Quit,
    Playlist(playlist::Command),
    Playback(playback::Command),
    UI(UICommand),
}
#[derive(Debug, Error)]
pub enum Error {
    #[error("Oneshot error")]
    Oneshot(#[from] tokio::sync::oneshot::error::RecvError),
    #[error("Something is not respecting the protocol")]
    WrongType,
    #[error("io error")]
    Io(#[from] std::io::Error),
    #[error("Loading")]
    Loading,
}

#[derive(Debug, Protocol)]
pub enum UICommand {
    #[protocol(output = NotificationId, args_name = [ prompt ])]
    PromptUser(String),
    #[protocol(output = NotificationId, args_name = [ message ])]
    InformUser(String),
    #[protocol(output = NotificationId, args_name = [ url ])]
    OpenUrl(String),
    #[protocol(output = (), args_name = [ notification_id ])]
    CloseNotification(NotificationId),
}
impl From<UICommand> for Command {
    fn from(cmd: UICommand) -> Self {
        Self::UI(cmd)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NotificationId(Uuid);
impl NotificationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}
impl Display for NotificationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
to_from_datatype!(NotificationId);
to_from_datatype!((), Unit);
to_from_datatype!(bool, Bool);
