use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub data_location: String,
    pub secrets_location: String,
    pub port: u32,
    pub yt_dlp_output_template: String,
    pub spotify_id: String,
    pub spotify_secret: String,
    pub lastfm_api_key: String
}

impl std::default::Default for Config {
    fn default() -> Self {
        Self {
            data_location: "data".to_string(),
            secrets_location: "data/secrets".to_string(),
            port: 8080,
            yt_dlp_output_template: "%(title)s.%(ext)s".to_string(),
            spotify_id: "".to_string(),
            spotify_secret: "".to_string(),
            lastfm_api_key: "".to_string(),
        }
    }
}

pub fn get_config() -> Config {
    confy::load("music_server", None).unwrap()
}
