use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
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

#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct Song {
    pub title: Arc<str>,
    pub artists: Arc<[String]>,
    pub tags: Vec<String>,
    pub id: Arc<str>,
    pub duration: Duration,
    pub url: String,
    pub downloaded: bool,
}

impl Song {
    pub fn new(
        title: Arc<str>,
        artists: Arc<[String]>,
        tags: Vec<String>,
        id: Arc<str>,
        duration: Duration,
        url: String,
    ) -> Self {
        Song {
            title,
            artists,
            tags,
            id,
            duration,
            url,
            downloaded: false,
        }
    }
}

impl Default for Song {
    fn default() -> Self {
        Song::new(
            "".into(),
            Arc::new(["".to_owned(); 0]),
            vec![],
            "".into(),
            Default::default(),
            Default::default(),
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Playlist {
    pub title: Arc<str>,
    pub tags: Vec<String>,
    pub id: Arc<str>,
    pub size: u32,
}

impl Default for Playlist {
    fn default() -> Self {
        Playlist {
            title: "".into(),
            tags: vec![],
            id: "".into(),
            size: 0,
        }
    }
}
