use std::{cell::RefCell, sync::Arc};

use crossterm::event::{EventStream, KeyCode};
use futures::StreamExt;
use protocol::{
    playback::{SeekMode, VolumeDelta, VolumeSetter},
    Action, Command, Duration, FullPlaylist, NotificationId, PlayerInfo, Playlist, Receive, Repeat,
    Song, UICommand,
};
use ratatui::{
    layout::{Constraint, Flex, Layout},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Clear, Paragraph, StatefulWidget},
    DefaultTerminal, Frame,
};
use tokio::{
    sync::{mpsc, Mutex},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

use crate::{player_widget::PlayerWiget, FullSource, Notification, Source};

#[derive(Default, Clone, Copy)]
enum Menu {
    Song,
    Playlist,
    #[default]
    Source,
}

impl Menu {
    pub fn next(self) -> Self {
        match self {
            Self::Source => Self::Playlist,
            Self::Playlist => Self::Song,
            Self::Song => Self::Song,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            Self::Source => Self::Source,
            Self::Playlist => Self::Source,
            Self::Song => Self::Playlist,
        }
    }
}
#[derive(Default, Clone)]
struct FullSourceWrapper {
    full_sources: FullSource,
    sources: Vec<Source>,
}
impl FullSourceWrapper {
    pub fn new(sources: FullSource) -> Self {
        Self {
            full_sources: sources,
            sources: Vec::new(),
        }
    }
    pub async fn update(&mut self) {
        let sources = self.full_sources.lock().await.sources.clone();
        self.sources = sources;
    }
}
impl ListWidget<FullSourceWrapper> {
    async fn update(&mut self) {
        self.elements.update().await
    }
}
trait Elements {
    type Element;
    fn elements(&self) -> &[Self::Element];
    fn elements_name(&self) -> impl Iterator<Item = String>;
}

#[derive(Clone)]
struct ListWidget<T: Elements> {
    elements: T,
    state: Option<usize>,
    name: String,
}

impl<T: Elements> ListWidget<T> {
    pub fn new(elements: T, name: String) -> Self {
        Self {
            elements,
            state: None,
            name,
        }
    }
    pub fn increase(&mut self) {
        if !self.elements.elements().is_empty() {
            if let Some(index) = self.state {
                self.state = index
                    .checked_add(1)
                    .map(|i| i.min(self.elements.elements().len().saturating_sub(1)));
            } else {
                self.state = Some(0)
            }
        }
    }
    pub fn decrease(&mut self) {
        if !self.elements.elements().is_empty() {
            self.state = self.state.map(|i| i.saturating_sub(1))
        }
    }
    pub fn current(&self) -> Option<&T::Element> {
        if let Some(index) = self.state {
            let elements = self.elements.elements();
            elements.get(index)
        } else {
            None
        }
    }
}
impl<T: Elements> StatefulWidget for ListWidget<T> {
    type State = Option<usize>;
    fn render(
        self,
        area: ratatui::prelude::Rect,
        buf: &mut ratatui::prelude::Buffer,
        state: &mut Self::State,
    ) {
        let outer_block = Block::bordered()
            .title(self.name)
            .title_alignment(ratatui::layout::Alignment::Left);
        let hl_style = Style::new().add_modifier(Modifier::REVERSED);
        let elements = self.elements.elements_name();
        let list = if self.elements.elements().is_empty() {
            ratatui::widgets::List::new(vec![String::from("Loading")])
        } else {
            ratatui::widgets::List::new(elements)
        };
        let list = list.highlight_style(hl_style).block(outer_block);
        let mut state = ratatui::widgets::ListState::default().with_selected(*state);
        StatefulWidget::render(list, area, buf, &mut state);
    }
}
impl<T: Elements + Default> Default for ListWidget<T> {
    fn default() -> Self {
        Self {
            elements: Default::default(),
            state: None,
            name: String::from("Default"),
        }
    }
}
impl Elements for FullSourceWrapper {
    type Element = crate::Source;
    fn elements(&self) -> &[Self::Element] {
        &self.sources
    }
    fn elements_name(&self) -> impl Iterator<Item = String> {
        self.sources.clone().into_iter().map(|s| s.name)
    }
}
impl Elements for Arc<[Playlist]> {
    type Element = Playlist;
    fn elements(&self) -> &[Self::Element] {
        self
    }
    fn elements_name(&self) -> impl Iterator<Item = String> {
        self.iter().map(|p| p.name().to_string())
    }
}
impl Elements for Arc<FullPlaylist> {
    type Element = Song;
    fn elements(&self) -> &[Self::Element] {
        self.songs()
    }
    fn elements_name(&self) -> impl Iterator<Item = String> {
        self.songs().iter().map(|s| s.title().to_string())
    }
}
#[derive(Default, Clone)]
struct State {
    sources: ListWidget<FullSourceWrapper>,
    playlists: ListWidget<Arc<[Playlist]>>,
    songs: ListWidget<Arc<FullPlaylist>>,
    info: Option<PlayerInfo>,
    menu: Menu,
}

impl State {
    pub fn next_menu(&mut self) {
        self.menu = self.menu.next()
    }
    pub fn prev_menu(&mut self) {
        self.menu = self.menu.prev()
    }
    pub fn increase(&mut self) {
        match self.menu {
            Menu::Source => {
                self.sources.increase();
                self.playlists.elements = Default::default();
                self.songs.elements = Default::default();
            }
            Menu::Playlist => {
                self.playlists.increase();
                self.songs.elements = Default::default();
            }
            Menu::Song => self.songs.increase(),
        }
    }
    pub fn decrease(&mut self) {
        match self.menu {
            Menu::Source => self.sources.decrease(),
            Menu::Playlist => self.playlists.decrease(),
            Menu::Song => self.songs.decrease(),
        }
    }
    pub fn autoplay(&self) -> bool {
        self.info.as_ref().is_some_and(|info| info.autoplay)
    }
    pub fn shuffled(&self) -> bool {
        self.info.as_ref().is_some_and(|info| info.shuffled)
    }
    pub fn repeat(&self) -> Repeat {
        self.info.as_ref().map_or(Repeat::Off, |info| info.repeat)
    }
}

enum StateControl {
    MenuNext,
    MenuPrevious,
    GoNextMenu,
    GoPreviousMenu,
}
impl From<StateControl> for Control {
    fn from(value: StateControl) -> Self {
        Self::State(value)
    }
}
enum SourceControl {
    NextSong,
    PreviousSong,
    ToggleShuffle,
    ToggleAutoplay,
    PlayPause,
    CycleRepeat,
    ChangeVolume(VolumeSetter),
    Seek(SeekMode),
}
impl From<SourceControl> for Control {
    fn from(value: SourceControl) -> Self {
        Self::Source(value)
    }
}

enum Control {
    Quit,
    PopNotification,
    State(StateControl),
    Source(SourceControl),
}

pub struct UI {
    cancel_token: CancellationToken,
    in_channel: mpsc::Receiver<Action>,
    terminal: RefCell<DefaultTerminal>,
    focused: bool,
    state: Arc<Mutex<State>>,
    notifications: Vec<(NotificationId, Notification)>,
    tasks: JoinSet<()>,
}

impl UI {
    pub fn new(
        cancel_token: CancellationToken,
        sources: FullSource,
        terminal: DefaultTerminal,
    ) -> (Self, mpsc::Sender<Action>) {
        let (ui_tx, ui_rx) = mpsc::channel(100);
        let state = State {
            sources: ListWidget::new(FullSourceWrapper::new(sources), String::from("Sources")),
            playlists: ListWidget::new(Default::default(), String::from("Playlists")),
            songs: ListWidget::new(Default::default(), String::from("Songs")),
            ..Default::default()
        };
        let state = Arc::new(Mutex::new(state));
        (
            Self {
                cancel_token,
                in_channel: ui_rx,
                terminal: RefCell::new(terminal),
                focused: true,
                state,
                notifications: Vec::default(),
                tasks: JoinSet::new(),
            },
            ui_tx,
        )
    }

    pub async fn run(&mut self) {
        let mut ui_timer = tokio::time::interval(std::time::Duration::from_millis(200));
        let mut terminal_event = EventStream::new();

        loop {
            tokio::select! {
                _ = ui_timer.tick() => {
                    if self.focused {
                        self.render().await;
                    }
                }
                Some(action) = self.in_channel.recv() =>  {
                    self.handle_action(action).await;
                }
                Some(Ok(event)) = terminal_event.next() => {
                    self.handle_terminal_event(event).await;
                }
                Some(_) = self.tasks.join_next() => {}
                _ = self.cancel_token.cancelled() => {
                    break
                }
            }
        }
    }

    async fn handle_command(&mut self, command: UICommand, action: Action) {
        match command {
            UICommand::OpenUrl(message) => match open::that(message) {
                Ok(_) => {
                    let notif_id = NotificationId::new_random();
                    self.notifications
                        .push((notif_id, Notification(String::from("Check your browser"))));
                    let _ = action.response.send(Ok(notif_id.into()));
                }
                Err(e) => {
                    let _ = action.response.send(Err(protocol::Error::Io(e)));
                }
            },
            UICommand::InformUser(message) => {
                let notif_id = NotificationId::new_random();
                self.notifications.push((notif_id, Notification(message)));
                let _ = action.response.send(Ok(notif_id.into()));
            }
            UICommand::PromptUser(_) => todo!(),
            UICommand::CloseNotification(notif_id) => {
                self.notifications.retain(|(id, _)| id != &notif_id)
            }
        }
    }
    async fn handle_action(&mut self, action: Action) {
        match action.command {
            Command::Refresh => todo!(),
            Command::Quit => self.cancel_token.cancel(),
            Command::Restart => unimplemented!(),
            Command::UI(ref ui_command) => self.handle_command(ui_command.clone(), action).await,
            _ => unimplemented!(),
        }
    }
    async fn handle_terminal_event(&mut self, event: crossterm::event::Event) {
        match event {
            crossterm::event::Event::Key(key_event) => self.handle_key_event(key_event).await,
            crossterm::event::Event::FocusLost => self.focused = false,
            crossterm::event::Event::FocusGained => self.focused = true,
            _ => (),
        }
    }

    async fn render(&mut self) {
        self.state.lock().await.sources.update().await;
        if let Some(source) = self.state.clone().lock().await.sources.current() {
            let state = self.state.clone();
            let out_channel = source.sender.clone();
            self.tasks.spawn(async move {
                let action = protocol::playback::Command::get_info();
                if let Ok(res) = action.try_send(&out_channel) {
                    if let Ok(info) = res.recv().await {
                        let mut state = state.lock().await;
                        state.info = Some(info);
                    }
                }
            });
            let state = self.state.clone();
            let out_channel = source.sender.clone();
            self.tasks.spawn(async move {
                let action = protocol::playlist::Command::list_all();
                if let Ok(res) = action.try_send(&out_channel) {
                    if let Ok(playlists) = res.recv().await {
                        let mut state = state.lock().await;
                        if playlists != state.playlists.elements {
                            state.songs.state = None;
                        }
                        state.playlists.elements = playlists
                    }
                }
            });
            let state = self.state.clone();
            let out_channel = source.sender.clone();
            self.tasks.spawn(async move {
                let lock_state = state.lock().await;
                if let Some(playlist) = lock_state.playlists.current() {
                    let action = protocol::playlist::Command::get(playlist.clone());
                    drop(lock_state);
                    if let Ok(res) = action.try_send(&out_channel) {
                        if let Ok(full_playlist) = res.recv().await {
                            let mut state = state.lock().await;
                            state.songs.elements = full_playlist;
                        }
                    }
                }
            });
        }
        let state = self.state.lock().await.clone();
        let mut terminal = self.terminal.try_borrow_mut().unwrap();
        let _ = terminal.draw(|frame| self.draw(frame, &state));
    }

    fn draw(&self, frame: &mut Frame, state: &State) {
        let [main_area, player_area] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(4)])
                .margin(1)
                .areas(frame.area());
        let [left, songs_area] =
            Layout::horizontal([Constraint::Percentage(25), Constraint::Fill(1)]).areas(main_area);
        let [sources, playlists_area, options_area] =
            Layout::vertical([Constraint::Fill(1); 3]).areas(left);
        let outer_block = Block::bordered()
            .title("YAMA")
            .title_alignment(ratatui::layout::Alignment::Center);

        frame.render_widget(Clear, frame.area());
        frame.render_widget(outer_block, frame.area());

        let mut state_sources = state.sources.state;
        frame.render_stateful_widget(state.sources.clone(), sources, &mut state_sources);
        let mut state_playlists = state.playlists.state;
        frame.render_stateful_widget(
            state.playlists.clone(),
            playlists_area,
            &mut state_playlists,
        );
        let mut state_songs = state.songs.state;
        frame.render_stateful_widget(state.songs.clone(), songs_area, &mut state_songs);
        if let Some(ref info) = state.info {
            let player_widget =
                PlayerWiget::new(info.status.clone()).block(Block::bordered().title("Player Info"));
            frame.render_widget(player_widget, player_area);
            let paragraph = {
                let text = vec![
                    Line::from(format!("Autoplay: {}", info.autoplay)),
                    Line::from(format!("Repeat: {:?}", info.repeat)),
                    Line::from(format!("Shuffle: {}", info.shuffled)),
                    Line::from(format!("Volume: {}/100", info.volume.to_u8())),
                ];
                Paragraph::new(text).block(Block::bordered().title("Options"))
            };
            frame.render_widget(paragraph, options_area);
        }
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
    async fn handle_control(&mut self, control: Control) {
        match control {
            Control::Quit => self.cancel_token.cancel(),
            Control::PopNotification => {
                let _ = self.notifications.pop();
            }
            Control::State(state_action) => {
                let mut state = self.state.lock().await;
                match state_action {
                    StateControl::MenuNext => state.increase(),
                    StateControl::MenuPrevious => state.decrease(),
                    StateControl::GoNextMenu => state.next_menu(),
                    StateControl::GoPreviousMenu => state.prev_menu(),
                }
            }
            Control::Source(source_action) => {
                let state = self.state.lock().await;
                if let Some(source) = state.sources.current() {
                    let mut full_state = state.sources.elements.full_sources.lock().await;
                    if full_state.active != state.sources.state {
                        let action = protocol::playback::Command::stop();
                        if let Some(source) = full_state.get_active() {
                            action.send(&source.sender).await;
                        }
                    }
                    full_state.active = state.sources.state;
                    drop(full_state);
                    match source_action {
                        SourceControl::NextSong => {
                            let action = protocol::playback::Command::next_song();
                            let _ = action.send(&source.sender).await;
                        }
                        SourceControl::PreviousSong => {
                            let action = protocol::playback::Command::previous_song();
                            let _ = action.send(&source.sender).await;
                        }
                        SourceControl::ToggleAutoplay => {
                            if let Some(playlist) = state.playlists.current() {
                                let autoplay = state.autoplay();
                                if !autoplay {
                                    let action = protocol::playback::Command::add_to_queue(
                                        protocol::Queue::Playlist(playlist.clone()),
                                    );
                                    action.send(&source.sender).await;
                                }
                                let action = protocol::playback::Command::set_autoplay(!autoplay);
                                let _ = action.send(&source.sender).await;
                            }
                        }
                        SourceControl::ToggleShuffle => {
                            let shuffled = state.shuffled();
                            let action = protocol::playback::Command::set_shuffle(!shuffled);
                            let _ = action.send(&source.sender).await;
                        }
                        SourceControl::PlayPause => {
                            let action = protocol::playback::Command::play_pause();
                            let _ = action.send(&source.sender).await;
                        }
                        SourceControl::CycleRepeat => {
                            let repeat = state.repeat();
                            let action =
                                protocol::playback::Command::set_repeat(cycle_repeat(repeat));
                            let _ = action.send(&source.sender).await;
                        }
                        SourceControl::ChangeVolume(setter) => {
                            let action = protocol::playback::Command::set_volume(setter);
                            let _ = action.send(&source.sender).await;
                        }
                        SourceControl::Seek(seek) => {
                            let action = protocol::playback::Command::seek(seek);
                            let _ = action.send(&source.sender).await;
                        }
                    }
                }
            }
        }
    }

    async fn handle_key_event(&mut self, key_event: crossterm::event::KeyEvent) {
        let control = match key_event.code {
            KeyCode::Char('q') => Control::Quit,
            KeyCode::Char('l') => StateControl::GoNextMenu.into(),
            KeyCode::Char('h') => StateControl::GoPreviousMenu.into(),
            KeyCode::Char('j') => StateControl::MenuNext.into(),
            KeyCode::Char('k') => StateControl::MenuPrevious.into(),
            KeyCode::Char('>') => SourceControl::NextSong.into(),
            KeyCode::Char('<') => SourceControl::PreviousSong.into(),
            KeyCode::Char('a') => SourceControl::ToggleAutoplay.into(),
            KeyCode::Char('y') => SourceControl::ToggleShuffle.into(),
            KeyCode::Char(' ') => SourceControl::PlayPause.into(),
            KeyCode::Char('r') => SourceControl::CycleRepeat.into(),
            KeyCode::Char('d') => {
                SourceControl::ChangeVolume(VolumeSetter::Relative(VolumeDelta::new(-5))).into()
            }
            KeyCode::Char('f') => {
                SourceControl::ChangeVolume(VolumeSetter::Relative(VolumeDelta::new(5))).into()
            }
            KeyCode::Left => SourceControl::Seek(SeekMode::Backward(Duration::from_secs(5))).into(),
            KeyCode::Right => SourceControl::Seek(SeekMode::Forward(Duration::from_secs(5))).into(),
            KeyCode::Esc => Control::PopNotification,
            _ => return,
        };
        self.handle_control(control).await;
        self.render().await;
    }
}

fn cycle_repeat(repeat: Repeat) -> Repeat {
    match repeat {
        Repeat::Off => Repeat::Song,
        Repeat::Song => Repeat::Playlist,
        Repeat::Playlist => Repeat::Off,
    }
}
