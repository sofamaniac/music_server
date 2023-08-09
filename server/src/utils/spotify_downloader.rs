use crate::request::Answer;
use crate::spotify::{SpotifyPlaylist, SpotifySong};
use deezer::models::track::Track;
use lastfm;
use log::*;
use reqwest;
use std::{path::Path, sync::Arc};
use tokio::{process::Command, sync::mpsc::Sender};

use super::*;

#[derive(Debug)]
pub enum Error {
    NotAvailable,
    NoISRC,
    ReqwestError,
    YtDLError,
    DeezerDLError,
}

pub type Result<T> = std::result::Result<T, Error>;
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mes = match self {
            Error::NotAvailable => "Not available on Deezer",
            Error::NoISRC => "No ISRC available for song",
            Error::ReqwestError => "Error while fetching infos",
            Error::YtDLError => "Youtube DL error",
            Error::DeezerDLError => "Deemix error",
        };
        write!(f, "{}", mes)
    }
}

impl std::error::Error for Error {}

async fn from_deezer(
    song: SpotifySong,
    client: Arc<str>,
    playlist_title: Arc<str>,
) -> Result<SpotifySong> {
    if let Some(isrc) = &song.isrc {
        let api_url = format!("https://api.deezer.com/track/isrc:{}", isrc);
        let response = match reqwest::get(&api_url).await {
            Ok(val) => val,
            Err(err) => {
                error!("Error while fetching deezer api {} {}", api_url, err);
                return Err(Error::ReqwestError);
            }
        };
        let track = match response.json::<Track>().await {
            Ok(val) => val,
            Err(err) => {
                error!("Error while parsing deezer api response {} {}", err, isrc);
                return Err(Error::ReqwestError);
            }
        };
        if track.available_countries.iter().any(|s| s == "FR") {
            let url = format!("https://www.deezer.com/fr/track/{}", track.id);
            debug!("Downloading from deezer");
            download_from_deezer(song, client, playlist_title, url).await
        } else {
            warn!("Not available in country : {}", song.get_title());
            Err(Error::NotAvailable)
        }
    } else {
        Err(Error::NoISRC)
    }
}

async fn download_from_deezer(
    mut song: SpotifySong,
    client: Arc<str>,
    playlist_title: Arc<str>,
    url: String,
) -> Result<SpotifySong> {
    let config = config::get_config();
    let folder: String = config.data_location + &format!("/music/{}/{}/", client, playlist_title);
    let folder = Path::new(&folder);
    let result = Command::new("/home/sofamaniac/Nextcloud/programmation/deemix/run.sh")
        .arg(folder)
        .arg(url)
        .output()
        .await;
    match result {
        Ok(result) => {
            let out = String::from_utf8(result.stdout).unwrap();
            debug!("{}", out);
            let path = out.trim();
            song.set_url(path.to_string());
            song.set_downloaded(true);
            Ok(song)
        }
        Err(err) => {
            error!("{}", err);
            Err(Error::DeezerDLError)
        }
    }
}

async fn from_youtube(
    song: SpotifySong,
    client: Arc<str>,
    playlist_title: Arc<str>,
) -> Result<SpotifySong> {
    let args = vec![
        Arg::new_with_arg("--default-search", "ytsearch"),
        Arg::new_with_arg("-I", "1"), // only download best result
    ];
    debug!("Querying lastfm");
    let mut url = "".to_string();
    if song.isrc.is_some() {
        if let Ok(mbid) = get_mbid(&song.isrc.clone().unwrap()).await {
            url = match lastfm::find_by_mbid(&mbid).await {
                Some(val) => val,
                None => match lastfm::find_youtube_link(
                    &song.get_title(),
                    &song.get_artists().join("+").replace(' ', "+"),
                )
                .await
                {
                    Some(url) => url,
                    None => {
                        info!("Not found on lastfm : {}", song.get_title());
                        format!(
                            "https://music.youtube.com/search?q={}+{}",
                            song.get_artists().join("+"),
                            song.get_title().replace(' ', "+")
                        )
                    }
                },
            }
        }
    };
    match download_song(song, client, playlist_title, &args, url).await {
        Ok(song) => Ok(song),
        Err(err) => Err(Error::YtDLError),
    }
}

pub async fn download_spotify_song(
    song: SpotifySong,
    client: Arc<str>,
    playlist_title: Arc<str>,
) -> UtilsResult<SpotifySong> {
    let res = from_deezer(song.clone(), client.clone(), playlist_title.clone()).await;
    match res {
        Ok(song) => {
            debug!("Download from deezer success");
            Ok(song)
        }
        Err(err) => match from_youtube(song.clone(), client, playlist_title).await {
            Ok(song) => {
                debug!("Download from Youtube success");
                Ok(song)
            }
            Err(_) => {
                debug!("Could not download song {}", song.get_title());
                Err(UtilsError::YtDLErr)
            }
        },
    }
}

pub async fn download(
    songs: &[SpotifySong],
    client: Arc<str>,
    playlist: SpotifyPlaylist,
    out_channel: Sender<Answer>,
) {
    download_playlist(songs, client, playlist, download_spotify_song, out_channel).await;
}

pub async fn get_mbid(isrc: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .user_agent("yauma/0.2.0 (antoine@grimod.fr)")
        .build();
    let client = match client {
        Ok(val) => val,
        Err(_) => return Err(Error::ReqwestError),
    };
    let api_url = format!("https://musicbrainz.org/ws/2/isrc/{}?fmt=json&inc=", isrc);
    let response = match client.get(&api_url).send().await {
        Ok(val) => val,
        Err(err) => {
            error!("Error while fetching deezer api {} {}", api_url, err);
            return Err(Error::ReqwestError);
        }
    };
    let json = response.json::<serde_json::Value>().await;
    let json = match json {
        Ok(val) => val,
        Err(_) => {
            error!("Error while parsing json");
            return Err(Error::ReqwestError);
        }
    };
    let recordings = match json.get("recordings") {
        Some(val) => val,
        None => {
            error!("No recording found");
            return Err(Error::ReqwestError);
        }
    };
    let id = &recordings[0]["id"];
    debug!("found mbid: {}", id);
    Ok(id.as_str().unwrap().to_string())
}
