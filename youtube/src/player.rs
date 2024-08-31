use libmpv::Mpv;
use protocol::{
    playback::{Volume, VolumeSetter},
    Repeat, Song,
};
use rand::{seq::SliceRandom, thread_rng};

pub(crate) struct Player {
    player: Mpv,
    // original song position, and song
    queue: Vec<(usize, Song)>,
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
        player.set_property("video", false);
        player.set_property("ytdl", true);
        Self {
            player,
            queue: Vec::new(),
            current_index: None,
            current_track: None,
            repeat: Repeat::Off,
            autoplay: false,
            shuffled: false,
        }
    }
    pub fn update(&mut self) {
        let position: i64 = self.player.get_property("percent-pos").unwrap_or_default();
        if position >= 100 && self.autoplay {
            self.next()
        }
    }
    fn play_current_index(&self) {
        if let Some(index) = self.current_index {
            if let Some((_, song)) = self.queue.get(index) {
                self.player.command("loadfile", &[song.url()]);
            }
        }
    }

    pub fn set_playlist(&mut self, playlist: Vec<Song>) {
        self.queue = playlist.into_iter().enumerate().collect();
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
        self.play_current_index();
    }

    pub fn previous(&mut self) {
        self.current_index = self.current_index.map(|index| index.saturating_sub(1));
        self.play_current_index();
    }

    pub fn play(&self) {
        self.player.unpause();
    }
    pub fn pause(&self) {
        self.player.pause();
    }
    pub fn stop(&self) {
        todo!()
    }
    pub fn set_volume(&mut self, volume: VolumeSetter) {}
    pub fn get_volume(&self) -> Volume {
        todo!()
    }
    pub fn play_song(&mut self, song: Song) {}
    pub fn set_autoplay(&mut self, autoplay: bool) {
        self.autoplay = autoplay;
        if autoplay {
            self.current_index = Some(0);
            self.play_current_index()
        }
    }
    pub fn set_repeat(&mut self, repeat: Repeat) {}
}
