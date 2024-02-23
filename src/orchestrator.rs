use std::{
    ops::{Deref, DerefMut},
    time::Duration,
};

use anyhow::Result;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio_util::sync::CancellationToken;

use crate::{
    client::interface::{
        Answer, GetRequest, PlayerAction, PlayerInfo, PlaylistInfo, Request, SongInfo,
    },
    tui,
};

#[derive(Debug)]
pub struct Client {
    /// name displayed
    name: String,
    /// channel used to receive requests from front end
    sender: Sender<Request>,
    /// channel used to receive answers from backend
    receiver: Receiver<Answer>,
    /// channel used to send event to `Orchestrator`
    event_tx: Sender<MyEvents>,

    // cache
    playlists_info: Vec<PlaylistInfo>,
    player_info: PlayerInfo,
}

/// Interface between the front end and one backend
impl Client {
    pub fn new(
        name: String,
        sender: Sender<Request>,
        receiver: Receiver<Answer>,
        event_tx: Sender<MyEvents>,
    ) -> Self {
        Self {
            name,
            sender,
            receiver,
            event_tx,
            playlists_info: Default::default(),
            player_info: Default::default(),
        }
    }
    pub async fn update(&mut self) {
        while let Ok(msg) = self.receiver.try_recv() {
            // read all messages received
            self.handle_answer(msg).await;
        }
    }
    pub async fn handle_answer(&mut self, msg: Answer) {
        match msg {
            Answer::PlayerInfo(info) => {
                self.player_info = info;
                // ignore the error if the orchestrator has dropped the connection
                let _ = self.event_tx.send(MyEvents::RefreshPlayerState).await;
            }
            Answer::PlaylistList(list_info) => self.playlists_info = list_info,
            Answer::Playlist(playlist_info) => {
                let id = playlist_info.id.clone();
                let maybe_index = self.playlists_info.iter().position(|p| p.id == id);
                if let Some(index) = maybe_index {
                    self.playlists_info[index] = playlist_info;
                } else {
                    self.playlists_info.push(playlist_info)
                }
            }
            Answer::Widget(widget) => {
                let _ = self.event_tx.send(MyEvents::Widget(widget)).await;
            }
            Answer::Ok => todo!(),
        }
    }
    pub async fn update_playlistlist(&mut self) {
        let request: Request = GetRequest::PlaylistList.into();
        // ignore the fact that backend has dropped connection
        let _ = self.send(request).await;
    }
    pub fn get_playlists(&self) -> Vec<PlaylistInfo> {
        self.playlists_info.clone()
    }
    pub async fn update_playlist(&mut self, index: Option<usize>) {
        if index.is_none() {
            return;
        }
        let playlist = index.unwrap();
        let request: Request =
            GetRequest::Playlist(self.playlists_info[playlist].id.clone()).into();
        // ignore the fact that backend has dropped connection
        let _ = self.send(request).await;
    }
    pub fn get_playlist(&self, playlist: Option<usize>) -> PlaylistInfo {
        if let Some(playlist) = playlist {
            self.playlists_info[playlist].clone()
        } else {
            Default::default()
        }
    }
    pub fn get_songs(&self, playlist: Option<usize>) -> Vec<SongInfo> {
        if let Some(playlist) = playlist {
            self.playlists_info[playlist].songs.clone()
        } else {
            Default::default()
        }
    }

    async fn update_player_info(&self) {
        let _ = self.send(Request::Get(GetRequest::PlayerInfo)).await;
    }

    fn get_player_info(&self) -> PlayerInfo {
        self.player_info.clone()
    }
}
impl Deref for Client {
    type Target = Sender<Request>;

    fn deref(&self) -> &Self::Target {
        &self.sender
    }
}

impl DerefMut for Client {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sender
    }
}

#[derive(Debug, Default, PartialEq, Clone, Copy)]
pub enum Menu {
    #[default]
    Client,
    Playlist,
    Song,
}

#[derive(Debug, Default, Clone)]
pub struct ListHolder<T> {
    pub entries: Vec<T>,
    pub select: Option<usize>,
}

pub trait ListHolderToString {
    fn get_strings(&self) -> Vec<String> {
        Vec::default()
    }
}

impl<T> ListHolder<T> {
    pub fn select(&mut self, select: Option<usize>) {
        self.select = select;
    }
    pub fn offset(&mut self, off: isize) {
        if self.entries.is_empty() {
            self.select(None);
            return;
        }
        if self.select.is_none() {
            if off >= 0 && (off as usize) < self.entries.len() {
                self.select(Some(off as usize))
            }
        } else if let Some(i) = self.select.unwrap().checked_add_signed(off) {
            // len is not 0
            self.select = Some(i.min(self.entries.len() - 1));
        }
    }
    pub fn get_selected(&self) -> Option<&T> {
        let select = self.select?;
        Some(&self.entries[select])
    }
}
impl<T: ToString> ListHolderToString for ListHolder<T> {
    fn get_strings(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.to_string()).collect()
    }
}
impl ListHolderToString for ListHolder<PlaylistInfo> {
    fn get_strings(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.title.clone()).collect()
    }
}
impl ListHolderToString for ListHolder<SongInfo> {
    fn get_strings(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.title.clone()).collect()
    }
}
#[derive(Debug, Default, Clone)]
pub struct State {
    pub clients: ListHolder<String>,
    pub playlists: ListHolder<PlaylistInfo>,
    pub songs: ListHolder<SongInfo>,
    /// list of alerts to display
    pub alerts: Vec<String>,
    /// current state of active player
    pub player: PlayerInfo,
    /// index of active player if any
    pub active_player: Option<usize>,
    /// current menu
    pub active_menu: Menu,
}

impl State {
    pub fn go_next_menu(&mut self) {
        self.active_menu = match self.active_menu {
            Menu::Client => Menu::Playlist,
            Menu::Playlist => Menu::Song,
            Menu::Song => Menu::Song,
        }
    }
    pub fn go_prev_menu(&mut self) {
        self.active_menu = match self.active_menu {
            Menu::Client => Menu::Client,
            Menu::Playlist => Menu::Client,
            Menu::Song => Menu::Playlist,
        }
    }
    pub fn is_active_menu(&self, menu: Menu) -> bool {
        self.active_menu == menu && self.alerts.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Copy, PartialEq, Eq)]
pub enum MenuCtrl {
    Next,
    Prev,
    NextMenu,
    PrevMenu,
    Offset(isize),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Action {
    Render,
    PauseRender(bool),
    Player(PlayerAction),
    Menu(MenuCtrl),
    Alert(String),
    ToggleAuto,
    CloseAlert,
    CommandPrompt,
    Quit,
    Update,
    GoToCurrent,
}

impl From<PlayerAction> for Action {
    fn from(value: PlayerAction) -> Self {
        Self::Player(value)
    }
}
impl From<MenuCtrl> for Action {
    fn from(value: MenuCtrl) -> Self {
        Self::Menu(value)
    }
}

#[derive(Debug)]
pub enum MyEvents {
    RefreshPlayerState,
    Action(Action),
    Command(String),
    Widget(crate::client::interface::Widget),
}
impl From<Action> for MyEvents {
    fn from(value: Action) -> Self {
        Self::Action(value)
    }
}
impl From<PlayerAction> for MyEvents {
    fn from(value: PlayerAction) -> Self {
        Self::Action(value.into())
    }
}
impl From<MenuCtrl> for MyEvents {
    fn from(value: MenuCtrl) -> Self {
        Self::Action(value.into())
    }
}
pub struct OrchestratorBuilder {
    clients: Vec<Client>,
    #[cfg(feature = "mpris")]
    dbus: Option<Sender<PlayerInfo>>,
    event_rx: Receiver<MyEvents>,
    event_tx: Sender<MyEvents>,
    tui_tx: Option<Sender<crate::tui::Event>>,
    cancel_token: CancellationToken,
}

impl OrchestratorBuilder {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(32);
        #[cfg(feature = "mpris")]
        {
            Self {
                clients: Vec::new(),
                dbus: None,
                event_rx,
                event_tx,
                tui_tx: None,
                cancel_token: CancellationToken::new(),
            }
        }
        #[cfg(not(feature = "mpris"))]
        {
            Self {
                clients: Vec::new(),
                event_rx,
                event_tx,
                tui_tx: None,
                cancel_token: CancellationToken::new(),
            }
        }
    }
    pub fn get_event_tx(&self) -> Sender<MyEvents> {
        self.event_tx.clone()
    }
    pub fn get_cancel_token(&self) -> CancellationToken {
        self.cancel_token.clone()
    }
    pub fn add_client(
        &mut self,
        name: String,
        chan_tx: Sender<Request>,
        chan_rx: Receiver<Answer>,
    ) {
        self.clients
            .push(Client::new(name, chan_tx, chan_rx, self.event_tx.clone()))
    }
    #[cfg(feature = "mpris")]
    pub fn set_dbus(&mut self, dbus_sender: Sender<PlayerInfo>) {
        self.dbus = Some(dbus_sender);
    }
    pub fn set_tui(&mut self, tui_tx: Sender<crate::tui::Event>) {
        self.tui_tx = Some(tui_tx)
    }
    pub fn build(self) -> Orchestrator {
        let tui = self.tui_tx.expect("No TUI provided");
        let clients = self.clients.iter().map(|c| c.name.clone()).collect();
        let clients = ListHolder {
            entries: clients,
            select: None,
        };
        let state = State {
            clients,
            ..Default::default()
        };
        Orchestrator {
            clients: self.clients,
            #[cfg(feature = "mpris")]
            dbus: self.dbus.expect("No DBus channel provided"),
            event_rx: self.event_rx,
            tui_tx: tui,
            state,
            cancel_token: self.cancel_token,
            tui_refresh: true,
            timeout_duration: Duration::from_millis(100),
        }
    }
}

pub struct Orchestrator {
    clients: Vec<Client>,
    /// channel to send info on DBus
    #[cfg(feature = "mpris")]
    dbus: Sender<PlayerInfo>,
    event_rx: Receiver<MyEvents>,
    tui_tx: Sender<crate::tui::Event>,
    state: State,
    cancel_token: CancellationToken,
    // should the screen be refreshed ?
    tui_refresh: bool,
    // duration before timing out when sending something to the TUI, the DBus or a client
    timeout_duration: Duration,
}

impl Orchestrator {
    pub async fn run(&mut self) -> Result<()> {
        self.state.clients.select(Some(0));
        let cancel_token = self.cancel_token.clone();
        let mut update_interval = tokio::time::interval(std::time::Duration::from_millis(100));
        let mut refresh_interval = tokio::time::interval(Duration::from_secs(1));
        let mut state_update = tokio::time::interval(Duration::from_millis(500));
        loop {
            let update_delay = update_interval.tick();
            // time before refreshing state
            let refresh_delay = refresh_interval.tick();
            // time before updating state
            let state_delay = state_update.tick();
            tokio::select! {
                _ = cancel_token.cancelled() => break,
                maybe_event = self.event_rx.recv() => {
                    if let Some(event) = maybe_event {
                        self.handle_event(event).await;
                    }
                },
                _ = update_delay => {
                    self.update_clients().await;
                }
                _ = refresh_delay => {
                    self.refresh().await;
                }
                _ = state_delay => {
                    self.update_state().await;
                    self.send_dbus(self.state.player.clone()).await;
                    self.render().await;
                }
            }
        }
        Ok(())
    }
    /// Allow clients to check if they have received any message from their
    /// backend
    async fn update_clients(&mut self) {
        for c in self.clients.iter_mut() {
            c.update().await
        }
    }
    /// Request that the current client updates its data
    /// by querying the backend
    async fn refresh(&mut self) {
        let index = self.state.playlists.select;
        if let Some(client) = self.get_current_client_mut() {
            client.update_playlistlist().await;
            client.update_playlist(index).await;
        }
        if let Some(player) = self.get_active_player() {
            self.clients[player].update_player_info().await;
        }
        self.update_state().await;
    }
    fn get_current_client(&self) -> Option<&Client> {
        let client = self.state.clients.select?;
        Some(&self.clients[client])
    }
    fn get_current_client_mut(&mut self) -> Option<&mut Client> {
        let client = self.state.clients.select?;
        Some(&mut self.clients[client])
    }

    fn get_active_player(&self) -> Option<usize> {
        self.state.active_player
    }
    async fn update_state(&mut self) {
        if let Some(player) = self.get_active_player() {
            self.clients[player].update().await;
            let player_info = self.clients[player].get_player_info();
            self.state.player = player_info;
        }
        if let Some(client) = self.state.clients.select {
            self.clients[client].update().await;
            let select = self.state.playlists.select;
            self.state.playlists.entries = self.clients[client].get_playlists();
            self.state.songs.entries = self.clients[client].get_songs(select);
        }
    }
    async fn send_dbus(&self, info: PlayerInfo) {
        // ignore errors when sending to dbus
        #[cfg(feature = "mpris")]
        {
            let _ = self.dbus.send_timeout(info, self.timeout_duration).await;
        }
    }
    async fn handle_event(&mut self, event: MyEvents) {
        match event {
            MyEvents::RefreshPlayerState => {
                self.update_state().await;
                // immediatly notify dbus and tui of new state
                self.send_dbus(self.state.player.clone()).await;
                self.render().await;
            }
            MyEvents::Action(action) => self.handle_action(action).await,
            MyEvents::Widget(widget) => {
                let _ = self.tui_tx.send(tui::Widget::Widget(widget).into()).await;
            }
            MyEvents::Command(command) => {
                if let Some(client) = self.state.clients.select {
                    let _ = self.clients[client].send(Request::Command(command)).await;
                }
            }
        }
    }

    async fn handle_action(&mut self, action: Action) {
        match action {
            Action::Render => self.render().await,
            Action::PauseRender(val) => self.tui_refresh = val,
            Action::Player(action) => self.handle_player(action).await,
            Action::Menu(action) => self.handle_menu(action).await,
            Action::Quit => self.quit().await,
            Action::Update => self.update_state().await,
            Action::CloseAlert => {
                let _ = self.state.alerts.pop();
            }
            Action::Alert(alert) => self.state.alerts.push(alert),
            Action::ToggleAuto => self.toggle_auto().await,
            Action::GoToCurrent => self.select_playing(),
            Action::CommandPrompt => {
                let _ = self.tui_tx.send(tui::Widget::CommandPrompt.into()).await;
            }
        }
    }

    async fn render(&mut self) {
        if self.tui_refresh {
            match self
                .tui_tx
                .send_timeout(
                    tui::Event::Render(Box::new(self.state.clone())),
                    self.timeout_duration,
                )
                .await
            {
                Ok(_) => (),
                Err(mpsc::error::SendTimeoutError::Closed(_)) => self.quit().await, // if the tui has
                // crashed quit
                Err(mpsc::error::SendTimeoutError::Timeout(_)) => (), // ignore if timeout
            }
        }
    }

    async fn quit(&mut self) {
        self.cancel_token.cancel();
        self.event_rx.close();
        while self.event_rx.recv().await.is_some() {}
    }

    async fn handle_player(&mut self, action: PlayerAction) {
        // TODO: avoid multiple active player at once
        if let Some(player) = self.get_active_player() {
            // TODO send_timeout to player
            if self.clients[player].send(action.into()).await.is_err() {
                // if the player has crashed, drop the client
                self.clients.remove(player);
                return;
            }
            self.update_state().await;
            self.render().await;
        }
    }

    async fn handle_menu(&mut self, action: MenuCtrl) {
        match action {
            MenuCtrl::Next => self.offset(1),
            MenuCtrl::Prev => self.offset(-1),
            MenuCtrl::NextMenu => {
                self.state.go_next_menu();
                self.offset(0)
            }
            MenuCtrl::PrevMenu => {
                self.state.go_prev_menu();
                self.offset(0)
            }
            MenuCtrl::Offset(off) => self.offset(off),
        }
        self.refresh().await;
        self.render().await;
    }

    fn offset(&mut self, offset: isize) {
        match self.state.active_menu {
            Menu::Client => {
                self.state.clients.offset(offset);
                self.state.playlists.entries = self.get_current_client().unwrap().get_playlists();
                self.state.playlists.select = None;
            }
            Menu::Playlist => {
                self.state.playlists.offset(offset);
                if let Some(client) = self.get_current_client() {
                    self.state.songs.entries = client.get_songs(self.state.playlists.select);
                }
                self.state.songs.select = None;
            }
            Menu::Song => {
                self.state.songs.offset(offset);
            }
        }
    }
    async fn send_client(&mut self, index: usize, request: Request) {
        match self.clients[index]
            .send_timeout(request, self.timeout_duration)
            .await
        {
            Ok(_) => (),
            Err(mpsc::error::SendTimeoutError::Timeout(_)) => (),
            Err(mpsc::error::SendTimeoutError::Closed(_)) => {
                // the client has drop the connection
                self.clients.remove(index);
                self.state.clients.select = None;
            }
        }
    }

    async fn toggle_auto(&mut self) {
        if self.state.player.autoplay {
            if let Some(player) = self.get_active_player() {
                self.send_client(player, PlayerAction::Autoplay(false).into())
                    .await;
                // immediatly stop the active player when deactivating autoplay
                // ensures that there will be no collision
                self.send_client(player, PlayerAction::Stop.into()).await
            }
        } else if let Some(select) = self.state.playlists.select {
            self.state.active_player = self.state.clients.select;
            if let Some(client) = self.state.clients.select {
                let playlist = self.clients[client].get_playlist(Some(select));
                self.send_client(client, PlayerAction::SetTrackList(playlist).into())
                    .await;
                self.send_client(client, PlayerAction::Autoplay(true).into())
                    .await;
            }
        }
    }

    fn select_playing(&mut self) {
        if let Some(player) = self.get_active_player() {
            if let Some(index) = self.state.player.track_index {
                self.state.clients.select = Some(player);
                self.state.playlists.select = self
                    .state
                    .playlists
                    .entries
                    .iter()
                    .position(|p| p.id == self.state.player.tracklist.id);
                self.state.songs.select = Some(index);
                self.state.active_menu = Menu::Song;
            }
        }
    }
}
