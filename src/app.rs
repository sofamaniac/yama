
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use crossterm::event::KeyCode;
#[derive(Debug, Default)]
pub struct App {
    current_menu: Menu,
    active_player: Option<usize>,
    clients: Vec<Client>,
    should_quit: bool,
    position: Position,
    dbus: Option<Sender<PlayerState>>,
}

impl App {
    pub fn add_client(&mut self, client: Client) {
        self.clients.push(client)
    }
    pub fn set_dbus(&mut self, dbus: Sender<PlayerState>) {
        self.dbus = Some(dbus);
    }
    pub async fn run(&mut self) -> Result<()> {
        let mut tui = Tui::new()?;
        debug!("Running App");
        tui.enter()?;

        while !self.should_quit {
            if let Some(event) = tui.next().await {
                let mut maybe_action = self.handle_event(event);
                while let Some(action) = maybe_action {
                    maybe_action = self.update(action, &mut tui).await;
                }
            }
            for client in self.clients.iter_mut() {
                match client.receiver.try_recv() {
                    Ok(msg) => client.handle_answer(msg).await,
                    Err(Empty) => (),
                    Err(Disconnected) => (),
                }
            }
        }
        Ok(())
    }
    pub fn handle_event(&self, event: Event) -> Option<Action> {
        let config: Config = confy::load("yamav3", None).expect("Cannot acces config");

        match event {
            Event::Key(KeyCode::Char(c)) => config.get_action(&c),
            Event::Render => Some(Action::Render),
            Event::Update => Some(Action::Update),
            Event::Key(_) => None,
        }
    }
    fn ui(&self, f: &mut Frame<'_>) {
        let player_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Percentage(80), Constraint::Max(4)])
            .split(f.size());
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Percentage(25), Constraint::Percentage(75)])
            .split(player_layout[0]);
        let left_column = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Max(8),
                Constraint::Max(8),
                Constraint::Max(6),
                Constraint::Min(0),
            ])
            .split(layout[0]);
        self.render_sources_widget(f, left_column[0]);
        self.render_playlist_widget(f, left_column[1]);
        self.render_song_widget(f, layout[1]);
        self.render_info_widget(f, left_column[2]);
        self.render_player_widget(f, player_layout[1]);
        self.render_alert_box(f);
    }
    fn render_sources_widget(&self, f: &mut Frame, layout: Rect) {
        let names: Vec<String> = self.clients.iter().map(|c| c.name.clone()).collect();
        let mut state = ListState::default();
        state.select(Some(self.position.client));
        let widget = make_list_widget(names, "Sources", self.current_menu == Menu::Client);
        f.render_stateful_widget(widget, layout, &mut state)
    }
    fn render_playlist_widget(&self, f: &mut Frame<'_>, layout: Rect) {
        let playlists = if let Some(client) = self.get_current_client() {
            client.get_playlists()
        } else {
            Default::default()
        };
        let mut state = ListState::default();
        state.select(self.position.playlist);
        let widget = make_list_widget(playlists, "Playlists", self.current_menu == Menu::Playlist);
        f.render_stateful_widget(widget, layout, &mut state);
    }
    fn render_song_widget(&self, f: &mut Frame<'_>, layout: Rect) {
        let songs = if let Some(client) = self.get_current_client() {
            client.get_songs(self.position.playlist)
        } else {
            Default::default()
        };
        let mut state = ListState::default();
        state.select(self.position.song);
        let widget = make_list_widget(songs, "Songs", self.current_menu == Menu::Song);
        f.render_stateful_widget(widget, layout, &mut state);
    }
    fn render_info_widget(&self, f: &mut Frame<'_>, layout: Rect) {
        let info = vec![
            "Auto:".to_string(),
            "Repeat:".to_string(),
            "Shuffle:".to_string(),
            "Volume:".to_string(),
        ];
        let widget = make_list_widget(info, "Options", true);
        f.render_widget(widget, layout);
    }
    fn render_player_widget(&self, f: &mut Frame<'_>, layout: Rect) {
        let block = Block::new()
            .borders(Borders::ALL)
            .title("Player Informations");
        let text = Paragraph::new("Placeholder").block(block);
        f.render_widget(text, layout)
    }
    fn render_alert_box(&self, f: &mut Frame<'_>) {
        if self.clients.is_empty() {
            return;
        }
        let alert = self.clients[self.position.client].alert.clone();
        if alert.is_none() {
            return;
        }
        let popup = Block::default()
            .title("Alert Box")
            .borders(Borders::ALL)
            .style(Style::default())
            .bg(Color::Reset);
        let text = Paragraph::new(alert.clone().unwrap())
            .block(popup.clone())
            .wrap(Wrap { trim: true });
        let area = centered_rec(f.size());
        let area = Layout::default()
            .direction(Direction::Horizontal)
            .margin(3)
            .constraints([Constraint::Percentage(100)])
            .split(area);
        f.render_widget(Clear, area[0]); // clear background
        f.render_widget(text, area[0]);
    }

    pub async fn update(&mut self, action: Action, tui: &mut Tui) -> Option<Action> {
        match action {
            Action::Render => {
                if let Err(err) = tui.draw(|f| self.ui(f)) {
                    debug!("Error in render : {err}");
                }
                None
            }
            Action::Quit => {
                self.should_quit = true;
                for client in self.clients.iter_mut() {
                    let (request, _) = Request::from_command(Command::Quit);
                    client.sender.send(request).await;
                }
                tui.exit();
                None
            }
            Action::PlayerAction(action) => self.handle_player_action(action).await,
            Action::MenuAction(action) => self.handle_menu_event(action),
            Action::Update => {
                let position = self.position;
                if let Some(client) = self.get_current_client_mut() {
                    client.update_playlistlist().await;
                    client.update_playlist(position.playlist).await
                }
                None
            }
        }
    }
    fn get_current_client_mut(&mut self) -> Option<&mut Client> {
        if self.clients.is_empty() {
            return None;
        }
        Some(&mut self.clients[self.position.client])
    }
    fn get_current_client(&self) -> Option<&Client> {
        if self.clients.is_empty() {
            return None;
        }
        Some(&self.clients[self.position.client])
    }
    fn handle_menu_event(&mut self, action: MenuCtrl) -> Option<Action> {
        let mut offset: i64 = 0;
        match action {
            MenuCtrl::Next => {
                offset = 1;
            }
            MenuCtrl::Prev => {
                offset = -1;
            }
            MenuCtrl::NextMenu => match self.current_menu {
                Menu::Client => self.current_menu = Menu::Playlist,
                Menu::Playlist => self.current_menu = Menu::Song,
                Menu::Song => (),
            },
            MenuCtrl::PrevMenu => match self.current_menu {
                Menu::Client => (),
                Menu::Playlist => {
                    self.current_menu = Menu::Client;
                    self.position.playlist = None
                }
                Menu::Song => {
                    self.current_menu = Menu::Playlist;
                    self.position.song = None
                }
            },
        }
        match self.current_menu {
            Menu::Client => {
                self.position.client =
                    clamp(Some(self.position.client), offset, self.clients.len()).unwrap()
            }
            Menu::Playlist => {
                self.position.playlist = clamp(
                    self.position.playlist,
                    offset,
                    self.clients[self.position.client].playlists_info.len(),
                )
            }
            Menu::Song => {
                self.position.song = clamp(
                    self.position.song,
                    offset,
                    self.clients[self.position.client].songs_info.len(),
                )
            }
        }
        Some(Action::Render)
    }

    async fn handle_player_action(&self, action: PlayerCommand) -> Option<Action> {
        if let Some(player) = self.active_player {
            let player = &self.clients[player];
            let (request, _) = Request::from_command(Command::Player(action));
            player.sender.send(request).await;
        };
        None
    }
}

fn centered_rec(size: Rect) -> Rect {
    let center_x = size.width / 2;
    let center_y = size.height / 2;
    let width = size.width * 3 / 4;
    let height = size.height * 3 / 4;
    let corner_x = center_x - (width / 2);
    let corner_y = center_y - (height / 2);
    Rect {
        x: corner_x,
        y: corner_y,
        width,
        height,
    }
}

fn clamp(start: Option<usize>, offset: i64, max: usize) -> Option<usize> {
    if max == 0 {
        return None;
    };
    if start.is_none() {
        return Some(0);
    };
    let start = start.unwrap();
    let res: i64 = start as i64 + offset;
    if res < 0 {
        Some(0)
    } else if (res as usize) < max {
        Some(res as usize)
    } else {
        Some(max - 1)
    }
}

fn make_list_widget(list: Vec<String>, title: &str, focused: bool) -> List<'_> {
    let list: Vec<ListItem<'_>> = list.iter().map(|s| ListItem::new(s.clone())).collect();
    let style = get_style(focused);
    let hg_style = get_highlight_style(focused);
    List::new(list)
        .block(Block::new().borders(Borders::ALL).title(title))
        .style(style)
        .highlight_style(hg_style)
}

fn get_style(focused: bool) -> Style {
    let config: Config = confy::load("yamav3", None).expect("Cannot access config");
    let fg = if focused {
        config.focused_fg
    } else {
        config.unfocused_fg
    };
    let bg = if focused {
        config.focused_bg
    } else {
        config.unfocused_bg
    };
    Style::default().fg(fg).bg(bg)
}

fn get_highlight_style(focused: bool) -> Style {
    let config: Config = confy::load("yamav3", None).expect("Cannot access config");
    let h_fg = if focused {
        config.focused_highlight_fg
    } else {
        config.unfocused_highlight_fg
    };
    let h_bg = if focused {
        config.focused_highlight_bg
    } else {
        config.unfocused_highlight_bg
    };
    Style::default().fg(h_fg).bg(h_bg)
}
