mod player;
mod youtube;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use player::Player;
use protocol::{playlist::PlaylistId, Action, DataType, FullPlaylist, Playlist, UICommand};

use tokio::{
    sync::{mpsc, RwLock},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use youtube::{get_all_playlists, get_playlist, Source, YtPlaylist};

pub struct Handler {
    youtube: Source,
    playlists: Arc<Mutex<HashMap<PlaylistId, YtPlaylist>>>,
    loading_playlists: bool,
    out_channel: mpsc::Sender<Action>,
    in_channel: mpsc::Receiver<Action>,
    should_quit: CancellationToken,
    tasks: JoinSet<()>,
    player: Player,
}

impl Handler {
    pub async fn new(
        out_channel: mpsc::Sender<Action>,
        cancel_token: CancellationToken,
    ) -> (Self, mpsc::Sender<Action>) {
        let (in_tx, in_rx) = mpsc::channel(100);
        let youtube = Source::new(out_channel.clone()).await;
        (
            Self {
                youtube,
                playlists: Arc::new(Mutex::new(HashMap::new())),
                loading_playlists: false,
                out_channel,
                in_channel: in_rx,
                tasks: JoinSet::new(),
                should_quit: cancel_token,
                player: Player::new(),
            },
            in_tx,
        )
    }
    pub async fn run(mut self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(3000));
        loop {
            tokio::select! {
                _ = self.should_quit.cancelled() => break,
                Some(action) = self.in_channel.recv() => {
                    self.handle_action(action).await
                }
                _ = interval.tick() => {
                    self.player.update();
                }
                Some(_) = self.tasks.join_next() => {}
            }
        }
    }
    async fn handle_action(&mut self, action: Action) {
        let Action { command, response } = action;
        let res: DataType = match command {
            protocol::Command::Refresh => todo!(),
            protocol::Command::Restart => todo!(),
            protocol::Command::Quit => {
                self.should_quit.cancel();
                ().into()
            }
            protocol::Command::Playlist(command) => {
                let res = self.handle_playlist_command(command).await;
                // Close open notification after successfull interacton with youtube api
                if let Ok(res) = res {
                    let mut youtube_notif_id = self.youtube.connection_notification_id.lock().await;
                    if let Some(notif_id) = *youtube_notif_id {
                        let action = UICommand::close_notification(notif_id);
                        // We ignore the result
                        let _ = action.send(&self.out_channel).await;
                        *youtube_notif_id = None;
                    }
                    res
                } else {
                    ().into()
                }
            }
            protocol::Command::Playback(command) => self.handle_player_command(command).await,
            protocol::Command::UI(_) => ().into(),
        };
        response.send(Ok(res));
    }

    pub async fn handle_playlist_command(
        &mut self,
        command: protocol::playlist::Command,
    ) -> protocol::Result<DataType> {
        match command {
            protocol::playlist::Command::ListAll => {
                let playlists = self.playlists.lock().unwrap();
                if !playlists.is_empty() {
                    self.loading_playlists = false;
                    let list: Vec<protocol::playlist::Playlist> = playlists
                        .values()
                        .map(|fp| fp.playlist.playlist().to_owned())
                        .collect();
                    let list: Arc<[protocol::Playlist]> = list.into();
                    Ok(list.into())
                } else {
                    drop(playlists);
                    if !self.loading_playlists {
                        self.loading_playlists = true;
                        let handler_playlists = self.playlists.clone();
                        let hub = self.youtube.hub.clone();
                        self.tasks.spawn(async move {
                            let playlists = get_all_playlists(&hub).await;
                            let mut handler_playlists = handler_playlists.lock().unwrap();
                            for playlist in playlists {
                                handler_playlists.insert(playlist.id().clone(), playlist);
                            }
                        });
                    }
                    Err(protocol::Error::Loading)
                }
            }
            protocol::playlist::Command::Get(playlist) => self.get_playlist(playlist).await,
            protocol::playlist::Command::Add(_, _) => todo!(),
            protocol::playlist::Command::Remove(_, _) => todo!(),
            protocol::playlist::Command::Delete(_) => todo!(),
            protocol::playlist::Command::Create(_) => todo!(),
        }
    }

    async fn handle_player_command(&mut self, command: protocol::playback::Command) -> DataType {
        match command {
            protocol::playback::Command::SetVolume(volume) => {
                self.player.set_volume(volume);
                ().into()
            }
            protocol::playback::Command::SetShuffle(shuffle) => self.player.shuffle(shuffle).into(),
            protocol::playback::Command::SetAutoplay(autoplay) => {
                self.player.set_autoplay(autoplay);
                ().into()
            }
            protocol::playback::Command::SetRepeat(repeat) => self.player.set_repeat(repeat).into(),
            protocol::playback::Command::Play(_) => todo!(),
            protocol::playback::Command::SetPause(value) => {
                if value {
                    self.player.pause()
                } else {
                    self.player.unpause()
                };
                ().into()
            }
            protocol::playback::Command::PlayPause => {
                self.player.playpause();
                ().into()
            }
            protocol::playback::Command::Stop => todo!(),
            protocol::playback::Command::Seek(mode) => self.player.seek(mode).into(),
            protocol::playback::Command::NextSong => self.player.next().into(),
            protocol::playback::Command::PreviousSong => self.player.previous().into(),
            protocol::playback::Command::AddToQueue(queue) => match queue {
                protocol::Queue::Songs(songs) => self.player.set_playlist(songs).into(),
                protocol::Queue::Playlist(playlist) => {
                    if let DataType::Playlist(playlist) = self
                        .handle_playlist_command(protocol::playlist::Command::Get(playlist))
                        .await
                        .unwrap()
                    {
                        let songs: Arc<[protocol::Song]> = playlist.songs().into();
                        self.player.set_playlist(songs);
                    }
                    ().into()
                }
            },
            protocol::playback::Command::GetQueue => todo!(),
            protocol::playback::Command::GetInfo => self.player.info().into(),
        }
    }

    async fn get_playlist(&mut self, playlist: Playlist) -> protocol::Result<DataType> {
        let mut lock = self.playlists.lock().unwrap();
        if let Some(yt_playlist) = lock.get_mut(playlist.id()) {
            if yt_playlist.fully_loaded {
                return Ok(Arc::new(yt_playlist.playlist.clone()).into());
            } else if !yt_playlist.loading {
                yt_playlist.loading = true;
                let handler_playlists = self.playlists.clone();
                let hub = self.youtube.hub.clone();
                let playlist = lock.get(playlist.id()).cloned();
                self.tasks.spawn(async move {
                    if let Some(playlist) = playlist {
                        let playlist = playlist.playlist.playlist().clone();
                        if let Some(playlist) = get_playlist(hub, playlist).await {
                            let mut handler_playlists = handler_playlists.lock().unwrap();
                            let playlist_mut = handler_playlists.get_mut(playlist.id()).unwrap();
                            playlist_mut.playlist = playlist;
                            playlist_mut.loading = false;
                            playlist_mut.fully_loaded = true;
                        }
                    }
                });
            }
        }
        Err(protocol::Error::Loading)
    }
}
