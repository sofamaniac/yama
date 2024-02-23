use std::{fs, path::PathBuf, time::Duration};

use log::debug;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use crate::{
    client::interface::{Answer, GetRequest, PlaylistInfo, Request, SongInfo},
    config,
};

pub struct Backend {
    request_rx: broadcast::Receiver<Request>,
    answer_tx: mpsc::Sender<Answer>,
    cancel_token: CancellationToken,
    folders: Vec<PlaylistInfo>,
}

impl Backend {
    pub fn init(
        request_rx: broadcast::Receiver<Request>,
        answer_tx: mpsc::Sender<Answer>,
        cancel_token: CancellationToken,
    ) -> Self {
        let config = config::get_config();
        let folders = config.folders;
        debug!("Folders to scan {:?}", folders);
        let folders = find_subfolders(folders);
        let folders = folders
            .iter()
            .map(get_playlist)
            .filter(|p| p.length > 0)
            .collect();
        Self {
            request_rx,
            answer_tx,
            cancel_token,
            folders,
        }
    }

    pub async fn main_loop(&mut self) {
        let delay = Duration::from_millis(100);
        let mut interval = tokio::time::interval(delay);
        while !self.cancel_token.is_cancelled() {
            use tokio::sync::broadcast::error;
            match self.request_rx.try_recv() {
                Ok(request) => self.handle_request(request).await,
                Err(err) => match err {
                    error::TryRecvError::Empty => (),
                    error::TryRecvError::Closed => self.cancel_token.cancel(),
                    error::TryRecvError::Lagged(_) => {
                        // resubscribe to broadcast ignoring all messages
                        // pending
                        self.request_rx = self.request_rx.resubscribe()
                    }
                },
            }
            interval.tick().await;
        }
    }

    async fn handle_request(&self, request: Request) {
        match request {
            Request::PlayerAction(_) => (),
            Request::Get(request) => self.handle_get(request).await,
            Request::Set(_) => todo!(),
            Request::Command(_) => (),
        }
    }

    async fn handle_get(&self, request: GetRequest) {
        match request {
            GetRequest::PlaylistList => {
                let _ = self
                    .answer_tx
                    .send(Answer::PlaylistList(self.folders.clone()))
                    .await;
            }
            GetRequest::Playlist(id) => {
                let playlist = self.folders.iter().find(|p| p.id == id).unwrap().clone();
                let _ = self.answer_tx.send(Answer::Playlist(playlist)).await;
            }
            GetRequest::PlayerInfo => (),
        }
    }
}

fn find_subfolders(folders: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut res: Vec<PathBuf> = folders.clone();
    for folder in folders {
        if let Ok(files) = fs::read_dir(folder) {
            let files: Vec<std::fs::DirEntry> = files.filter_map(|s| s.ok()).collect();
            for path in files {
                if let Ok(ft) = path.file_type() {
                    if ft.is_dir() {
                        res.push(path.path())
                    }
                }
            }
        }
    }

    res
}

fn get_playlist(folder: &PathBuf) -> PlaylistInfo {
    if let Ok(files) = fs::read_dir(folder) {
        let songs: Vec<SongInfo> = files.filter_map(|s| s.ok()).filter_map(get_song).collect();
        PlaylistInfo {
            title: folder
                .file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap()
                .to_string(),
            length: songs.len(),
            cover_url: Default::default(),
            songs,
            id: folder.display().to_string(),
        }
    } else {
        debug!("Checking folder {:?} failed", folder);
        Default::default()
    }
}

fn get_song(path: std::fs::DirEntry) -> Option<SongInfo> {
    debug!("checking {:?}", path);
    if let Ok(ft) = path.file_type() {
        if ft.is_file() {
            let path = path.path();
            make_song(&path)
        } else {
            None
        }
    } else {
        None
    }
}

fn make_song(path: &PathBuf) -> Option<SongInfo> {
    // TODO get artist and cover url
    if let Ok(song) = metadata::media_file::MediaFileMetadata::new(path) {
        let abs_path = fs::canonicalize(song.path.clone()).unwrap();
        Some(SongInfo {
            title: song.title.unwrap_or(song.file_name.clone()),
            artist: Default::default(),
            cover_url: Default::default(),
            id: song.file_name,
            url: format!("file://{}", abs_path.display()),
            duration: Duration::from_secs_f64(song._duration.unwrap_or_default()),
        })
    } else {
        None
    }
}
