use serde::{Deserialize, Serialize};
use std::{fmt, sync::Arc};
use tokio::sync::mpsc::{error::SendError, Sender};

use crate::source_types::{Playlist, Song, SourceError};

pub type RequestResult<T> = Result<T, RequestError>;

#[derive(Debug)]
pub enum RequestError {
    SendErr(SendError<Answer>),
    JsonErr(serde_json::error::Error),
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum RequestType {
    GetAll(ObjRequest),
    Error(Arc<str>),
    Set,
    Add,
    Remove,
    Get(Attr),
    Download(ObjRequest),
    Message(Arc<str>),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ObjRequest {
    PlaylistList,
    Playlist(Arc<str>),
    Song,
    Client(Arc<str>),
    ClientList,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum AnswerType {
    PlaylistList(Vec<Playlist>),
    Playlist(Playlist),
    Songs(Playlist, Vec<Song>),
    Song(Song),
    Client(Arc<str>),
    Message(Arc<str>),
    Error(ErrorType),
    DownloadFinish(Playlist),
    DownloadProgress(Playlist, u64, u64), // #downloaded / #total
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ErrorType {
    SourceError(SourceError),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Attr {
    Name,
    Url,
    Id,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Request {
    pub client: Arc<str>,
    pub ty: RequestType,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Answer {
    pub client: Arc<str>,
    pub data: AnswerType,
}

impl Answer {
    pub fn new(client: Arc<str>, data: AnswerType) -> Self {
        Answer { client, data }
    }
}

pub async fn send_request(channel: Sender<Answer>, request: Answer) -> RequestResult<()> {
    match channel.send(request).await {
        Ok(_) => Ok(()),
        Err(err) => Err(RequestError::SendErr(err)),
    }
}

pub async fn handle_request(json: String) -> RequestResult<Request> {
    match serde_json::from_str(&json) {
        Ok(val) => Ok(val),
        Err(err) => Err(RequestError::JsonErr(err)),
    }
}

pub async fn get_answer(json: String) -> RequestResult<Answer> {
    match serde_json::from_str(&json) {
        Ok(val) => Ok(val),
        Err(err) => Err(RequestError::JsonErr(err)),
    }
}

pub fn prepare_message(message: String) -> Vec<u8> {
    let mut bytes = message.into_bytes();
    let mut size = bytes.len().to_be_bytes().to_vec();
    size.append(&mut bytes);
    size
}
