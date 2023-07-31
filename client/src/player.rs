use libmpv::{FileState, Mpv};
use music_server::source_types::Song;

pub struct Player {
    player: Mpv,
    shuffled: bool,
    in_playlist: bool,
}

pub struct State {
    pub duration: i64,
    pub time_pos: i64,
    pub volume: i64,
    pub title: String,
}

impl Player {
    pub fn new() -> Self {
        let player = Mpv::new().unwrap();
        player.set_property("video", false).unwrap();
        player.set_property("ytdl", true).unwrap();
        Player {
            player,
            shuffled: false,
            in_playlist: false,
        }
    }

    pub fn get_state(&self) -> State {
        let duration = self.player.get_property("duration").unwrap_or_default();
        let time_pos = self.player.get_property("time-pos").unwrap_or_default();
        let volume = self.player.get_property("volume").unwrap_or_default();
        let title = self.player.get_property("media-title").unwrap_or_default();
        State {
            duration,
            time_pos,
            volume,
            title,
        }
    }

    pub fn playpause(&mut self) {
        let pause: bool = self.player.get_property("pause").unwrap();
        if pause {
            self.player.unpause();
        } else {
            self.player.pause();
        }
    }

    pub fn play(&mut self, url: &str) {
        // It is necessary to surround the url with quotes to avoid errors
        match self.player.command("loadfile", &[&format!("\"{}\"", url)]) {
            Ok(_) => (),
            Err(e) => eprintln!("error {:?}", e),
        };
    }

    pub fn get_volume(&self) -> i64 {
        self.player.get_property("volume").unwrap()
    }

    pub fn incr_volume(&mut self, dv: i64) {
        let volume = self.get_volume();
        let volume = std::cmp::min(volume + dv, 100);
        let volume = std::cmp::max(volume, 0);
        self.player.set_property("volume", volume);
    }

    pub fn shuffle(&mut self) {
        if !self.in_playlist {
            return;
        }
        if self.shuffled {
            self.player.command("playlist-unshuffle", &[]).unwrap();
        } else {
            self.player.command("playlist-shuffle", &[]).unwrap();
        }
        self.shuffled = !self.shuffled;
    }

    pub fn set_auto(&mut self, playlist: &[&str]) {
        self.player.playlist_clear().unwrap();
        if !self.in_playlist {
            let files: Vec<(&str, FileState, Option<_>)> = playlist
                .iter()
                .cloned()
                .map(|s| (s, FileState::AppendPlay, None))
                .collect();
            self.player.playlist_load_files(&files).unwrap();
        }
        self.in_playlist = !self.in_playlist;
    }

    pub fn next(&self) {
        self.player.playlist_next_weak().unwrap();
    }

    pub fn prev(&self) {
        self.player.playlist_previous_weak().unwrap();
    }

    pub fn is_shuffled(&self) -> bool {
        self.shuffled
    }

    pub fn is_in_playlist(&self) -> bool {
        self.in_playlist
    }
}
