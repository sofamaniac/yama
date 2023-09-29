use std::fmt::Display;
use std::time::Duration;
use async_trait::async_trait;

use color_eyre::Result;
use enum_dispatch::enum_dispatch;

pub mod youtube;
use youtube::Client as YtClient;
use youtube::YtPlaylist;

pub mod local;
use local::Client as LocalClient;

#[derive(Clone, Default, Debug)]
pub struct Song {
    pub title: String,
    pub artists: Vec<String>,
    pub duration: Duration,
}

#[async_trait]
#[enum_dispatch(Playlist)]
pub trait PlaylistTrait: Send {

    fn get_title(&self) -> String;
    fn get_number_entries(&self) -> usize;
    fn get_entries(&self) -> Vec<Song>;
    async fn add_entry(&mut self) -> Result<()>;
    async fn rm_entry(&mut self) -> Result<()>;
    async fn load(&mut self) -> Result<Vec<Song>>;
    fn download_song(&self, index: usize) -> Result<()>;
    fn get_url_song(&self, index: usize) -> Result<String>;
    fn is_loading(&self) -> bool;

}

#[enum_dispatch]
pub enum Playlist {
    YtPlaylist
}

#[async_trait]
#[enum_dispatch(Client)]
pub trait ClientTrait {

    async fn connect(&mut self) -> Result<()>;
    async fn load_playlists(&mut self) -> Result<Vec<Playlist>>;
    async fn get_playlists(&self) -> Vec<Playlist>;
    fn is_connected(&self) -> bool;

}



#[enum_dispatch]
pub enum Client {
    YtClient,
    LocalClient,
}


impl Display for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Client::YtClient(_) => write!(f, "Youtube"),
            Client::LocalClient(_) => write!(f, "Local files"),
        }
    }
}
