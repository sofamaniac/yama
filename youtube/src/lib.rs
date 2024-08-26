use std::collections::HashMap;

use google_youtube3::{
    api::{Playlist, PlaylistItemListResponse, PlaylistListResponse},
    hyper, hyper_rustls,
    oauth2::{self, authenticator_delegate::InstalledFlowDelegate},
    YouTube,
};

use protocol::{playlist::PlaylistId, Action, UICommand};
use tokio::sync::mpsc::Sender;

type Result<T> = protocol::Result<T>;

#[derive(Debug)]
struct YtPlaylist(protocol::playlist::Playlist);
pub struct Source {
    hub: YouTube<HubType>,
    ui_channel: Sender<Action>,
    playlists: Vec<YtPlaylist>,
}
struct Authenticator {
    out_channel: Sender<Action>,
}

// type HubType = hyper::Client<hyper_rustls::HttpsConnector<hyper::client::HttpConnector>>;
type HubType = hyper_rustls::HttpsConnector<hyper::client::HttpConnector>;

impl Source {
    pub async fn new(ui_channel: Sender<Action>) -> Self {
        let secret =
            oauth2::read_application_secret("/home/sofamaniac/.config/yamav3/yt_secrets.json")
                .await
                .expect("Could not read secrets");
        let auth = oauth2::InstalledFlowAuthenticator::builder(
            secret,
            oauth2::InstalledFlowReturnMethod::HTTPRedirect,
        )
        .flow_delegate(Box::new(Authenticator {
            out_channel: ui_channel.clone(),
        }))
        .build()
        .await
        .unwrap();
        let hub = YouTube::new(
            hyper::Client::builder().build(
                hyper_rustls::HttpsConnectorBuilder::new()
                    .with_native_roots()
                    .unwrap()
                    .https_or_http()
                    .enable_http1()
                    .build(),
            ),
            auth,
        );
        Self {
            hub,
            ui_channel,
            playlists: Vec::new(),
        }
    }
    pub async fn get_all_playlists(&mut self) {
        let (_, playlists) = self
            .hub
            .playlists()
            .list(&vec![String::from("snippet")])
            .mine(true)
            .doit()
            .await
            .unwrap();
        let items = playlists.items.unwrap_or_default();
        self.playlists = items.into_iter().map(Into::into).collect();
        let (cmd, _) = UICommand::inform_user(format!("{:#?}", self.playlists));
        self.ui_channel.send(cmd).await;
    }
}

impl From<Playlist> for YtPlaylist {
    fn from(value: Playlist) -> Self {
        let snippet = value.snippet.unwrap();
        Self(protocol::playlist::Playlist::new(
            PlaylistId::new(value.id.unwrap()),
            snippet.title.unwrap(),
        ))
    }
}

impl InstalledFlowDelegate for Authenticator {
    fn present_user_url<'a>(
        &'a self,
        url: &'a str,
        need_code: bool,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = std::result::Result<String, String>> + Send + 'a>,
    > {
        Box::pin(self.make_url(url, need_code))
    }
}

impl Authenticator {
    async fn make_url<'a>(
        &'a self,
        url: &'a str,
        need_code: bool,
    ) -> std::result::Result<String, String> {
        // First we try to open the url in a browser
        let (cmd, back_channel) = UICommand::open_url(url.to_string());
        self.out_channel.send(cmd).await;
        if back_channel.await.is_ok() {
            return Ok(String::new());
        };
        // Ask the user to open the url in a browser
        let message = format!("Please go to {url} and follow the instructions");
        let (cmd, back_channel) = UICommand::inform_user(message);
        self.out_channel.send(cmd).await;
        if back_channel.await.is_ok() {
            return Ok(String::new());
        }
        if need_code {
            let message = format!("Please open {url} and copy back the code");
            let (cmd, back_channel) = UICommand::prompt_user(message);
            self.out_channel.send(cmd).await;
            // TODO: Handle code
            return Ok(String::new());
        }
        Ok(String::new())
    }
}
