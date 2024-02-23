use std::time::Duration;

use libmpv::{Mpv};

use log::{debug, error};
use rand::seq::SliceRandom;
use rand::thread_rng;
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::client::interface::{
    Answer, GetRequest, Playback, PlayerAction, PlayerInfo, PlaylistInfo, Repeat, Request,
    SeekMode, SongInfo, Volume,
};

pub struct Player {
    player: Mpv,
    stopped: bool,
}

pub struct State {
    pub duration: Duration,
    pub time_pos: Duration,
    pub volume: i64,
    pub playpause: Playback,
    pub eof: bool,
}

impl Player {
    pub fn new() -> Self {
        let player = Mpv::new().unwrap();
        player.set_property("video", false).unwrap();
        player.set_property("ytdl", true).unwrap();
        Self {
            player,
            stopped: true,
        }
    }

    pub fn get_state(&self) -> State {
        let duration: i64 = self.player.get_property("duration").unwrap_or_default();
        let duration = Duration::from_secs(duration as u64);
        let time_pos: i64 = self.player.get_property("time-pos").unwrap_or_default();
        let time_pos = Duration::from_secs(time_pos as u64);
        let volume = self.player.get_property("volume").unwrap_or_default();
        let eof: bool = self.player.get_property("eof-reached").unwrap_or_default()
            || self.player.get_property("idle-active").unwrap_or_default();
        let playback_status = self.get_playback_status();
        State {
            duration,
            time_pos,
            volume,
            playpause: playback_status,
            eof,
        }
    }

    pub fn get_playback_status(&self) -> Playback {
        if self.is_stopped() {
            Playback::Stop
        } else if self.paused() {
            Playback::Pause
        } else {
            Playback::Play
        }
    }

    pub fn paused(&self) -> bool {
        self.player.get_property("pause").unwrap_or(true)
    }

    pub fn playpause(&self) {
        if self.paused() {
            let _ = self.player.unpause();
        } else {
            let _ = self.player.pause();
        }
    }

    pub fn play(&mut self, url: &str) {
        // It is necessary to surround the url with quotes to avoid errors
        match self.player.command("loadfile", &[&format!("\"{url}\"")]) {
            Ok(_) => self.stopped = false,
            Err(e) => error!("error loading file {:?}", e),
        };
    }

    pub fn get_volume(&self) -> i64 {
        self.player.get_property("volume").unwrap_or(100)
    }

    pub fn incr_volume(&self, dv: i64) {
        let volume = self.get_volume();
        let volume = std::cmp::min(volume + dv, 100);
        let volume = std::cmp::max(volume, 0);
        let _ = self.player.set_property("volume", volume);
    }

    pub fn stop(&mut self) {
        self.player
            .command("stop", &[])
            .unwrap_or_else(|_| error!("Failed to stop"));
        self.stopped = true;
    }

    pub const fn is_stopped(&self) -> bool {
        self.stopped
    }

    pub fn seek_relative(&self, dt: i32) {
        self.player.seek_forward(f64::from(dt)).unwrap_or(()); // silent failure
    }

    pub fn seek_percent(&self, percent: usize) {
        // seek_percent_absolute is the same as seek_percent
        // because of a typo in the lib
        // self.player.seek_percent_absolute(pct).unwrap();
        self.player
            .command("seek", &[&format!("{percent}"), "absolute-percent"])
            .unwrap_or(());
    }
    fn seek_absolute(&self, dt: i64) {
        self.player
            .command("seek", &[&format!("{dt}"), "absolute"])
            .unwrap_or(());
    }

    pub fn set_repeat(&self, repeat: Repeat) {
        match repeat {
            Repeat::Off => {
                let _ = self.player.set_property("loop-playlist", "no");
                let _ = self.player.set_property("loop-file", "no");
            }
            Repeat::Song => {
                let _ = self.player.set_property("loop-playlist", "no");
                let _ = self.player.set_property("loop-file", "inf");
            }
            Repeat::Playlist => {
                let _ = self.player.set_property("loop-playlist", "inf");
                let _ = self.player.set_property("loop-file", "no");
            }
        }
    }
}

pub struct PlaylistHandler {
    /// list of songs
    playlist: Option<PlaylistInfo>,
    /// order in which to play the songs
    indices: Option<Vec<usize>>,
    /// index in `indices` of the current song if one is playing
    current: Option<usize>,
}

impl PlaylistHandler {
    pub fn new() -> Self {
        Self {
            playlist: None,
            indices: None,
            current: None,
        }
    }
    pub fn is_some(&self) -> bool {
        self.playlist.is_some()
    }
    pub fn set_playlist(&mut self, playlist: PlaylistInfo) {
        if playlist.songs.is_empty() {
            return;
        }
        self.indices = Some((0..playlist.songs.len()).collect());
        self.playlist = Some(playlist);
        self.current = Some(0);
    }
    pub fn shuffle(&mut self) {
        if self.indices.is_some() {
            self.indices.as_mut().unwrap().shuffle(&mut thread_rng())
        }
    }
    pub fn unshuffle(&mut self) {
        if let Some(playlist) = &self.playlist {
            self.indices = Some((0..playlist.songs.len()).collect());
        }
    }
    pub fn next(&mut self) {
        if let Some(indices) = &self.indices {
            if let Some(current) = self.current {
                self.current = Some((current + 1).min(indices.len() - 1));
            }
        }
    }
    pub fn prev(&mut self) {
        if self.indices.is_some() {
            if let Some(current) = self.current {
                if let Some(val) = current.checked_sub(1) {
                    self.current = Some(val)
                }
            }
        }
    }
    /// return `true` if the playlist is on the last element
    /// return `false` if `self.songs` is `None`
    pub fn is_at_end(&self) -> bool {
        match (self.current, &self.playlist) {
            (Some(current), Some(playlist)) => current == playlist.songs.len() - 1,
            _ => false,
        }
    }

    fn current_song(&self) -> Option<SongInfo> {
        match (&self.playlist, &self.indices, self.current) {
            (Some(playlist), Some(indices), Some(current)) => {
                Some(playlist.songs[indices[current]].clone())
            }
            _ => None,
        }
    }

    fn get_current(&self) -> Option<usize> {
        match (&self.current, &self.indices) {
            (Some(current), Some(indices)) => Some(indices[*current]),
            _ => None,
        }
    }
}

pub struct PlayerHandler {
    player: Player,
    request_rx: Receiver<Request>,
    answer_tx: Sender<Answer>,
    playlist: PlaylistHandler,
    current_track: Option<SongInfo>,
    shuffle: bool,
    autoplay: bool,
    repeat: Repeat,
    cancel_token: CancellationToken,
}

impl PlayerHandler {
    pub fn new(
        request_rx: Receiver<Request>,
        answer_tx: Sender<Answer>,
        cancel_token: CancellationToken,
    ) -> Self {
        let player = Player::new();
        Self {
            player,
            request_rx,
            answer_tx,
            playlist: PlaylistHandler::new(),
            current_track: None,
            shuffle: false,
            autoplay: false,
            repeat: Repeat::Off,
            cancel_token,
        }
    }

    pub async fn main_loop(&mut self) {
        let mut update_interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            let update_delay = update_interval.tick();
            tokio::select! {
                _ = self.cancel_token.cancelled() => break,
                _ = update_delay => self.update(),
                maybe_request = self.request_rx.recv() => {
                    use tokio::sync::broadcast::error as error;
                    match maybe_request {
                        Ok(request) => self.handle_request(request).await,
                        Err(error::RecvError::Closed) => break,
                        Err(error::RecvError::Lagged(_)) => {
                            // resubscribe to the channel
                            // dropping all unread messages
                            self.request_rx = self.request_rx.resubscribe()
                        }
                    }
                }
            }
        }
    }
    fn update(&mut self) {
        let state = self.player.get_state();
        if state.playpause != Playback::Play {
            return;
        }
        if self.autoplay && self.playlist.current_song().is_some() && state.eof {
            // go to next song if current one is finished
            self.weak_next()
        }
    }

    async fn handle_request(&mut self, request: Request) {
        match request {
            Request::PlayerAction(action) => {
                self.handle_action(action);
                self.send_info().await
            }
            Request::Get(GetRequest::PlayerInfo) => self.send_info().await,
            _ => (),
        }
    }
    /// send back the player state through [`Self::answer_tx`]
    /// if the channel is closed, cancel [`Self::cancel_token`]
    async fn send_info(&mut self) {
        let state = self.player.get_state();
        let song_info = if let Some(song) = self.playlist.current_song() {
            Some(song)
        } else {
            self.current_track.clone()
        };
        let info = PlayerInfo {
            playback: state.playpause,
            song_info,
            tracklist: self.playlist.playlist.clone().unwrap_or_default(),
            track_index: self.playlist.get_current(),
            shuffled: self.shuffle,
            autoplay: self.autoplay,
            repeat: self.repeat,
            volume: state.volume as u8,
            position: state.time_pos,
            can_seek: true,
        };
        if self.answer_tx.send(Answer::PlayerInfo(info)).await.is_err() {
            self.cancel_token.cancel();
        }
    }

    /// handle action received by the handler
    /// and send back information on completion
    fn handle_action(&mut self, action: PlayerAction) {
        match action {
            PlayerAction::PlayPause(target) => {
                if target != self.player.paused() {
                    self.player.playpause();
                }
            }
            PlayerAction::PlayPauseToggle => self.player.playpause(),
            PlayerAction::Stop => self.player.stop(),
            PlayerAction::Shuffle(target) => self.shuffle(target),
            PlayerAction::ShuffleToggle => self.shuffle_toggle(),
            PlayerAction::Autoplay(target) => self.autoplay(target),
            PlayerAction::AutoplayToggle => self.autoplay_toggle(),
            PlayerAction::Seek { dt, mode } => self.seek(dt, mode),
            PlayerAction::Prev => self.strong_prev(),
            PlayerAction::Next => self.strong_next(),
            PlayerAction::SetVolume(volume) => self.set_volume(volume),
            PlayerAction::SetTrackList(tracks) => {
                debug!("Setting track list");
                self.playlist.set_playlist(tracks)
            }
            PlayerAction::SetRepeat(repeat) => self.set_repeat(repeat),
            PlayerAction::CycleRepeat => self.cycle_repeat(),
        }
    }
    fn shuffle(&mut self, target: bool) {
        if target {
            self.playlist.shuffle();
        } else {
            self.playlist.unshuffle();
        }
        self.shuffle = target;
    }
    fn shuffle_toggle(&mut self) {
        self.shuffle(!self.shuffle)
    }

    fn autoplay(&mut self, target: bool) {
        if self.playlist.is_some() {
            self.autoplay = target;
            if target {
                self.play_playlist();
            }
        } else {
            self.autoplay = false;
        }
    }
    fn autoplay_toggle(&mut self) {
        self.autoplay(!self.autoplay)
    }
    /// goes to next track in playlist
    /// ignoring [Self::repeat] setting
    fn strong_next(&mut self) {
        self.playlist.next();
        self.play_playlist();
    }
    /// goes to prev track in playlist
    /// ignoring [Self::repeat] setting
    fn strong_prev(&mut self) {
        let state = self.player.get_state();
        if state.time_pos <= Duration::from_secs(5) {
            // if at the beginning of the song go to previous one
            self.playlist.prev();
            self.play_playlist();
        } else {
            // otherwise go to start of current song
            self.seek(0, SeekMode::Absolute);
        }
    }
    fn play_playlist(&mut self) {
        if let Some(song) = self.playlist.current_song() {
            self.player.play(&song.url);
            debug!("Playing {}", song.url);
        }
    }

    fn seek(&self, dt: i64, mode: SeekMode) {
        match mode {
            SeekMode::Absolute => self.player.seek_absolute(dt),
            SeekMode::Relative => self.player.seek_relative(dt as i32),
            SeekMode::AbsolutePercent => self.player.seek_percent(dt as usize),
            SeekMode::RelativePercent => todo!(),
        }
    }

    fn set_volume(&self, volume: Volume) {
        match volume {
            Volume::Absolute(target) => {
                let dv: i64 = (target as i64) - self.player.get_volume();
                self.player.incr_volume(dv)
            }
            Volume::Relative(dv) => self.player.incr_volume(dv as i64),
        }
    }

    fn set_repeat(&mut self, repeat: Repeat) {
        self.repeat = repeat;
        self.player.set_repeat(repeat);
    }

    fn cycle_repeat(&mut self) {
        match self.repeat {
            Repeat::Off => self.set_repeat(Repeat::Playlist),
            Repeat::Playlist => self.set_repeat(Repeat::Song),
            Repeat::Song => self.set_repeat(Repeat::Off),
        }
    }

    /// goes to next track in playlist
    /// respecting [`Self::repeat`]
    fn weak_next(&mut self) {
        if self.repeat != Repeat::Song {
            self.playlist.next();
        }
        if self.repeat == Repeat::Playlist && self.playlist.is_at_end() {
            //return to begin of playlist
            self.playlist.current = Some(0)
        }
        self.play_playlist();
    }
}
