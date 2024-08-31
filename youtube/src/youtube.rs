use std::{collections::HashMap, sync::Arc};

use google_youtube3::{
    api::{self, Playlist, PlaylistItem, PlaylistItemListResponse, PlaylistListResponse},
    hyper, hyper_rustls,
    oauth2::{self, authenticator_delegate::InstalledFlowDelegate},
    YouTube,
};

use protocol::{
    playlist::{FullPlaylist, PlaylistId, Song, SongId},
    Action, NotificationId, UICommand,
};
use tokio::sync::{mpsc::Sender, Mutex};

type Result<T> = protocol::Result<T>;

#[derive(Debug)]
struct YtSong(pub(crate) Song);

#[derive(Debug, Clone)]
pub struct YtPlaylist {
    pub(crate) playlist: protocol::playlist::FullPlaylist,
    pub(crate) fully_loaded: bool,
    pub(crate) loading: bool,
}
impl YtPlaylist {
    pub fn id(&self) -> &PlaylistId {
        self.playlist.id()
    }
}
const MAX_RESULT: u32 = 50;
pub struct Source {
    pub(crate) hub: YouTube<HubType>,
    ui_channel: Sender<Action>,
    // playlists: HashMap<PlaylistId, YtPlaylist>,
    pub(crate) connection_notification_id: Arc<Mutex<Option<NotificationId>>>,
}
struct Authenticator {
    out_channel: Sender<Action>,
    connection_notification_id: Arc<Mutex<Option<NotificationId>>>,
}

// type HubType = hyper::Client<hyper_rustls::HttpsConnector<hyper::client::HttpConnector>>;
pub type HubType = hyper_rustls::HttpsConnector<hyper::client::HttpConnector>;

impl Source {
    pub async fn new(ui_channel: Sender<Action>) -> Self {
        let connection_notif_id = Arc::new(Mutex::new(None));
        let secret =
            oauth2::read_application_secret("/home/sofamaniac/.config/yamav3/yt_secrets.json")
                .await
                .expect("Could not read secrets");
        let auth = oauth2::InstalledFlowAuthenticator::builder(
            secret,
            oauth2::InstalledFlowReturnMethod::HTTPRedirect,
        )
        .flow_delegate(Box::new(Authenticator {
            out_channel: ui_channel.clone(),
            connection_notification_id: connection_notif_id.clone(),
        }))
        .build()
        .await
        .unwrap();
        let hub = YouTube::new(
            hyper::Client::builder().build(
                hyper_rustls::HttpsConnectorBuilder::new()
                    .with_native_roots()
                    .unwrap()
                    .https_or_http()
                    .enable_http1()
                    .build(),
            ),
            auth,
        );
        Self {
            hub,
            ui_channel,
            // playlists: HashMap::new(),
            connection_notification_id: connection_notif_id,
        }
    }
    /* pub async fn get_all_playlists(&mut self) -> Vec<protocol::playlist::Playlist> {
        if self.playlists.is_empty() {
            let (_, playlists) = self
                .hub
                .playlists()
                .list(&vec![String::from("snippet")])
                .max_results(MAX_RESULT)
                .mine(true)
                .doit()
                .await
                .unwrap();
            // TODO handle pagination
            let items = playlists.items.unwrap_or_default();
            let playlists = items.into_iter().map(Into::<YtPlaylist>::into);
            for playlist in playlists {
                if !self.playlists.contains_key(playlist.id()) {
                    self.playlists.insert(playlist.id().clone(), playlist);
                }
            }
            self.get_likes().await;
        }
        self.playlists
            .values()
            .map(|playlist| playlist.playlist.playlist().clone())
            .collect()
    }
    pub async fn get_likes(&mut self) {
        let (_, playlists) = self
            .hub
            .playlists()
            .list(&vec![String::from("snippet")])
            .max_results(MAX_RESULT)
            .add_id("LL")
            .doit()
            .await
            .unwrap();
        let items = playlists.items.unwrap_or_default();
        let playlists = items.into_iter().map(Into::<YtPlaylist>::into);
        for playlist in playlists {
            if !self.playlists.contains_key(playlist.id()) {
                self.playlists.insert(playlist.id().clone(), playlist);
            }
        }
    }

    pub async fn get_playlist(
        &mut self,
        id: &PlaylistId,
    ) -> Option<protocol::playlist::FullPlaylist> {
        let playlist = self.playlists.get(id)?;
        if !playlist.fully_loaded {
            self.load_playlist(id).await;
        }
        Some(self.playlists.get(id).unwrap().playlist.clone())
    }

    pub async fn load_playlistpage(
        &mut self,
        id: &PlaylistId,
        page_token: Option<String>,
    ) -> Option<String> {
        let (_, songs) = self
            .hub
            .playlist_items()
            .list(&vec![
                String::from("snippet"),
                String::from("contentDetails"),
            ])
            .playlist_id(&id.to_string())
            .max_results(MAX_RESULT)
            .page_token(&page_token.unwrap_or_default())
            .doit()
            .await
            .unwrap();
        let yt_playlist = self.playlists.get_mut(id).unwrap();
        for song in songs.items.unwrap_or_default().into_iter() {
            let song: YtSong = song.into();
            yt_playlist.playlist.add_song(song.into());
        }
        songs.next_page_token
    }

    pub async fn load_playlist(&mut self, id: &PlaylistId) {
        let mut page_token: Option<String> = None;
        while let Some(new_token) = self.load_playlistpage(id, page_token).await {
            page_token = Some(new_token)
        }
        self.playlists.get_mut(id).unwrap().fully_loaded = true;
    } */
}

impl From<api::Playlist> for YtPlaylist {
    fn from(playlist: Playlist) -> Self {
        let snippet = playlist.snippet.unwrap();
        let playlist = protocol::playlist::Playlist::new(
            PlaylistId::new(playlist.id.unwrap()),
            snippet.title.unwrap(),
        );
        let playlist = FullPlaylist::new(playlist);
        Self {
            playlist,
            fully_loaded: false,
            loading: false,
        }
    }
}

impl From<PlaylistItem> for YtSong {
    fn from(song: PlaylistItem) -> Self {
        let snippet = song.snippet.unwrap_or_default();
        let details = song.content_details.unwrap_or_default();
        let song_id = SongId::new(details.video_id.unwrap_or_default());
        let artist = snippet.channel_title.unwrap_or_default();
        let url = format!("https://youtube.com/watch?v={song_id}");
        let mut song = Song::new(song_id, url, snippet.title.unwrap().clone());
        song.add_artist(artist);
        YtSong(song)
    }
}

impl From<YtSong> for Song {
    fn from(value: YtSong) -> Self {
        value.0
    }
}

impl InstalledFlowDelegate for Authenticator {
    fn present_user_url<'a>(
        &'a self,
        url: &'a str,
        need_code: bool,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = std::result::Result<String, String>> + Send + 'a>,
    > {
        Box::pin(self.make_url(url, need_code))
    }
}

impl Authenticator {
    async fn make_url<'a>(
        &'a self,
        url: &'a str,
        need_code: bool,
    ) -> std::result::Result<String, String> {
        // If we are already trying to connect do nothing
        if self.connection_notification_id.lock().await.is_some() {
            return Ok(String::new());
        }
        // First we try to open the url in a browser
        let action = UICommand::open_url(url.to_string());
        let res = action.send(&self.out_channel).await;
        if let Ok(notif_id) = res.recv().await {
            let mut lock = self.connection_notification_id.lock().await;
            *lock = Some(notif_id);
            return Ok(String::new());
        };
        // Ask the user to open the url in a browser
        let message = format!("Please go to {url} and follow the instructions");
        let action = UICommand::inform_user(message);
        let res = action.send(&self.out_channel).await;
        if let Ok(notif_id) = res.recv().await {
            let mut lock = self.connection_notification_id.lock().await;
            *lock = Some(notif_id);
            return Ok(String::new());
        };
        if need_code {
            let message = format!("Please open {url} and copy back the code");
            let action = UICommand::prompt_user(message);
            let res = action.send(&self.out_channel).await;
            // TODO: Handle code and send it back
            return Ok(String::new());
        }
        Ok(String::new())
    }
}
pub async fn get_all_playlists(hub: &YouTube<HubType>) -> Vec<YtPlaylist> {
    let (_, playlists) = hub
        .playlists()
        .list(&vec![String::from("snippet")])
        .max_results(MAX_RESULT)
        .mine(true)
        .doit()
        .await
        .unwrap();
    // TODO handle pagination
    let items = playlists.items.unwrap_or_default();
    // TODO get likes
    items.into_iter().map(Into::<YtPlaylist>::into).collect()
}
pub async fn get_playlist(
    hub: YouTube<HubType>,
    playlist: protocol::playlist::Playlist,
) -> Option<FullPlaylist> {
    let mut page_token: Option<String> = None;
    let id = playlist.id().clone();
    let mut full_playlist = FullPlaylist::new(playlist);
    while let Some(new_token) = load_playlistpage(&hub, &id, &mut full_playlist, page_token).await {
        page_token = Some(new_token)
    }
    Some(full_playlist)
}

async fn load_playlistpage(
    hub: &YouTube<HubType>,
    id: &PlaylistId,
    playlist: &mut FullPlaylist,
    page_token: Option<String>,
) -> Option<String> {
    let (_, songs) = hub
        .playlist_items()
        .list(&vec![
            String::from("snippet"),
            String::from("contentDetails"),
        ])
        .playlist_id(&id.to_string())
        .max_results(MAX_RESULT)
        .page_token(&page_token.unwrap_or_default())
        .doit()
        .await
        .unwrap();
    for song in songs.items.unwrap_or_default().into_iter() {
        let song: YtSong = song.into();
        playlist.add_song(song.into());
    }
    songs.next_page_token
}
