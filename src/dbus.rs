use std::collections::HashMap;
use std::{future::pending, sync::Arc};

use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use zbus::{dbus_interface, Connection, ConnectionBuilder};

//use crate::ui::Ui;
//use crate::event::Event;
use crate::ui::Ui;
use log::*;

struct BaseInterface {}

#[dbus_interface(name = "org.mpris.MediaPlayer2")]
impl BaseInterface {
    fn identity(&self) -> String {
        "yauma".to_string()
    }

    #[dbus_interface(property)]
    fn can_raise(&self) -> bool {
        false
    }

    fn raise(&self) {}

    fn quit(&self) {}

    #[dbus_interface(property)]
    fn can_quit(&self) -> bool {
        false
    }

    #[dbus_interface(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[dbus_interface(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Default::default()
    }

    #[dbus_interface(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Default::default()
    }
}
#[derive(Clone)]
pub struct PlayerInterface {
    ui: Arc<Mutex<Ui>>,
}

#[dbus_interface(name = "org.mpris.MediaPlayer2.Player")]
impl PlayerInterface {
    async fn next(&self) {
        self.ui.lock().await.next().await;
    }
    async fn previous(&self) {
        self.ui.lock().await.prev().await;
    }
    async fn pause(&self) {
        self.ui.lock().await.set_pause_val(true).await;
    }
    async fn unpause(&self) {
        self.ui.lock().await.set_pause_val(false).await;
    }
    async fn play_pause(&self) {
        self.ui.lock().await.player.playpause().await;
    }
    async fn play(&self) {
        self.unpause().await;
    }
    async fn stop(&self) {
        self.ui.lock().await.player.stop().await;
    }
    const fn seek(&self) {}
    const fn set_position(&self) {}
    const fn open_uri(&self) {}

    #[dbus_interface(property)]
    async fn playback_status(&self) -> String {
        let ui = self.ui.lock().await;
        format!("{}", ui.player.get_playback_status())
    }

    #[dbus_interface(property)]
    async fn loop_status(&self) -> String {
        let ui = self.ui.lock().await;
        let state = ui.player.get_state();
        format!("{}", state.repeat)
    }

    #[dbus_interface(property)]
    const fn rate(&self) -> f32 {
        1.0
    }
    #[dbus_interface(property)]
    async fn shuffle(&self) -> bool {
        let ui = self.ui.lock().await;
        let state = ui.player.get_state();
        state.shuffled
    }
    #[dbus_interface(property)]
    async fn volume(&self) -> f32 {
        let ui = self.ui.lock().await;
        let state = ui.player.get_state();
        (state.volume as f32) / 100.
    }
    #[dbus_interface(property)]
    async fn position(&self) -> u64 {
        let ui = self.ui.lock().await;
        let state = ui.player.get_state();
        (state.time_pos * 1_000_000) as u64
    }
    #[dbus_interface(property)]
    async fn metadata(&self) -> HashMap<&str, zbus::zvariant::Value> {
        use zbus::zvariant::Value;
        let mut res = HashMap::new();
        let ui = self.ui.lock().await;
        if let Some(song) = ui.get_playing_song_info() {
            //res.insert("mpris:trackid", Value::Str(song.id.clone().into()));
            res.insert("mpris:length", Value::U64(song.duration.as_micros() as u64));
            res.insert("xesam:title", Value::Str(song.title.clone().into()));
            res.insert("xesam:artist", Value::Str(song.artists.join(", ").into()));
        }
        res
    }

    #[dbus_interface(property)]
    const fn can_go_next(&self) -> bool {
        true
    }
    #[dbus_interface(property)]
    const fn can_go_previous(&self) -> bool {
        true
    }
    #[dbus_interface(property)]
    const fn can_play(&self) -> bool {
        true
    }
    #[dbus_interface(property)]
    const fn can_pause(&self) -> bool {
        true
    }
    #[dbus_interface(property)]
    const fn can_seek(&self) -> bool {
        true
    }
    #[dbus_interface(property)]
    const fn can_control(&self) -> bool {
        true
    }
}

pub async fn start(
    ui: Arc<Mutex<Ui>>,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    debug!("Starting dbus");
    let base = BaseInterface {};
    let player = PlayerInterface { ui: ui.clone() };
    let conn = ConnectionBuilder::session()?
        .name("org.mpris.MediaPlayer2.yama")?
        .serve_at("/org/mpris/MediaPlayer2", base)?
        .serve_at("/org/mpris/MediaPlayer2", player)?
        .build()
        .await?;
    let player_iface_ref = conn
        .object_server()
        .interface::<_, PlayerInterface>("/org/mpris/MediaPlayer2")
        .await?;
    let mut state = ui.lock().await.player.get_state();
    loop {
        sleep(Duration::from_millis(100)).await;
        let player_iface = player_iface_ref.get_mut().await;
        let context = player_iface_ref.signal_context();
        let new_state = ui.lock().await.player.get_state();
        if state.path != new_state.path {
            player_iface.metadata_changed(context).await?;
        }
        if state.repeat != new_state.repeat {
            player_iface.loop_status_changed(context).await?;
        }
        if state.shuffled != new_state.shuffled {
            player_iface.shuffle_changed(context).await?;
        }
        if state.volume != new_state.volume {
            player_iface.volume_changed(context).await?;
        }
        if state.playpause != new_state.playpause {
            player_iface.playback_status_changed(context).await?;
        }
        state = new_state;
    }
}
