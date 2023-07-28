#![warn(clippy::unwrap_used)]
extern crate google_youtube3 as youtube3;
use crate::request::{send_request, Answer, AnswerType, Request};
use crate::source::Song as YoutubeSong;
pub use crate::source::{Playlist, Song, Source, SourceError, SourceResult};
use crate::utils::parse_duration;
use crate::{db, utils};
use async_trait::async_trait;
use futures::stream::StreamExt;
use google_youtube3::hyper::client::HttpConnector;
use google_youtube3::hyper_rustls::HttpsConnector;
use google_youtube3::oauth2::authenticator_delegate::InstalledFlowDelegate;
use std::default::Default;
use std::future::Future;
use std::pin::Pin;
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc::Sender;
use youtube3::api::Playlist as YtPlaylist;
use youtube3::api::{PlaylistItem, PlaylistListResponse};
use youtube3::{hyper, hyper_rustls, oauth2, YouTube};

use super::PlaylistTrait;
use crate::config;

const MAX_RESULT: u32 = 50;

#[derive(Clone, Default)]
struct YoutubePlaylist {
    playlist: Playlist,
    songs: Vec<YoutubeSong>,
    id: String,
    next_page_token: String,
    etag: String, // used to check if playlist has changed
    hub: Option<YouTube<HttpsConnector<HttpConnector>>>,
    is_loaded: bool,
    source: String,
}

impl YoutubePlaylist {
    pub fn new(
        name: String,
        id: String,
        size: u32,
        next_page: String,
        etag: String,
        hub: YouTube<HttpsConnector<HttpConnector>>,
        source: String,
    ) -> Self {
        YoutubePlaylist {
            playlist: Playlist {
                title: name,
                tags: Default::default(),
                id: id.clone(),
                size,
            },
            songs: Vec::with_capacity(size as usize),
            id,
            next_page_token: next_page,
            etag,
            hub: Some(hub),
            is_loaded: false,
            source,
        }
    }

    async fn load_page(&mut self) -> Option<String> {
        let hub = self.hub.as_ref()?;
        let request = hub
            .playlist_items()
            .list(&vec!["snippet".to_string(), "contentDetails".to_string()])
            .playlist_id(&self.id)
            .max_results(MAX_RESULT)
            .page_token(&self.next_page_token.to_string());
        let result = request.doit().await.unwrap_or_default();
        let (_, result) = result;
        let items = result.items.unwrap_or_default();
        let songs: Vec<YoutubeSong> = items.into_iter().flat_map(song_from_item).collect();
        for s in songs.into_iter() {
            self.songs.push(s)
        }
        result.next_page_token
    }

    async fn fetch_songs_data(&mut self) {
        let songs_id: Vec<String> = self.songs.iter().map(|s| s.id.clone()).collect();
        let chunks = songs_id.chunks(MAX_RESULT as usize);
        let chunk_list: Vec<&[String]> = chunks.collect();
        if self.hub.is_none() {
            return;
        }
        let hub = self.hub.as_mut().unwrap();
        for chunk in chunk_list {
            let request = hub
                .videos()
                .list(&vec!["snippet".to_string(), "contentDetails".to_string()])
                .max_results(MAX_RESULT);
            let request = chunk.iter().fold(request, |req, id| req.add_id(id));
            let result = request.doit().await.unwrap_or_default();
            let (_, result) = result;
            let items = result.items.unwrap_or_default();
            for s in items.into_iter() {
                let id = s.id.unwrap_or_default();
                let snippet = s.snippet.unwrap_or_default();
                let tags = snippet.tags.unwrap_or_default();
                let content = s.content_details.unwrap_or_default();
                let duration = content.duration.unwrap_or_default();
                let song_pos = self
                    .songs
                    .iter()
                    .position(|sg| sg.id == id)
                    .unwrap_or_default();
                self.songs[song_pos].tags = tags;
                self.songs[song_pos].duration = parse_duration(&duration);
            }
        }
    }

    async fn load_all(&mut self) {
        if self.is_fully_loaded() || self.load_from_db() {
            return;
        };
        loop {
            println!("Loading Page {}", self.playlist.title);
            match self.load_page().await {
                Some(s) => self.next_page_token = s,
                None => {
                    self.is_loaded = true;
                    break;
                }
            }
        }
        self.fetch_songs_data().await;
        let _ = db::add_playlist(&self.source, self.to_playlist(), &self.songs, &self.etag);
    }

    async fn load_all_clone(mut self) -> Box<YoutubePlaylist> {
        self.load_all().await;
        Box::new(self)
    }

    fn is_fully_loaded(&self) -> bool {
        self.is_loaded
    }
    fn load_from_db(&mut self) -> bool {
        let db_bool = db::playlist_needs_update(&self.id, &self.source, &self.etag);
        if db_bool && !self.is_loaded {
            let playlist = db::load_playlist(&self.id, &self.source).unwrap_or_default();
            self.playlist.title = playlist.title;
            self.playlist.size = playlist.size;
            self.songs = db::get_playlist_songs(&self.id, &self.source).unwrap_or_default();
            self.is_loaded = true;
        }
        db_bool
    }
}

#[async_trait]
impl PlaylistTrait for YoutubePlaylist {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_source(&self) -> String {
        self.source.clone()
    }
    async fn get_songs(&mut self) -> Vec<Song> {
        if !self.is_fully_loaded() {
            self.load_all().await
        };
        self.songs.clone()
    }
    fn to_playlist(&self) -> Playlist {
        self.playlist.clone()
    }
}

pub struct Client {
    pub hub: YouTube<HttpsConnector<HttpConnector>>,
    pub name: String,
    playlists: Vec<YoutubePlaylist>,
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
    ) -> std::result::Result<Self, std::io::Error> {
        // Get an ApplicationSecret instance by some means. It contains the `client_id` and
        // `client_secret`, among other things.
        let secrets_location = config::get_config().secrets_location;
        let credentials_path = format!("{}/youtube_credentials.json", secrets_location);
        let token_path = format!("{}/youtube_tokencache.json", secrets_location);
        let secret = oauth2::read_application_secret(credentials_path).await;
        let secret = match secret {
            Err(e) => {
                println!("Cannot find credentials for youtube client : {}", e);
                return Err(e);
            }
            Ok(secret) => secret,
        };
        // Instantiate the authenticator. It will choose a suitable authentication flow for you,
        // unless you replace  `None` with the desired Flow.
        // Provide your own `AuthenticatorDelegate` to adjust the way it operates and get feedback about
        // what's going on. You probably want to bring in your own `TokenStorage` to persist tokens and
        // retrieve them from storage.
        let auth = oauth2::InstalledFlowAuthenticator::builder(
            secret,
            oauth2::InstalledFlowReturnMethod::HTTPRedirect,
        )
        .persist_tokens_to_disk(token_path)
        .flow_delegate(Box::new(CustomFlowDelegate::new(out_channel.clone())))
        .build()
        .await
        .unwrap();
        let hub = YouTube::new(
            hyper::Client::builder().build(
                hyper_rustls::HttpsConnectorBuilder::new()
                    .with_native_roots()
                    .https_or_http()
                    .enable_http1()
                    .enable_http2()
                    .build(),
            ),
            auth,
        );
        Ok(Client {
            hub,
            name: name.to_string(),
            playlists: Default::default(),
            in_channel,
            out_channel,
            playlist_loaded: false,
            all_loaded: false,
        })
    }
    pub async fn fetch_all_playlists(&mut self) {
        if !self.playlist_loaded {
            let mut liked_videos = self.load_playlist_by_id("LL").await;
            liked_videos.playlist.title = "Liked Videos".to_string();
            let mut playlists_list = self.load_all_playlists_mine().await;
            playlists_list.push(liked_videos);
            self.playlists = playlists_list;
            self.playlist_loaded = true;
        }
    }

    async fn load_all_playlists(&mut self) -> Vec<Playlist> {
        if !self.all_loaded {
            self.fetch_all_playlists().await;
            let playlists_list: Vec<_> = futures::stream::iter(self.playlists.clone())
                .map(|p| p.load_all_clone())
                .buffer_unordered(10)
                .collect()
                .await;
            self.all_loaded = true;
            self.playlists = playlists_list.into_iter().map(|p| *p).collect();
        }
        self.playlists.iter().map(|p| p.to_playlist()).collect()
    }

    async fn load_all_playlists_mine(&self) -> Vec<YoutubePlaylist> {
        let request = self
            .hub
            .playlists()
            .list(&vec!["snippet".to_string(), "contentDetails".to_string()])
            .mine(true)
            .max_results(MAX_RESULT);
        let result = request.doit().await.unwrap_or_default();
        let (_, result) = result;
        convert_playlist_list(result, &self.hub)
    }

    async fn load_playlist_by_id(&self, id: &str) -> YoutubePlaylist {
        match self.playlists.iter().find(|p| p.id == id) {
            Some(p) => p.clone(),
            None => {
                let request = self
                    .hub
                    .playlists()
                    .list(&vec!["snippet".to_string(), "contentDetails".to_string()])
                    .add_id(id)
                    .max_results(MAX_RESULT);
                let result = request.doit().await.unwrap_or_default();
                let (_, result) = result;

                convert_playlist_list(result, &self.hub)
                    .into_iter()
                    .next()
                    .unwrap()
            }
        }
    }
}
fn convert_playlist_list(
    content: PlaylistListResponse,
    hub: &YouTube<HttpsConnector<HttpConnector>>,
) -> Vec<YoutubePlaylist> {
    let items = content.items.unwrap_or_default();
    let mut playlists = vec![];
    for i in items {
        playlists.push(convert_playlist(i, hub.clone()));
    }
    playlists
}

fn song_from_item(item: PlaylistItem) -> Option<YoutubeSong> {
    let details = item.snippet.unwrap_or_default();
    let title = &details.title.unwrap_or_default();
    let id = details
        .resource_id
        .unwrap_or_default()
        .video_id
        .unwrap_or_default();
    let artists = match details.video_owner_channel_title.as_ref() {
        Some(artist) => artist,
        None => "", // if no artists then the video is private or deleted
    };
    if artists.is_empty() {
        None
    } else {
        Some(YoutubeSong::new(
            title.to_string(),
            vec![artists.to_string()],
            Default::default(),
            id,
            Default::default(),
            Default::default(),
        ))
    }
}

fn convert_playlist(
    playlist: YtPlaylist,
    hub: YouTube<HttpsConnector<HttpConnector>>,
) -> YoutubePlaylist {
    let snippet = playlist.snippet.unwrap_or_default();
    let content = playlist.content_details.unwrap_or_default();
    let title = snippet.title.unwrap_or_default();
    let size = content.item_count.unwrap_or_default();
    let id = playlist.id.unwrap_or_default();
    let etag = playlist.etag.unwrap_or_default();
    YoutubePlaylist::new(
        title,
        id,
        size,
        "".to_string(),
        etag,
        hub,
        "Youtube".to_string(),
    )
}

#[async_trait]
impl Source for Client {
    fn get_name(&self) -> String {
        self.name.to_string()
    }
    async fn get_all_playlists(&mut self) -> std::vec::Vec<Playlist> {
        self.load_all_playlists().await
    }

    fn get_number_of_playlist(&self) -> usize {
        self.playlists.len()
    }

    async fn listen(&mut self) {
        println!("Start listening");
        loop {
            let _ = match self.in_channel.recv().await {
                Ok(msg) => self.handle_request(msg).await,
                Err(e) => {
                    eprintln!("failed to read from socket; err = {:?}", e);
                } // TODO handle socket closing
            };
        }
    }

    async fn send(&self, data: Answer) {
        self.out_channel.send(data).await;
    }

    async fn init(&mut self) {
        self.fetch_all_playlists().await;
        self.get_all_playlists().await;
    }

    async fn download_songs(&self, songs: &[Song], playlist_title: String) {
        let songs = songs.to_vec();
        let name = self.name.clone();
        tokio::spawn(async move { utils::download_yt_playlist(songs, name, playlist_title).await });
    }

    async fn get_playlist_by_id(&mut self, id: &str) -> SourceResult<Box<dyn PlaylistTrait>> {
        let _ = self.load_all_playlists().await;
        let playlist = self.playlists.iter().cloned().find(|p| p.id == id);
        match playlist {
            Some(playlist) => Ok(Box::new(playlist)),
            None => Err(SourceError::PlaylistNotFound),
        }
    }
}

struct CustomFlowDelegate {
    out: Sender<Answer>,
}

impl CustomFlowDelegate {
    pub fn new(out: Sender<Answer>) -> Self {
        CustomFlowDelegate { out }
    }
}

impl InstalledFlowDelegate for CustomFlowDelegate {
    /// Configure a custom redirect uri if needed.
    fn redirect_uri(&self) -> Option<&str> {
        None
    }

    /// We need the user to navigate to a URL using their browser and potentially paste back a code
    /// (or maybe not). Whether they have to enter a code depends on the InstalledFlowReturnMethod
    /// used.
    fn present_user_url<'a>(
        &'a self,
        url: &'a str,
        need_code: bool,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
        Box::pin(present_user_url(url, need_code, self.out.clone()))
    }
}

async fn present_user_url(
    url: &str,
    need_code: bool,
    out: Sender<Answer>,
) -> Result<String, String> {
    let message: String = if need_code {
        "Inputting code to authenticate not supported".to_owned()
    } else {
        format!(
            "Please direct your browser to {} and follow the instructions displayed \
             there.",
            url
        )
    };
    send_request(
        out,
        Answer::new("Youtube".to_string(), AnswerType::Message(message)),
    )
    .await;
    Ok(String::new())
}
