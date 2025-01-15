use anyhow::Result;
use log::debug;
use protocol::playback::{PlayerStatus, SeekMode, VolumeSetter};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use tokio_util::sync::CancellationToken;
use zbus::conn::Builder;
use zbus::zvariant::{ObjectPath, Value};
use zbus::{interface, zvariant};

use protocol::{Duration, PlayerInfo, Receive, Repeat, Song, TypedAction, Volume};

use crate::FullSource;

/// Create [ObjectPath] from `song`, note that the DBus specification asks
/// that trackid be unique for each entrie in a tracklist, including duplicates
/// which is not guaranteed by this function
fn make_trackid(song: &Song) -> ObjectPath {
    // create valid string by hashing the id
    let mut hasher = DefaultHasher::new();
    song.id().clone().hash(&mut hasher);
    let trackid: u64 = hasher.finish();
    ObjectPath::try_from(format!("/org/mpris/MediaPlayer2/TrackList/{}", trackid)).unwrap()
}

fn make_metadata(song: &Song) -> HashMap<&str, Value> {
    let mut res = HashMap::new();
    let duration: std::time::Duration = song.duration();
    res.insert("mpris:trackid", make_trackid(song).into());
    res.insert(
        "mpris:length",
        Value::U64(u64::try_from(duration.as_micros()).unwrap_or_default()),
    );
    res.insert("xesam:title", Value::Str(song.title().into()));
    res.insert("xesam:artist", Value::Str(song.artists().join(",").into()));
    res.insert("xesam:url", Value::Str(song.url().into()));
    res.insert(
        "mpris:artUrl",
        Value::Str(song.cover_url().cloned().unwrap_or_default().into()),
    );

    res
}

trait SourceHandler {
    fn sources(&self) -> &FullSource;

    async fn send<T>(&self, action: TypedAction<T>) {
        let sources = self.sources().lock().await;
        if let Some(source) = sources.get_active() {
            let _ = action.send(&source.sender).await;
        }
    }
}
struct BaseInterface {
    sources: FullSource,
}
impl SourceHandler for BaseInterface {
    fn sources(&self) -> &FullSource {
        &self.sources
    }
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl BaseInterface {
    fn identity(&self) -> String {
        "yama".to_string()
    }

    #[zbus(property)]
    const fn can_raise(&self) -> bool {
        false
    }

    const fn raise(&self) {}

    async fn quit(&self) {
        // ignore failure to send message
        let action = protocol::Command::quit();
        self.send(action).await;
    }

    #[zbus(property)]
    const fn can_quit(&self) -> bool {
        true
    }

    #[zbus(property)]
    const fn has_track_list(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::default()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::default()
    }
}

pub struct TrackListInterface {
    state: PlayerInfo,
}

#[interface(name = "org.mpris.MediaPlayer2.TrackList")]
impl TrackListInterface {
    fn get_tracks_metadata(
        &self,
        ids: Vec<zvariant::ObjectPath>,
    ) -> Vec<HashMap<&str, zvariant::Value>> {
        self.state
            .queue
            .iter()
            .filter(|s| ids.contains(&make_trackid(s)))
            .map(|s| make_metadata(s))
            .collect()
    }

    const fn add_track(&self) {}
    const fn remove_track(&self) {}

    const fn go_to(&self) {}

    #[zbus(property)]
    async fn tracks(&self) -> Vec<zvariant::ObjectPath> {
        // as per recommendation of the specification
        // limit the number of items returned to 20
        self.state.queue.iter().map(make_trackid).take(20).collect()
    }

    #[zbus(property)]
    const fn can_edit_tracks(&self) -> bool {
        false
    }

    // TODO: send signal when tracklist has been replaced
}

pub struct PlayerInterface {
    state: PlayerInfo,
    sources: FullSource,
}
impl SourceHandler for PlayerInterface {
    fn sources(&self) -> &FullSource {
        &self.sources
    }
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl PlayerInterface {
    async fn next(&self) {
        let action = protocol::playback::Command::next_song();
        self.send(action).await;
    }
    async fn previous(&self) {
        let action = protocol::playback::Command::previous_song();
        self.send(action).await;
    }
    async fn pause(&self) {
        let action = protocol::playback::Command::set_pause(true);
        self.send(action).await;
    }
    async fn unpause(&self) {
        let action = protocol::playback::Command::set_pause(false);
        self.send(action).await;
    }
    async fn play_pause(&self) {
        let action = protocol::playback::Command::play_pause();
        self.send(action).await;
    }
    async fn play(&self) {
        let action = protocol::playback::Command::set_pause(false);
        self.send(action).await;
    }
    async fn stop(&self) {
        let action = protocol::playback::Command::stop();
        self.send(action).await;
    }
    /// seek to current position + `offset` with `offset` in microseconds
    async fn seek(&self, offset: i64) {
        let seek_mode = if offset < 0 {
            let duration = Duration::from_micros(offset.unsigned_abs());
            SeekMode::Backward(duration)
        } else {
            let duration = Duration::from_micros(offset.unsigned_abs());
            SeekMode::Forward(duration)
        };
        let action = protocol::playback::Command::seek(seek_mode);
        self.send(action).await;
    }
    /// `position` is in microseconds, ignore if `trackid` is different
    /// from the currently playing `trackid`
    async fn set_position(&self, trackid: ObjectPath<'_>, position: i64) {
        if let PlayerStatus::Playing { song, .. } = &self.state.status {
            // position in seconds
            let position = position / 1_000_000;
            if position < 0
                || Duration::from_secs(position as u64) > song.duration()
                || trackid != make_trackid(song)
            {
                // ignore if position is not in range
                // or if the track id does not match
            } else {
                let position = Duration::from_micros(position.unsigned_abs());
                let action = protocol::playback::Command::seek(SeekMode::Absolute(position));
                self.send(action).await;
            }
        }
    }
    const fn open_uri(&self) {}

    #[zbus(property)]
    fn playback_status(&self) -> String {
        match &self.state.status {
            PlayerStatus::Stopped => String::from("Stopped"),
            _ if self.state.paused => String::from("Paused"),
            _ => String::from("Playing"),
        }
    }

    #[zbus(property)]
    fn loop_status(&self) -> String {
        match self.state.repeat {
            Repeat::Off => "None",
            Repeat::Playlist => "Playlist",
            Repeat::Song => "Track",
        }
        .to_string()
    }

    #[zbus(property)]
    const fn rate(&self) -> f32 {
        1.0
    }
    #[zbus(property)]
    const fn maximum_rate(&self) -> f32 {
        1.0
    }
    #[zbus(property)]
    const fn minimum_rate(&self) -> f32 {
        1.0
    }
    #[zbus(property)]
    fn shuffle(&self) -> bool {
        self.state.shuffled
    }
    #[zbus(property)]
    fn volume(&self) -> f32 {
        self.state.volume.to_u8() as f32 / 100.0
    }
    #[zbus(property)]
    async fn set_volume(&self, val: f64) {
        let target: u8 = ((val * 100.0) as u8).min(100);
        let volume = VolumeSetter::Absolute(Volume::new(target));
        let action = protocol::playback::Command::set_volume(volume);
        self.send(action).await;
    }
    #[zbus(property)]
    fn position(&self) -> i64 {
        if let PlayerStatus::Playing { position, .. } = &self.state.status {
            position.as_micros() as i64
        } else {
            Default::default()
        }
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<&str, Value> {
        if let PlayerStatus::Playing { song, .. } = &self.state.status {
            make_metadata(song)
        } else {
            Default::default()
        }
    }

    #[zbus(property)]
    const fn can_go_next(&self) -> bool {
        true
    }
    #[zbus(property)]
    const fn can_go_previous(&self) -> bool {
        true
    }
    #[zbus(property)]
    const fn can_play(&self) -> bool {
        true
    }
    #[zbus(property)]
    const fn can_pause(&self) -> bool {
        true
    }
    #[zbus(property)]
    const fn can_seek(&self) -> bool {
        true
    }
    #[zbus(property)]
    const fn can_control(&self) -> bool {
        true
    }
}

pub async fn start(sources: FullSource, cancel_token: CancellationToken) -> Result<()> {
    debug!("Starting dbus");
    let base = BaseInterface {
        sources: sources.clone(),
    };
    let player = PlayerInterface {
        sources: sources.clone(),
        state: PlayerInfo::default(),
    };
    let tracklist = TrackListInterface {
        state: PlayerInfo::default(),
    };
    let mut old_state = PlayerInfo::default();
    let conn = Builder::session()?
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
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
    // run until the connection is closed
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => { break }
            _ = interval.tick() => {
                if let Some(sender) = sources.lock().await.get_active() {
                    let action = protocol::playback::Command::get_info();
                    if let Ok(state) = action.send(&sender.sender).await.recv().await {
                        // getting interface objects
                        let mut player_iface = player_iface_ref.get_mut().await;
                        // copying new state to interfaces
                        // in order to send up to date info on the dbus
                        player_iface.state = state.clone();

                        let context = player_iface_ref.signal_emitter();
                        match (&old_state.status, &state.status) {
                            (
                                PlayerStatus::Playing { song: old_song, .. },
                                PlayerStatus::Playing { song: new_song, .. },
                            ) if old_song != new_song => {
                                player_iface.metadata_changed(context).await?;
                            }
                            (PlayerStatus::Stopped, PlayerStatus::Playing {..})
                            | (PlayerStatus::Playing {..}, PlayerStatus::Stopped) => {
                                player_iface.playback_status_changed(context).await?;
                            }
                            _ => ()
                        }
                        if old_state.paused != state.paused {
                            player_iface.playback_status_changed(context).await?;
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
                }
            }
        };
    }
    Ok(())
}
