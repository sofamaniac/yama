use std::sync::Arc;

use libmpv::Mpv;
use protocol::{
    playback::{PlayerStatus, Volume, VolumeSetter},
    Duration, PlayerInfo, Repeat, Song,
};
use rand::{seq::SliceRandom, thread_rng};

pub(crate) struct Player {
    player: Mpv,
    // original song position, and song
    queue: Vec<(usize, Song)>,
    queue_arc: Arc<[Song]>,
    current_track: Option<Song>,
    // current position in queue
    current_index: Option<usize>,
    repeat: Repeat,
    autoplay: bool,
    shuffled: bool,
}

impl Player {
    pub fn new() -> Self {
        let player = Mpv::new().unwrap();
        let _ = player.set_property("video", false);
        let _ = player.set_property("ytdl", true);
        let _ = player.set_property("ytdl-raw-options", "cookies-from-browser=firefox");
        Self {
            player,
            queue: Vec::new(),
            current_index: None,
            current_track: None,
            repeat: Repeat::Off,
            autoplay: false,
            shuffled: false,
            queue_arc: Default::default(),
        }
    }
    pub fn update(&mut self) {
        if self.autoplay {
            let eof = self.player.get_property("eof-reached").unwrap_or_default()
                || self.player.get_property("idle-active").unwrap_or_default();
            if eof {
                self.next()
            }
        }
    }
    fn get_time_pos(&self) -> Duration {
        let pos: f64 = self.player.get_property("time-pos").unwrap_or_default();
        Duration::try_from_secs_f64(pos).unwrap_or_default()
    }
    fn play_current_index(&mut self) {
        if let Some(index) = self.current_index {
            if let Some((_, song)) = self.queue.get(index) {
                self.current_track = Some(song.clone());
                let _ = self.player.command("loadfile", &[song.url()]);
            }
        }
    }

    pub fn set_playlist(&mut self, playlist: Arc<[Song]>) {
        self.queue_arc = playlist.clone();
        self.queue = Vec::with_capacity(playlist.len());
        for e in playlist.iter().cloned().enumerate() {
            self.queue.push(e)
        }
    }

    pub fn shuffle(&mut self, shuffle: bool) {
        if shuffle {
            let mut rng = thread_rng();
            self.queue.shuffle(&mut rng)
        } else {
            // Sort back to original order
            self.queue.sort_by(|s1, s2| s1.0.cmp(&s2.0))
        }
        self.shuffled = shuffle;
    }
    pub fn next(&mut self) {
        if let Some(index) = self.current_index {
            let new_index = index + 1;
            if new_index >= self.queue.len() {
                self.current_index = match self.repeat {
                    Repeat::Playlist => Some(0),
                    Repeat::Song => None,
                    Repeat::Off => None,
                }
            } else {
                self.current_index = Some(new_index)
            }
        }
        self.current_track = None;
        self.play_current_index();
    }

    pub fn previous(&mut self) {
        let info = self.info();
        if let PlayerInfo {
            status: PlayerStatus::Playing { song: _, position },
            ..
        } = info
        {
            if std::convert::Into::<std::time::Duration>::into(position)
                > std::time::Duration::from_secs(5)
            {
                self.seek(protocol::playback::SeekMode::Absolute(Duration::default()));
                return;
            }
        }
        self.current_index = self.current_index.map(|index| index.saturating_sub(1));
        self.play_current_index();
    }

    pub fn pause(&self) {
        let _ = self.player.pause();
    }
    pub fn unpause(&self) {
        let _ = self.player.unpause();
    }
    pub fn playpause(&self) {
        let _ = self.player.cycle_property("pause", true);
    }
    #[allow(dead_code)]
    pub fn stop(&self) {
        todo!()
    }
    pub fn set_repeat(&mut self, repeat: Repeat) {
        if match repeat {
            Repeat::Off => self.player.set_property("loop", "no"),
            Repeat::Song => self.player.set_property("loop", "inf"),
            Repeat::Playlist => Ok(()),
        }
        .is_ok()
        {
            self.repeat = repeat
        }
    }
    pub fn set_volume(&mut self, volume_setter: VolumeSetter) {
        let volume = self.get_volume();
        let _ = match volume_setter {
            VolumeSetter::Absolute(volume) => {
                self.player.set_property("volume", volume.to_u8() as i64)
            }
            VolumeSetter::Relative(delta) => self
                .player
                .set_property("volume", volume.add_delta(delta).to_u8() as i64),
        };
    }
    pub fn get_volume(&self) -> Volume {
        let val: i64 = self.player.get_property("volume").unwrap_or_default();
        Volume::new(val as u8)
    }
    #[allow(dead_code)]
    pub fn play_song(&mut self, _song: Song) {
        todo!()
    }
    pub fn set_autoplay(&mut self, autoplay: bool) {
        self.autoplay = autoplay;
        if autoplay {
            self.current_index = Some(0);
            self.play_current_index()
        }
    }

    pub(crate) fn info(&self) -> PlayerInfo {
        let status = match &self.current_track {
            Some(song) => PlayerStatus::Playing {
                song: song.clone(),
                position: self.get_time_pos(),
            },
            None => PlayerStatus::Stopped,
        };
        PlayerInfo {
            status,
            paused: self.get_pause(),
            autoplay: self.autoplay,
            shuffled: self.shuffled,
            repeat: self.repeat,
            volume: self.get_volume(),
            queue: self.queue_arc.clone(),
        }
    }

    pub(crate) fn seek(&self, mode: protocol::playback::SeekMode) {
        match mode {
            protocol::playback::SeekMode::Absolute(dur) => {
                let duration: protocol::Duration = dur;
                let seconds = duration.as_secs_f64();
                let _ = self.player.seek_absolute(seconds);
            }
            protocol::playback::SeekMode::Forward(dur) => {
                let duration: protocol::Duration = dur;
                let seconds = duration.as_secs_f64();
                let _ = self.player.seek_forward(seconds);
            }
            protocol::playback::SeekMode::Backward(dur) => {
                let duration: protocol::Duration = dur;
                let seconds = duration.as_secs_f64();
                let _ = self.player.seek_backward(seconds);
            }
            protocol::playback::SeekMode::Percent(percent) => {
                let _ = self.player.seek_percent_absolute(percent as usize);
            }
        }
    }

    fn get_pause(&self) -> bool {
        self.player.get_property("pause").unwrap_or_default()
    }
}
