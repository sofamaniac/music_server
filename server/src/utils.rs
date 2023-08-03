use futures::{Future, StreamExt};
use music_server::{
    request::{Answer, AnswerType},
    source_types::Playlist,
};
use regex::{Regex, RegexSet};
use reqwest::Url;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{mpsc::Sender, Mutex};
use ytd_rs::{Arg, YoutubeDL};

use crate::{
    config, db, lastfm,
    source::{self, spotify::SpotifySong, youtube::YoutubeSong, Song, SongTrait},
};
pub type UtilsResult<T> = Result<T, UtilsError>;

#[derive(Debug)]
pub enum UtilsError {
    YtDLErr,
}

pub fn parse_duration(duration: &str) -> Duration {
    // TODO Add week support (cf. RCF 3339 Appendix A)
    let patterns = [
        r"^P(.+)$",
        r"^P(?:(?<y>\d+)Y)?(?:(?<mo>\d+)M)?(?:(?<d>\d+)D)?",
        r"^P(.*)T(.+)$",
        r"T(?:(?<h>\d+)H)?(?:(?<mi>\d+)M)?(?:(?<s>\d+)S)?$",
    ];
    let regex_set = RegexSet::new(patterns).unwrap();
    let regexes: Vec<_> = regex_set
        .patterns()
        .iter()
        .map(|pat| Regex::new(pat).unwrap())
        .collect();
    let matches = regex_set.matches(duration);
    let is_correct =
        (matches.matched(0) && matches.matched(1)) || (matches.matched(2) && matches.matched(3));
    if !is_correct {
        Default::default()
    }
    let duration_long: Duration = if matches.matched(0) {
        let caps = regexes[1].captures(duration).unwrap();
        let sec_day = 24 * 3600;
        let year: u64 = match caps.name("y") {
            Some(val) => val.as_str().parse().unwrap_or_default(),
            _ => 0,
        };
        let month: u64 = match caps.name("mo") {
            Some(val) => val.as_str().parse().unwrap_or_default(),
            _ => 0,
        };
        let day: u64 = match caps.name("d") {
            Some(val) => val.as_str().parse().unwrap_or_default(),
            _ => 0,
        };
        // We consider that there is 365 days in a year and 30 days in a month
        Duration::from_secs(365 * sec_day * year + 30 * sec_day * month + sec_day * day)
    } else {
        Default::default()
    };
    let duration_short: Duration = if matches.matched(2) {
        let caps = regexes[3].captures(duration).unwrap();

        let hours: u64 = match caps.name("h") {
            Some(val) => val.as_str().parse().unwrap_or_default(),
            _ => 0,
        };
        let min: u64 = match caps.name("mi") {
            Some(val) => val.as_str().parse().unwrap_or_default(),
            _ => 0,
        };
        let sec: u64 = match caps.name("s") {
            Some(val) => val.as_str().parse().unwrap_or_default(),
            _ => 0,
        };

        Duration::from_secs(3600 * hours + 60 * min + sec)
    } else {
        Default::default()
    };
    duration_long + duration_short
}
async fn download_song<T: SongTrait>(
    mut song: T,
    client: Arc<str>,
    playlist_title: Arc<str>,
    args: Vec<Arg>,
    link: String,
) -> UtilsResult<T> {
    let config = config::get_config();
    let folder: String = config.data_location + &format!("/music/{}/{}/", client, playlist_title);
    let folder = PathBuf::from(folder);
    println!("Downloading {}", song.get_title());
    let ytdlp = YoutubeDL::new(&folder, args, &link).unwrap();
    let result = tokio::task::spawn_blocking(move || ytdlp.download())
        .await
        .unwrap();
    match result {
        Ok(result) => {
            let path = result.output().trim();
            song.set_url(path.to_string());
            song.set_downloaded(true);
            Ok(song)
        }
        Err(err) => {
            println!("{}", err);
            Err(UtilsError::YtDLErr)
        }
    }
}

pub async fn download_yt_song(
    song: YoutubeSong,
    client: Arc<str>,
    playlist_title: Arc<str>,
) -> UtilsResult<YoutubeSong> {
    let config = config::get_config();
    let out_template = &config.yt_dlp_output_template;
    let args = vec![
        Arg::new("--quiet"),
        Arg::new("--extract-audio"),
        //Arg::new("--embed-metadata"),
        Arg::new("--embed-thumbnail"),
        Arg::new("--add-metadata"),
        Arg::new_with_arg("--output", out_template),
        Arg::new_with_arg("--print", "after_move:filepath"),
    ];
    let link = format!("https://youtube.com/watch?v={}", song.get_id());
    download_song(song, client, playlist_title, args, link).await
}

async fn get_song_title(song: &SpotifySong) -> String {
    match &song.isrc {
        Some(isrc) => { //format!("https://music.youtube.com/search?q={}+{}#Songs", title, isrc),
        let title = song.get_title().to_string();
        let artists = &song.get_artists().join(" ");
        let url = Url::parse_with_params("https://youtube.com/search?", [("q", title + " " + artists)]);
        url.unwrap().as_str().to_string()
        }
        None => {
            match lastfm::find_youtube_link(
                &song.get_title(),
                &song.get_artists().join("+").replace(' ', "+"),
            )
            .await
            {
                Some(url) => url,
                None => format!(
                    "https://music.youtube.com/search?q={}+{}",
                    song.get_artists().join("+"),
                    song.get_title().replace(' ', "+")
                ),
            }
        }
    }
}

pub async fn download_spotify_song(
    song: SpotifySong,
    client: Arc<str>,
    playlist_title: Arc<str>,
) -> UtilsResult<SpotifySong> {
    let config = config::get_config();
    let out_template = &config.yt_dlp_output_template;
    let args = vec![
        Arg::new("--quiet"),
        Arg::new("--extract-audio"),
        //Arg::new("--embed-metadata"),
        Arg::new("--embed-thumbnail"),
        Arg::new("--add-metadata"),
        Arg::new_with_arg("--default-search", "ytsearch"),
        Arg::new_with_arg("--output", out_template),
        Arg::new_with_arg("--print", "after_move:filepath"),
        Arg::new_with_arg("-I", "1") // only download best result
    ];
    let title = get_song_title(&song).await;
    println!("{}", title);
    download_song(song, client, playlist_title, args, title).await
}

async fn download_playlist<T: SongTrait, F, Fut>(
    songs: &[T],
    client: Arc<str>,
    playlist: &Playlist,
    downloader: F,
    out_channel: Sender<Answer>,
) where
    F: Fn(T, Arc<str>, Arc<str>) -> Fut,
    Fut: Future<Output = UtilsResult<T>>,
{
    println!("Start Downloading");
    let songs = db::remove_downloaded(&songs, &client).unwrap();
    let total = songs.len() as u64;
    let counter = Arc::new(Mutex::new(0));
    let title = playlist.title.clone();
    let songs: Vec<UtilsResult<T>> = futures::stream::iter(songs)
        .map(|song| async {
            let song = downloader(song, client.clone(), title.clone()).await;
            let mut counter = counter.lock().await;
            *counter += 1;
            out_channel
                .send(Answer {
                    client: client.clone(),
                    data: AnswerType::DownloadProgress(playlist.clone(), *counter, total),
                })
                .await;
            song
        })
        .buffer_unordered(4)
        .collect()
        .await;
    let songs_ok: Vec<T> = songs.into_iter().filter_map(|s| s.ok()).collect();
    let _ = db::update_songs(&songs_ok, &client);
    out_channel
        .send(Answer {
            client: client.clone(),
            data: AnswerType::DownloadFinish(playlist.clone()),
        })
        .await;
    println!("Done Downloading");
}

pub async fn download_spotify_playlist(
    songs: &[SpotifySong],
    client: Arc<str>,
    playlist: &Playlist,
    out_channel: Sender<Answer>,
) {
    download_playlist(songs, client, playlist, download_spotify_song, out_channel).await;
}

pub async fn download_yt_playlist(
    songs: &[YoutubeSong],
    client: Arc<str>,
    playlist: &Playlist,
    out_channel: Sender<Answer>,
) {
    download_playlist(songs, client, playlist, download_yt_song, out_channel).await;
}
