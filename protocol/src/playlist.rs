use crate::to_from_datatype;

use super::{Command as Cmd, DataType, Result, TypedAction, TypedResult};
use core::fmt;
use protocol_derive::Protocol;
use std::{collections::HashMap, fmt::Display, sync::Arc, time::Duration};
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlaylistId(String);
impl PlaylistId {
    // TODO: change visibility
    pub fn new(id: String) -> Self {
        Self(id)
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
#[derive(Debug, Default, Clone)]
pub struct Song {
    title: String,
    artists: Vec<String>,
    duration: Duration,
    cover_url: Option<String>,
    id: SongId,
    url: String,
}
impl Song {
    pub fn new(id: SongId, url: String, title: String) -> Self {
        Self {
            title,
            id,
            url,
            ..Default::default()
        }
    }
    pub fn id(&self) -> &SongId {
        &self.id
    }
    pub fn add_artist(&mut self, artist: String) {
        self.artists.push(artist)
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
}
#[derive(Debug, Clone)]
pub struct Playlist {
    id: PlaylistId,
    name: String,
    cover_url: Option<String>,
}
#[derive(Debug, Clone)]
pub struct FullPlaylist {
    playlist: Playlist,
    songs: Vec<Song>,
}

impl Playlist {
    pub fn new(id: PlaylistId, name: String) -> Self {
        Self {
            id,
            name,
            cover_url: None,
        }
    }
    pub fn set_cover_url(&mut self, cover_url: String) {
        self.cover_url = Some(cover_url)
    }
    pub fn name(&self) -> &String {
        &self.name
    }
    pub fn id(&self) -> &PlaylistId {
        &self.id
    }
}
impl FullPlaylist {
    pub fn new(playlist: Playlist) -> Self {
        Self {
            playlist,
            songs: Vec::new(),
        }
    }
    pub fn playlist(&self) -> &Playlist {
        &self.playlist
    }
    pub fn playlist_mut(&mut self) -> &mut Playlist {
        &mut self.playlist
    }
    /// Add song to playlist, ignore duplicates
    pub fn add_song(&mut self, song: Song) {
        if !self.songs.iter().any(|s| s.id() == song.id()) {
            self.songs.push(song)
        }
    }
    pub fn id(&self) -> &PlaylistId {
        self.playlist.id()
    }
    pub fn songs(&self) -> &Vec<Song> {
        &self.songs
    }
    pub fn name(&self) -> &String {
        self.playlist.name()
    }
}

to_from_datatype!(Arc<FullPlaylist>, Playlist);
to_from_datatype!(Arc<[Playlist]>, Playlists);
