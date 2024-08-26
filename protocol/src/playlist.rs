use super::{Action, Command as Cmd, DataType, Result};
use core::fmt;
use protocol_derive::Protocol;
use std::{collections::HashMap, fmt::Display, time::Duration};
use tokio::sync::oneshot::{Receiver, Sender};
#[derive(Debug, Protocol)]
pub enum Command {
    #[protocol(output = Vec<PlaylistId>)]
    ListAll,
    #[protocol(output = Result<Playlist>, args_name = [playlist_id])]
    Get(PlaylistId),
    #[protocol(output = Result<()>, args_name = [playlist_id, song_id])]
    Add(PlaylistId, SongId),
    #[protocol(output = Result<()>, args_name = [playlist_id, song_id])]
    Remove(PlaylistId, SongId),
    #[protocol(output = Result<()>, args_name = [playlist_id])]
    Delete(PlaylistId),
    #[protocol(output = Result<PlaylistId>, args_name = [playlist_name])]
    Create(String),
}

impl From<Command> for Cmd {
    fn from(cmd: Command) -> Self {
        Self::Playlist(cmd)
    }
}

#[derive(Debug)]
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
impl From<PlaylistId> for DataType {
    fn from(value: PlaylistId) -> Self {
        Self::PlaylistId(value)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SongId(String);
#[derive(Debug)]
pub struct Song {
    title: String,
    artists: Vec<String>,
    duration: Duration,
    cover_url: Option<String>,
    id: SongId,
}
impl Song {
    pub fn new() -> Self {
        todo!()
    }
    pub fn id(&self) -> &SongId {
        &self.id
    }
}
#[derive(Debug)]
pub struct Playlist {
    id: PlaylistId,
    name: String,
    songs_order: Vec<SongId>,
    songs: HashMap<SongId, Song>,
    cover_url: Option<String>,
}

impl Playlist {
    pub fn new(id: PlaylistId, name: String) -> Self {
        Self {
            id,
            name,
            songs_order: Vec::new(),
            songs: HashMap::new(),
            cover_url: None,
        }
    }
    pub fn set_cover_url(&mut self, cover_url: String) {
        self.cover_url = Some(cover_url)
    }
    pub fn add_song(&mut self, song: Song) {
        self.songs_order.push(song.id().clone());
        self.songs.insert(song.id().clone(), song);
    }
}

impl From<Playlist> for DataType {
    fn from(value: Playlist) -> Self {
        Self::Playlist(value)
    }
}
impl From<Vec<PlaylistId>> for DataType {
    fn from(value: Vec<PlaylistId>) -> Self {
        Self::Playlists(value)
    }
}
