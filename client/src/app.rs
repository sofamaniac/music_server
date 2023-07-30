use std::{
    sync::{Arc, Mutex},
    vec,
};

use music_server::{
    request::{Answer, AnswerType, ObjRequest, Request, RequestType, self},
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

enum Panel {
    Sources,
    Playlists,
    Songs,
}

pub enum Direction {
    Up,
    Down,
    RightPanel,
    LeftPanel,
    UpPanel,
    DownPanel,
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
    current_panel: Panel,
}

impl App {
    pub fn new(stream: OwnedWriteHalf) -> Self {
        App {
            stream,
            sources: Default::default(),
            state: Default::default(),
            current_panel: Panel::Sources,
        }
    }

    pub async fn handle_answer(&mut self, answer: Answer) {
        eprintln!("{:?}", answer);
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
        eprintln!("{:?}", request);
        let json = serde_json::to_string(request).unwrap();
        let message = request::prepare_message(json);
        self.stream.writable().await;
        match self.stream.try_write(&message) {
            Ok(n) => (),
            Err(err) => eprintln!("{:?}", err)
        };
    }

    pub async fn add_source(&mut self, source: String) {
        self.sources.push(SourceWidget::new(source.clone()));
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

    pub fn get_playlist_widget(&self) -> List<'_> {
        match self.state.selected() {
            Some(i) if i < self.sources.len() => {
                let source = &self.sources[i];
                source.get_playlists_widget()
            }
            _ => make_list(vec![], "Playlists"),
        }
    }

    pub fn get_playlists_state(&self) -> ListState {
        match self.state.selected() {
            Some(i) if i < self.sources.len() => {
                let source = &self.sources[i];
                source.state.clone()
            }
            _ => Default::default(),
        }
    }

    pub fn set_playlist_state(&mut self, off: i32) {
        match self.state.selected() {
            Some(i) if i < self.sources.len() => {
                let source = &mut self.sources[i];
                let selected = source.state.selected();
                source
                    .state
                    .select(Some(compute_new_i(selected, off, source.playlist.len())));
            }
            _ => (),
        }
    }

    pub fn set_song_state(&mut self, off: i32) {
        match self.state.selected() {
            Some(i) if i < self.sources.len() => {
                let source = &mut self.sources[i];
                match source.state.selected() {
                    Some(i) if i < source.playlist.len() => {
                        let playlist = &mut source.playlist[i];
                        let selected = playlist.state.selected();
                        playlist.state.select(Some(compute_new_i(
                            selected,
                            off,
                            playlist.playlist.size as usize,
                        )));
                    }
                    _ => (),
                }
            }
            _ => (),
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

    pub async fn handle_event(&mut self, event: Direction) {
        match event {
            Direction::RightPanel => self.current_panel = Panel::Songs,
            Direction::LeftPanel => self.current_panel = Panel::Playlists,
            Direction::Up => self.move_current_panel(-1),
            Direction::Down => self.move_current_panel(1),
            Direction::DownPanel => self.current_panel = Panel::Playlists,
            Direction::UpPanel => self.current_panel = Panel::Sources,
        }
        self.move_current_panel(0);
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
        eprintln!("{:?}", playlist.songs)
    }

    pub fn get_songs_widget(&self) -> List<'_> {
        match self.state.selected() {
            Some(i) if i < self.sources.len() => {
                let source = &self.sources[i];
                match source.state.selected() {
                    Some(i) if i < source.playlist.len() => {
                        let playlist = &source.playlist[i];
                        let items = playlist
                            .songs
                            .iter()
                            .map(|s| ListItem::new(s.title.clone()))
                            .collect();
                        make_list(items, "Songs")
                    }
                    _ => make_list(vec![], "Songs"),
                }
            }
            _ => make_list(vec![], "Songs"),
        }
    }

    pub fn get_songs_state(&self) -> ListState {
        match self.state.selected() {
            Some(i) if i < self.sources.len() => {
                let source = &self.sources[i];
                match source.state.selected() {
                    Some(i) if i < source.playlist.len() => {
                        let playlist = &source.playlist[i];
                        playlist.state.clone()
                    }
                    _ => Default::default(),
                }
            }
            _ => Default::default(),
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
