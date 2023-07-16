pub use async_trait::async_trait;
use serde::Serialize;
use std::time::Duration;
pub mod youtube;

#[derive(Serialize, Clone)]
pub struct Song {
    pub title: String,
    pub artits: Vec<String>,
    pub tags: Vec<String>,
    pub id: String,
    pub duration: Duration,
    pub url: String,
    pub downloaded: bool,
}

impl Song {
    pub fn new(
        title: String,
        artits: Vec<String>,
        tags: Vec<String>,
        id: String,
        duration: Duration,
        url: String,
    ) -> Self {
        Song {
            title,
            artits,
            tags,
            id,
            duration,
            url,
            downloaded: false,
        }
    }
}

#[derive(Serialize, Clone)]
pub struct Playlist {
    pub title: String,
    pub tags: Vec<String>,
    pub id: String,
    pub size: u32,
}

pub trait SongTrait {
    fn to_song(&self) -> Song;
}

#[async_trait]
pub trait PlaylistTrait {
    fn get_element_at_index(&self, index: u32) -> Song;
    async fn get_songs(&mut self) -> Vec<Song>;
    fn to_playlist(&self) -> Playlist;
}

#[async_trait]
pub trait Source {
    fn get_name(&self) -> String;
    async fn get_all_playlists(&mut self) -> Vec<Playlist>;
    async fn get_playlist_by_id(&self, id: String) -> Playlist;
    fn get_number_of_playlist(&self) -> usize;
    async fn get_playlist_by_index(&self, index: u32) -> Playlist;
    async fn listen(&mut self) -> ();
    async fn send(&self, data: String) -> ();
    async fn init(&mut self) -> ();
}
