use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum SourceError {
    PlaylistNotFound,
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Serialize, Clone, Debug, Deserialize, Default)]
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

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Playlist {
    pub title: String,
    pub tags: Vec<String>,
    pub id: String,
    pub size: u32,
}
