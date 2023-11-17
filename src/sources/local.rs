use super::{ClientTrait, Playlist};
use async_trait::async_trait;
use color_eyre::Result;

#[derive(Clone)]
pub struct Client {}

#[async_trait]
impl ClientTrait for Client {
    async fn connect(&mut self) -> Result<()> {
        todo!()
    }
    async fn load_playlists(&mut self) -> Result<Vec<Playlist>> {
        todo!()
    }
    async fn get_playlists(&self) -> Vec<Playlist> {
        todo!()
    }
    fn is_connected(&self) -> bool {
        todo!()
    }
}
