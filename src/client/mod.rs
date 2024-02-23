pub mod interface;
#[cfg(feature = "mpv")]
mod mpv;
#[cfg(feature = "local")]
pub mod local;
#[cfg(feature = "spotify")]
pub mod spotify;
#[cfg(feature = "youtube")]
pub mod youtube;
