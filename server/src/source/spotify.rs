#![warn(clippy::unwrap_used)]
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use futures::TryStreamExt;
use rspotify::clients::{BaseClient, OAuthClient};
use rspotify::model::{PlayableItem, PlaylistId, SimplifiedPlaylist};
use serde::{Serialize, Deserialize};
use tokio::sync::broadcast::error::RecvError;

use crate::{config, db, utils};
use music_server::request::{send_request, Answer, AnswerType, Request, RequestType};

use super::{Song, SongTrait};
use super::{Playlist, PlaylistTrait, Source, SourceError, SourceResult};
use rspotify::{self, AuthCodeSpotify};
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc::{self, Sender};

const MAX_RESULT: u32 = 50;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SpotifySong {
    pub song: Song,
    pub isrc: Option<String>,
}

impl SongTrait for SpotifySong {
    fn to_song(&self) -> Song {
        self.song.clone()
    }
    fn get_url(&self) -> String {
        self.song.url.clone()
    }
    fn get_downloaded(&self) -> bool {
        self.song.downloaded
    }

    fn get_id(&self) -> Arc<str> {
        self.song.get_id()
    }

    fn get_title(&self) -> Arc<str> {
        self.song.get_title()
    }

    fn get_artists(&self) -> Arc<[String]> {
        self.song.artists.clone()
    }

    fn set_downloaded(&mut self, val: bool) {
        self.song.downloaded = val;
    }

    fn set_url(&mut self, val: String) {
        self.song.url = val;
    }
}

#[derive(Clone, Debug)]
pub struct SpotifyPlaylist {
    playlist: Playlist,
    songs: Vec<SpotifySong>,
    id: Arc<str>,
    etag: String, // used to check if playlist has changed
    is_loaded: bool,
    client: AuthCodeSpotify,
    source: Arc<str>,
}

impl Default for SpotifyPlaylist {
    fn default() -> Self {
        SpotifyPlaylist { id: "".into(),..Default::default() }
    }
}

async fn load_all_songs(client: &AuthCodeSpotify, id: PlaylistId<'_>) -> Vec<SpotifySong> {
    let mut playlist_items = client.playlist_items(id, None, None);
    let mut songs = vec![];
    while let Ok(page) = playlist_items.try_next().await {
        if page.is_none() {
            break;
        }
        let items = page.unwrap_or_default();
        let tracks = items.track.unwrap();
        match tracks {
            PlayableItem::Episode(_) => (),
            PlayableItem::Track(fulltrack) => songs.push(SpotifySong {
                song: Song::new(fulltrack.name.into(),
                fulltrack
                    .artists
                    .iter()
                    .cloned()
                    .map(|artist| artist.name)
                    .collect(),
                Default::default(),
                if fulltrack.id.is_some() {
                    fulltrack.id.unwrap().to_string().into()
                } else {
                    "".into()
                },
                fulltrack.duration.to_std().unwrap(),
                Default::default(),),
                isrc: fulltrack.external_ids.get("isrc").cloned()
            }),
        }
    }
    songs
}

impl SpotifyPlaylist {
    pub async fn new(id: &str, client: AuthCodeSpotify, source: Arc<str>) -> Self {
        let id = PlaylistId::from_uri(id).unwrap();
        let playlist = client.playlist(id, None, None).await.unwrap();
        SpotifyPlaylist {
            playlist: Playlist {
                title: playlist.name.into(),
                tags: Default::default(),
                id: playlist.id.to_string().into(),
                size: playlist.tracks.total,
            },
            songs: Vec::with_capacity(playlist.tracks.total as usize),
            id: playlist.id.to_string().into(),
            etag: playlist.snapshot_id,
            is_loaded: false,
            client,
            source,
        }
    }

    pub async fn load_all(&mut self) {
        if self.is_loaded || self.load_from_db() {
            return;
        };
        let id = PlaylistId::from_uri(&self.id).unwrap();
        self.songs = load_all_songs(&self.client, id).await;
        let _ = db::add_playlist(&self.source, self.to_playlist(), &self.songs, &self.etag);
    }
    fn load_from_db(&mut self) -> bool {
        let db_bool = db::playlist_needs_update(&self.id, &self.source, &self.etag);
        if db_bool && !self.is_loaded {
            let playlist = db::load_playlist(&self.id, &self.source).unwrap();
            self.playlist.title = playlist.title;
            self.playlist.size = playlist.size;
            self.songs = db::get_playlist_songs(&self.id, &self.source).unwrap_or_default();
            self.is_loaded = true;
        }
        db_bool
    }
}

#[async_trait]
impl PlaylistTrait<SpotifySong> for SpotifyPlaylist {
    fn get_id(&self) -> Arc<str> {
        self.id.clone()
    }

    fn get_source(&self) -> Arc<str> {
        self.source.clone()
    }

    async fn get_songs(&mut self) -> Vec<SpotifySong> {
        if !self.is_loaded {
            self.load_all().await
        };
        self.songs.clone()
    }

    fn to_playlist(&self) -> Playlist {
        self.playlist.clone()
    }
}

async fn convert_playlist(
    playlist: SimplifiedPlaylist,
    client: AuthCodeSpotify,
) -> SpotifyPlaylist {
    SpotifyPlaylist::new(&playlist.id.to_string(), client, "Spotify".into()).await
}

pub struct Client {
    client: rspotify::AuthCodeSpotify,
    pub name: Arc<str>,
    playlists: Vec<SpotifyPlaylist>,
    in_channel: Receiver<Request>,
    out_channel: Sender<Answer>,
    playlist_loaded: bool,
    all_loaded: bool,
}

impl Client {
    pub async fn new(
        name: &str,
        in_channel: Receiver<Request>,
        out_channel: Sender<Answer>,
    ) -> Client {
        let config = config::get_config();
        let credentials = rspotify::Credentials::new(&config.spotify_id, &config.spotify_secret);
        let secrets = config.secrets_location;
        let oauth = rspotify::OAuth {
            redirect_uri: "https://localhost:8888/callback".to_string(),
            scopes: rspotify::scopes!("user-read-recently-played"),
            ..Default::default()
        };
        let client_config: rspotify::Config = rspotify::Config {
            token_cached: true,
            token_refreshing: true,
            pagination_chunks: MAX_RESULT,
            cache_path: Path::new(&format!("{}/spotify.cache", secrets)).to_path_buf(),
            ..Default::default()
        };
        let client = rspotify::AuthCodeSpotify::with_config(credentials, oauth, client_config);
        Client {
            client,
            name: name.into(),
            playlists: Default::default(),
            in_channel,
            out_channel,
            playlist_loaded: false,
            all_loaded: false,
        }
    }

    async fn reauth(&mut self) {
        let url = self.client.get_authorize_url(false).unwrap_or_default();
        send_request(
            self.out_channel.clone(),
            Answer::new("Spotify".into(), AnswerType::Message(url.into())),
        )
        .await;
        loop {
            match self.in_channel.recv().await {
                Ok(request) => {
                    if request.client == self.name {
                        match request.ty {
                            RequestType::Message(url) => {
                                let code =
                                    self.client.parse_response_code(&url).unwrap_or_default();
                                self.client.request_token(&code).await;
                                break;
                            }
                            _ => continue,
                        }
                    }
                }
                Err(RecvError::Closed) => {
                    break;
                },
                _ => continue
            };
        }
    }
    pub async fn authenticate(&mut self) {
        match self.client.read_token_cache(true).await {
            Ok(Some(new_token)) => {
                let expired = new_token.is_expired();

                // Load token into client regardless of whether it's expired o
                // not, since it will be refreshed later anyway.
                *self.client.get_token().lock().await.unwrap() = Some(new_token);

                if expired {
                    // Ensure that we actually got a token from the refetch
                    let token = self.client.refetch_token().await;
                    match token {
                        Err(err) => println!("Error: {}", err),
                        Ok(val) => match val {
                            Some(refreshed_token) => {
                                *self.client.get_token().lock().await.unwrap() =
                                    Some(refreshed_token)
                            }
                            // If not, prompt the user for it
                            None => {
                                self.reauth().await;
                            }
                        },
                    }
                }
            }
            // Otherwise following the usual procedure to get the token.
            _ => {
                println!("no token found");
                self.reauth().await;
            }
        }

        match self.client.write_token_cache().await {
            Ok(_) => (),
            Err(e) => println!("{}", e),
        }
    }

    pub async fn fetch_all_playlists(&mut self) {
        if self.playlist_loaded {
            return;
        }
        let (tx, mut rx) = mpsc::channel(32);
        let playlists = self.client.current_user_playlists();
        let client = &self.client;
        playlists
            .try_for_each_concurrent(10, |playlist| async {
                tx.send(convert_playlist(playlist, client.clone()).await)
                    .await;
                Ok(())
            })
            .await
            .unwrap();
        drop(tx);

        let res = tokio::spawn(async move {
            let mut playlists = vec![];
            while let Some(playlist) = rx.recv().await {
                playlists.push(playlist)
            }
            playlists
        });
        let res = res.await.unwrap();
        self.playlists = res;
        self.playlist_loaded = true;
    }
}

#[async_trait]
impl Source<SpotifySong, SpotifyPlaylist> for Client {
    fn get_name(&self) -> Arc<str> {
        self.name.clone()
    }
    fn get_number_of_playlist(&self) -> usize {
        self.playlists.len()
    }

    async fn send(&self, data: Answer) {
        self.out_channel.send(data).await;
    }
    async fn get_all_playlists(&mut self) -> Vec<Playlist> {
        futures::future::join_all(
            self.playlists
                .iter_mut()
                .map(|p| async { p.load_all().await }),
        )
        .await;
        self.playlists.iter().map(|p| p.to_playlist()).collect()
    }
    async fn get_playlist_by_id(&mut self, id: &str) -> SourceResult<SpotifyPlaylist> {
        let _ = self.get_all_playlists().await;
        let playlist = self.playlists.iter().cloned().find(|p| *p.id == *id);
        match playlist {
            Some(playlist) => Ok(playlist),
            None => Err(SourceError::PlaylistNotFound),
        }
    }
    async fn init(&mut self) -> () {
        self.fetch_all_playlists().await;
        self.get_all_playlists().await;
    }
    async fn listen(&mut self) {
        println!("Start listening");
        loop {
            let _ = match self.in_channel.recv().await {
                Ok(msg) => self.handle_request(msg).await,
                Err(RecvError::Closed) => {
                    break;
                },
                _ => continue
            };
        }
    }
    async fn download_songs(&self, songs: &[SpotifySong], playlist: Playlist) {
        let songs = songs.to_vec();
        let name = self.name.clone();
        let out_channel = self.out_channel.clone();
        tokio::spawn(
            async move { utils::download_spotify_playlist(&songs, name, &playlist, out_channel).await },
        );
    }
}
