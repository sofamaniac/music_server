use crate::config;
use log::*;

pub async fn extract_link(url: &str) -> Option<String> {
    let page = reqwest::get(url).await.unwrap().text().await.unwrap();
    let doc = scraper::Html::parse_document(&page);
    let link_selector = scraper::Selector::parse("a").unwrap();
    let mut ret: Option<String> = None;
    // we look through all the links
    for link in doc.select(&link_selector) {
        if let Some(url) = link.value().attr("data-youtube-url") {
            ret = Some(url.to_owned());
            break;
        }
    }
    if ret.is_none() {
        info!("No Youtube link at {}", url);
    }
    ret
}

pub async fn find_by_mbid(mbid: &str) -> Option<String> {
    let config = config::get_config();
    let lastfm_key = config.lastfm_api_key;
    let api_url = format!(
        "http://ws.audioscrobbler.com/2.0/?method=track.getInfo&api_key={}&mbid={}&format=json",
        lastfm_key, mbid
    );
    debug!("{}", api_url);
    let res = reqwest::get(api_url).await.unwrap();
    let Ok(json) = res.json::<serde_json::Value>().await else {
        error!("Cannot parse json");
        return None
    };
    let track = json.get("track")?;
    let url = track.get("url")?;
    let url = url.as_str()?;
    extract_link(url).await
}

pub async fn find_youtube_link(title: &str, artist: &str) -> Option<String> {
    // TODO use artist in some way
    // TODO fail gracefuly
    let url = format!(
        "https://www.last.fm/search/tracks?q={}+{}",
        title.replace(' ', "+"),
        artist
    );
    let res = reqwest::get(url).await.unwrap().text().await.unwrap();
    let doc = scraper::Html::parse_document(&res);
    let table_selector = scraper::Selector::parse("tbody").unwrap();
    let song_selector = scraper::Selector::parse("tr").unwrap();
    let link_selector = scraper::Selector::parse("a").unwrap();
    let mut ret: Option<String> = None;
    if let Some(songs_table) = doc.select(&table_selector).next() {
        // we get the first song
        let song = songs_table.select(&song_selector).next()?;
        // we look through all the links
        for link in song.select(&link_selector) {
            if let Some(url) = link.value().attr("data-youtube-url") {
                ret = Some(url.to_owned());
                break;
            }
        }
        if ret.is_none() {
            println!("No Youtube link for {}", title);
        }
    } else {
        println!("Song not found {}", title);
    }
    ret
}
