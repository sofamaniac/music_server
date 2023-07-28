use crate::{
    db,
    request::{Answer, AnswerType, ErrorType, ObjRequest, Request, RequestType},
};
pub use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;
use RequestType::*;
pub mod spotify;
pub mod youtube;

pub type SourceResult<T> = Result<T, SourceError>;

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

pub trait SongTrait {
    fn to_song(&self) -> Song;
}

#[async_trait]
pub trait PlaylistTrait: Sync + Send {
    fn to_playlist(&self) -> Playlist;
    fn get_id(&self) -> String;
    fn get_source(&self) -> String;
    async fn get_songs(&mut self) -> Vec<Song>;
    async fn load_from_db(&self) -> Playlist {
        db::load_playlist(&self.get_id(), &self.get_source()).unwrap()
    }
}

#[async_trait]
pub trait Source: Sync + Send {
    fn get_name(&self) -> String;
    fn get_number_of_playlist(&self) -> usize;
    async fn get_all_playlists(&mut self) -> Vec<Playlist>;
    async fn get_playlist_by_id(&mut self, id: &str) -> SourceResult<Box<dyn PlaylistTrait>>;
    async fn init(&mut self) -> ();
    async fn send(&self, data: Answer) -> ();
    async fn listen(&mut self) -> ();
    async fn download_songs(&self, songs: &[Song], playlist_title: String);

    async fn send_with_name(&self, data: AnswerType) {
        self.send(Answer::new(self.get_name(), data)).await
    }

    async fn handle_request(&mut self, request: Request) {
        if request.client == self.get_name() || request.client == "all" {
            match request.ty {
                GetAll(ObjRequest::PlaylistList) => {
                    let playlists = self.get_all_playlists().await;
                    self.send_with_name(AnswerType::PlaylistList(playlists))
                        .await;
                }
                GetAll(ObjRequest::Playlist(id)) => {
                    let playlist = self.get_playlist_by_id(&id).await;
                    match playlist {
                        Err(err) => {
                            self.send_with_name(AnswerType::Error(ErrorType::SourceError(err)))
                                .await
                        }
                        Ok(mut playlist) => {
                            let songs = playlist.get_songs().await;
                            self.send_with_name(AnswerType::Songs(songs)).await;
                        }
                    }
                }
                Download(ObjRequest::Playlist(id)) => {
                    let playlist = self.get_playlist_by_id(&id).await;
                    match playlist {
                        Err(err) => {
                            self.send_with_name(AnswerType::Error(ErrorType::SourceError(err)))
                                .await
                        }
                        Ok(mut playlist) => {
                            let songs = playlist.get_songs().await;
                            self.download_songs(&songs, playlist.to_playlist().title)
                                .await;
                        }
                    }
                }

                GetAll(ObjRequest::ClientList) => {
                    let answer = AnswerType::Client(self.get_name());
                    self.send_with_name(answer).await;
                }

                _ => println!("TODO"),
            }
        }
    }
}
