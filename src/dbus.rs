use anyhow::Result;
use log::debug;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};
use zbus::zvariant::{ObjectPath, Value};
use zbus::{dbus_interface, zvariant, ConnectionBuilder};

use crate::client::interface::{
    Playback, PlayerAction, PlayerInfo, Repeat, SeekMode, SongInfo, Volume,
};
use crate::orchestrator::{Action, MyEvents};

/// Create [ObjectPath] from `song`, note that the DBus specification asks
/// that trackid be unique for each entrie in a tracklist, including duplicates
/// which is not guaranteed by this function
fn make_trackid(song: &SongInfo) -> ObjectPath {
    // create valid string by hashing the id
    let mut hasher = DefaultHasher::new();
    song.id.clone().hash(&mut hasher);
    let trackid: u64 = hasher.finish();
    ObjectPath::try_from(format!("/org/mpris/MediaPlayer2/TrackList/{}", trackid)).unwrap()
}

fn make_metadata(song: &SongInfo) -> HashMap<&str, Value> {
    let mut res = HashMap::new();
    res.insert("mpris:trackid", make_trackid(song).into());
    res.insert(
        "mpris:length",
        Value::U64(u64::try_from(song.duration.as_micros()).unwrap_or_default()),
    );
    res.insert("xesam:title", Value::Str(song.title.clone().into()));
    res.insert("xesam:artist", Value::Str(song.artist.clone().into()));
    res.insert("xesam:url", Value::Str(song.url.clone().into()));
    res.insert("mpris:artUrl", Value::Str(song.cover_url.clone().into()));

    res
}

struct BaseInterface {
    sender: Sender<MyEvents>,
}

#[dbus_interface(name = "org.mpris.MediaPlayer2")]
impl BaseInterface {
    fn identity(&self) -> String {
        "yama".to_string()
    }

    #[dbus_interface(property)]
    const fn can_raise(&self) -> bool {
        false
    }

    const fn raise(&self) {}

    async fn quit(&self) {
        // ignore failure to send message
        let _ = self.sender.send(Action::Quit.into()).await;
    }

    #[dbus_interface(property)]
    const fn can_quit(&self) -> bool {
        true
    }

    #[dbus_interface(property)]
    const fn has_track_list(&self) -> bool {
        true
    }

    #[dbus_interface(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::default()
    }

    #[dbus_interface(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::default()
    }
}

pub struct TrackListInterface {
    state: PlayerInfo,
    sender: Sender<MyEvents>,
}

#[dbus_interface(name = "org.mpris.MediaPlayer2.TrackList")]
impl TrackListInterface {
    fn get_tracks_metadata(
        &self,
        ids: Vec<zvariant::ObjectPath>,
    ) -> Vec<HashMap<&str, zvariant::Value>> {
        self.state
            .tracklist
            .songs
            .iter()
            .filter(|s| ids.contains(&make_trackid(s)))
            .map(|s| make_metadata(s))
            .collect()
    }

    const fn add_track(&self) {}
    const fn remove_track(&self) {}

    const fn go_to(&self) {}

    #[dbus_interface(property)]
    async fn tracks(&self) -> Vec<zvariant::ObjectPath> {
        // as per recommendation of the specification
        // limit the number of items returned to 20
        if let Some(start) = self.state.track_index {
            // not sure if necessary to compute the end or if it cannot go
            // outside of range by default
            let end = (start + 20).min(self.state.tracklist.songs.len());
            self.state.tracklist.songs[start..end]
                .iter()
                .map(|s| make_trackid(s))
                .collect()
        } else {
            Default::default()
        }
    }

    #[dbus_interface(property)]
    const fn can_edit_tracks(&self) -> bool {
        false
    }

    // TODO: send signal when tracklist has been replaced
}

pub struct PlayerInterface {
    state: PlayerInfo,
    sender: Sender<MyEvents>,
}

#[dbus_interface(name = "org.mpris.MediaPlayer2.Player")]
impl PlayerInterface {
    async fn next(&self) {
        let _ = self.sender.send(PlayerAction::Next.into()).await;
    }
    async fn previous(&self) {
        let _ = self.sender.send(PlayerAction::Prev.into()).await;
    }
    async fn pause(&self) {
        let _ = self.sender.send(PlayerAction::PlayPause(true).into()).await;
    }
    async fn unpause(&self) {
        let _ = self
            .sender
            .send(PlayerAction::PlayPause(false).into())
            .await;
    }
    async fn play_pause(&self) {
        let _ = self.sender.send(PlayerAction::PlayPauseToggle.into()).await;
    }
    async fn play(&self) {
        let _ = self
            .sender
            .send(PlayerAction::PlayPause(self.state.playback == Playback::Pause).into())
            .await;
    }
    async fn stop(&self) {
        let _ = self.sender.send(PlayerAction::Stop.into()).await;
    }
    /// seek to current position + `offset` with `offset` in microseconds
    async fn seek(&self, offset: i64) {
        let offset = offset / 1_000_000;
        let _ = self
            .sender
            .send(
                PlayerAction::Seek {
                    dt: offset,
                    mode: SeekMode::Relative,
                }
                .into(),
            )
            .await;
    }
    /// `position` is in microseconds, ignore if `trackid` is different
    /// from the currently playing `trackid`
    async fn set_position(&self, trackid: ObjectPath<'_>, position: i64) {
        if let Some(song) = self.state.song_info.as_ref() {
            // position in seconds
            let position = position / 1_000_000;
            if position < 0
                || Duration::from_secs(position as u64) > song.duration
                || trackid != make_trackid(song)
            {
                // ignore if position is not in range
                // or if the track id does not match
            } else {
                let _ = self
                    .sender
                    .send(
                        PlayerAction::Seek {
                            dt: position,
                            mode: SeekMode::Absolute,
                        }
                        .into(),
                    )
                    .await;
            }
        }
    }
    const fn open_uri(&self) {}

    #[dbus_interface(property)]
    fn playback_status(&self) -> String {
        format!("{}", self.state.playback)
    }

    #[dbus_interface(property)]
    fn loop_status(&self) -> String {
        match self.state.repeat {
            Repeat::Off => "None",
            Repeat::Playlist => "Playlist",
            Repeat::Song => "Track",
        }
        .to_string()
    }

    #[dbus_interface(property)]
    const fn rate(&self) -> f32 {
        1.0
    }
    #[dbus_interface(property)]
    const fn maximum_rate(&self) -> f32 {
        1.0
    }
    #[dbus_interface(property)]
    const fn minimum_rate(&self) -> f32 {
        1.0
    }
    #[dbus_interface(property)]
    fn shuffle(&self) -> bool {
        self.state.shuffled
    }
    #[dbus_interface(property)]
    fn volume(&self) -> f32 {
        self.state.volume as f32 / 100.0
    }
    #[dbus_interface(property)]
    async fn set_volume(&self, val: f64) {
        let target: usize = ((val * 100.0) as usize).min(100);
        let _ = self
            .sender
            .send(PlayerAction::SetVolume(Volume::Absolute(target)).into())
            .await;
    }
    #[dbus_interface(property)]
    fn position(&self) -> i64 {
        self.state.position.as_micros() as i64
    }
    #[dbus_interface(property)]
    fn metadata(&self) -> HashMap<&str, Value> {
        if let Some(song) = self.state.song_info.as_ref() {
            make_metadata(song)
        } else {
            Default::default()
        }
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

pub async fn start(sender: Sender<MyEvents>, receiver: &mut Receiver<PlayerInfo>) -> Result<()> {
    debug!("Starting dbus");
    let base = BaseInterface {
        sender: sender.clone(),
    };
    let player = PlayerInterface {
        sender: sender.clone(),
        state: PlayerInfo::default(),
    };
    let tracklist = TrackListInterface {
        sender,
        state: PlayerInfo::default(),
    };
    let mut old_state = PlayerInfo::default();
    let conn = ConnectionBuilder::session()?
        .name("org.mpris.MediaPlayer2.yama")?
        .serve_at("/org/mpris/MediaPlayer2", base)?
        .serve_at("/org/mpris/MediaPlayer2", player)?
        .serve_at("/org/mpris/MediaPlayer2", tracklist)?
        .build()
        .await?;
    let player_iface_ref = conn
        .object_server()
        .interface::<_, PlayerInterface>("/org/mpris/MediaPlayer2")
        .await?;
    let tracklist_iface_ref = conn
        .object_server()
        .interface::<_, PlayerInterface>("/org/mpris/MediaPlayer2")
        .await?;
    // run until the connection is closed
    while let Some(state) = receiver.recv().await {
        // getting interface objects
        let mut player_iface = player_iface_ref.get_mut().await;
        // copying new state to interfaces
        // in order to send up to date info on the dbus
        player_iface.state = state.clone();

        let context = player_iface_ref.signal_context();
        if old_state.playback != state.playback {
            player_iface.playback_status_changed(context).await?;
        }
        let old_info = old_state.song_info.as_ref();
        let new_info = state.song_info.as_ref();
        if old_info != new_info {
            debug!("[DBus] metadata changed]");
            player_iface.metadata_changed(context).await?;
        }
        if old_state.shuffled != state.shuffled {
            player_iface.shuffle_changed(context).await?;
        }
        if old_state.repeat != state.repeat {
            player_iface.loop_status_changed(context).await?;
        }
        if old_state.volume != state.volume {
            player_iface.volume_changed(context).await?;
        }
        old_state = state.clone();
        // /!\ MUST be dropped before accessing interface
        drop(player_iface);
        let mut tracklist_iface = tracklist_iface_ref.get_mut().await;
        tracklist_iface.state = state.clone();
        // TODO send tracklistchanged signal when necessary
        drop(tracklist_iface);
    }
    Ok(())
}
