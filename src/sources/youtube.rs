use std::{sync::Arc, time::Duration};

use super::{ClientTrait, Playlist, PlaylistTrait, Song};
use crate::config;
use async_trait::async_trait;
use color_eyre::Result;
use futures::stream::StreamExt;
use google_youtube3::{
    api,
    hyper::{self, client::HttpConnector},
    hyper_rustls::{self, HttpsConnector},
    oauth2, YouTube,
};
use log::*;
use tokio::sync::Mutex;

const MAX_RESULT: u32 = 50;

#[derive(Clone, Default, Debug)]
pub struct YtSong {
    song: Song,
    id: String,
}

#[derive(Clone, Default)]
pub struct YtPlaylist {
    title: String,
    entries: Vec<YtSong>,
    id: String,
    next_page: Option<String>,
    hub: Option<Arc<Mutex<YouTube<HttpsConnector<HttpConnector>>>>>,
    is_loading: bool,
}

impl std::fmt::Debug for YtPlaylist {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "YtPlaylist {{")?;
        writeln!(f, "\ttitle: {:?}", self.title)?;
        writeln!(f, "\tentries: {:?}", self.entries)?;
        writeln!(f, "\tid: {:?}", self.id)?;
        writeln!(f, "}}")?;
        Ok(())
    }
}

impl YtPlaylist {
    async fn load_page(&mut self) {
        let hub = self.hub.as_ref().unwrap().lock().await;
        let request = hub
            .playlist_items()
            .list(&vec!["snippet".to_string(), "contentDetails".to_string()])
            .playlist_id(&self.id)
            .max_results(MAX_RESULT)
            .page_token(&self.next_page.as_ref().unwrap().to_string());
        let result = request.doit().await.unwrap_or_default();
        let (_, result) = result;
        let items = result.items.unwrap_or_default();
        let songs: Vec<YtSong> = items.into_iter().flat_map(song_from_item).collect();
        for s in songs {
            self.entries.push(s);
        };
        self.next_page = result.next_page_token;
    }
    const fn is_loaded(&self) -> bool {
        self.next_page.is_none()
    }
}

#[async_trait]
impl PlaylistTrait for YtPlaylist {
    fn get_title(&self) -> String {
        self.title.clone()
    }

    fn get_number_entries(&self) -> usize {
        self.entries.len()
    }

    fn get_entries(&self) -> Vec<Song> {
        self.entries.iter().map(|s| s.song.clone()).collect()
    }

    fn is_loading(&self) -> bool {
        self.is_loading
    }

    async fn add_entry(&mut self) -> Result<()> {
        todo!()
    }

    async fn rm_entry(&mut self) -> Result<()> {
        todo!()
    }

    async fn load(&mut self) -> Result<Vec<Song>> {
        self.is_loading = true;
        if self.is_loaded() {
            self.is_loading = false;
            return Ok(self.get_entries());
        };
        loop {
            debug!("Loading Page {}", self.title);
            match &self.next_page {
                Some(_) => self.load_page().await,
                None => {
                    break;
                }
            }
        }
        self.is_loading = false;
        Ok(self.get_entries())
    }

    fn download_song(&self, index: usize) -> Result<()> {
        todo!()
    }

    fn get_url_song(&self, index: usize) -> Result<String> {
        todo!()
    }
}

#[derive(Default, Clone)]
pub struct Client {
    pub hub: Option<Arc<Mutex<YouTube<HttpsConnector<HttpConnector>>>>>,
    playlists: Arc<Mutex<Vec<YtPlaylist>>>,
    all_loaded: bool,
}

impl Client {
    pub fn new() -> Self {
        Self::default()
    }
    pub async fn fetch_all_playlists(&mut self) {
        let mut liked_videos = self.load_playlist_by_id("LL").await;
        liked_videos.title = "Liked Videos".into();
        let mut playlists_list = self.load_all_playlists_mine().await;
        playlists_list.push(liked_videos);
        self.playlists = Arc::new(Mutex::new(playlists_list));
    }

    async fn load_all_playlists_mine(&self) -> Vec<YtPlaylist> {
        let hub = self.hub.as_ref().unwrap().lock().await;
        // TODO: does not work if more than MAX_RESULT playlists
        let request = hub
            .playlists()
            .list(&vec!["snippet".to_string(), "contentDetails".to_string()])
            .mine(true)
            .max_results(MAX_RESULT);
        let result = request.doit().await.unwrap_or_default();
        let (_, result) = result;
        convert_playlist_list(result, &self.hub)
    }

    async fn load_playlist_by_id(&self, id: &str) -> YtPlaylist {
        if let Some(p) = self.playlists.lock().await.iter().find(|p| *p.id == *id) {
            p.clone()
        } else {
            let hub = self.hub.as_ref().unwrap().lock().await;
            let request = hub
                .playlists()
                .list(&vec!["snippet".to_string(), "contentDetails".to_string()])
                .add_id(id)
                .max_results(MAX_RESULT);
            let result = request.doit().await.unwrap_or_default();
            let (_, result) = result;

            convert_playlist_list(result, &self.hub)
                .into_iter()
                .next() // get the first item
                .unwrap_or_default()
        }
    }
}


#[async_trait]
impl ClientTrait for Client {
    async fn connect(&mut self) -> Result<()> {
        // TODO properly display url on screen

        // Get an ApplicationSecret instance by some means. It contains the `client_id` and
        // `client_secret`, among other things.
        let secrets_location = config::get().secrets_location;
        // TODO create valide Path
        let credentials_path = format!("{secrets_location}/youtube_credentials.json");
        let token_path = format!("{secrets_location}/youtube_tokencache.json");
        let secret = oauth2::read_application_secret(credentials_path).await;
        let secret = match secret {
            Err(e) => {
                error!("Cannot find credentials for youtube client : {}", e);
                return Err(e.into());
            }
            Ok(secret) => secret,
        };
        // Instantiate the authenticator. It will choose a suitable authentication flow for you,
        // unless you replace  `None` with the desired Flow.
        // Provide your own `AuthenticatorDelegate` to adjust the way it operates and get feedback about
        // what's going on. You probably want to bring in your own `TokenStorage` to persist tokens and
        // retrieve them from storage.
        let auth = oauth2::InstalledFlowAuthenticator::builder(
            secret,
            oauth2::InstalledFlowReturnMethod::HTTPRedirect,
        )
        .persist_tokens_to_disk(token_path)
        .build()
        .await
        .unwrap();
        // NOTE: flow will not be initiated until a request is made
        let hub = YouTube::new(
            hyper::Client::builder().build(
                hyper_rustls::HttpsConnectorBuilder::new()
                    .with_native_roots()
                    .https_or_http()
                    .enable_http1()
                    .enable_http2()
                    .build(),
            ),
            auth,
        );
        // Force flow to start
        let _ = hub.videos().list(&vec!["gubergren".into()]).doit().await;
        self.hub = Some(Arc::new(Mutex::new(hub)));
        Ok(())
    }

    async fn load_playlists(&mut self) -> Result<Vec<Playlist>> {
        if !self.all_loaded {
            self.fetch_all_playlists().await;
            futures::stream::iter(self.playlists.lock().await.iter_mut())
                .for_each_concurrent(10, |p| async { let _ = p.load().await; })
                .await;
            self.all_loaded = true;
        }
        Ok(self
            .playlists
            .lock()
            .await
            .iter()
            .map(|p| p.clone().into())
            .collect())
    }

    async fn get_playlists(&self) -> Vec<Playlist> {
        // self.load_playlists().await;
        self.playlists
            .lock()
            .await
            .iter()
            .map(|p| p.clone().into())
            .collect()
    }

    fn is_connected(&self) -> bool {
        todo!()
    }
}
fn convert_playlist_list(
    result: api::PlaylistListResponse,
    hub: &Option<Arc<Mutex<YouTube<HttpsConnector<HttpConnector>>>>>,
) -> Vec<YtPlaylist> {
    let items = result.items.unwrap_or_default();
    items.into_iter().map(|p| convert_playlist(p, hub.clone())).collect()
}
fn convert_playlist(
    playlist: api::Playlist,
    hub: Option<Arc<Mutex<YouTube<HttpsConnector<HttpConnector>>>>>,
) -> YtPlaylist {
    let snippet = playlist.snippet.unwrap_or_default();
    let content = playlist.content_details.unwrap_or_default();
    let title = snippet.title.unwrap_or_default();
    let size = content.item_count.unwrap_or_default();
    let id = playlist.id.unwrap_or_default();
    YtPlaylist {
        title,
        id,
        entries: Vec::with_capacity(size.try_into().unwrap()),
        hub,
        next_page: Some(String::new()),
        is_loading: false,
    }
}
fn song_from_item(item: api::PlaylistItem) -> Option<YtSong> {
    let details = item.snippet.unwrap_or_default();
    let title = details.title.unwrap_or_default();
    let id = details
        .resource_id
        .unwrap_or_default()
        .video_id
        .unwrap_or_default();
    let artists = details.video_owner_channel_title.as_ref().map_or("", |artist| artist);
    if artists.is_empty() {
        None
    } else {
        Some(YtSong {
            song: Song {
                title,
                artists: vec![artists.into()],
                duration: Duration::default(),
            },
            id
        })
    }
}
