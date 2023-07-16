#![warn(clippy::unwrap_used)]
extern crate google_youtube3 as youtube3;
use crate::request::{send_request, Request, RequestType};
use crate::source::Song as YoutubeSong;
pub use crate::source::{Playlist, Song, Source};
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
use RequestType::*;

use super::PlaylistTrait;
use rusqlite::Connection;

const MAX_RESULT: u32 = 50;

#[derive(Clone)]
struct YoutubePlaylist {
    playlist: Playlist,
    songs: Vec<YoutubeSong>,
    id: String,
    next_page_token: String,
    nb_loaded: u32,
    etag: String, // used to check if playlist has changed
    hub: YouTube<HttpsConnector<HttpConnector>>,
    is_loaded: bool,
}

impl YoutubePlaylist {
    pub fn new(
        name: String,
        id: String,
        size: u32,
        next_page: String,
        nb_loaded: u32,
        etag: String,
        hub: YouTube<HttpsConnector<HttpConnector>>,
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
            nb_loaded,
            etag,
            hub,
            is_loaded: false,
        }
    }

    async fn load_page(&mut self, page_token: String) -> Option<String> {
        let request = self
            .hub
            .playlist_items()
            .list(&vec!["snippet".to_string(), "contentDetails".to_string()])
            .playlist_id(&self.id)
            .max_results(MAX_RESULT)
            .page_token(&page_token.to_string());
        let result = request.doit().await.unwrap();
        let (_, result) = result;
        let items = result.items.unwrap();
        let songs: Vec<YoutubeSong> = items.iter().flat_map(song_from_item).collect();
        for s in songs.into_iter() {
            self.songs.push(s)
        }
        result.next_page_token
    }

    async fn load_all(&mut self) {
        if self.is_fully_loaded() {
            return;
        };
        loop {
            println!("Loading Page {}", self.playlist.title);
            match self.load_page(self.next_page_token.clone()).await {
                Some(s) => self.next_page_token = s,
                None => {
                    self.is_loaded = true;
                    break;
                }
            }
        }
    }

    async fn load_all_clone(self) -> Box<YoutubePlaylist> {
        let mut p = self.clone();
        p.load_all().await;
        Box::new(p)
    }

    fn is_fully_loaded(&self) -> bool {
        self.is_loaded
    }
}

#[async_trait]
impl PlaylistTrait for YoutubePlaylist {
    fn get_element_at_index(&self, index: u32) -> Song {
        self.songs[index as usize].clone()
    }
    async fn get_songs(&mut self) -> Vec<Song> {
        if !self.is_fully_loaded() {
            self.load_all().await
        };
        let mut songs: Vec<Song> = Vec::new();
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
    out_channel: Sender<String>,
    playlist_loaded: bool,
    all_loaded: bool,
}

impl Client {
    pub async fn new(
        name: String,
        in_channel: Receiver<Request>,
        out_channel: Sender<String>,
    ) -> std::result::Result<Self, std::io::Error> {
        // Get an ApplicationSecret instance by some means. It contains the `client_id` and
        // `client_secret`, among other things.
        let secret = oauth2::read_application_secret("data/secrets/youtube_credentials.json").await;
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
        .persist_tokens_to_disk("data/secrets/youtube_tokencache.json")
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
            name,
            playlists: Default::default(),
            in_channel,
            out_channel,
            playlist_loaded: false,
            all_loaded: false,
        })
    }
    pub async fn fetch_all_playlists(&mut self) {
        if !self.playlist_loaded {
            let mut liked_videos = self.get_playlist_by_id("LL".to_string()).await;
            liked_videos.playlist.title = "Liked Videos".to_string();
            let mut playlists_list = self.get_all_playlists_mine().await;
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
        self.playlists
            .clone()
            .into_iter()
            .map(|p| p.to_playlist())
            .collect()
    }

    async fn get_all_playlists_mine(&self) -> Vec<YoutubePlaylist> {
        let request = self
            .hub
            .playlists()
            .list(&vec!["snippet".to_string(), "contentDetails".to_string()])
            .mine(true)
            .max_results(MAX_RESULT);
        let result = request.doit().await.unwrap();
        let (_, result) = result;
        convert_playlist_list(result, self.hub.clone())
    }

    async fn get_playlist_by_id(&self, id: String) -> YoutubePlaylist {
        match self.playlists.iter().find(|p| p.id == id) {
            Some(p) => p.clone(),
            None => {
                let request = self
                    .hub
                    .playlists()
                    .list(&vec!["snippet".to_string(), "contentDetails".to_string()])
                    .add_id(&id)
                    .max_results(MAX_RESULT);
                let result = request.doit().await.unwrap();
                let (_, result) = result;

                convert_playlist_list(result, self.hub.clone())
                    .into_iter()
                    .next()
                    .unwrap()
            }
        }
    }

    async fn handle_request(&mut self, request: Request) {
        println!("{}", request.client);
        if request.client == self.name || request.client == "all" {
            match request.ty {
                GetAll(crate::request::Obj::PlaylistList) => {
                    let playlists = self.get_all_playlists().await;
                    let serialized = serde_json::to_string(&playlists).unwrap();
                    self.out_channel.send(serialized).await.expect("hoho");
                }
                GetAll(crate::request::Obj::Playlist(id)) => {
                    let _ = self.load_all_playlists().await;
                    let mut playlist = self
                        .playlists
                        .clone()
                        .into_iter()
                        .find(|p| p.id == id)
                        .unwrap();
                    let serialized = serde_json::to_string(&playlist.get_songs().await).unwrap();
                    self.out_channel.send(serialized).await.expect("hoho");
                }
                Get(crate::request::Attr::Name) => self
                    .out_channel
                    .send(self.name.clone())
                    .await
                    .expect("hoho"),

                _ => println!("TODO"),
            }
        }
    }
}
fn convert_playlist_list(
    content: PlaylistListResponse,
    hub: YouTube<HttpsConnector<HttpConnector>>,
) -> Vec<YoutubePlaylist> {
    let items = content.items.unwrap();
    let mut playlists = vec![];
    for i in items {
        playlists.push(convert_playlist(i, hub.clone()));
    }
    playlists
}

fn song_from_item(item: &PlaylistItem) -> Option<YoutubeSong> {
    let details = item.snippet.as_ref().unwrap();
    let title = &details.title.as_ref().unwrap();
    let id = details
        .resource_id
        .as_ref()
        .unwrap()
        .video_id
        .as_ref()
        .unwrap();
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
            id.to_string(),
            Default::default(),
            Default::default(),
        ))
    }
}

fn convert_playlist(
    playlist: YtPlaylist,
    hub: YouTube<HttpsConnector<HttpConnector>>,
) -> YoutubePlaylist {
    let title = playlist.snippet.unwrap().title.unwrap();
    let size = playlist.content_details.unwrap().item_count.unwrap();
    let id = playlist.id.unwrap();
    let etag = playlist.etag.unwrap();
    YoutubePlaylist::new(title, id, size, "".to_string(), 0, etag, hub)
}

#[async_trait]
impl Source for Client {
    fn get_name(&self) -> String {
        self.name.to_string()
    }
    async fn get_all_playlists(&mut self) -> std::vec::Vec<Playlist> {
        self.load_all_playlists().await
    }

    async fn get_playlist_by_id(&self, id: String) -> Playlist {
        let p = self.get_playlist_by_id(id).await;
        p.to_playlist()
    }

    fn get_number_of_playlist(&self) -> usize {
        self.playlists.len()
    }

    async fn get_playlist_by_index(&self, index: u32) -> Playlist {
        todo!()
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

    async fn send(&self, data: String) {
        self.out_channel.send(data).await;
    }

    async fn init(&mut self) {
        self.fetch_all_playlists().await;
        self.get_all_playlists().await;
    }
}

struct CustomFlowDelegate {
    out: Sender<String>,
}

impl CustomFlowDelegate {
    pub fn new(out: Sender<String>) -> Self {
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
    out: Sender<String>,
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
        Request::new("youtube".to_string(), RequestType::Message(message)),
    )
    .await;
    Ok(String::new())
}
