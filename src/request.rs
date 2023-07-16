use serde::{Serialize, Deserialize};
use tokio::sync::mpsc::Sender;

#[derive(Serialize, Deserialize)]
#[derive(Clone)]
pub enum RequestType {
    GetAll(Obj),
    Message(String),
    Error(String),
    Set,
    Add,
    Remove,
    Answer(String),
    Get(Attr),
}

#[derive(Serialize, Deserialize)]
#[derive(Clone)]
pub enum Attr {
    Name,
    Url,
    Id,
}

#[derive(Serialize, Deserialize)]
#[derive(Clone)]
pub struct Request {
    pub client: String,
    pub ty: RequestType
}

impl Request {
    pub fn new(client: String, ty: RequestType) -> Self {
        Request { client, ty }
    }
}

#[derive(Serialize, Deserialize)]
#[derive(Clone)]
pub enum Obj {
    PlaylistList,
    Playlist(String),
    Song,
    Client,
}

pub async fn send_request(channel: Sender<String>, request: Request) -> Result<(), tokio::sync::mpsc::error::SendError<String>> {
    let request = serde_json::to_string(&request).unwrap();
    match channel.send(request).await {
        Ok(_) => Ok(()),
        Err(err) => Err(err),
    }
}

pub async fn handle_request(json: String) -> Result<Request, serde_json::Error> {
    serde_json::from_str(&json)
}
