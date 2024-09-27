use crate::to_from_datatype;

use super::Duration;
use super::{Command as Cmd, DataType, Result, TypedAction, TypedResult};
use core::fmt;
use protocol_derive::Protocol;
use std::{fmt::Display, sync::Arc};
#[derive(Debug, Protocol)]
pub enum Command {
    #[protocol(output = Arc<[Playlist]>)]
    ListAll,
    #[protocol(output = Arc<FullPlaylist>, args_name = [playlist_id])]
    Get(Playlist),
    #[protocol(output = (), args_name = [playlist_id, song_id])]
    Add(Playlist, SongId),
    #[protocol(output = (), args_name = [playlist_id, song_id])]
    Remove(Playlist, SongId),
    #[protocol(output = (), args_name = [playlist_id])]
    Delete(Playlist),
    #[protocol(output = PlaylistId, args_name = [playlist_name])]
    Create(String),
}

impl From<Command> for Cmd {
    fn from(cmd: Command) -> Self {
        Self::Playlist(cmd)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlaylistId(String);
impl PlaylistId {
    pub fn new(id: String) -> Self {
        Self(id)
    }
    pub fn to_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for PlaylistId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
to_from_datatype!(PlaylistId);
#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SongId(String);
impl SongId {
    pub fn new(id: String) -> Self {
        Self(id)
    }
}
impl Display for SongId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Song {
    title: String,
    artists: Vec<String>,
    duration: Duration,
    cover_url: Option<String>,
    id: SongId,
    url: String,
}
impl Song {
    pub fn new(
        id: SongId,
        url: String,
        title: String,
        duration: Duration,
        artists: Vec<String>,
    ) -> Self {
        Self {
            title,
            id,
            url,
            duration,
            artists,
            ..Default::default()
        }
    }
    pub fn id(&self) -> &SongId {
        &self.id
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn title_clone(&self) -> String {
        self.title.clone()
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn artists(&self) -> &[String] {
        &self.artists
    }

    pub fn cover_url(&self) -> Option<&String> {
        self.cover_url.as_ref()
    }
}
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct Playlist {
    id: PlaylistId,
    name: String,
    cover_url: Option<String>,
}
#[derive(Debug, Default, Clone)]
pub struct FullPlaylist {
    playlist: Playlist,
    songs: Arc<[Song]>,
}

impl Playlist {
    pub fn new(id: PlaylistId, name: String) -> Self {
        Self {
            id,
            name,
            cover_url: None,
        }
    }
    pub fn with_cover_url(self, cover_url: String) -> Self {
        Self {
            cover_url: Some(cover_url),
            ..self
        }
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn id(&self) -> &PlaylistId {
        &self.id
    }
}
impl FullPlaylist {
    pub fn new(playlist: Playlist, songs: Arc<[Song]>) -> Self {
        Self { playlist, songs }
    }
    pub fn playlist(&self) -> &Playlist {
        &self.playlist
    }
    pub fn id(&self) -> &PlaylistId {
        self.playlist.id()
    }
    pub fn songs(&self) -> &[Song] {
        &self.songs
    }
    pub fn name(&self) -> &str {
        self.playlist.name()
    }
}

to_from_datatype!(Arc<FullPlaylist>, Playlist);
to_from_datatype!(Arc<[Playlist]>, Playlists);
