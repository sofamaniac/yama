use std::{collections::HashMap, sync::Arc};

use google_youtube3::{
    api::{self, Playlist, PlaylistItem, PlaylistItemListResponse, PlaylistListResponse},
    hyper, hyper_rustls,
    oauth2::{self, authenticator_delegate::InstalledFlowDelegate},
    YouTube,
};

use protocol::{
    playlist::{FullPlaylist, PlaylistId, Song, SongId},
    Action, Duration, NotificationId, Receive, UICommand,
};
use tokio::sync::{mpsc::Sender, Mutex};

type Result<T> = protocol::Result<T>;

#[derive(Debug)]
struct YtSong(pub(crate) Song);
impl YtSong {
    pub fn new(song: PlaylistItem, duration: Duration, artist: String) -> Self {
        let snippet = song.snippet.unwrap_or_default();
        let details = song.content_details.unwrap_or_default();
        let song_id = SongId::new(details.video_id.unwrap_or_default());
        let url = format!("https://youtube.com/watch?v={song_id}");
        let song = Song::new(
            song_id,
            url,
            snippet.title.unwrap().clone(),
            duration,
            vec![artist],
        );
        YtSong(song)
    }
}

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
}

impl From<api::Playlist> for YtPlaylist {
    fn from(playlist: Playlist) -> Self {
        let snippet = playlist.snippet.unwrap();
        let playlist = protocol::playlist::Playlist::new(
            PlaylistId::new(playlist.id.unwrap()),
            snippet.title.unwrap(),
        );
        let playlist = FullPlaylist::new(playlist, Arc::new([]));
        Self {
            playlist,
            fully_loaded: false,
            loading: false,
        }
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
    let playlists_request = || {
        hub.playlists()
            .list(&vec![String::from("snippet")])
            .max_results(MAX_RESULT)
    };
    let (_, all_playlists) = playlists_request().mine(true).doit().await.unwrap();
    // TODO handle pagination
    let mut items = all_playlists.items.unwrap_or_default();
    let (_, likes) = playlists_request().add_id("LL").doit().await.unwrap();
    items.append(&mut likes.items.unwrap_or_default());
    items.into_iter().map(Into::<YtPlaylist>::into).collect()
}
pub async fn get_playlist(
    hub: YouTube<HubType>,
    playlist: protocol::playlist::Playlist,
) -> Option<FullPlaylist> {
    let mut page_token: Option<String> = None;
    let id = playlist.id().clone();
    let mut songs = Vec::new();
    while let (mut new_songs, Some(new_token)) = load_playlistpage(&hub, &id, page_token).await {
        songs.append(&mut new_songs);
        page_token = Some(new_token)
    }
    let songs: Vec<Song> = songs.into_iter().map(Into::into).collect();
    let full_playlist = FullPlaylist::new(playlist, songs.into());
    Some(full_playlist)
}

async fn load_playlistpage(
    hub: &YouTube<HubType>,
    id: &PlaylistId,
    page_token: Option<String>,
) -> (Vec<YtSong>, Option<String>) {
    let (_, body) = hub
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
    let songs = body.items.unwrap_or_default();
    let songs_id: Vec<String> = songs.iter().map(get_video_id).collect();
    let mut durations = get_video_length_and_artist(hub, &songs_id).await;
    let songs = songs
        .into_iter()
        .filter_map(|song| {
            let id = get_video_id(&song);
            let (duration, artist) = durations.remove(&id).unwrap_or_default();
            // Video without artists are not available
            if !artist.is_empty() {
                Some(YtSong::new(song, duration, artist))
            } else {
                None
            }
        })
        .collect();
    (songs, body.next_page_token)
}

fn get_video_id(item: &PlaylistItem) -> String {
    item.content_details
        .clone()
        .unwrap()
        .video_id
        .unwrap_or_default()
}

async fn get_video_length_and_artist(
    hub: &YouTube<HubType>,
    ids: &[String],
) -> HashMap<String, (Duration, String)> {
    let request = hub
        .videos()
        .list(&vec![
            String::from("contentDetails"),
            String::from("snippet"),
        ])
        .max_results(MAX_RESULT);
    let request = ids.iter().fold(request, |acc, id| acc.add_id(id));
    let (_, songs_details) = request.doit().await.unwrap();

    let mut res = HashMap::new();
    for song in songs_details.items.expect("items not found") {
        let details = song.content_details.expect("details not found");
        let duration = details.duration.expect("duration not found");
        let duration = protocol::iso8601::duration(&duration).expect("could not parse duration");
        let duration: std::time::Duration = duration.into();
        let artist = song
            .snippet
            .unwrap_or_default()
            .channel_title
            .unwrap_or_default();
        res.insert(song.id.unwrap(), (duration, artist));
    }
    res
}
