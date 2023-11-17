use crate::{
    event::{self, Move, MoveDirection, PlayerCtrl},
    player::Player,
    sources::{Client, ClientTrait, PlaylistTrait, Song},
};
use color_eyre::Result;
use crossterm::{
    event::DisableMouseCapture,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    prelude::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState},
    Terminal,
};
use std::{
    io::{self, Stdout},
    sync::Arc,
    time::Duration,
};
use strum_macros::EnumIter;
use tokio::sync::Mutex;

pub trait DialogueBox {
    fn display_message(&self);
    fn get_response(&self);
}

#[derive(Debug, Default, Clone)]
pub struct State {
    client: ListState,
    playlist: ListState,
    song: ListState,
}

impl State {
    pub fn get(&self, panel: Panel) -> ListState {
        match panel {
            Panel::Sources => self.client.clone(),
            Panel::Playlists => self.playlist.clone(),
            Panel::Songs => self.song.clone(),
        }
    }

    pub fn get_mut(&mut self, panel: Panel) -> &mut ListState {
        match panel {
            Panel::Sources => &mut self.client,
            Panel::Playlists => &mut self.playlist,
            Panel::Songs => &mut self.song,
        }
    }
}

pub struct Ui {
    clients: Vec<Arc<Mutex<Client>>>,
    terminal: Arc<Mutex<Terminal<CrosstermBackend<Stdout>>>>,
    current_panel: Panel,
    state: State,
    pub player: Player,
    pub quit: bool,
}

#[derive(EnumIter, Hash, PartialEq, Eq, Clone, Copy)]
pub enum Panel {
    Sources,
    Playlists,
    Songs,
}

impl Ui {
    pub async fn init() -> Result<Self> {
        let yt: Arc<Mutex<crate::sources::Client>> =
            Arc::new(Mutex::new(crate::sources::youtube::Client::new().into()));
        yt.lock().await.connect().await?;
        yt.lock().await.load_playlists().await?;
        let player = Player::new();
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        let mut state: State = State::default();

        state.client.select(Some(0));

        Ok(Self {
            clients: vec![yt],
            terminal: Arc::new(Mutex::new(terminal)),
            current_panel: Panel::Sources,
            state,
            player,
            quit: false,
        })
    }

    pub fn load(&self) {
        let yt = self.clients[0].clone();
        tokio::spawn(async move {
            yt.lock().await.connect().await;
            yt.lock().await.load_playlists().await;
        });
    }

    async fn get_clients(&self) -> Vec<ListItem> {
        futures::stream::iter(self.clients.clone())
            .then(|c| async move { format!("{}", c.lock().await) })
            .map(ListItem::new)
            .collect()
            .await
    }

    async fn get_playlists(&self) -> Vec<ListItem> {
        let client = self.state.get(Panel::Sources).selected();
        if let Some(n) = client {
            self.clients[n]
                .lock()
                .await
                .get_playlists()
                .await
                .iter()
                .map(|p| {
                    format!(
                        "{}{}",
                        PlaylistTrait::get_title(p),
                        if p.is_loading() { " ⟳" } else { "" }
                    )
                })
                .map(ListItem::new)
                .collect()
        } else {
            vec![]
        }
    }

    async fn get_songs(&self) -> Vec<ListItem> {
        let client = self.state.get(Panel::Sources).selected();
        let playlist = if let Some(n) = client {
            self.clients[n].lock().await.get_playlists().await
        } else {
            vec![]
        };
        let p = self.state.get(Panel::Playlists).selected();
        let songs = p.map_or_else(Vec::new, |k| {
            if k < playlist.len() {
                playlist[k].get_entries()
            } else {
                vec![]
            }
        });
        songs
            .iter()
            .map(|s| s.title.clone())
            .map(ListItem::new)
            .collect()
    }

    pub async fn draw(&mut self) -> Result<()> {
        let clients = self.get_clients().await;
        let playlists = self.get_playlists().await;
        let songs = self.get_songs().await;
        let mut state = self.state.clone();

        self.terminal.lock().await.draw(|f| {
            let size = f.size();
            let block = Block::default()
                .borders(Borders::ALL)
                .title("YAMA")
                .title_alignment(Alignment::Center)
                .border_type(BorderType::Rounded);
            f.render_widget(block, size);

            // Bottom chunk = player
            let main_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(5)])
                .margin(1)
                .split(f.size());
            // Right chunk = Songs
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
                .split(main_chunks[0]);
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(5),
                    Constraint::Length(10),
                    Constraint::Length(6),
                    Constraint::Length(6),
                ])
                .split(chunks[0]);
            let sources = self.make_list(&clients, "Clients", Panel::Sources);
            f.render_stateful_widget(sources, left_chunks[0], state.get_mut(Panel::Sources));
            let playlists = self.make_list(&playlists, "Playlists", Panel::Playlists);
            f.render_stateful_widget(playlists, left_chunks[1], state.get_mut(Panel::Playlists));
            let songs = self.make_list(&songs, "Songs", Panel::Songs);
            f.render_stateful_widget(songs, chunks[1], state.get_mut(Panel::Songs));

            let state = self.player.get_state();
            let options = self.make_list(
                &[
                    ListItem::new(format!("Auto: {}", self.player.is_in_playlist())),
                    ListItem::new(format!("Repeat: {}", state.repeat)),
                    ListItem::new(format!("Shuffle: {}", state.shuffled)),
                    ListItem::new(format!("Volume: {}/100", state.volume)),
                ],
                "Options",
                Panel::Songs,
            );
            f.render_widget(options, left_chunks[2]);
        })?;
        Ok(())
    }

    pub async fn exit(&mut self) -> Result<()> {
        // restore terminal
        disable_raw_mode()?;
        let mut terminal = self.terminal.lock().await;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;
        drop(terminal);
        Ok(())
    }

    async fn get_max(&self, panel: &Panel) -> usize {
        match panel {
            Panel::Sources => self.clients.len(),
            Panel::Playlists => self.get_playlists().await.len(),
            Panel::Songs => self.get_songs().await.len(),
        }
    }

    async fn update_panel_state(&mut self, off: i64) {
        let max = self.get_max(&self.current_panel).await;
        let state = self.state.get_mut(self.current_panel);
        if let Some(n) = state.selected() {
            let n_signed = i64::try_from(n).unwrap_or_default();
            let r = n_signed + off;
            if r >= 0 && (TryInto::<usize>::try_into(r).unwrap_or_default() < max) {
                let r: usize = (n_signed + off).try_into().unwrap_or_default();
                state.select(Some(r));
            } else if TryInto::<usize>::try_into(r).unwrap_or_default() >= max {
                // ensures that we are not beyond the limit
                // if so select the last element
                if max == 1 {
                    state.select(Some(0));
                } else {
                    state.select(Some(max - 1));
                }
            }
        } else {
            state.select(Some(0));
        }
    }

    pub async fn movement(&mut self, dir: Move) {
        type MD = MoveDirection;
        match dir {
            Move::Little(dir) => match dir {
                MD::Up => self.update_panel_state(-1).await,
                MD::Down => self.update_panel_state(1).await,
                _ => (),
            },
            Move::Big(dir) => self.change_panel(&dir),
            Move::Enter => self.handle_enter().await,
        };
        // ensure selection is not out of bond
        self.update_panel_state(0).await;
    }

    async fn handle_enter(&mut self) {
        match self.current_panel {
            Panel::Sources => self.current_panel = Panel::Playlists,
            Panel::Playlists => self.current_panel = Panel::Songs,
            Panel::Songs => self.player_ctrl(PlayerCtrl::Play).await,
        }
    }

    async fn play(&mut self) {
        let url = self.get_current_song_url().await;
        self.player.play(&url, self.state.clone());
    }

    async fn get_current_song_url(&self) -> String {
        let client_index = self.state.get(Panel::Sources).selected().unwrap();
        let playlist_index = self.state.get(Panel::Playlists).selected().unwrap();
        let song_index = self.state.get(Panel::Songs).selected().unwrap();
        self.clients[client_index]
            .lock()
            .await
            .get_playlists()
            .await[playlist_index]
            .get_url_song(song_index)
            .unwrap()
    }

    fn change_panel(&mut self, dir: &MoveDirection) {
        match dir {
            MoveDirection::Down => {
                if self.current_panel == Panel::Sources {
                    self.current_panel = Panel::Playlists;
                }
            }
            MoveDirection::Up => {
                if self.current_panel == Panel::Playlists {
                    self.current_panel = Panel::Sources;
                }
            }
            MoveDirection::Left => self.current_panel = Panel::Playlists,
            MoveDirection::Right => self.current_panel = Panel::Songs,
        }
    }

    pub async fn player_ctrl(&mut self, action: PlayerCtrl) {
        match action {
            PlayerCtrl::Pause => self.player.playpause(),
            PlayerCtrl::Play => self.play().await,
            PlayerCtrl::Shuffle => self.player.shuffle(),
            PlayerCtrl::Repeat => self.player.cycle_repeat(),
            PlayerCtrl::Auto => self.start_auto().await,
            PlayerCtrl::SeekForwardRel => self.player.seek_relative(5),
            PlayerCtrl::SeekBackwardRel => self.player.seek_relative(-5),
            PlayerCtrl::SeekPercent(n) => self.player.seek_percent(n as usize),
            PlayerCtrl::VolumeUp => self.player.incr_volume(5),
            PlayerCtrl::VolumeDown => self.player.incr_volume(-5),
            PlayerCtrl::Prev => self.player.prev(),
            PlayerCtrl::Next => self.player.next(),
        }
    }

    pub async fn start_auto(&mut self) {
        let client_index = self.state.get(Panel::Sources).selected().unwrap();
        let playlist_index = self.state.get(Panel::Playlists).selected().unwrap();
        let urls: Vec<String> = self.clients[client_index]
            .lock()
            .await
            .get_playlists()
            .await[playlist_index]
            .get_all_url()
            .unwrap();
        let urls: Vec<&str> = urls.iter().map(String::as_str).collect();
        self.player.set_auto(&urls, self.state.clone());
    }

    pub async fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        event::handle(self, key).await;
    }
    fn make_list<'a>(&self, list: &[ListItem<'a>], title: &'a str, panel: Panel) -> List<'a> {
        let (bg, fg) = if panel == self.current_panel {
            (Color::White, Color::Black)
        } else {
            (Color::DarkGray, Color::White)
        };

        List::new(list.to_owned())
            .block(Block::default().title(title).borders(Borders::ALL))
            .style(Style::default().fg(Color::White))
            .highlight_style(Style::default().bg(bg).fg(fg))
    }

    pub fn next(&self) {
        self.player.next();
    }
    pub fn prev(&self) {
        self.player.prev();
    }
    pub fn set_pause_val(&self, val: bool) {
        if self.player.paused() != val {
            self.player.playpause();
        }
    }
    pub fn get_playing_song_info(&self) -> Song {
        let state = self.player.get_state();
        Song {
            title: state.title,
            artists: vec![],
            duration: Duration::from_millis(state.duration as u64),
        }
    }
}
