use std::collections::HashMap;
use std::sync::Arc;

use crossterm::event::{Event, KeyCode, KeyEvent};
use futures::StreamExt;
use protocol::playlist::{FullPlaylist, Playlist, Song};
use protocol::{playlist::PlaylistId, Action};
use protocol::{DataType, NotificationId};

use color_eyre::Result;
use ratatui::crossterm::event::EventStream;
use ratatui::layout::{Constraint, Flex, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Clear, List, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
struct Notification(pub String);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Menu {
    Source,
    Playlist,
    Song,
}

impl Menu {
    pub fn next(self) -> Self {
        match self {
            Self::Source => Self::Playlist,
            Self::Playlist => Self::Song,
            Self::Song => self,
        }
    }
    pub fn previous(self) -> Self {
        match self {
            Self::Source => self,
            Self::Playlist => Self::Source,
            Self::Song => Self::Playlist,
        }
    }
}

struct App {
    should_quit: CancellationToken,
    in_channel: mpsc::Receiver<Action>,
    youtube: Source,
    focused: bool,
    notifications: Vec<(NotificationId, Notification)>,
    redraw_sender: mpsc::Sender<()>,
    redraw_receiver: mpsc::Receiver<()>,
    current_menu: Menu,
}

struct Source {
    out_channel: mpsc::Sender<Action>,
    playlists: RwLock<Arc<[Playlist]>>,
    name: String,
    current_playlist: usize,
    current_song: usize,
}

impl Source {
    pub fn new(out_channel: mpsc::Sender<Action>, name: String) -> Self {
        Self {
            out_channel,
            playlists: RwLock::new(Arc::new([])),
            name,
            current_playlist: 0,
            current_song: 0,
        }
    }
    pub async fn init(&mut self) {
        let action = protocol::playlist::Command::list_all();
        let playlists = action.send(&self.out_channel).await.recv().await.unwrap();
        *self.playlists.write().await = playlists;
    }
    pub async fn playlists_widget(&self) -> (List, ListState) {
        let action = protocol::playlist::Command::list_all();
        let res = action.send(&self.out_channel).await;
        let maybe_res = res.recv().await;
        let playlists = match maybe_res {
            Ok(playlists) => playlists,
            _ => Default::default(),
        };
        let items: Vec<String> = if !playlists.is_empty() {
            if (tokio::time::timeout(std::time::Duration::from_millis(50), async {
                *self.playlists.write().await = playlists.clone()
            })
            .await)
                .is_ok()
            {
                playlists.iter().map(|p| p.name().clone()).collect()
            } else {
                vec![String::from("Timeout on write")]
            }
        } else {
            vec![String::from("Loading")]
        };
        let state = if self.current_playlist < items.len() {
            Some(self.current_playlist)
        } else {
            None
        };
        let list = List::new(items)
            .block(
                Block::bordered()
                    .title("Playlists")
                    .title_alignment(ratatui::layout::Alignment::Left),
            )
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        (list, ListState::default().with_selected(state))
    }
    pub async fn songs_widget(&self) -> (List, ListState) {
        let songs: Vec<String> = {
            if let Ok(playlists) = self.playlists.try_read() {
                //let playlists = self.playlists.read().await;
                if self.current_playlist < playlists.len() {
                    let playlist = playlists[self.current_playlist].clone();
                    let action = protocol::playlist::Command::get(playlist);
                    let res = action.send(&self.out_channel).await;
                    let maybe_res =
                        tokio::time::timeout(std::time::Duration::from_millis(500), res.recv())
                            .await;
                    match maybe_res {
                        Ok(Ok(playlist)) => {
                            playlist.songs().iter().map(Song::title_clone).collect()
                        }
                        Err(err) => vec![err.to_string(), format!("{playlists:?}")],
                        Ok(Err(err)) => vec![err.to_string()],
                    }
                } else {
                    vec![String::from("out of range")]
                }
            } else {
                vec![String::from("try_read failed")]
            }
        };
        let state = if self.current_song < songs.len() {
            Some(self.current_song)
        } else {
            None
        };
        let list = List::new(songs)
            .block(
                Block::bordered()
                    .title("Songs")
                    .title_alignment(ratatui::layout::Alignment::Left),
            )
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        (list, ListState::default().with_selected(state))
    }
    pub fn increase_playlist(&mut self) {
        if let Ok(playlists) = self.playlists.try_read() {
            if self.current_playlist < playlists.len().saturating_sub(1) {
                self.current_playlist += 1;
                self.current_song = 0;
            }
        }
    }
    pub fn decrease_playlist(&mut self) {
        if self.current_playlist > 0 {
            self.current_playlist -= 1;
            self.current_song = 0;
        }
    }
    pub fn increase_song(&mut self) {
        self.current_song += 1;
    }
    pub fn decrease_song(&mut self) {
        if self.current_song > 0 {
            self.current_song -= 1;
        }
    }

    async fn set_autoplay(&self, autoplay: bool) {
        if autoplay {
            let action = protocol::playback::Command::add_to_queue(protocol::Queue::Playlist(
                self.playlists.read().await[self.current_playlist].clone(),
            ));
            action.send(&self.out_channel).await;
        }
        let action = protocol::playback::Command::set_autoplay(autoplay);
        action.send(&self.out_channel).await;
    }
}

impl App {
    /// Number of frames per second
    const FRAMERATE: u64 = 5;
    pub async fn new() -> Self {
        let cancel_token = CancellationToken::new();
        let (ui_tx, ui_rx) = tokio::sync::mpsc::channel(100);
        let (yt_tx, yt_rx) = tokio::sync::mpsc::channel(100);
        let yt = youtube::Handler::new(yt_rx, ui_tx, cancel_token.child_token()).await;
        tokio::task::spawn(async move {
            yt.run().await;
        });
        let (redraw_tx, redrax_rx) = tokio::sync::mpsc::channel(10);
        Self {
            should_quit: cancel_token,
            in_channel: ui_rx,
            youtube: Source::new(yt_tx, String::from("youtube")),
            focused: true,
            notifications: Vec::new(),
            redraw_sender: redraw_tx,
            redraw_receiver: redrax_rx,
            current_menu: Menu::Playlist,
        }
    }

    pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_millis(1000 / Self::FRAMERATE));
        let mut event = EventStream::new();
        //let youtube = self.youtube.clone();
        // tokio::spawn(async move { youtube.lock().await.init().await });
        'runloop: loop {
            tokio::select! {
                _ = self.should_quit.cancelled() => break 'runloop,
                _ = interval.tick() => {
                    if self.focused {
                        self.execute_draw(&mut terminal).await;
                    };
                },
                Some(Ok(event)) = event.next() => self.handle_event(&event).await,
                Some(action) = self.in_channel.recv() => self.handle_action(action),
                Some(()) = self.redraw_receiver.recv() => {
                    if self.focused {
                        self.execute_draw(&mut terminal).await
                    }
                }
            }
        }
        Ok(())
    }
    async fn execute_draw(&mut self, terminal: &mut DefaultTerminal) {
        let (playlists, p_state) = self.youtube.playlists_widget().await;
        let (songs, s_state) = self.youtube.songs_widget().await;
        terminal.draw(|frame| self.draw(frame, playlists, songs, p_state, s_state));
    }
    fn draw(
        &self,
        frame: &mut Frame<'_>,
        playlists: List,
        songs: List,
        mut playlist_state: ListState,
        mut song_state: ListState,
    ) {
        let [main_area, player_area] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(4)])
                .margin(1)
                .areas(frame.area());
        let [left, songs_area] =
            Layout::horizontal([Constraint::Percentage(25), Constraint::Fill(1)]).areas(main_area);
        let [sources, playlists_area, options] =
            Layout::vertical([Constraint::Fill(1); 3]).areas(left);
        let outer_block = Block::bordered()
            .title("YAMA")
            .title_alignment(ratatui::layout::Alignment::Center);

        let sources_block = Block::bordered().title("Sources");
        let options_block = Block::bordered().title("Options");
        let player_block = Block::bordered().title("Player Info");

        frame.render_widget(Clear, frame.area());
        frame.render_widget(outer_block, frame.area());
        frame.render_widget(sources_block, sources);
        frame.render_widget(options_block, options);
        frame.render_widget(player_block, player_area);
        frame.render_stateful_widget(playlists, playlists_area, &mut playlist_state);
        frame.render_stateful_widget(songs, songs_area, &mut song_state);

        self.draw_notification(frame);
    }

    fn draw_notification(&self, frame: &mut Frame<'_>) {
        if let Some((_notif_id, notif)) = self.notifications.last() {
            let [area] = Layout::horizontal([Constraint::Percentage(30)])
                .flex(Flex::Center)
                .areas(frame.area());
            let [area] = Layout::vertical([Constraint::Percentage(30)])
                .flex(Flex::Center)
                .areas(area);
            let paragraph =
                Paragraph::new(notif.0.clone()).block(Block::bordered().title("Notification"));
            frame.render_widget(Clear, area);
            frame.render_widget(paragraph, area);
        }
    }
    fn handle_action(&mut self, action: Action) {
        let Action { command, response } = action;
        match command {
            protocol::Command::Refresh => todo!(),
            protocol::Command::Restart => todo!(),
            protocol::Command::Quit => self.should_quit.cancel(),
            protocol::Command::UI(ui_comamnd) => match ui_comamnd {
                protocol::UICommand::PromptUser(_) => todo!(),
                protocol::UICommand::InformUser(message) => {
                    let _ = response.send(Ok(self.new_notification(message).into()));
                }
                protocol::UICommand::OpenUrl(message) => {
                    let res: protocol::Result<DataType> = open::that(message)
                        .map(|_| {
                            self.new_notification(String::from("Check your browser"))
                                .into()
                        })
                        .map_err(Into::<protocol::Error>::into);
                    response.send(res);
                }
                protocol::UICommand::CloseNotification(notification_id) => {
                    self.notifications
                        .retain(|&(notif_id, _)| notif_id != notification_id);
                    response.send(Ok(().into()));
                }
            },
            _ => (),
        }
    }
    fn new_notification(&mut self, message: String) -> NotificationId {
        let notif_id = NotificationId::new();
        self.notifications.push((notif_id, Notification(message)));
        notif_id
    }

    async fn handle_event(&mut self, event: &Event) {
        match event {
            Event::FocusGained => self.focused = true,
            Event::FocusLost => self.focused = false,
            Event::Key(event) => self.handle_key_event(event).await,
            Event::Mouse(_) => todo!(),
            Event::Paste(_) => todo!(),
            Event::Resize(_, _) => todo!(),
        }
    }

    async fn handle_key_event(&mut self, event: &KeyEvent) {
        match event.code {
            KeyCode::Char('q') => self.should_quit.cancel(),
            KeyCode::Char('j') => match self.current_menu {
                Menu::Song => self.youtube.increase_song(),
                Menu::Playlist => self.youtube.increase_playlist(),
                Menu::Source => (),
            },
            KeyCode::Char('k') => match self.current_menu {
                Menu::Song => self.youtube.decrease_song(),
                Menu::Playlist => self.youtube.decrease_playlist(),
                Menu::Source => (),
            },
            KeyCode::Char('h') => self.current_menu = self.current_menu.previous(),
            KeyCode::Char('l') => self.current_menu = self.current_menu.next(),
            KeyCode::Char('a') => self.youtube.set_autoplay(true).await,
            KeyCode::Char('A') => self.youtube.set_autoplay(false).await,
            _ => (),
        }
        self.redraw_sender.send(()).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();
    let app_result = App::new().await.run(terminal).await;
    ratatui::restore();
    app_result
}
