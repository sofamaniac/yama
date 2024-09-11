use std::sync::Mutex;

use crossterm::event::{Event, KeyCode, KeyEvent};
use futures::StreamExt;
use protocol::playback::{SeekMode, VolumeDelta, VolumeSetter};
use protocol::{Action, Duration};
use protocol::{DataType, NotificationId};

use color_eyre::Result;
use ratatui::crossterm::event::EventStream;
use ratatui::layout::{Constraint, Flex, Layout};
use ratatui::widgets::{Block, Clear, List, ListState, Paragraph, Widget};
use ratatui::{DefaultTerminal, Frame};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

mod player_widget;
mod source;
use source::Source;

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
    notifications: Mutex<Vec<(NotificationId, Notification)>>,
    redraw_sender: mpsc::Sender<()>,
    redraw_receiver: mpsc::Receiver<()>,
    current_menu: Menu,
}

impl App {
    /// Number of frames per second
    const FRAMERATE: u64 = 5;
    pub async fn new() -> Self {
        let cancel_token = CancellationToken::new();
        let (ui_tx, ui_rx) = tokio::sync::mpsc::channel(100);
        let (yt_tx, yt_rx) = tokio::sync::mpsc::channel(100);
        let yt_cancel_token = cancel_token.child_token();
        tokio::task::spawn(async move {
            let yt = youtube::Handler::new(yt_rx, ui_tx, yt_cancel_token).await;
            yt.run().await;
        });
        let (redraw_tx, redrax_rx) = tokio::sync::mpsc::channel(10);
        Self {
            should_quit: cancel_token,
            in_channel: ui_rx,
            youtube: Source::new(yt_tx, String::from("youtube")),
            focused: true,
            notifications: Mutex::new(Vec::new()),
            redraw_sender: redraw_tx,
            redraw_receiver: redrax_rx,
            current_menu: Menu::Playlist,
        }
    }

    pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_millis(1000 / Self::FRAMERATE));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut event = EventStream::new();
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
        let (playlists, p_state) = self.youtube.make_playlist_widget().await;
        let (songs, s_state) = self.youtube.make_song_widget().await;
        let options = self.youtube.option_widget().await;
        let player = self.youtube.player_widget().await;
        let _ = terminal
            .draw(|frame| self.draw(frame, playlists, songs, p_state, s_state, options, player));
    }
    fn draw(
        &self,
        frame: &mut Frame<'_>,
        playlists: List,
        songs: List,
        mut playlist_state: ListState,
        mut song_state: ListState,
        options: impl Widget,
        player: impl Widget,
    ) {
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

        let sources_block = Block::bordered().title("Sources");

        frame.render_widget(Clear, frame.area());
        frame.render_widget(outer_block, frame.area());
        frame.render_widget(sources_block, sources);
        frame.render_widget(options, options_area);
        frame.render_widget(player, player_area);
        frame.render_stateful_widget(playlists, playlists_area, &mut playlist_state);
        frame.render_stateful_widget(songs, songs_area, &mut song_state);

        self.draw_notification(frame);
    }

    fn draw_notification(&self, frame: &mut Frame<'_>) {
        if let Some((_notif_id, notif)) = self.notifications.lock().unwrap().last() {
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
                    let _ = response.send(res);
                }
                protocol::UICommand::CloseNotification(notification_id) => {
                    self.notifications
                        .lock()
                        .unwrap()
                        .retain(|&(notif_id, _)| notif_id != notification_id);
                    let _ = response.send(Ok(().into()));
                }
            },
            _ => (),
        }
    }
    fn new_notification(&mut self, message: String) -> NotificationId {
        let notif_id = NotificationId::new();
        self.notifications
            .lock()
            .unwrap()
            .push((notif_id, Notification(message)));
        notif_id
    }

    async fn handle_event(&mut self, event: &Event) {
        match event {
            Event::FocusGained => self.focused = true,
            Event::FocusLost => self.focused = false,
            Event::Key(event) => self.handle_key_event(event).await,
            Event::Mouse(_) => todo!(),
            Event::Paste(_) => todo!(),
            Event::Resize(_, _) => {
                self.redraw_sender.send(()).await;
            }
        }
    }

    async fn handle_key_event(&mut self, event: &KeyEvent) {
        match event.code {
            KeyCode::Char('q') => self.should_quit.cancel(),
            KeyCode::Char('j') => match self.current_menu {
                Menu::Song => {
                    self.youtube.increase_song();
                }
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
            KeyCode::Char('a') => self.youtube.cycle_autoplay().await,
            KeyCode::Char(' ') => self.youtube.toggle_pause().await,

            KeyCode::Char('d') => {
                self.youtube
                    .set_volume(VolumeSetter::Relative(VolumeDelta::new(-5)))
                    .await
            }
            KeyCode::Char('f') => {
                self.youtube
                    .set_volume(VolumeSetter::Relative(VolumeDelta::new(5)))
                    .await
            }
            KeyCode::Char('<') => self.youtube.previous_song().await,
            KeyCode::Char('>') => self.youtube.next_song().await,
            KeyCode::Char('y') => self.youtube.cycle_shuffle().await,
            KeyCode::Char('r') => self.youtube.cycle_repeat().await,
            KeyCode::Left => {
                self.youtube
                    .seek(protocol::playback::SeekMode::Backward(new_delta_duration(
                        5,
                    )))
                    .await
            }
            KeyCode::Right => {
                self.youtube
                    .seek(SeekMode::Forward(new_delta_duration(5)))
                    .await
            }
            _ => (),
        }
        let _ = self.redraw_sender.send(()).await;
    }
}

fn new_delta_duration(second: u32) -> Duration {
    Duration::YMDHMS {
        year: 0,
        month: 0,
        day: 0,
        hour: 0,
        minute: 0,
        second,
        millisecond: 0,
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
