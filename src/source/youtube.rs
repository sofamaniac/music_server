extern crate google_youtube3 as youtube3;
pub use crate::source::{Playlist, Song, Source};
use async_trait::async_trait;
use google_youtube3::hyper::client::HttpConnector;
use google_youtube3::hyper_rustls::HttpsConnector;
use google_youtube3::oauth2::authenticator_delegate::InstalledFlowDelegate;
use serde::Serialize;
use std::default::Default;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::runtime::Handle;
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc::Sender;
use youtube3::api::Playlist as YtPlaylist;
use youtube3::api::{PlaylistItem, PlaylistItemListResponse, PlaylistListResponse};
use youtube3::{chrono, hyper, hyper_rustls, oauth2, FieldMask, YouTube};

const MAX_RESULT: u32 = 50;

#[derive(Serialize)]
struct YoutubeSong {
    uri: String,
    id: String,
    title: String,
    duration: Duration,
    artists: Vec<String>,
    tags: Vec<String>,
}

impl YoutubeSong {
    pub fn new(
        id: String,
        title: String,
        duration: Duration,
        artists: Vec<String>,
        tags: Vec<String>,
    ) -> Self {
        YoutubeSong {
            uri: Default::default(),
            id,
            title,
            duration,
            artists,
            tags,
        }
    }
}

impl Song for YoutubeSong {
    fn get_title(&self) -> String {
        self.title.to_string()
    }

    fn get_artists(&self) -> Vec<String> {
        self.artists.iter().map(|s| s.to_string()).collect()
    }

    fn get_duration(&self) -> Duration {
        todo!()
    }

    fn get_url(&self) -> Vec<String> {
        todo!()
    }
}

#[derive(Serialize)]
struct YoutubePlaylist {
    name: String,
    songs: Vec<YoutubeSong>,
    size: u32,
    id: String,
    next_page_token: String,
    nb_loaded: u32,
}

impl YoutubePlaylist {
    pub fn new(name: String, id: String, size: u32, next_page: String, nb_loaded: u32) -> Self {
        YoutubePlaylist {
            name,
            songs: Vec::with_capacity(size as usize),
            size,
            id,
            next_page_token: next_page,
            nb_loaded,
        }
    }
}

impl Playlist for YoutubePlaylist {
    fn get_name(&self) -> String {
        todo!()
    }

    fn get_size(&self) -> u32 {
        todo!()
    }

    fn get_element_at_index(&self, index: u32) -> Box<dyn Song> {
        todo!()
    }
}

pub struct Client {
    pub hub: YouTube<HttpsConnector<HttpConnector>>,
    pub name: String,
    playlists: Vec<YoutubePlaylist>,
    in_channel: Receiver<String>,
    out_channel: Sender<String>,
}

impl Client {
    pub async fn new(
        name: String,
        in_channel: Receiver<String>,
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
        })
    }

    async fn load_all_playlists(&self) -> Vec<Box<dyn Playlist>> {
        let mut liked_videos = self.get_playlist_by_id("LL".to_string()).await;
        liked_videos.name = "Liked Videos".to_string();
        let mut playlists_list = self.get_all_playlists_mine().await;
        playlists_list.push(liked_videos);
        let mut result = vec![];
        for p in playlists_list {
            let p: Box<dyn Playlist> = Box::new(p);
            result.push(p)
        }
        result
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
        convert_playlist_list(result)
    }

    async fn get_playlist_by_id(&self, id: String) -> YoutubePlaylist {
        let request = self
            .hub
            .playlist_items()
            .list(&vec!["snippet".to_string(), "contentDetails".to_string()])
            .playlist_id(&id)
            .max_results(MAX_RESULT);
        let result = request.doit().await.unwrap();
        let (_, result) = result;
        convert_playlist_item(result, id)
    }
}
fn convert_playlist_list(content: PlaylistListResponse) -> Vec<YoutubePlaylist> {
    let items = content.items.unwrap();
    let mut playlists = vec![];
    for i in items {
        playlists.push(convert_playlist(i));
    }
    for p in &playlists {
        println!("{}", p.name);
    }
    playlists
}

fn song_from_item(item: &PlaylistItem) -> YoutubeSong {
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
        None => {
            println!("No artist for {}", title);
            ""
        }
    };
    println!("{:?}", title);
    YoutubeSong::new(
        id.to_string(),
        title.to_string(),
        Default::default(),
        vec![artists.to_string()],
        Default::default(),
    )
}

fn convert_playlist_item(content: PlaylistItemListResponse, id: String) -> YoutubePlaylist {
    let items = content.items.unwrap();
    let len = items.len();
    let songs: Vec<YoutubeSong> = items.iter().map(song_from_item).collect();
    let size = content.page_info.unwrap().total_results.unwrap();
    let next_page = content.next_page_token.unwrap();
    let mut playlist =
        YoutubePlaylist::new("title".to_string(), id, size as u32, next_page, len as u32);
    playlist.nb_loaded = songs.len() as u32;
    playlist.songs = songs;
    playlist
}

fn convert_playlist(playlist: YtPlaylist) -> YoutubePlaylist {
    let title = playlist.snippet.unwrap().title.unwrap();
    let size = playlist.content_details.unwrap().item_count.unwrap();
    let id = playlist.id.unwrap();
    YoutubePlaylist::new(title, id, size, "".to_string(), 0)
}

#[async_trait]
impl Source for Client {
    fn get_name(&self) -> String {
        self.name.to_string()
    }
    async fn get_all_playlists(&self) -> std::vec::Vec<std::boxed::Box<(dyn Playlist)>> {
        self.load_all_playlists().await
    }

    async fn get_playlist_by_id(&self, id: String) -> Box<(dyn Playlist)> {
        let playlist = self.get_playlist_by_id(id).await;
        Box::new(playlist)
    }

    fn get_number_of_playlist(&self) -> usize {
        self.playlists.len()
    }

    async fn get_playlist_by_index(&self, index: u32) -> Box<dyn Playlist> {
        todo!()
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
    use tokio::io::AsyncBufReadExt;
    println!("In present_user_url {}", need_code);
    if need_code {
        let message = format!(
            "Please direct your browser to {}, follow the instructions and enter the \
             code displayed here: ",
            url
        );
        match out.send(message).await {
            Ok(_) => println!("message send successfully"),
            Err(err) => println!("{}", err),
        };
        println!(
            "Please direct your browser to {}, follow the instructions and enter the \
             code displayed here: ",
            url
        );
        let mut user_input = String::new();
        tokio::io::BufReader::new(tokio::io::stdin())
            .read_line(&mut user_input)
            .await
            .map_err(|e| format!("couldn't read code: {}", e))?;
        // remove trailing whitespace.
        user_input.truncate(user_input.trim_end().len());
        Ok(user_input)
    } else {
        let message = format!(
            "Please direct your browser to {} and follow the instructions displayed \
             there.",
            url
        );
        match out.send(message).await {
            Ok(_) => println!("message send successfully"),
            Err(err) => println!("{}", err),
        };
        Ok(String::new())
    }
}
