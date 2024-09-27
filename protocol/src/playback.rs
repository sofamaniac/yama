use std::sync::Arc;

use super::Duration;
use crate::{
    playlist::{PlaylistId, Song, SongId},
    to_from_datatype, Playlist,
};

use super::{Command as Cmd, DataType, Result, TypedAction, TypedResult};
use protocol_derive::Protocol;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Volume(u8);
impl Volume {
    pub fn new(val: u8) -> Self {
        Self(val.min(100))
    }
    pub fn add_delta(self, delta: VolumeDelta) -> Volume {
        let VolumeDelta(delta) = delta;
        Self::new(self.0.saturating_add_signed(delta))
    }
    pub fn to_u8(self) -> u8 {
        self.0
    }
}
to_from_datatype!(Volume);
#[derive(Debug)]
pub struct VolumeDelta(pub(crate) i8);
impl VolumeDelta {
    pub fn new(delta: i8) -> Self {
        Self(delta)
    }
}
#[derive(Debug)]
pub enum VolumeSetter {
    Absolute(Volume),
    Relative(VolumeDelta),
}
#[derive(Debug)]
pub enum SeekMode {
    Absolute(Duration),
    Forward(Duration),
    Backward(Duration),
    Percent(u8),
}
#[derive(Debug, Clone)]
pub struct PlayerInfo {
    pub status: PlayerStatus,
    pub paused: bool,
    pub autoplay: bool,
    pub shuffled: bool,
    pub repeat: Repeat,
    pub volume: Volume,
    pub queue: Arc<[Song]>,
}
impl Default for PlayerInfo {
    fn default() -> Self {
        Self {
            status: PlayerStatus::Stopped,
            paused: false,
            autoplay: false,
            shuffled: false,
            repeat: Default::default(),
            volume: Volume(0),
            queue: Default::default(),
        }
    }
}

to_from_datatype!(PlayerInfo);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    Song,
    Playlist,
}
to_from_datatype!(Repeat);
#[derive(Debug, Clone)]
pub enum PlayerStatus {
    Stopped,
    Playing { song: Song, position: Duration },
}
#[derive(Debug)]
pub enum Queue {
    Songs(Arc<[Song]>),
    Playlist(Playlist),
}
to_from_datatype!(Queue);

#[derive(Debug, Protocol)]
pub enum Command {
    #[protocol(output = Volume, args_name = [volume])]
    SetVolume(VolumeSetter),
    #[protocol(output = bool, args_name = [shuffle])]
    SetShuffle(bool),
    #[protocol(output = bool, args_name = [autoplay])]
    SetAutoplay(bool),
    #[protocol(output = Repeat, args_name = [repeat])]
    SetRepeat(Repeat),
    #[protocol(output = (), args_name = [pause])]
    SetPause(bool),
    #[protocol(output = (), args_name = [song_id])]
    Play(Song),
    #[protocol(output = ())]
    PlayPause,
    #[protocol(output = ())]
    Stop,
    #[protocol(output = (), args_name = [seek_mode])]
    Seek(SeekMode),
    #[protocol(output = ())]
    NextSong,
    #[protocol(output = ())]
    PreviousSong,
    #[protocol(output = (), args_name = [queue])]
    AddToQueue(Queue),
    #[protocol(output = Queue)]
    GetQueue,
    #[protocol(output = PlayerInfo)]
    GetInfo,
}

impl From<Command> for Cmd {
    fn from(cmd: Command) -> Self {
        Self::Playback(cmd)
    }
}
