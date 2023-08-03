use std::sync::Arc;

use crate::db;
use music_server::request::{Answer, AnswerType, ErrorType, ObjRequest, Request, RequestType};
pub use async_trait::async_trait;
use RequestType::*;
pub use music_server::source_types::*;
use serde::{Serialize, de::DeserializeOwned};
pub mod spotify;
pub mod youtube;

pub type SourceResult<T> = Result<T, SourceError>;

pub trait SongTrait: Serialize + DeserializeOwned+ Clone + Sync + Send {
    fn to_song(&self) -> Song;
    fn get_url(&self) -> String;
    fn get_downloaded(&self) -> bool;
    fn get_id(&self) -> Arc<str>;
    fn get_title(&self) -> Arc<str>;
    fn get_artists(&self) -> Arc<[String]>;
    fn set_downloaded(&mut self, val: bool);
    fn set_url(&mut self, val: String);
}

#[async_trait]
pub trait PlaylistTrait<S: SongTrait>: Sync + Send {
    fn to_playlist(&self) -> Playlist;
    fn get_id(&self) -> Arc<str>;
    fn get_source(&self) -> Arc<str>;
    async fn get_songs(&mut self) -> Vec<S>;
    async fn load_from_db(&self) -> Playlist {
        db::load_playlist(&self.get_id(), &self.get_source()).unwrap()
    }
}

#[async_trait]
pub trait Source<S: SongTrait, P: PlaylistTrait<S>>: Sync + Send {
    fn get_name(&self) -> Arc<str>;
    fn get_number_of_playlist(&self) -> usize;
    async fn get_all_playlists(&mut self) -> Vec<Playlist>;
    async fn get_playlist_by_id(&mut self, id: &str) -> SourceResult<P>;
    async fn init(&mut self) -> ();
    async fn send(&self, data: Answer) -> ();
    async fn listen(&mut self) -> ();
    async fn download_songs(&self, songs: &[S], playlist: Playlist);

    async fn send_with_name(&self, data: AnswerType) {
        self.send(Answer::new(self.get_name(), data)).await
    }

    async fn handle_request(&mut self, request: Request) {
        if request.client == self.get_name() || *request.client == *"all" {
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
                            let songs = playlist.get_songs().await.iter().map(|s| s.to_song()).collect();
                            self.send_with_name(AnswerType::Songs(playlist.to_playlist(), songs)).await;
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
                            self.download_songs(&songs, playlist.to_playlist())
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
