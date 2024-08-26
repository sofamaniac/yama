use std::time::Duration;

use crate::playlist::{PlaylistId, Song, SongId};

use super::{Action, Command as Cmd, DataType, Result};
use protocol_derive::Protocol;
use tokio::sync::oneshot::{Receiver, Sender};

#[derive(Debug)]
pub struct Volume(u8);
impl From<Volume> for DataType {
    fn from(value: Volume) -> Self {
        Self::Volume(value)
    }
}
#[derive(Debug)]
pub struct VolumeDelta(i8);
#[derive(Debug)]
pub enum VolumeSetter {
    Absolute(Volume),
    Relative(VolumeDelta),
}
#[derive(Debug)]
pub enum SeekMode {
    Absolute,
    Forward,
    Backward,
}
#[derive(Debug)]
pub struct PlayerInfo {
    status: PlayerStatus,
    autoplay: bool,
    shuffled: bool,
    repeat: Repeat,
    volume: Volume,
}
impl From<PlayerInfo> for DataType {
    fn from(value: PlayerInfo) -> Self {
        Self::PlayerInfo(value)
    }
}
#[derive(Debug)]
pub enum Repeat {
    Off,
    Song,
    Playlist,
}
impl From<Repeat> for DataType {
    fn from(value: Repeat) -> Self {
        Self::Repeat(value)
    }
}
#[derive(Debug)]
pub enum PlayerStatus {
    Stopped,
    Playing { song: Song, position: Duration },
}
#[derive(Debug)]
pub enum Queue {
    Songs(Vec<SongId>),
    Playlist(PlaylistId),
}
impl From<Queue> for DataType {
    fn from(value: Queue) -> Self {
        Self::Queue(value)
    }
}
impl From<bool> for DataType {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

#[derive(Debug, Protocol)]
pub enum Command {
    #[protocol(output = Result<Volume>, args_name = [volume])]
    SetVolume(VolumeSetter),
    #[protocol(output = Result<Volume>)]
    GetVolume,
    #[protocol(output = Result<bool>, args_name = [shuffle])]
    SetShuffle(bool),
    #[protocol(output = Result<bool>)]
    GetShuffle,
    #[protocol(output = Result<bool>, args_name = [autoplay])]
    SetAutoplay(bool),
    #[protocol(output = Result<bool>)]
    GetAutoplay,
    #[protocol(output = Result<Repeat>, args_name = [repeat])]
    SetRepeat(Repeat),
    #[protocol(output = Result<Repeat>)]
    GetRepeat,
    #[protocol(output = Result<()>, args_name = [song_id])]
    Play(SongId),
    #[protocol(output = Result<()>)]
    Pause,
    #[protocol(output = Result<()>)]
    Stop,
    #[protocol(output = Result<()>, args_name = [seek_mode, duration])]
    Seek(SeekMode, Duration),
    #[protocol(output = Result<()>)]
    NextSong,
    #[protocol(output = Result<()>)]
    PreviousSong,
    #[protocol(output = Result<()>, args_name = [queue])]
    AddToQueue(Queue),
    #[protocol(output = Result<Queue>)]
    GetQueue,
    #[protocol(output = PlayerInfo)]
    GetInfo,
}

impl From<Command> for Cmd {
    fn from(cmd: Command) -> Self {
        Self::Playback(cmd)
    }
}
