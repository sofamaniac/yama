use std::sync::{Arc, Mutex};

use protocol::playback::SeekMode;
use protocol::{playback::VolumeSetter, Action, FullPlaylist, Playlist, Song};
use protocol::{DataType, Receive, TypedAction, TypedResult};
use ratatui::{
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, List, ListState, Paragraph, Widget},
};
use tokio::sync::mpsc;

use crate::player_widget::PlayerWiget;

enum Error {
    Protocol(protocol::Error),
    Timeout,
}
impl From<protocol::Error> for Error {
    fn from(value: protocol::Error) -> Self {
        Error::Protocol(value)
    }
}
impl From<tokio::time::error::Elapsed> for Error {
    fn from(_value: tokio::time::error::Elapsed) -> Self {
        Error::Timeout
    }
}

type Result<T> = std::result::Result<T, Error>;

pub struct Source {
    out_channel: mpsc::Sender<Action>,
    name: String,
    playlist_widget: PlaylistWidget,
    state: State,
}

#[derive(Clone, Copy, Default)]
struct State {
    repeat: protocol::Repeat,
    autoplay: bool,
    shuffle: bool,
}

impl State {
    pub fn cycle_repeat(self) -> Self {
        let repeat = match self.repeat {
            protocol::Repeat::Off => protocol::Repeat::Song,
            protocol::Repeat::Song => protocol::Repeat::Playlist,
            protocol::Repeat::Playlist => protocol::Repeat::Off,
        };
        Self { repeat, ..self }
    }
    pub fn cycle_autoplay(self) -> Self {
        let autoplay = !self.autoplay;
        Self { autoplay, ..self }
    }
    pub fn cycle_shuffle(self) -> Self {
        let shuffle = !self.shuffle;
        Self { shuffle, ..self }
    }

    fn repeat(&self) -> protocol::Repeat {
        self.repeat
    }

    fn autoplay(&self) -> bool {
        self.autoplay
    }

    fn shuffle(&self) -> bool {
        self.shuffle
    }
}

impl std::ops::DerefMut for Source {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.playlist_widget
    }
}

impl std::ops::Deref for Source {
    type Target = PlaylistWidget;

    fn deref(&self) -> &Self::Target {
        &self.playlist_widget
    }
}

pub struct PlaylistWidget {
    out_channel: mpsc::Sender<Action>,
    playlists: Mutex<Arc<[Playlist]>>,
    songs_widget: Option<SongsWidget>,
    current_playlist: Option<usize>,
}

trait ActionTimeout {
    fn out_channel(&self) -> &mpsc::Sender<Action>;
    async fn action_timeout<T>(&self, action: TypedAction<protocol::Result<T>>) -> Result<T>
    where
        TypedResult<protocol::Result<T>>: Receive<protocol::Result<T>>,
    {
        use tokio::time::timeout;
        let duration = std::time::Duration::from_millis(100);
        let future = action.send(self.out_channel());
        if let Ok(typed_res) = timeout(duration, future).await {
            let future = typed_res.recv();
            let res = timeout(duration, future).await?;
            let res = res?;
            Ok(res)
        } else {
            Err(Error::Timeout)
        }
    }
}

impl ActionTimeout for PlaylistWidget {
    fn out_channel(&self) -> &mpsc::Sender<Action> {
        &self.out_channel
    }
}
impl ActionTimeout for SongsWidget {
    fn out_channel(&self) -> &mpsc::Sender<Action> {
        &self.out_channel
    }
}

impl PlaylistWidget {
    pub(crate) fn new(out_channel: mpsc::Sender<Action>) -> Self {
        Self {
            out_channel,
            playlists: Mutex::new(Arc::new([])),
            current_playlist: None,
            songs_widget: None,
        }
    }
    pub async fn make_playlist_widget(&self) -> (List, ListState) {
        let block = Block::bordered()
            .title("Playlists")
            .title_alignment(ratatui::layout::Alignment::Left);
        let hl_style = Style::new().add_modifier(Modifier::REVERSED);
        if self.playlists.lock().unwrap().is_empty() {
            let action = protocol::playlist::Command::list_all();
            if let Ok(res) = self.action_timeout(action).await {
                *self.playlists.lock().unwrap() = res;
            } else {
                return (
                    List::new(vec![String::from("Loading")])
                        .block(block)
                        .highlight_style(hl_style),
                    ListState::default().with_selected(None),
                );
            };
        };
        let playlists = self.playlists.lock().unwrap();
        let items: Vec<String> = playlists.iter().map(|p| p.name().to_string()).collect();
        let list = List::new(items).block(block).highlight_style(hl_style);
        (
            list,
            ListState::default().with_selected(self.current_playlist),
        )
    }
    pub async fn make_song_widget(&self) -> (List, ListState) {
        if let Some(song_widget) = &self.songs_widget {
            song_widget.make_widget().await
        } else {
            let block = Block::bordered()
                .title("Playlists")
                .title_alignment(ratatui::layout::Alignment::Left);
            let hl_style = Style::new().add_modifier(Modifier::REVERSED);
            (
                List::new(vec![String::from("Loading")])
                    .block(block)
                    .highlight_style(hl_style),
                ListState::default().with_selected(None),
            )
        }
    }
    fn update_song_widget(&mut self) {
        if let Some(index) = self.current_playlist {
            let new_id = self.playlists.lock().unwrap()[index].clone();
            self.songs_widget = Some(SongsWidget::new(self.out_channel.clone(), new_id));
        }
    }

    pub fn increase_playlist(&mut self) {
        if let Some(index) = self.current_playlist {
            let max_index = self.playlists.lock().unwrap().len();
            let max_index = max_index.saturating_sub(1);
            self.current_playlist = Some((index + 1).min(max_index));
            self.update_song_widget();
        } else if self.playlists.lock().unwrap().len() > 0 {
            self.current_playlist = Some(0);
            self.update_song_widget();
        }
    }
    pub fn decrease_playlist(&mut self) {
        if let Some(index) = self.current_playlist {
            self.current_playlist = Some(index.saturating_sub(1));
            self.update_song_widget();
        }
    }

    pub fn decrease_song(&mut self) {
        if let Some(songs) = &mut self.songs_widget {
            songs.decrease_song()
        }
    }

    pub fn increase_song(&mut self) {
        if let Some(songs) = &mut self.songs_widget {
            songs.increase_song()
        }
    }

    pub fn get_current_playlist(&self) -> Option<Playlist> {
        self.current_playlist
            .map(|index| self.playlists.lock().unwrap()[index].clone())
    }
}

struct SongsWidget {
    out_channel: mpsc::Sender<Action>,
    songs: Mutex<Option<Arc<FullPlaylist>>>,
    current_song: Option<usize>,
    playlist: Playlist,
}

impl SongsWidget {
    pub fn new(out_channel: mpsc::Sender<Action>, playlist: Playlist) -> Self {
        Self {
            out_channel,
            songs: Mutex::new(None),
            current_song: None,
            playlist,
        }
    }
    pub async fn make_widget(&self) -> (List, ListState) {
        let block = Block::bordered()
            .title("Songs")
            .title_alignment(ratatui::layout::Alignment::Left);
        let hl_style = Style::new().add_modifier(Modifier::REVERSED);
        if self.songs.lock().unwrap().is_none() {
            let action = protocol::playlist::Command::get(self.playlist.clone());
            if let Ok(res) = self.action_timeout(action).await {
                *self.songs.lock().unwrap() = Some(res);
            } else {
                return (
                    List::new(vec![String::from("Loading")])
                        .block(block)
                        .highlight_style(hl_style),
                    ListState::default().with_selected(None),
                );
            };
        };
        let playlist = self.songs.lock().unwrap().clone().unwrap().clone();
        let items: Vec<String> = playlist.songs().iter().map(Song::title_clone).collect();
        let list = List::new(items).block(block).highlight_style(hl_style);
        (list, ListState::default().with_selected(self.current_song))
    }

    pub fn increase_song(&mut self) {
        if let Some(playlist) = self.songs.lock().unwrap().clone() {
            if let Some(index) = self.current_song {
                let max_index = playlist.songs().len();
                let max_index = max_index.saturating_sub(1);
                self.current_song = Some((index + 1).min(max_index))
            } else if !playlist.songs().is_empty() {
                self.current_song = Some(0)
            }
        }
    }
    pub fn decrease_song(&mut self) {
        if let Some(index) = self.current_song {
            self.current_song = Some(index.saturating_sub(1));
        }
    }
}

impl ActionTimeout for Source {
    fn out_channel(&self) -> &mpsc::Sender<Action> {
        &self.out_channel
    }
}

impl Source {
    pub fn new(out_channel: mpsc::Sender<Action>, name: String) -> Self {
        Self {
            out_channel: out_channel.clone(),
            name,
            playlist_widget: PlaylistWidget::new(out_channel),
            state: State::default(),
        }
    }
    pub async fn cycle_repeat(&mut self) {
        self.state = self.state.cycle_repeat();
        self.repeat(self.state.repeat()).await;
    }
    pub async fn cycle_shuffle(&mut self) {
        self.state = self.state.cycle_shuffle();
        self.shuffle(self.state.shuffle()).await;
    }
    pub async fn cycle_autoplay(&mut self) {
        self.state = self.state.cycle_autoplay();
        self.set_autoplay(self.state.autoplay()).await;
    }
    pub async fn next_song(&self) {
        let action = protocol::playback::Command::next_song();
        let _ = action.send(&self.out_channel).await;
    }
    pub async fn previous_song(&self) {
        let action = protocol::playback::Command::previous_song();
        let _ = action.send(&self.out_channel).await;
    }

    pub async fn set_volume(&self, volume: VolumeSetter) {
        let action = protocol::playback::Command::set_volume(volume);
        let _ = action.send(&self.out_channel).await;
    }

    async fn set_autoplay(&self, autoplay: bool) {
        if autoplay {
            if let Some(playlist) = self.playlist_widget.get_current_playlist() {
                let action =
                    protocol::playback::Command::add_to_queue(protocol::Queue::Playlist(playlist));
                action.send(&self.out_channel).await;
            }
        }
        let action = protocol::playback::Command::set_autoplay(autoplay);
        let _ = action.send(&self.out_channel).await;
    }

    pub async fn toggle_pause(&self) {
        let action = protocol::playback::Command::play_pause();
        let _ = action.send(&self.out_channel).await;
    }

    pub async fn player_widget(&self) -> PlayerWiget {
        let action = protocol::playback::Command::get_info();
        let info = self.action_timeout(action).await;
        let widget = if let Ok(info) = info {
            PlayerWiget::new(info.status)
        } else {
            PlayerWiget::default()
        };
        widget.block(Block::bordered().title("Player"))
    }

    pub async fn option_widget(&self) -> Paragraph {
        let action = protocol::playback::Command::get_info();
        let info = self.action_timeout(action).await;
        let paragraph = if let Ok(info) = info {
            let text = vec![
                Line::from(format!("Autoplay: {}", info.autoplay)),
                Line::from(format!("Repeat: {:?}", info.repeat)),
                Line::from(format!("Shuffle: {}", info.shuffled)),
                Line::from(format!("Volume: {}/100", info.volume.to_u8())),
            ];
            Paragraph::new(text)
        } else {
            Paragraph::new("")
        };
        paragraph.block(Block::bordered().title("Options"))
    }

    async fn shuffle(&self, shuffled: bool) {
        let action = protocol::playback::Command::set_shuffle(shuffled);
        action.send(&self.out_channel).await;
    }

    async fn repeat(&self, repeat: protocol::Repeat) {
        let action = protocol::playback::Command::set_repeat(repeat);
        action.send(&self.out_channel).await;
    }

    pub async fn seek(&self, seek: SeekMode) {
        let action = protocol::playback::Command::seek(seek);
        action.send(&self.out_channel).await;
    }
}
