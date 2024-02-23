use core::fmt::{self, Display};
use std::{fs::File, io::BufReader, path::PathBuf, time::Duration};

use anyhow::Result;
use futures::StreamExt;
use google_youtube3::chrono::TimeDelta;
use log::{debug, error, warn};
use rspotify::{
    clients::{pagination::Paginator, BaseClient, OAuthClient},
    model::{
        CurrentPlaybackContext, CurrentUserQueue, Device, FullTrack, PlayableItem, PlaylistId,
        PlaylistItem, RepeatState, SimplifiedPlaylist,
    },
    scopes, AuthCodeSpotify, ClientResult, Credentials, OAuth,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast::Receiver, mpsc::Sender, oneshot};
use tokio_util::sync::CancellationToken;

use crate::{
    client::interface::{
        Answer, GetRequest, Playback, PlayerAction, PlayerInfo, PlaylistInfo, Repeat, Request,
        SeekMode, SongInfo, Volume, Widget,
    },
    config,
};

#[derive(Debug, Clone)]
pub struct Error;

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Error in spotify backend")
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }

    fn description(&self) -> &str {
        "description() is deprecated; use Display"
    }

    fn cause(&self) -> Option<&dyn std::error::Error> {
        self.source()
    }
}

pub struct Playlist<'a> {
    id: PlaylistId<'a>,
    songs: Vec<SongInfo>,
    title: String,
    cover_url: String,
    length: usize,
}

impl<'a> Playlist<'a> {
    pub fn new(playlist: SimplifiedPlaylist) -> Self {
        let cover_url = if let Some(cover) = playlist.images.first() {
            cover.url.clone()
        } else {
            String::new()
        };
        Self {
            id: playlist.id,
            songs: Vec::new(),
            title: playlist.name,
            cover_url,
            length: playlist.tracks.total as usize,
        }
    }
    pub fn get_songs(&self) -> Vec<SongInfo> {
        self.songs.clone()
    }
    pub async fn load<'b>(&mut self, mut pages: Paginator<'b, ClientResult<PlaylistItem>>) {
        if !self.songs.is_empty() {
            // already loaded
            return;
        }
        let mut songs: Vec<SongInfo> = Vec::new();
        while let Some(page) = pages.next().await {
            if let Ok(item) = page {
                if let Some(track) = item.track {
                    match track {
                        rspotify::model::PlayableItem::Track(track) => {
                            if track.id.is_none() {
                                continue;
                            };
                            songs.push(track.into())
                        }
                        rspotify::model::PlayableItem::Episode(_) => todo!(),
                    }
                }
            }
        }
        self.songs = songs;
    }
    pub fn get_info(&self) -> PlaylistInfo {
        PlaylistInfo {
            title: self.title.clone(),
            length: self.length,
            cover_url: self.cover_url.clone(),
            id: self.id.to_string(),
            songs: self.get_songs(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Creds {
    pub id: String,
    pub secret: String,
}

pub struct Backend<'a> {
    request_rx: Receiver<Request>,
    answer_tx: Sender<Answer>,
    cancel_token: CancellationToken,
    spotify: AuthCodeSpotify,
    playlists: Vec<Playlist<'a>>,
    shuffled: bool,
    autoplay: bool,
    last_info: PlayerInfo,
    device: Option<Device>,
}

impl<'a> Backend<'a> {
    pub async fn init(
        request_rx: Receiver<Request>,
        answer_tx: Sender<Answer>,
        cancel_token: CancellationToken,
    ) -> Result<Self> {
        let file = File::open(config::get_config().spotify_secret_location).unwrap();
        let reader = BufReader::new(file);
        let creds: Creds = serde_json::from_reader(reader).unwrap();
        let creds = Credentials::new(&creds.id, &creds.secret);
        let dirs = config::get_dirs();
        let cache = dirs.cache_dir();
        let mut cache = PathBuf::from(cache);
        cache.push("spotify_token_cache.json");
        let config = rspotify::Config {
            cache_path: cache,
            token_cached: true,
            token_refreshing: true,
            ..Default::default()
        };

        let oauth = OAuth {
            redirect_uri: "http://localhost:8888/callback".to_string(),
            scopes: scopes!("user-read-recently-played"),
            ..Default::default()
        };

        let spotify = AuthCodeSpotify::with_config(creds, oauth, config);
        if let Ok(Some(token)) = spotify.read_token_cache(true).await {
            // this is stupid, read_token_cache does not update the token
            *spotify.get_token().lock().await.unwrap() = Some(token)
        }
        Ok(Self {
            request_rx,
            answer_tx,
            cancel_token,
            spotify,
            playlists: Vec::new(),
            shuffled: false,
            autoplay: false,
            last_info: PlayerInfo::default(),
            device: None,
        })
    }

    pub async fn main_loop(&mut self) {
        // Obtaining the access token
        // self.reconnect().await;
        self.check_connection().await;
        let connection_check_duration = Duration::from_secs(5);
        let mut connection_check_delay = tokio::time::interval(connection_check_duration);
        loop {
            let connection_check = connection_check_delay.tick();
            tokio::select! {
                // _ = connection_check => self.check_connection().await,
                _ = connection_check => self.check_connection().await,
                _ = self.cancel_token.cancelled() => break,
                request = self.request_rx.recv() => {
                    use tokio::sync::broadcast::error as error;
                    match request {
                        Ok(command) => self.handle_request(command).await,
                        Err(err) => match err {
                            error::RecvError::Closed => self.cancel_token.cancel(),
                            error::RecvError::Lagged(_) => {
                                // resubscribe to broadcast ignoring all messages
                                // pending
                                self.request_rx = self.request_rx.resubscribe()
                            }
                        }
                    }
                },
            };
        }
    }
    async fn reconnect(&self) {
        log::info!("[Spotify] Reconnecting");
        let url = self.spotify.get_authorize_url(false).unwrap();
        log::debug!("{url}");
        if let Err(err) = open::that(url.clone()) {
            warn!("Could not open browser: {err}");
        }
        let (sender, recv) = oneshot::channel();
        let msg = format!("Go to {url}, and paste back the resulting url");
        if let Err(err) = self
            .answer_tx
            .send(
                Widget::PromptBox {
                    title: "Connect to Spotify".to_string(),
                    content: msg,
                    backchannel: sender,
                }
                .into(),
            )
            .await
        {
            debug!("Error while sending auth url: {err}");
        }
        if let Ok(code) = recv.await {
            if let Some(code) = self.spotify.parse_response_code(&code) {
                if let Err(err) = self.spotify.request_token(&code).await {
                    error!("Request token failed {err}");
                }
                if let Err(err) = self.spotify.write_token_cache().await {
                    error!("Writing to cache failed {err}");
                }
            }
        }
    }
    async fn check_connection(&self) {
        debug!("[Spotify] Checking connection");
        if (self.spotify.auto_reauth().await).is_err() {
            self.reconnect().await
        }
    }
    pub async fn handle_request<'b>(&'b mut self, request: Request) {
        debug!("[Spotify] Handling request {:?}", request);
        match request {
            Request::PlayerAction(action) => self.handle_player(action).await,
            Request::Get(get) => self.handle_get(get).await,
            Request::Set(_) => todo!(),
            Request::Command(command) => self.handle_command(command).await,
        }
    }

    async fn handle_get<'b>(&'b mut self, get: GetRequest) {
        match get {
            GetRequest::PlaylistList => {
                if self.playlists.is_empty() {
                    self.get_playlists().await;
                }
                let _ = self
                    .answer_tx
                    .send(Answer::PlaylistList(
                        self.playlists.iter().map(|p| p.get_info()).collect(),
                    ))
                    .await;
            }
            GetRequest::Playlist(id) => {
                let playlist = self
                    .playlists
                    .iter()
                    .find(|p| p.id.to_string() == id)
                    .unwrap();
                let _ = self
                    .answer_tx
                    .send(Answer::Playlist(playlist.get_info()))
                    .await;
            }
            GetRequest::PlayerInfo => {
                let info = self.player_info().await;
                let _ = self.answer_tx.send(Answer::PlayerInfo(info)).await;
            }
        }
    }

    async fn get_playlists<'b>(&'b mut self) {
        log::debug!("trying to get playlists");
        let mut pages = self.spotify.current_user_playlists();
        log::debug!("got playlist");
        while let Some(page) = pages.next().await {
            if let Ok(playlist) = page {
                self.playlists.push(Playlist::new(playlist));
            }
        }
        for playlist in self.playlists.iter_mut() {
            let pages = self.spotify.playlist_items(playlist.id.clone(), None, None);
            playlist.load(pages).await;
        }
    }
    async fn get_devices(&self) -> Vec<Device> {
        debug!("[Spotify] Getting devices");
        self.spotify.device().await.unwrap_or_default()
    }
    fn get_device_id(&self) -> Option<String> {
        self.device.as_ref().map(|d| d.id.clone().unwrap_or_default())
    }

    async fn prev(&self) {
        debug!("[Spotify] Playing previous track");
        let _ = self.spotify.previous_track(self.get_device_id().as_deref()).await;
    }
    async fn next(&self) {
        debug!("[Spotify] Playing next track");
        let _ = self.spotify.next_track(self.get_device_id().as_deref()).await;
    }
    async fn pause(&self) {
        debug!("[Spotify] pausing");
        let _ = self.spotify.pause_playback(self.get_device_id().as_deref()).await;
    }
    async fn shuffle(&mut self, target: bool) {
        debug!("[Spotify] shuffling");
        let _ = self.spotify.shuffle(target, self.get_device_id().as_deref()).await;
        self.shuffled = target;
    }
    async fn set_repeat(&self, repeat: Repeat) {
        debug!("[Spotify] setting repeat state");
        let _ = self.spotify.repeat(repeat.into(), self.get_device_id().as_deref()).await;
    }
    async fn playpause_toggle(&self) {
        debug!("[Spotify] playpause");
        if self.last_info.playback == Playback::Play {
            self.pause().await;
        } else {
        let _ = self.spotify.resume_playback(self.get_device_id().as_deref(), None).await;
        }
    }
    async fn player_info(&mut self) -> PlayerInfo {
        let context = self.get_playback_state().await;
        if context.is_none() {
            debug!("[Spotify] no playback context");
            return self.last_info.clone();
        };
        let context = context.unwrap();
        debug!("[Spotify] getting queue");
        let queue = self.spotify.current_user_queue().await.expect("No queue");
        self.last_info = PlayerInfo {
            playback: if context.is_playing {
                Playback::Play
            } else {
                Playback::Pause
            },
            song_info: context.item.map(|track| track.into()),
            tracklist: queue.into(),
            track_index: Some(0),
            shuffled: self.shuffled,
            autoplay: context.is_playing,
            repeat: context.repeat_state.into(),
            volume: context.device.volume_percent.unwrap_or_default() as u8,
            position: context
                .progress
                .unwrap_or_default()
                .to_std()
                .unwrap_or_default(),
            can_seek: true,
        };
        debug!("[Spotify] Sending info");
        self.last_info.clone()
    }

    async fn handle_player(&mut self, action: PlayerAction) {
        match action {
            PlayerAction::PlayPause(target) => self.playpause(target).await,
            PlayerAction::PlayPauseToggle => self.playpause_toggle().await,
            PlayerAction::Stop => self.pause().await,
            PlayerAction::Shuffle(target) => self.shuffle(target).await,
            PlayerAction::ShuffleToggle => self.shuffle(!self.shuffled).await,
            PlayerAction::Autoplay(target) => self.autoplay(target).await,
            PlayerAction::AutoplayToggle => self.autoplay(!self.autoplay).await,
            PlayerAction::Seek { dt, mode } => self.seek(dt, mode).await,
            PlayerAction::Prev => self.prev().await,
            PlayerAction::Next => self.next().await,
            PlayerAction::SetVolume(volume) => self.set_volume(volume).await,
            PlayerAction::SetTrackList(tracklist) => self.set_tracklist(tracklist).await,
            PlayerAction::SetRepeat(repeat) => self.set_repeat(repeat).await,
            PlayerAction::CycleRepeat => self.cycle_repeat().await,
        }
    }

    async fn set_tracklist(&self, tracklist: PlaylistInfo) {
        let playlist = self
            .playlists
            .iter()
            .find(|p| p.id.to_string() == tracklist.id)
            .unwrap();
        let _ = self
            .spotify
            .start_context_playback(
                rspotify::prelude::PlayContextId::Playlist(playlist.id.clone()),
                None,
                None,
                Some(TimeDelta::zero()),
            )
            .await;
    }

    async fn playpause(&self, target: bool) {
        if target {
            let _ = self.spotify.resume_playback(self.get_device_id().as_deref(), None).await;
        } else {
            self.pause().await;
        }
    }

    async fn autoplay(&mut self, target: bool) {
        self.playpause(target).await;
        self.autoplay = target;
    }

    async fn cycle_repeat(&self) {
        if let Ok(Some(playback)) = self
            .spotify
            .current_playback(None, None as Option<Vec<_>>)
            .await
        {
            match playback.repeat_state {
                RepeatState::Off => self.set_repeat(Repeat::Song).await,
                RepeatState::Track => self.set_repeat(Repeat::Playlist).await,
                RepeatState::Context => self.set_repeat(Repeat::Off).await,
            }
        }
    }

    async fn get_playback_state(&self) -> Option<CurrentPlaybackContext> {
        self.spotify
            .current_playback(None, None as Option<Vec<_>>)
            .await
            .unwrap_or_default()
    }

    async fn set_volume(&self, volume: Volume) {
        match volume {
            Volume::Absolute(target) => {
                let _ = self.spotify.volume(target as u8, self.get_device_id().as_deref()).await;
            }
            Volume::Relative(delta) => {
                let volume = self.get_volume().await;
                let _ = self
                    .spotify
                    .volume(
                        volume.checked_add_signed(delta as i32).unwrap_or_default() as u8,
                        self.get_device_id().as_deref(),
                    )
                    .await;
            }
        }
    }

    async fn get_volume(&self) -> u32 {
        if let Some(context) = self.get_playback_state().await {
            context.device.volume_percent.unwrap_or_default()
        } else {
            0
        }
    }

    async fn seek(&self, dt: i64, mode: SeekMode) {
        let progress = self
            .get_playback_state()
            .await
            .map(|ctxt| ctxt.progress.unwrap_or_default())
            .unwrap_or_default()
            .to_std()
            .unwrap_or_default();
        let length: Duration = self
            .get_playback_state()
            .await
            .map(|ctxt| {
                ctxt.item.map(|i| {
                    if let PlayableItem::Track(track) = i {
                        track.duration.to_std().unwrap_or_default()
                    } else {
                        Duration::default()
                    }
                })
            })
            .unwrap_or_default()
            .unwrap_or_default();
        let target = match mode {
            SeekMode::Absolute => Duration::from_secs(dt as u64),
            SeekMode::Relative => {
                let len = length.as_secs().checked_add_signed(dt).unwrap_or_default();
                Duration::from_secs(len)
            }
            SeekMode::AbsolutePercent => {
                let target = length.as_secs() * (dt as u64) / 100;
                Duration::from_secs(target)
            }
            SeekMode::RelativePercent => {
                let target = progress.as_secs() + length.as_secs() * (dt as u64) / 100;
                Duration::from_secs(target)
            }
        };
        let _ = self
            .spotify
            .seek_track(TimeDelta::from_std(target).unwrap_or_default(), self.get_device_id().as_deref())
            .await;
    }

    async fn handle_command(&mut self, command: String) {
        if command == "devices list" {
            let devices = self.get_devices().await;
            let devices: Vec<String> = devices
                .into_iter()
                .map(|device| device.name.to_owned())
                .collect();
            let devices = devices.join("\n");
            let _ = self.answer_tx
                .send(
                    Widget::Alert {
                        title: "Spotify devices".to_string(),
                        content: devices,
                    }
                    .into(),
                )
                .await;
        }
        if command.starts_with("devices select") {
            let parts = command.split_whitespace();
            if parts.clone().count() != 3 {
                return;
            }
            self.device = self.find_device_by_name(parts.last().unwrap()).await;
        }   
    }

    async fn find_device_by_name(&self, name: &str) -> Option<Device> {
        let devices = self.get_devices().await;
        devices.into_iter().find(|d| d.name == name)
    }
}

impl From<Repeat> for RepeatState {
    fn from(value: Repeat) -> Self {
        match value {
            Repeat::Off => RepeatState::Off,
            Repeat::Playlist => RepeatState::Context,
            Repeat::Song => RepeatState::Track,
        }
    }
}
impl From<RepeatState> for Repeat {
    fn from(value: RepeatState) -> Self {
        match value {
            RepeatState::Off => Repeat::Off,
            RepeatState::Track => Repeat::Song,
            RepeatState::Context => Repeat::Playlist,
        }
    }
}

impl From<FullTrack> for SongInfo {
    fn from(track: FullTrack) -> Self {
        if track.id.is_none() {
            return SongInfo::default();
        };
        let cover_url = if let Some(cover) = track.album.images.first() {
            cover.url.clone()
        } else {
            String::new()
        };
        SongInfo {
            title: track.name,
            artist: track.artists.iter().map(|a| a.name.clone()).collect(),
            cover_url,
            id: track.id.unwrap().to_string(),
            url: track.href.unwrap_or_default(),
            duration: track.duration.to_std().unwrap_or_default(),
        }
    }
}

impl From<CurrentUserQueue> for PlaylistInfo {
    fn from(value: CurrentUserQueue) -> Self {
        Self {
            title: String::new(),
            length: value.queue.len(),
            cover_url: String::new(),
            id: String::new(),
            songs: value.queue.into_iter().map(|item| item.into()).collect(),
        }
    }
}

impl From<PlayableItem> for SongInfo {
    fn from(value: PlayableItem) -> Self {
        match value {
            PlayableItem::Track(track) => track.into(),
            // TODO implement episode
            PlayableItem::Episode(_) => SongInfo::default(),
        }
    }
}
