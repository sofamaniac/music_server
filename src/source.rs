pub use async_trait::async_trait;
use std::time::Duration;
pub mod youtube;
use erased_serde::{Serialize, serialize_trait_object};

pub trait Song: Serialize + Send {
    fn get_title(&self) -> String;
    fn get_artists(&self) -> Vec<String>;
    fn get_duration(&self) -> Duration;
    fn get_url(&self) -> Vec<String>;
}

pub trait Playlist: Serialize + Send {
    fn get_name(&self) -> String;
    fn get_size(&self) -> u32;
    fn get_element_at_index(&self, index: u32) -> Box<dyn Song>;
}

// required to be able to make objects of traits
serialize_trait_object!(Song);
serialize_trait_object!(Playlist);

#[async_trait]
pub trait Source {
    fn get_name(&self) -> String;
    async fn get_all_playlists(&self) -> Vec<Box<dyn Playlist>>;
    async fn get_playlist_by_id(&self, id: String) -> Box<dyn Playlist>;
    fn get_number_of_playlist(&self) -> usize;
    async fn get_playlist_by_index(&self, index: u32) -> Box<dyn Playlist>;
}

enum Action {
    Get,
    Select,
    GetAll,
}

enum ResourceType {
    Song,
    Playlist,
}

struct Request {
    pub id: Option<String>,
    pub action: Action,
    pub resource: ResourceType,
    pub attribute: Option<String>,
}
