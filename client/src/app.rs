use std::{
    sync::{Arc, Mutex},
    vec,
};

use music_server::{
    request::{self, Answer, AnswerType, ObjRequest, Request, RequestType},
    source_types::{Playlist, Song},
};
use tokio::{
    io::WriteHalf,
    net::{tcp::OwnedWriteHalf, TcpStream},
};
use tui::{
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::player::Player;

pub enum Panel {
    Sources,
    Playlists,
    Songs,
}

pub enum Event {
    Move(Direction),
    Play,
    Pause,
    Stop,
    Shuffle,
    VolumeUp,
    VolumeDown,
    Download,
    Auto,
    Next,
    Prev,
    SeekForward,
    SeekBackward,
}

pub enum Direction {
    Up,
    Down,
    RightPanel,
    LeftPanel,
    UpPanel,
    DownPanel,
}

#[derive(Debug)]
struct Route {
    source: Option<usize>,
    playlist: Option<usize>,
    song: Option<usize>,
}

#[derive(Default, Clone)]
pub struct SourceWidget {
    state: ListState,
    playlist: Vec<PlaylistWidget>,
    pub name: String,
}

#[derive(Default, Clone)]
pub struct PlaylistWidget {
    state: ListState,
    playlist: Playlist,
    pub name: String,
    songs: Vec<Song>,
}

impl PlaylistWidget {
    pub fn from_playlist(playlist: Playlist) -> Self {
        PlaylistWidget {
            state: Default::default(),
            playlist: playlist.clone(),
            name: playlist.title,
            songs: Default::default(),
        }
    }

    fn add_songs(&mut self, songs: Vec<Song>) {
        self.songs = songs;
    }
}

impl SourceWidget {
    pub fn new(name: String) -> Self {
        SourceWidget {
            state: Default::default(),
            playlist: Default::default(),
            name,
        }
    }
    pub fn get_playlists_widget(&self) -> List<'_> {
        make_list(
            self.playlist
                .iter()
                .cloned()
                .map(|p| ListItem::new(p.name))
                .collect(),
            "Playlists",
        )
    }

    fn add_playlistlist(&mut self, playlistlist: Vec<Playlist>) {
        self.playlist = playlistlist
            .into_iter()
            .map(PlaylistWidget::from_playlist)
            .collect();
    }
}

pub struct App {
    pub stream: OwnedWriteHalf,
    sources: Vec<SourceWidget>,
    pub state: ListState,
    pub current_panel: Panel,
    pub player: Player,
}

impl App {
    pub fn new(stream: OwnedWriteHalf) -> Self {
        App {
            stream,
            sources: Default::default(),
            state: Default::default(),
            current_panel: Panel::Sources,
            player: Player::new(),
        }
    }

    fn get_current_route(&self) -> Route {
        let source = self.state.selected();
        let playlist = match source {
            Some(i) if i < self.sources.len() => self.sources[i].state.selected(),
            _ => Default::default(),
        };
        let song = match playlist {
            Some(i) if i < self.sources[source.unwrap()].playlist.len() => {
                self.sources[source.unwrap()].playlist[i].state.selected()
            }
            _ => Default::default(),
        };

        Route {
            source,
            playlist,
            song,
        }
    }

    pub async fn handle_answer(&mut self, answer: Answer) {
        match answer.data {
            AnswerType::Client(name) => self.add_source(name).await,
            AnswerType::PlaylistList(playlistlist) => {
                self.add_playlist_list(answer.client.clone(), playlistlist.clone());
                for p in playlistlist {
                    self.load_playlist(answer.client.clone(), p).await;
                }
            }
            AnswerType::Songs(playlist, songs) => self.add_songs(answer.client, playlist, songs),
            _ => (),
        }
    }

    pub async fn send_request(&self, request: &Request) {
        let json = serde_json::to_string(request).unwrap();
        let message = request::prepare_message(json);
        self.stream.writable().await;
        match self.stream.try_write(&message) {
            Ok(n) => (),
            Err(err) => eprintln!("{:?}", err),
        };
    }

    pub async fn add_source(&mut self, source: String) {
        self.sources.push(SourceWidget::new(source.clone()));
        if self.state.selected().is_none() {
            self.state.select(Some(0));
        }
        self.send_request(&Request {
            client: source,
            ty: RequestType::GetAll(ObjRequest::PlaylistList),
        })
        .await;
    }

    pub async fn request_sources(&self) {
        let request = Request {
            client: "all".to_owned(),
            ty: RequestType::GetAll(ObjRequest::ClientList),
        };
        self.send_request(&request).await;
    }

    pub fn get_sources_widget(&self) -> List<'_> {
        let sources: Vec<ListItem> = self
            .sources
            .iter()
            .cloned()
            .map(|s| ListItem::new(s.name))
            .collect();
        make_list(sources, "Sources")
    }

    pub fn get_playlists_widget(&self) -> List<'_> {
        let route = self.get_current_route();
        match route.source {
            Some(i) => self.sources[i].get_playlists_widget(),
            _ => make_list(vec![], "Playlist"),
        }
    }

    pub fn get_playlists_state(&self) -> ListState {
        let route = self.get_current_route();
        match route.source {
            Some(i) => self.sources[i].state.clone(),
            _ => Default::default(),
        }
    }

    pub fn set_playlist_state(&mut self, off: i32) {
        let route = self.get_current_route();
        if let Some(i) = route.source {
            let source = &mut self.sources[i];
            source.state.select(Some(compute_new_i(
                route.playlist,
                off,
                source.playlist.len(),
            )));
        }
    }

    pub fn set_song_state(&mut self, off: i32) {
        let route = self.get_current_route();
        if let Some(s) = route.source {
            if let Some(p) = route.playlist {
                let playlist = &mut self.sources[s].playlist[p];
                playlist.state.select(Some(compute_new_i(
                    route.song,
                    off,
                    playlist.playlist.size as usize,
                )));
            }
        }
    }

    pub async fn load_playlist(&mut self, client: String, playlist: Playlist) {
        let request = Request {
            client: client.clone(),
            ty: RequestType::GetAll(ObjRequest::Playlist(playlist.id.clone())),
        };
        self.send_request(&request).await;
    }

    fn add_playlist_list(&mut self, client: String, playlistlist: Vec<Playlist>) {
        let source: &mut SourceWidget = self.sources.iter_mut().find(|s| s.name == client).unwrap();
        source.add_playlistlist(playlistlist);
    }
    pub fn handle_move(&mut self, dir: Direction) {
        match dir {
            Direction::RightPanel => self.current_panel = Panel::Songs,
            Direction::LeftPanel => self.current_panel = Panel::Playlists,
            Direction::Up => self.move_current_panel(-1),
            Direction::Down => self.move_current_panel(1),
            Direction::DownPanel => self.current_panel = Panel::Playlists,
            Direction::UpPanel => self.current_panel = Panel::Sources,
        }
    }
    pub fn play(&mut self) {
        let route = self.get_current_route();
        if let Some(s) = route.source {
            if let Some(p) = route.playlist {
                if let Some(c) = route.song {
                    let song = &self.sources[s].playlist[p].songs[c];
                    self.player.play(&song.url);
                }
            }
        }
    }

    pub async fn handle_event(&mut self, event: Event) {
        match event {
            Event::Move(dir) => self.handle_move(dir),
            Event::Play => self.play(),
            Event::Pause => {
                self.player.playpause();
            }
            Event::VolumeUp => self.player.incr_volume(5),
            Event::VolumeDown => self.player.incr_volume(-5),
            Event::Download => self.download().await,
            Event::Shuffle => self.player.shuffle(),
            Event::Prev => self.player.prev(),
            Event::Next => self.player.next(),
            Event::Auto => self.auto(),
            Event::SeekForward => self.player.seek(5),
            Event::SeekBackward => self.player.seek(-5),
            _ => (),
        }
        self.move_current_panel(0);
    }

    async fn download(&self) {
        let route = self.get_current_route();
        if let Some(p) = route.playlist {
            let source_name = self.sources[route.source.unwrap()].name.clone();
            let playlist_id = self.sources[route.source.unwrap()].playlist[p].playlist.id.clone();
            self.send_request(&Request {client: source_name, ty: RequestType::Download(ObjRequest::Playlist(playlist_id))}).await;
        }
    }

    pub fn move_current_panel(&mut self, off: i32) {
        match self.current_panel {
            Panel::Sources => {
                let index = self.state.selected();
                self.state
                    .select(Some(compute_new_i(index, off, self.sources.len())))
            }
            Panel::Playlists => self.set_playlist_state(off),
            Panel::Songs => self.set_song_state(off),
        }
    }

    fn add_songs(&mut self, client: String, playlist: Playlist, songs: Vec<Song>) {
        let source: &mut SourceWidget = self.sources.iter_mut().find(|s| s.name == client).unwrap();
        let playlist = source
            .playlist
            .iter_mut()
            .find(|p| p.playlist.id == playlist.id)
            .unwrap();
        playlist.add_songs(songs);
    }

    pub fn get_songs_widget(&self) -> List<'_> {
        let route = self.get_current_route();
        if let Some(s) = route.source {
            if let Some(p) = route.playlist {
                let playlist = &self.sources[s].playlist[p];
                let items = playlist
                    .songs
                    .iter()
                    .map(|s| ListItem::new(s.title.clone()))
                    .collect();
                make_list(items, "Songs")
            } else {
                make_list(vec![], "Songs")
            }
        } else {
            make_list(vec![], "Songs")
        }
    }

    pub fn get_songs_state(&self) -> ListState {
        let route = self.get_current_route();
        if let Some(s) = route.source {
            if let Some(p) = route.playlist {
                let playlist = &self.sources[s].playlist[p];
                playlist.state.clone()
            } else {
                Default::default()
            }
        } else {
            Default::default()
        }
    }

    pub fn get_options_widget(&self) -> List<'_> {
        let items = vec![
            ListItem::new(format!("Auto: {}", self.player.is_in_playlist())),
            ListItem::new("Repeat"),
            ListItem::new(format!("Shuffle: {}", self.player.is_shuffled())),
            ListItem::new(format!("Volume: {}/100", self.player.get_volume())),
        ];
        make_list(items, "Options")
    }

    pub fn get_info_widget(&self) -> List<'_> {
        let route = self.get_current_route();
        if let Some(song) = route.song {
            let playlist = route.playlist.unwrap();
            let source = route.source.unwrap();
            let song = &self.sources[source].playlist[playlist].songs[song];
            let items = vec![
                ListItem::new(format!("Title:\n {}", song.title.clone())),
                ListItem::new(format!("Artists:\n {}", song.artists.clone().join(","))),
            ];
            make_list(items, "Information")
        } else {
            make_list(vec![], "Information")
        }
    }

    pub fn get_current_song(&self) -> Song {
        let route = self.get_current_route();
        if let Some(song) = route.song {
            let playlist = route.playlist.unwrap();
            let source = route.source.unwrap();
            let song = &self.sources[source].playlist[playlist].songs[song];
            song.clone()
        } else {
            Default::default()
        }
    }

    pub fn get_playing_song_info(&self) -> Song {
        let state = self.player.get_state();
        let parts: Vec<&str> = state.path.split('/').collect();
        if parts.len() < 3 {
            return Default::default()
        }
        let playlist = parts[parts.len() -2];
        let source = parts[parts.len() -3];
        if let Some(source) = self.sources.iter().find(|s| s.name == source) {
            if let Some(playlist) = source.playlist.iter().find(|p| p.name == playlist) {
                if let Some(song) = playlist.songs.iter().find(|s| s.title == state.title) {
                    song.clone()
                } else { Default::default()}
            } else { Default::default() }
        } else { Default::default() }
    }

    fn auto(&mut self) {
        let route = self.get_current_route();
        if let Some(p) = route.playlist {
            let source = &self.sources[route.source.unwrap()];
            let playlist = &source.playlist[p].songs;
            // a bit ugly, but there seems to be no good solution
            // to convert from Vec<String> to Vec<&str>
            let songs: Vec<&str> = playlist.iter().map(|s| String::as_ref(&s.url)).collect();
            self.player.set_auto(&songs);
        }
    }

    pub fn is_auto(&self) -> bool {
        self.player.is_in_playlist()
    }

    pub fn set_auto_val(&mut self, val: bool)  {
        if val == self.player.is_in_playlist() {
            return;
        } else {
            self.auto()
        }
    }

    pub fn set_pause_val(&mut self, val: bool)  {
        if val == self.player.paused() {
            return
        } else {
            self.player.playpause()
        }
    }
}

fn make_list<'a>(items: Vec<ListItem<'a>>, title: &'a str) -> List<'a> {
    List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::ITALIC)
                .bg(Color::White)
                .fg(Color::Black),
        )
}

fn compute_new_i(i: Option<usize>, off: i32, max: usize) -> usize {
    match i {
        None => {
            if off < 0 {
                0
            } else {
                off as usize
            }
        }
        Some(i) => {
            let res = (i as i32) + off;
            if res < 0 {
                0
            } else {
                std::cmp::min((i as i32 + off) as usize, max - 1)
            }
        }
    }
}
