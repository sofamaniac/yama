use std::{fmt::Display, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    PlayerAction(PlayerAction),
    Get(GetRequest),
    Set(SetRequest),
    Command(String),
}

impl From<PlayerAction> for Request {
    fn from(value: PlayerAction) -> Self {
        Self::PlayerAction(value)
    }
}
impl From<GetRequest> for Request {
    fn from(value: GetRequest) -> Self {
        Self::Get(value)
    }
}
impl From<SetRequest> for Request {
    fn from(value: SetRequest) -> Self {
        Self::Set(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlayerAction {
    PlayPause(bool),
    PlayPauseToggle,
    Stop,
    Shuffle(bool),
    ShuffleToggle,
    Autoplay(bool),
    AutoplayToggle,
    Seek { dt: i64, mode: SeekMode },
    Prev,
    Next,
    SetVolume(Volume),
    SetTrackList(PlaylistInfo),
    SetRepeat(Repeat),
    CycleRepeat,
}
#[derive(Debug, Clone, Serialize, Deserialize, Copy, PartialEq, Eq)]
pub enum SeekMode {
    Absolute,
    Relative,
    AbsolutePercent,
    RelativePercent,
}

#[derive(Debug, Clone, Serialize, Deserialize, Copy, PartialEq, Eq)]
pub enum Volume {
    Absolute(usize),
    Relative(isize),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GetRequest {
    PlaylistList,
    Playlist(String),
    PlayerInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SetRequest {
    AddSongToPlaylist { song: String, playlist: String },
    RemoveSongFromPlaylist { song: String, playlist: String },
}
#[derive(Debug, Clone, Default)]
pub struct PlayerInfo {
    /// current playback status
    pub playback: Playback,
    /// current song playing
    pub song_info: Option<SongInfo>,
    /// tracklist
    pub tracklist: PlaylistInfo,
    /// index in [`Self::tracklist`] of current song
    pub track_index: Option<usize>,
    pub shuffled: bool,
    pub autoplay: bool,
    pub repeat: Repeat,
    pub volume: u8,
    pub position: Duration,
    pub can_seek: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    Playlist,
    Song,
}
impl Display for Repeat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match &self {
            Repeat::Off => "Off",
            Repeat::Playlist => "Playlist",
            Repeat::Song => "Song",
        };
        write!(f, "{text}")
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Playback {
    #[default]
    Stop,
    Play,
    Pause,
}
impl Display for Playback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match &self {
            Self::Stop => "Stopped",
            Self::Pause => "Paused",
            Self::Play => "Playing",
        };
        write!(f, "{text}")
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SongInfo {
    pub title: String,
    pub artist: String,
    pub cover_url: String,
    pub id: String,
    pub url: String,
    pub duration: Duration,
}

#[derive(Debug)]
pub enum Widget {
    Alert {
        title: String,
        content: String,
    },
    Checkboxes {
        title: String,
        content: Vec<(bool, String)>,
        backchannel: oneshot::Sender<Vec<(bool, String)>>,
    },
    Radioboxes {
        title: String,
        content: Vec<(bool, String)>,
        backchannel: oneshot::Sender<usize>,
    },
    PromptBox {
        title: String,
        content: String,
        backchannel: oneshot::Sender<String>,
    },
}

impl Widget {
    pub fn captures_output(&self) -> bool {
        match self {
            Widget::Alert { .. } => false,
            _ => true,
        }
    }
    pub fn title(&self) -> &String {
        match self {
            Widget::Alert { title, .. }
            | Widget::Checkboxes { title, .. }
            | Widget::Radioboxes { title, .. }
            | Widget::PromptBox { title, .. } => title,
        }
    }
}

#[derive(Debug)]
pub enum Answer {
    PlayerInfo(PlayerInfo),
    PlaylistList(Vec<PlaylistInfo>),
    Playlist(PlaylistInfo),
    Widget(Widget),
    Ok,
}

impl From<Widget> for Answer {
    fn from(value: Widget) -> Self {
        Answer::Widget(value)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaylistInfo {
    pub title: String,
    pub length: usize,
    pub cover_url: String,
    pub id: String,
    pub songs: Vec<SongInfo>,
}
