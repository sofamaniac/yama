use std::time::Duration;

use crate::{
    playlist::{PlaylistId, Song, SongId},
    to_from_datatype, Playlist,
};

use super::{Command as Cmd, DataType, Result, TypedAction, TypedResult};
use protocol_derive::Protocol;
use tokio::sync::oneshot::{Receiver, Sender};

#[derive(Debug)]
pub struct Volume(u8);
to_from_datatype!(Volume);
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
to_from_datatype!(PlayerInfo);
#[derive(Debug, PartialEq, Eq)]
pub enum Repeat {
    Off,
    Song,
    Playlist,
}
to_from_datatype!(Repeat);
#[derive(Debug)]
pub enum PlayerStatus {
    Stopped,
    Playing { song: Song, position: Duration },
}
#[derive(Debug)]
pub enum Queue {
    Songs(Vec<Song>),
    Playlist(Playlist),
}
to_from_datatype!(Queue);

#[derive(Debug, Protocol)]
pub enum Command {
    #[protocol(output = Volume, args_name = [volume])]
    SetVolume(VolumeSetter),
    #[protocol(output = Volume)]
    GetVolume,
    #[protocol(output = bool, args_name = [shuffle])]
    SetShuffle(bool),
    #[protocol(output = bool)]
    GetShuffle,
    #[protocol(output = bool, args_name = [autoplay])]
    SetAutoplay(bool),
    #[protocol(output = bool)]
    GetAutoplay,
    #[protocol(output = Repeat, args_name = [repeat])]
    SetRepeat(Repeat),
    #[protocol(output = Repeat)]
    GetRepeat,
    #[protocol(output = (), args_name = [song_id])]
    Play(Song),
    #[protocol(output = ())]
    Pause,
    #[protocol(output = ())]
    Stop,
    #[protocol(output = (), args_name = [seek_mode, duration])]
    Seek(SeekMode, Duration),
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
