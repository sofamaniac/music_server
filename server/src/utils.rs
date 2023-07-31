use futures::{StreamExt, Future};
use regex::{Regex, RegexSet};
use std::{path::PathBuf, time::Duration};
use ytd_rs::{Arg, YoutubeDL};

use crate::{config, db, source::Song};
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
async fn download_song(
    mut song: Song,
    client: String,
    playlist_title: String,
    args: Vec<Arg>,
    link: String,
) -> UtilsResult<Song> {
    let config = config::get_config();
    let folder: String = config.data_location + &format!("/music/{}/{}/", client, playlist_title);
    let folder = PathBuf::from(folder);
    println!("Downloading {}", song.title);
    let ytdlp = YoutubeDL::new(
        &folder,
        args,
        &link,
    )
    .unwrap();
    let result = tokio::task::spawn_blocking(move || ytdlp.download())
        .await
        .unwrap();
    match result {
        Ok(result) => {
            let path = result.output().trim();
            song.url = path.to_string();
            song.downloaded = true;
            Ok(song)
        }
        Err(err) => {
            println!("{}", err);
            Err(UtilsError::YtDLErr)
        }
    }
}

pub async fn download_yt_song(
    song: Song,
    client: String,
    playlist_title: String,
) -> UtilsResult<Song> {
    let config = config::get_config();
    let out_template = &config.yt_dlp_output_template;
    let args = vec![
        Arg::new("--quiet"),
        Arg::new("--extract-audio"),
        Arg::new("--embed-metadata"),
        Arg::new_with_arg("--output", out_template),
        Arg::new_with_arg("--print", "after_move:filepath"),
    ];
    let link = format!("https://youtube.com/watch?v={}", song.id);
    download_song(song, client, playlist_title, args, link).await
}

fn get_song_title(song: &Song) -> String {
    format!("{} - {}", song.artists.join(", "), song.title)
}

pub async fn download_spotify_song(
    song: Song,
    client: String,
    playlist_title: String,
) -> UtilsResult<Song> {
    let config = config::get_config();
    let out_template = &config.yt_dlp_output_template;
    let args = vec![
        Arg::new("--quiet"),
        Arg::new("--extract-audio"),
        Arg::new("--embed-metadata"),
        Arg::new_with_arg("--default-search", "ytsearch"),
        Arg::new_with_arg("--output", out_template),
        Arg::new_with_arg("--print", "after_move:filepath"),
    ];
    let title = get_song_title(&song);
    download_song(song, client, playlist_title, args, title).await
}


async fn download_playlist<F, Fut>(songs: Vec<Song>, client: String, playlist_title: String, downloader: F)
where F: Fn(Song, String, String) -> Fut, 
      Fut:Future<Output = UtilsResult<Song>>{
    println!("Start Downloading");
    let songs = db::remove_downloaded(&songs, &client).unwrap();
    let songs: Vec<UtilsResult<Song>> = futures::stream::iter(songs)
        .map(|song| async { downloader(song, client.clone(), playlist_title.clone()).await })
        .buffer_unordered(4)
        .collect()
        .await;
    let songs_ok: Vec<Song> = songs.into_iter().filter_map(|s| s.ok()).collect();
    let _ = db::update_songs(&songs_ok, &client);
    println!("Done Downloading");
}

pub async fn download_spotify_playlist(songs: Vec<Song>, client: String, playlist_title: String) {
    download_playlist(songs, client, playlist_title, download_spotify_song).await;
}

pub async fn download_yt_playlist(songs: Vec<Song>, client: String, playlist_title: String) {
    download_playlist(songs, client, playlist_title, download_yt_song).await;
}
