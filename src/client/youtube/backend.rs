use tokio::sync::broadcast::Receiver as BroadReceiver;
use tokio::sync::mpsc::{Receiver as MpscReceiver, Sender as MpscSender};
extern crate google_youtube3 as youtube3;
use anyhow::Result;
use google_youtube3::hyper::client::HttpConnector;
use google_youtube3::hyper_rustls::HttpsConnector;
use google_youtube3::oauth2::authenticator_delegate::InstalledFlowDelegate;
use log::{debug, error};
use std::collections::{HashMap, VecDeque};
use std::default::Default;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use youtube3::api::{Playlist as YtPlaylist, PlaylistItemListResponse, Video};
use youtube3::api::{PlaylistItem, PlaylistListResponse};
use youtube3::{hyper, hyper_rustls, oauth2, YouTube};

use crate::{client::interface::{Answer, GetRequest, PlaylistInfo, Request, SongInfo, Widget}, config};

type Hub = YouTube<HttpsConnector<HttpConnector>>;
const MAX_RESULT: u32 = 50;

#[derive(Debug, Clone)]
struct Song {
    artist: String,
    title: String,
    id: String,
    art_url: String,
    duration: Duration,
}

impl Song {
    pub fn new(song: PlaylistItem) -> Self {
        let snippet = song.clone().snippet.unwrap_or_default();
        let content_details = song.clone().content_details.unwrap_or_default();
        let title = snippet.clone().title.unwrap_or_default();
        let id = content_details.video_id.unwrap_or_default();
        let artist = snippet
            .clone()
            .video_owner_channel_title
            .unwrap_or_default();
        let art_url = snippet
            .thumbnails
            .unwrap_or_default()
            .default
            .unwrap_or_default()
            .url
            .unwrap_or_default();
        Song {
            title,
            id,
            art_url,
            artist,
            duration: Default::default(),
        }
    }
    pub fn info(&self) -> SongInfo {
        SongInfo {
            title: self.title.clone(),
            artist: self.artist.clone(),
            cover_url: self.art_url.clone(),
            id: self.id.clone(),
            url: format!("https://youtu.be/{}", self.id),
            duration: self.duration,
        }
    }
}
impl From<Song> for SongInfo {
    fn from(val: Song) -> Self {
        val.info()
    }
}

#[derive(Debug, Clone)]
struct Playlist {
    title: String,
    id: String,
    length: usize,
    songs: Vec<Song>,
    art_url: String,
    /// None means the playlist if fully loaded, initialized to empty string
    next_page_token: Option<String>,
    /// Index in the playlists list
    index: usize,
}

impl Playlist {
    pub fn new(playlist: YtPlaylist, index: Option<usize>) -> Self {
        let snippet = playlist.clone().snippet.unwrap_or_default();
        let details = playlist.clone().content_details.unwrap_or_default();
        let title = snippet.title.unwrap_or_default();
        let length = details.item_count.unwrap_or_default() as usize;
        let id = playlist.clone().id.unwrap_or_default();
        let art_url = snippet
            .thumbnails
            .unwrap_or_default()
            .default
            .unwrap_or_default()
            .url
            .unwrap_or_default();
        Self {
            title,
            id,
            length,
            songs: Default::default(),
            art_url,
            next_page_token: Some(String::new()),
            index: index.unwrap_or_default(),
        }
    }
    pub fn id(&self) -> String {
        self.id.clone()
    }
    pub fn vec_songs_info(&self) -> Vec<SongInfo> {
        self.songs.iter().map(|s| s.info()).collect()
    }
    pub fn info(&self) -> PlaylistInfo {
        PlaylistInfo {
            title: self.title.clone(),
            id: self.id.clone(),
            length: self.length,
            cover_url: self.art_url.clone(),
            songs: self.vec_songs_info(),
        }
    }
    async fn add_songs(&mut self, songs: &PlaylistItemListResponse, hub: &Hub) {
        let songs_items = songs.clone().items.unwrap_or_default();
        let songs: Vec<Song> = songs_items.iter().map(|s| Song::new(s.clone())).collect();
        let songs: Vec<Song> = self.filter(&songs, hub).await;
        for s in songs {
            self.songs.push(s);
        }
    }
    async fn load_page(&mut self, hub: &Hub) {
        if self.is_loaded() {
            // fully loaded
            return;
        };
        let next_page = self.next_page_token.as_ref().unwrap();
        let request = hub
            .playlist_items()
            .list(&vec!["snippet".to_string(), "contentDetails".to_string()])
            .playlist_id(&self.id)
            .max_results(MAX_RESULT)
            .page_token(next_page);
        let (_, result) = request.doit().await.unwrap_or_default();
        self.next_page_token = result.next_page_token.clone();
        self.add_songs(&result, hub).await;
        if self.next_page_token.is_none() {
            self.length = self.songs.len();
        }
    }

    async fn load_all(&mut self, hub: &Hub, tasks: MpscSender<Task>) {
        self.load_page(hub).await;
        if !self.is_loaded() {
            // ignore failure to send task
            let _ = tasks
                .send(Task::Playlist(self.id(), ActionPlaylist::LoadAll))
                .await;
        }
    }

    fn is_loaded(&self) -> bool {
        // returns true if fully loaded
        self.next_page_token.is_none()
    }

    async fn handle_task(&mut self, task: ActionPlaylist, hub: &Hub, tasks: MpscSender<Task>) {
        match task {
            ActionPlaylist::LoadAll => self.load_all(hub, tasks).await,
            ActionPlaylist::LoadPage => todo!(),
        }
    }

    async fn filter(&self, songs: &[Song], hub: &Hub) -> Vec<Song> {
        let ids: Vec<String> = songs.iter().map(|s| s.id.clone()).collect();
        let request = hub
            .videos()
            .list(&vec![
                "snippet".to_string(),
                "contentDetails".to_string(),
                "status".to_string(),
            ])
            .max_results(MAX_RESULT);
        let request = ids.iter().fold(request, |r, s| r.add_id(s));
        let (_, result) = request.doit().await.unwrap_or_default();
        let videos: Vec<Video> = result.items.unwrap_or_default();
        let videos: Vec<&Video> = videos
            .iter()
            .filter(|&v| check_video_available(v))
            .collect();
        let ids_video: Vec<String> = videos
            .iter()
            .map(|v| v.id.clone().unwrap_or_default())
            .collect();
        let songs: Vec<&Song> = songs.iter().filter(|s| ids_video.contains(&s.id)).collect();
        let songs: Vec<Song> = songs
            .to_owned()
            .iter()
            .map(|&s| {
                let song: Song = s.clone();
                let video = videos
                    .iter()
                    .find(|v| v.id.clone().unwrap_or_default() == s.id)
                    .unwrap();
                let duration = video
                    .content_details
                    .clone()
                    .unwrap_or_default()
                    .duration
                    .unwrap_or_default();
                let duration = duration.parse::<iso8601_duration::Duration>().unwrap();
                let duration = duration.to_std().unwrap_or_default();
                Song { duration, ..song }
            })
            .collect();
        songs
    }
}

impl From<Playlist> for PlaylistInfo {
    fn from(val: Playlist) -> Self {
        val.info()
    }
}

#[derive(Debug, Clone)]
enum ActionPlaylist {
    LoadAll,
    LoadPage, // load *one* page only
}
#[derive(Debug, Clone)]
enum ActionPlaylistList {
    FetchPage(String),
    FetchAll,
}

#[derive(Debug)]
enum Task {
    PlaylistList(ActionPlaylistList),
    Playlist(String, ActionPlaylist),
    Command(Request),
}

pub struct Backend {
    receiver: BroadReceiver<Request>,
    sender: MpscSender<Answer>,
    playlists: HashMap<String, Playlist>,
    hub: Hub,
    all_playlist_fetched: bool,
    cancel_token: CancellationToken,
    tasks: VecDeque<Task>,
    task_receiver: MpscReceiver<Task>,
    task_sender: MpscSender<Task>,
}

impl Backend {
    pub async fn init(
        receiver: BroadReceiver<Request>,
        sender: MpscSender<Answer>,
        cancel_token: CancellationToken,
    ) -> Result<Self> {
        let hub = Self::create_hub(sender.clone()).await?;
        let (task_sender, task_receiver) = tokio::sync::mpsc::channel(50);
        let client = Backend {
            receiver,
            sender,
            cancel_token,
            playlists: Default::default(),
            hub,
            all_playlist_fetched: false,
            tasks: Default::default(),
            task_sender,
            task_receiver,
        };
        Ok(client)
    }

    async fn fetch_all_playlists(&mut self) {
        if self.all_playlist_fetched {
            // ignore if already fetched
            return;
        };
        self.fetch_liked_playlist().await;
        // TODO: load multiple pages
        let request = self
            .hub
            .playlists()
            .list(&vec!["snippet".to_string(), "contentDetails".to_string()])
            .mine(true)
            .max_results(MAX_RESULT);
        let (_, result) = request.doit().await.unwrap();
        self.set_playlists(result);
        self.all_playlist_fetched = true;
    }
    async fn fetch_liked_playlist(&mut self) {
        let request = self
            .hub
            .playlists()
            .list(&vec!["snippet".to_string(), "contentDetails".to_string()])
            .add_id("LL")
            .max_results(MAX_RESULT);
        let (_, result) = request.doit().await.unwrap();
        let results = result.items.unwrap_or_default();
        if !results.is_empty() {
            let playlist = Playlist::new(results[0].clone(), Some(0));
            self.playlists.insert(playlist.id.clone(), playlist);
        }
    }
    fn set_playlists(&mut self, playlists: PlaylistListResponse) {
        let playlists = playlists.items.unwrap_or_default();
        // index 0 is reserved for Liked videos
        let mut index = 1;
        for playlist in playlists {
            let playlist = Playlist::new(playlist, Some(index));
            self.playlists.insert(playlist.id(), playlist);
            index += 1;
        }
    }

    pub async fn main_loop(&mut self) {
        let delay = Duration::from_millis(100);
        let mut interval = tokio::time::interval(delay);
        while !self.cancel_token.is_cancelled() {
            if let Some(task) = self.tasks.pop_front() {
                self.handle_task(task).await;
            }
            use tokio::sync::broadcast::error;
            match self.receiver.try_recv() {
                Ok(command) => self.tasks.push_back(Task::Command(command)),
                Err(err) => match err {
                    error::TryRecvError::Empty => (),
                    error::TryRecvError::Closed => self.cancel_token.cancel(),
                    error::TryRecvError::Lagged(_) => {
                        // resubscribe to broadcast ignoring all messages
                        // pending
                        self.receiver = self.receiver.resubscribe()
                    }
                },
            }
            if let Ok(task) = self.task_receiver.try_recv() {
                self.tasks.push_back(task);
            }
            interval.tick().await;
        }
    }
    async fn handle_command(&mut self, request: Request) {
        match request {
            Request::PlayerAction(_) => (),
            Request::Get(request) => self.handle_get(request).await,
            Request::Set(_) => todo!(),
            Request::Command(_) => (),
        }
    }
    async fn send_playlistlist(&mut self) {
        self.fetch_all_playlists().await;
        let mut playlistlist: Vec<&Playlist> = vec![];
        for (_, p) in self.playlists.iter() {
            playlistlist.push(p)
        }
        playlistlist.sort_unstable_by_key(|playlist| playlist.index);
        let playlistlist = playlistlist.iter().map(|p| p.info()).collect();
        self.send(Answer::PlaylistList(playlistlist)).await;
    }
    pub async fn load_all_playlists(&mut self) {
        self.fetch_all_playlists().await;
        for (_, p) in self.playlists.iter() {
            self.tasks
                .push_back(Task::Playlist(p.id.clone(), ActionPlaylist::LoadAll));
        }
    }
    async fn send_playlist(&mut self, id: String) {
        self.fetch_all_playlists().await; //ensure all playlist are loaded
        if let Some(p) = self.playlists.get(&id) {
            self.tasks
                .push_back(Task::Playlist(id, ActionPlaylist::LoadAll));
            self.send(Answer::Playlist(p.info())).await;
        }
    }
    async fn handle_get(&mut self, request: GetRequest) {
        match request {
            GetRequest::PlaylistList => self.send_playlistlist().await,
            GetRequest::Playlist(id) => self.send_playlist(id).await,
            GetRequest::PlayerInfo => (),
        }
    }

    async fn send(&mut self, answer: Answer) {
        if self.sender.send(answer).await.is_err() {
            self.cancel_token.cancel()
        }
    }

    async fn handle_task(&mut self, task: Task) {
        match task {
            Task::PlaylistList(_) => todo!(),
            Task::Playlist(id, task) => {
                if let Some(playlist) = self.playlists.get_mut(&id) {
                    playlist
                        .handle_task(task, &self.hub, self.task_sender.clone())
                        .await
                }
            }
            Task::Command(command) => self.handle_command(command).await,
        }
    }

    async fn create_hub(sender: MpscSender<Answer>) -> Result<Hub> {
        // Get an ApplicationSecret instance by some means. It contains the `client_id` and
        // `client_secret`, among other things.
        // TODO: set own configuration
        let secrets_location = config::get_config().yt_secret_location;
        let secret_path = PathBuf::from(secrets_location);
        let secret = oauth2::read_application_secret(secret_path).await;
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
        let dirs = config::get_dirs();
        let mut cache = dirs.cache_dir().to_path_buf();
        cache.push("youtube_token_cache.json");
        let auth = oauth2::InstalledFlowAuthenticator::builder(
            secret,
            oauth2::InstalledFlowReturnMethod::HTTPRedirect,
        )
        .persist_tokens_to_disk(cache)
        .flow_delegate(Box::new(CustomFlowDelegate::new(sender)))
        .build()
        .await
        .unwrap();

        Ok(YouTube::new(
            hyper::Client::builder().build(
                hyper_rustls::HttpsConnectorBuilder::new()
                    .with_native_roots()
                    .https_or_http()
                    .enable_http1()
                    .enable_http2()
                    .build(),
            ),
            auth,
        ))
    }
}

struct CustomFlowDelegate {
    out: MpscSender<Answer>,
}

impl CustomFlowDelegate {
    // requires that out implement at least Clone
    pub fn new(out: MpscSender<Answer>) -> Self {
        CustomFlowDelegate { out }
    }
}

impl InstalledFlowDelegate for CustomFlowDelegate {
    /// Configure a custom redirect uri if needed.
    fn redirect_uri(&self) -> Option<&str> {
        None
    }

    /// We need the user to navigate to a URL using their browser and potentially paste back a code
    /// (or maybe not). Whether they have to enter a code depends on the InstalledFlowReturnMethod
    /// used.
    fn present_user_url<'a>(
        &'a self,
        url: &'a str,
        need_code: bool,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
        Box::pin(present_user_url(url, need_code, self.out.clone()))
    }
}

async fn present_user_url(
    url: &str,
    need_code: bool,
    out: MpscSender<Answer>,
) -> Result<String, String> {
    debug!("[Youtube] Initiating flow");
    // try to open url in browser
    let _ = out
        .send(
            Widget::Alert {
                title: "Connect to Youtube".to_string(),
                content: "Go to your browser to authenticate".to_string(),
            }
            .into(),
        )
        .await;
    if open::that(url).is_ok() {
        return Ok(String::new());
    };
    debug!("Could not open in browser");
    // if it didn't work, pass url to app
    let url_message = format!(
        "Please direct your browser to {} and follow the instructions displayed \
             there.",
        url
    );
    let message: &str = if need_code {
        "Inputting code to authenticate not supported"
    } else {
        &url_message
    };
    if let Err(e) = out
        .send(
            Widget::Alert {
                title: "Youtube authentication".to_string(),
                content: message.to_string(),
            }
            .into(),
        )
        .await
    {
        debug!("Error while sending message to app {}", e);
        Err(e.to_string())
    } else {
        Ok(String::new())
    }
}

fn check_video_available(video: &Video) -> bool {
    let content_details = video.content_details.clone().unwrap_or_default();
    let region_restriction = content_details.region_restriction.unwrap_or_default();
    let status = video.status.clone().unwrap_or_default();
    let mut available = true;
    /* if let Some(allowed) = region_restriction.allowed {
        available = allowed.contains(&"fr".to_string());
    } */
    if let Some(blocked) = region_restriction.blocked {
        available = available && !blocked.contains(&"fr".to_string());
    }
    if let Some(privacy) = status.privacy_status {
        available = available && privacy != *"private";
    }
    available
}
