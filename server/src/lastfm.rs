use reqwest;
use scraper;

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
        let song = songs_table.select(&song_selector).next().unwrap();
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
