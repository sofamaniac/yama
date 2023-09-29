use std::{fmt::Display, sync::Arc};

use libmpv::{FileState, Mpv};
use zbus::SignalContext;

use log::*;

use crate::dbus::PlayerInterface;
use crate::ui::State as Route;

#[derive(Copy, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Repeat {
    Off,
    Song,
    Playlist,
}

impl Display for Repeat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match &self {
            Repeat::Off => "no",
            Repeat::Song => "song",
            Repeat::Playlist => "playlist",
        };
        write!(f, "{}", text)
    }
}

#[derive(Copy, Debug, Clone, PartialEq, Eq)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}
impl Display for PlaybackStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match &self {
            PlaybackStatus::Stopped => "Stopped",
            PlaybackStatus::Paused => "Paused",
            PlaybackStatus::Playing => "Playing",
        };
        write!(f, "{}", text)
    }
}

pub struct Player {
    player: Mpv,
    shuffled: bool,
    in_playlist: bool,
    stopped: bool,
    repeat: Repeat,
    pub route: Route,
}

pub struct State {
    pub duration: i64, // in secs
    pub time_pos: i64, // in secs
    pub volume: i64,
    pub title: Arc<str>,
    pub path: Arc<str>,
    pub repeat: Repeat,
    pub playpause: PlaybackStatus,
    pub shuffled: bool,
}

impl Player {
    pub fn new() -> Self {
        let player = Mpv::new().unwrap();
        player.set_property("video", false).unwrap();
        player.set_property("ytdl", true).unwrap();
        Player {
            player,
            shuffled: false,
            in_playlist: false,
            stopped: true,
            route: Default::default(),
            repeat: Repeat::Off,
        }
    }

    pub fn get_state(&self) -> State {
        let duration = self.player.get_property("duration").unwrap_or_default();
        let time_pos = self.player.get_property("time-pos").unwrap_or_default();
        let volume = self.player.get_property("volume").unwrap_or_default();
        let title: String = self.player.get_property("media-title").unwrap_or_default();
        let path: String = self.player.get_property("path").unwrap_or_default();
        let playback_status = self.get_playback_status();
        let shuffled = self.shuffled;
        State {
            duration,
            time_pos,
            volume,
            title: Arc::from(title),
            path: Arc::from(path),
            repeat: self.repeat,
            playpause: playback_status,
            shuffled,
        }
    }

    pub fn get_playback_status(&self) -> PlaybackStatus {
        if self.is_stopped() {
            PlaybackStatus::Stopped
        } else if self.paused() {
            PlaybackStatus::Paused
        } else {
            PlaybackStatus::Playing
        }
    }

    pub fn paused(&self) -> bool {
        self.player.get_property("pause").unwrap_or(true)
    }

    pub async fn playpause(&mut self) {
        if self.paused() {
            let _ = self.player.unpause();
        } else {
            let _ = self.player.pause();
        }
    }

    pub fn play(&mut self, url: &str, route: Route) {
        // It is necessary to surround the url with quotes to avoid errors
        match self.player.command("loadfile", &[&format!("\"{}\"", url)]) {
            Ok(_) => self.stopped = false,
            Err(e) => error!("error loading file {:?}", e),
        };
        self.route = route;
    }

    pub fn get_volume(&self) -> i64 {
        self.player.get_property("volume").unwrap_or(100)
    }

    pub fn get_repeat(&self) -> Repeat {
        self.repeat
    }

    pub fn incr_volume(&mut self, dv: i64) {
        let volume = self.get_volume();
        let volume = std::cmp::min(volume + dv, 100);
        let volume = std::cmp::max(volume, 0);
        let _ = self.player.set_property("volume", volume);
    }

    pub fn shuffle(&mut self) {
        if !self.in_playlist {
            return;
        }
        if self.shuffled {
            self.player
                .command("playlist-unshuffle", &[])
                .unwrap_or_else(|_| error!("Failed to unshuffle"));
        } else {
            self.player
                .command("playlist-shuffle", &[])
                .unwrap_or_else(|_| error!("Failed to shuffle"));
        }
        self.shuffled = !self.shuffled;
    }

    pub async fn set_auto(&mut self, playlist: &[&str], route: Route) {
        self.stop().await;
        self.player.playlist_clear().unwrap_or(()); // silently ignore failure
        if !self.in_playlist {
            let files: Vec<(&str, FileState, Option<_>)> = playlist
                .iter()
                .cloned()
                .map(|s| (s, FileState::AppendPlay, None))
                .collect();
            self.player.playlist_load_files(&files).unwrap_or(()); // silently ignore failure
            self.stopped = false;
        }
        self.in_playlist = !self.in_playlist;
        self.shuffled = false;
        self.route = route
    }

    pub fn next(&self) {
        self.player
            .playlist_next_weak()
            .unwrap_or_else(|_| error!("Failed to go next"));
    }

    pub fn prev(&self) {
        self.player
            .playlist_previous_weak()
            .unwrap_or_else(|_| error!("Failed to go previous"));
    }

    pub fn is_shuffled(&self) -> bool {
        self.shuffled
    }

    pub fn is_in_playlist(&self) -> bool {
        self.in_playlist
    }

    pub async fn stop(&mut self) {
        self.player
            .command("stop", &[])
            .unwrap_or_else(|_| error!("Failed to stop"));
        self.stopped = true;
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    pub fn seek_relative(&mut self, dt: i64) {
        self.player.seek_forward(dt as f64).unwrap_or(()); // silent failure
    }

    pub fn seek_percent(&mut self, percent: usize) {
        // seek_percent_absolute is the same as seek_percent
        // because of a typo in the lib
        // self.player.seek_percent_absolute(pct).unwrap();
        self.player
            .command("seek", &[&format!("{}", percent), "absolute-percent"])
            .unwrap_or(());
    }

    pub fn cycle_repeat(&mut self) {
        match self.repeat {
            Repeat::Off => {
                self.repeat = Repeat::Song;
                self.player.set_property("loop-file", "inf");
            }
            Repeat::Song => {
                self.repeat = Repeat::Playlist;
                self.player.set_property("loop-file", "no");
                self.player.set_property("loop-playlist", "inf");
            }
            Repeat::Playlist => {
                self.repeat = Repeat::Off;
                self.player.set_property("loop-playlist", "no");
            }
        }
    }
}
