use crate::{sources::{Client, ClientTrait, PlaylistTrait, Song}, player::Player};
use color_eyre::{eyre::eyre, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    prelude::{Backend, Alignment, Layout, Direction, Constraint},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, BorderType, ListState},
    Terminal,
};
use tokio::sync::Mutex;
use std::{
    io::{self, Stdout},
    time::Duration, sync::Arc,
};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

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
    pub fn get(&self, panel: &Panel) -> ListState {
        match panel {
            Panel::Sources => self.client.clone(),
            Panel::Playlists => self.playlist.clone(),
            Panel::Songs => self.song.clone(),
        }
    }

    pub fn get_mut<'a>(&'a mut self, panel: &Panel) -> &'a mut ListState {
        match panel {
            Panel::Sources => &mut self.client,
            Panel::Playlists => &mut self.playlist,
            Panel::Songs => &mut self.song,
        }
    }
}

enum Move {
    Little(MoveDirection),
    Big(MoveDirection)
}

enum MoveDirection {
    Down,
    Up,
    Left,
    Right,
}

pub struct Ui {
    clients: Vec<Arc<Mutex<Client>>>,
    terminal: Arc<Mutex<Terminal<CrosstermBackend<Stdout>>>>,
    current_panel: Panel,
    state: State,
    pub player: Player,
}

#[derive(EnumIter, Hash, PartialEq, Eq, Clone, Copy)]
pub enum Panel {
    Sources,
    Playlists,
    Songs
}

impl Ui {
    pub fn init() -> Result<Self> {
        let yt: Arc<Mutex<crate::sources::Client>> = Arc::new(Mutex::new(crate::sources::youtube::Client::new().into()));
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
        })
    }

    pub fn load(&self) {
        let yt = self.clients[0].clone();
        tokio::spawn( async move {
            yt.lock().await.connect().await;
            yt.lock().await.load_playlists().await;
        }
        );
    }
    pub async fn event_loop(&mut self) -> Result<()> {
        loop {
            if event::poll(Duration::from_millis(100))? {
                let e = event::read()?;
                if let event::Event::Key(key) = e {
                    if self.handle_key(key).await.is_err() { break; }
                };
            }
            self.draw().await?;
        }

        Ok(())
    }

    async fn get_clients(&self) -> Vec<ListItem> {
        futures::stream::iter(self.clients.clone()).then(|c| async move { format!("{}", c.lock().await)}).map(ListItem::new).collect().await
    }

    async fn get_playlists(&self) -> Vec<ListItem> {

        let client = self.state.get(&Panel::Sources).selected();
        if let Some(n) = client {
            self.clients[n].lock().await
                .get_playlists()
                .await
                .iter()
                .map(|p| format!("{}{}", PlaylistTrait::get_title(p), if p.is_loading() { " ⟳" } else {""}))
                .map(ListItem::new)
                .collect()
        } else {
            vec![]
        }

    }

    async fn get_songs(&self) -> Vec<ListItem> {
        let client = self.state.get(&Panel::Sources).selected();
        let playlist = if let Some(n) = client {
            self.clients[n].lock().await
                .get_playlists()
                .await
        } else {
            vec![]
        };
        let p = self.state.get(&Panel::Playlists).selected();
        let songs = p.map_or_else(Vec::new, |k| if k < playlist.len() {
                playlist[k].get_entries()
            } else {
                vec![]
            });
        songs.iter().map(|s| s.title.clone()).map(ListItem::new).collect()
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
                .constraints([Constraint::Length(5), Constraint::Length(10), Constraint::Length(5)])
                .split(chunks[0]);
            let sources = self.make_list(clients, "Clients", Panel::Sources);
            f.render_stateful_widget(sources, left_chunks[0], state.get_mut(&Panel::Sources));
            let playlists = self.make_list(playlists, "Playlists", Panel::Playlists);
            f.render_stateful_widget(playlists, left_chunks[1], state.get_mut(&Panel::Playlists));
            let songs = self.make_list(songs, "Songs", Panel::Songs);
            f.render_stateful_widget(songs, chunks[1], state.get_mut(&Panel::Songs));
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
        let state = self.state.get_mut(&self.current_panel);
        if let Some(n) = state.selected() {
            let r = (n as i64) + off;
            if r >= 0 && (TryInto::<usize>::try_into(r).unwrap_or_default() < max) {
                let r: usize = ((n as i64)+off).try_into().unwrap_or_default();
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
        }
        else { 
            state.select(Some(0));
        }
    }

    async fn movement(&mut self, dir: Move) -> Result<()> {
        type MD = MoveDirection;
        match dir {
            Move::Little(dir) => match dir {
                MD::Up => self.update_panel_state(-1).await,
                MD::Down => self.update_panel_state(1).await,
                _ => ()
            },
            Move::Big(dir) => {self.change_panel(&dir); self.update_panel_state(0).await },
        };
        Ok(())
    }

    fn change_panel(&mut self, dir: &MoveDirection) {
        match dir {
            MoveDirection::Down => if self.current_panel == Panel::Sources { self.current_panel = Panel::Playlists },
            MoveDirection::Up => if self.current_panel == Panel::Playlists { self.current_panel = Panel::Sources },
            MoveDirection::Left => self.current_panel = Panel::Playlists,
            MoveDirection::Right => self.current_panel = Panel::Songs,
        }

    }

    pub async fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        use crossterm::event::KeyCode as KC;
        type MD = MoveDirection;

        match key.code {
            KC::Char('q') => Err(eyre!("quit")),

            KC::Char('j') => self.movement(Move::Little(MD::Down)).await,
            KC::Char('k') => self.movement(Move::Little(MD::Up)).await,
            KC::Char('h') => self.movement(Move::Little(MD::Left)).await,
            KC::Char('l') => self.movement(Move::Little(MD::Right)).await,

            KC::Char('J') => self.movement(Move::Big(MD::Down)).await,
            KC::Char('K') => self.movement(Move::Big(MD::Up)).await,
            KC::Char('H') => self.movement(Move::Big(MD::Left)).await,
            KC::Char('L') => self.movement(Move::Big(MD::Right)).await,
            _ => Ok(()),
        }
    }
    fn make_list<'a>(&self, list:Vec<ListItem<'a>>, title: &'a str, panel: Panel) -> List<'a> {

        let (bg, fg) = if panel == self.current_panel {
            (Color::White, Color::Black)
        } else { (Color::DarkGray, Color::White) };

        List::new(list.clone())
            .block(Block::default().title(title.clone()).borders(Borders::ALL))
            .style(Style::default().fg(Color::White))
            .highlight_style(Style::default().bg(bg).fg(fg))
    }

    pub async fn next(&self) {
        todo!()
    }
    pub async fn prev(&self) {
        todo!()
    }
    pub async fn set_pause_val(&self, val: bool) {
        todo!()
    }
    pub fn get_playing_song_info(&self) -> Option<Song> {
        todo!()
    }
}

