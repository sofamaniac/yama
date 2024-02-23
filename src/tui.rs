use std::{
    fmt::{self, Display},
    ops::{Deref, DerefMut},
    time::Duration,
};

use anyhow::Result;
use crossterm::{
    cursor,
    event::{DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEventKind},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::{FutureExt, StreamExt};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use thiserror::Error;
use tokio::{
    sync::mpsc::{self, Receiver, Sender},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::{
    client::interface::Widget as InterfaceWidget,
    config::{self, Config},
    orchestrator::{Action, ListHolderToString, Menu, MenuCtrl, MyEvents, State},
};

type Backend<T> = CrosstermBackend<T>;
#[derive(Debug, Clone, Error)]
pub struct Error;

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Error while sending event")
    }
}

#[derive(Debug)]
pub(crate) enum Widget {
    Widget(InterfaceWidget),
    CommandPrompt,
}

impl Widget {
    pub fn captures_output(&self) -> bool {
        match self {
            Widget::Widget(widget) => widget.captures_output(),
            Widget::CommandPrompt => true,
        }
    }
}

impl From<InterfaceWidget> for Widget {
    fn from(value: InterfaceWidget) -> Self {
        Widget::Widget(value)
    }
}

#[derive(Debug)]
pub enum Event {
    Render(Box<State>),
    Widget(Widget),
}

impl From<Widget> for Event {
    fn from(value: Widget) -> Self {
        Event::Widget(value)
    }
}

struct RenderWidget {
    title: String,
    content: String,
    prompt: Option<String>,
    max_height: Option<u16>,
}

pub struct Tui {
    terminal: ratatui::Terminal<Backend<std::io::Stderr>>,
    tasks: JoinHandle<()>,
    framerate: f64,
    cancel_token: CancellationToken,
    orchestrator_tx: Sender<MyEvents>,
    event_rx: Receiver<Event>,
    widgets: Vec<Widget>,
    prompt_string: String,
    pub event_tx: Sender<Event>,
    /// Accumulate events to send a single [MenuCtrl::Offset] event, instead of overloading the
    /// channel with [MenuCtrl::Prev] or [MenuCtrl::Next] events
    offset: isize,
}

impl Tui {
    pub fn new(orchestrator_tx: Sender<MyEvents>, cancel_token: CancellationToken) -> Result<Self> {
        let framerate = 10.0;
        let terminal = ratatui::Terminal::new(Backend::new(std::io::stderr()))?;
        let (event_tx, event_rx) = mpsc::channel(32);
        let tasks = tokio::spawn(async {});
        Ok(Self {
            terminal,
            tasks,
            framerate,
            cancel_token,
            orchestrator_tx,
            event_rx,
            event_tx,
            widgets: Vec::new(),
            offset: 0,
            prompt_string: String::new(),
        })
    }
    pub async fn run(&mut self) {
        let frame_duration = std::time::Duration::from_secs_f64(1.0 / self.framerate);
        let cancel_token = self.cancel_token.clone();
        let mut reader = crossterm::event::EventStream::new();
        let mut render_interval = tokio::time::interval(frame_duration);
        loop {
            let render_delay = render_interval.tick();
            let event = reader.next().fuse();
            tokio::select! {
                _ = cancel_token.cancelled() => break,
                Some(event) = event => {
                    if let Ok(event) =  event {
                        if let Some(event) = self.handle_tui_event(event).await {
                            let _ = self.send_event(event, frame_duration).await;
                        };
                    } else {
                        todo!("handle error");
                    };
                },
                _ = render_delay => {
                    if self.offset != 0 {
                        if let Ok(()) = self.send_event(MenuCtrl::Offset(self.offset).into(), frame_duration).await {
                            self.offset = 0
                        }
                    }
                    if self.orchestrator_tx.send(Action::Render.into()).await.is_err() {
                        let _ = self.exit();
                    }
                },
                event = self.event_rx.recv() => {
                    if let Some(event) = event {
                    self.handle_event(event)
                    }
                }
            }
        }
    }
    async fn send_event(&mut self, event: MyEvents, timeout: Duration) -> Result<()> {
        match self.orchestrator_tx.send_timeout(event, timeout).await {
            Ok(_) => Ok(()),
            Err(mpsc::error::SendTimeoutError::Timeout(_)) => Err(Error.into()),
            Err(mpsc::error::SendTimeoutError::Closed(_)) => {
                self.exit().unwrap();
                Err(Error.into())
            }
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Render(state) => self.render(&state),
            Event::Widget(widget) => self.widgets.push(widget),
        }
    }
    pub fn enter(&mut self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(
            std::io::stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            cursor::Hide
        )?;
        Ok(())
    }
    pub fn exit(&mut self) -> Result<()> {
        self.stop()?;
        if crossterm::terminal::is_raw_mode_enabled()? {
            self.flush()?;
            crossterm::execute!(
                std::io::stdout(),
                LeaveAlternateScreen,
                DisableMouseCapture,
                cursor::Show
            )?;
            crossterm::terminal::disable_raw_mode()?;
        }
        Ok(())
    }
    pub fn stop(&mut self) -> Result<()> {
        self.cancel();
        let mut counter = 0;
        while !self.tasks.is_finished() {
            std::thread::sleep(Duration::from_millis(1));
            counter += 1;
            if counter > 50 {
                self.tasks.abort();
            }
            if counter > 100 {
                log::error!("Failed to abort task in 100 milliseconds for unknown reason");
                break;
            }
        }
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.cancel_token.cancel();
    }
    fn in_prompt(&self) -> bool {
        !self.widgets.is_empty()
    }

    fn render(&mut self, state: &State) {
        // ignore any failure
        let prompt_string = self.prompt_string.clone();
        let widget = self
            .widgets
            .last()
            .map(|w| make_render_widget(w, prompt_string));
        let _ = self.draw(|f| ui(f, state, widget));
    }
    async fn handle_tui_event(&mut self, event: crossterm::event::Event) -> Option<MyEvents> {
        use crossterm::event;
        match event {
            event::Event::FocusGained => Some(Action::PauseRender(false).into()),
            event::Event::FocusLost => Some(Action::PauseRender(true).into()),
            event::Event::Key(key) => {
                if !self.widgets.is_empty() {
                    self.widget_event(key).await;
                    None
                } else if key.kind == KeyEventKind::Press {
                    let action = config::get_config().get_action(&key.code)?;
                    Some(action.into())
                } else {
                    None
                }
            }
            event::Event::Mouse(event) => match event.kind {
                event::MouseEventKind::Down(_) => None, // TODO handle mouse click
                event::MouseEventKind::ScrollDown => {
                    self.offset -= 1;
                    None
                }
                event::MouseEventKind::ScrollUp => {
                    self.offset += 1;
                    None
                }
                _ => None,
            },
            event::Event::Paste(string) => {
                if self.in_prompt() {
                    self.prompt_string.push_str(&string)
                };
                None
            }
            event::Event::Resize(_, _) => None,
        }
    }

    async fn handle_widget_send(&mut self) {
        let widget = self.widgets.pop().unwrap();
        match widget {
            Widget::Widget(widget) => match widget {
                crate::client::interface::Widget::Alert { .. } => todo!(),
                crate::client::interface::Widget::Checkboxes { .. } => todo!(),
                crate::client::interface::Widget::Radioboxes { .. } => todo!(),
                crate::client::interface::Widget::PromptBox {
                    title: _,
                    content: _,
                    backchannel,
                } => {
                    let _ = backchannel.send(self.prompt_string.clone());
                }
            },
            Widget::CommandPrompt => {
                let _ = self
                    .orchestrator_tx
                    .send(MyEvents::Command(self.prompt_string.clone()))
                    .await;
                self.prompt_string = String::new();
            }
        }
    }

    async fn widget_event(&mut self, key: crossterm::event::KeyEvent) {
        if key.kind == KeyEventKind::Press {
            match key.code {
                KeyCode::Char(c) => {
                    if self.widgets.last().unwrap().captures_output() {
                        self.prompt_string.push(c);
                    }
                }
                KeyCode::Enter => self.handle_widget_send().await,
                KeyCode::Backspace => {
                    if self.widgets.last().unwrap().captures_output() {
                        self.prompt_string.pop();
                    }
                }
                KeyCode::Esc => {
                    self.widgets.pop();
                    self.prompt_string = String::new()
                }
                _ => (),
            }
        }
    }
}
fn centered_rec(size: Rect, max_height: Option<u16>) -> Rect {
    let center_x = size.width / 2;
    let center_y = size.height / 2;
    let width = size.width * 3 / 4;
    let height = max_height.unwrap_or(size.height * 3 / 4);
    let corner_x = center_x - (width / 2);
    let corner_y = center_y - (height / 2);
    Rect {
        x: corner_x,
        y: corner_y,
        width,
        height,
    }
}
fn make_list_widget<'a>(list: &'a [String], title: &'a str, focused: bool) -> List<'a> {
    let list: Vec<ListItem<'_>> = list.iter().map(|s| ListItem::new(s.clone())).collect();
    let style = get_style(focused);
    let hg_style = get_highlight_style(focused);
    List::new(list)
        .block(
            Block::new()
                .borders(Borders::ALL)
                .title(title)
                .style(get_border_style(focused)),
        )
        .style(style)
        .highlight_style(hg_style)
}

fn get_border_style(focused: bool) -> Style {
    let config: Config = confy::load("yamav3", None).expect("Cannot access config");
    let fg = if focused {
        config.border_focus
    } else {
        config.border_unfocus
    };
    Style::default().fg(fg)
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

fn ui(f: &mut Frame<'_>, state: &State, widget: Option<RenderWidget>) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("YAMA")
        .title_alignment(Alignment::Center)
        .border_type(BorderType::Rounded);
    f.render_widget(block, f.size());
    let player_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Percentage(80), Constraint::Max(4)])
        .margin(1)
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
    render_sources_widget(f, left_column[0], state);
    render_playlist_widget(f, left_column[1], state);
    render_song_widget(f, layout[1], state);
    render_info_widget(f, left_column[2], state);
    render_player_widget(f, player_layout[1], state);
    if let Some(widget) = widget {
        render_widget(f, widget)
    }
}
fn render_widget(f: &mut Frame<'_>, widget: RenderWidget) {
    let popup = Block::default()
        .title(widget.title)
        .borders(Borders::ALL)
        .style(Style::default())
        .bg(Color::Reset);
    let mut text = widget.content.clone();
    if let Some(prompt) = widget.prompt {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&prompt)
    }
    let text = Paragraph::new(text)
        .block(popup.clone())
        .wrap(Wrap { trim: true });
    let area = centered_rec(f.size(), widget.max_height);
    let area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(100)])
        .split(area);
    f.render_widget(Clear, area[0]); // clear background
    f.render_widget(text, area[0]);
}
fn render_sources_widget(f: &mut Frame, layout: Rect, state: &State) {
    let mut names = state.clients.get_strings();
    if let Some(player) = state.active_player {
        names[player].push_str(" ");
    }
    let mut tui_state = ListState::default();
    tui_state.select(state.clients.select);
    let widget = make_list_widget(&names, "Sources", state.is_active_menu(Menu::Client));
    f.render_stateful_widget(widget, layout, &mut tui_state)
}
fn render_playlist_widget(f: &mut Frame<'_>, layout: Rect, state: &State) {
    //let playlists = &state.playlists.get_strings();
    let playlists: &Vec<String> = &state
        .playlists
        .entries
        .iter()
        .map(|p| format!("{} ({}/{})", p.title.clone(), p.songs.len(), p.length))
        .collect();
    let mut tui_state = ListState::default();
    tui_state.select(state.playlists.select);
    let widget = make_list_widget(playlists, "Playlists", state.is_active_menu(Menu::Playlist));
    f.render_stateful_widget(widget, layout, &mut tui_state);
}
fn render_song_widget(f: &mut Frame<'_>, layout: Rect, state: &State) {
    let songs = &state.songs.get_strings();
    let mut tui_state = ListState::default();
    tui_state.select(state.songs.select);
    let title = if let Some(select) = state.playlists.get_selected() {
        &select.title
    } else {
        "Songs"
    };
    let widget = make_list_widget(songs, title, state.is_active_menu(Menu::Song));
    f.render_stateful_widget(widget, layout, &mut tui_state);
}
fn render_info_widget(f: &mut Frame<'_>, layout: Rect, state: &State) {
    let player = &state.player;
    let info = vec![
        format!("Auto: {}", player.autoplay),
        format!("Repeat: {}", player.repeat),
        format!("Shuffle: {}", player.shuffled),
        format!("Volume: {}/100", player.volume),
    ];
    let widget = make_list_widget(&info, "Options", true);
    f.render_widget(widget, layout);
}

/// Convert `dur` to string in the format `HH:MM:SS` if duration is longer than an hour otherwise
/// converts to `MM:SS`
fn duration_to_string(dur: &Duration) -> String {
    let secs = dur.as_secs();
    let mins = secs / 60;
    let hours = mins / 60;
    if hours >= 1 {
        // if more than an hour
        format!("{:0>2}:{:0>2}:{:0>2}", hours, mins % 60, secs % 60)
    } else {
        format!("{:0>2}:{:0>2}", mins % 60, secs % 60)
    }
}
fn build_player_string(pos: &Duration, dur: &Duration, length: usize) -> String {
    let pos = pos.as_secs();
    let dur = dur.as_secs();
    if length <= 2 || dur == 0 || pos > dur {
        String::new()
    } else {
        let ratio: f32 = pos as f32 / dur as f32;
        let bascule = (length as f32 * ratio).floor() as usize;
        let mut res: Vec<char> = Vec::with_capacity(length);
        for _ in 0..bascule {
            res.push('█')
        }
        for _ in bascule..length {
            res.push('─')
        }
        res[0] = '├';
        res[length - 1] = '┤';
        // from vec to string
        res.iter().collect()
    }
}
fn render_player_widget(f: &mut Frame<'_>, layout: Rect, state: &State) {
    let block = Block::new()
        .borders(Borders::ALL)
        .title("Player Informations");
    let duration = if let Some(song) = state.player.song_info.clone() {
        song.duration
    } else {
        Default::default()
    };
    let title = state.player.song_info.clone().unwrap_or_default().title;
    let player_string = build_player_string(
        &state.player.position,
        &duration,
        (layout.width.checked_sub(2).unwrap_or_default()) as usize,
    );
    let position = duration_to_string(&state.player.position);
    let duration = duration_to_string(&duration);
    let text = Paragraph::new(format!(
        "{}/{} {}\n{}",
        position, duration, title, player_string
    ))
    .block(block);
    f.render_widget(text, layout)
}
fn make_render_widget(widget: &Widget, prompt_string: String) -> RenderWidget {
    match widget {
        Widget::Widget(widget) => match widget {
            InterfaceWidget::Alert { title, content } => RenderWidget {
                title: title.clone(),
                content: content.clone(),
                prompt: None,
                max_height: None,
            },
            InterfaceWidget::Checkboxes { .. } => todo!(),
            InterfaceWidget::Radioboxes { .. } => todo!(),
            InterfaceWidget::PromptBox { title, content, .. } => RenderWidget {
                title: title.clone(),
                content: content.clone(),
                prompt: Some(prompt_string.clone()),
                max_height: None,
            },
        },
        Widget::CommandPrompt => RenderWidget {
            title: "Command Prompt".to_string(),
            content: String::new(),
            prompt: Some(prompt_string.clone()),
            max_height: Some(3),
        },
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        self.exit().unwrap()
    }
}

impl Deref for Tui {
    type Target = ratatui::Terminal<Backend<std::io::Stderr>>;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl DerefMut for Tui {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}
