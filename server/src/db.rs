#![warn(clippy::unwrap_used)]

use std::path::Path;

use rusqlite::{Connection, Statement};
use serde::{Deserialize, Serialize};

use log::*;

use crate::{source::{Playlist, SongTrait}, config};

pub type Result<T> = rusqlite::Result<T>;

fn get_db_path() -> String {
    let data_loc = config::get_config().data_location;
    format!("{}/db.db3", data_loc)
}

fn prepare<'a>(conn: &'a Connection, query: &str) -> Statement<'a> {
    let stmt = conn.prepare(query);
    match stmt {
        Ok(stmt) => stmt,
        Err(err) => {
            error!("query: {}, err: {}", query, err);
            panic!()
        }
    }
}

fn from_json<'a, T: Deserialize<'a>>(json: &'a str) -> T {
    serde_json::from_str(json).unwrap_or_else(|_| panic!("Invalid data read from db {}", json))
}

fn to_json<T: Serialize>(obj: &T) -> String {
    serde_json::to_string(obj).expect("Could not serialize object")
}

pub fn init() -> Result<()> {
    let conn = Connection::open(get_db_path())?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS TblSong (
            uid INTEGER PRIMARY KEY,
            id TEXT NOT NULL,
            source TEXT NOT NULL,
            song TEXT NOT NULL,
            unique (id, source))",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS TblPlaylist (
            uid INTEGER PRIMARY KEY,
            id TEXT NOT NULL,
            title TEXT NOT NULL,
            size INTEGER NOT NULL,
            etag TEXT NOT NULL,
            source TEXT NOT NULL,
            unique (id, source))",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS TblPlaylistSongs (
            uidPlaylist INTEGER NOT NULL,
            uidSong INTEGER NOT NULL,
            unique (uidPlaylist, uidSong))",
        (),
    )?;

    Ok(())
}

pub fn playlist_needs_update(id: &str, source: &str, etag: &str) -> bool {
    // returns true if the db is inaccessible
    let conn = match Connection::open(get_db_path()) {
        Ok(val) => val,
        Err(_) => return true
    };
    let query = "SELECT * FROM TblPlaylist WHERE source = ?1 AND id = ?2 AND etag = ?3";
    let mut stmt = prepare(&conn, query);
    stmt.exists(rusqlite::params![source, id, etag]).unwrap_or(true)
}

pub fn add_playlist<T: SongTrait>(
    source: &str,
    playlist: Playlist,
    songs: &[T],
    etag: &str,
) -> Result<()> {
    let conn = Connection::open(get_db_path())?;
    conn.execute(
        "REPLACE INTO TblPlaylist (uid, id, title, size, etag, source) VALUES ((SELECT uid FROM TblPlaylist WHERE id = ?1 AND source = ?5), ?1, ?2, ?3, ?4, ?5)",
        (
            &playlist.id,
            playlist.title,
            playlist.size,
            etag,
            source,
        ),
    )?;
    let query = "SELECT uid FROM TblPlaylist WHERE  source = ?1 AND id = ?2";
    let mut stmt = prepare(&conn, query);
    let uid_playlist: i32 =
        stmt.query_row((source, playlist.id), |row| row.get(0))?;
    for s in songs.iter() {
        conn.execute(
            "REPLACE INTO TblSong (uid, id, source, song) VALUES ((SELECT uid FROM TblSong WHERE source = ?2 AND id = ?1), ?1, ?2, ?3)",
            (
                &s.get_id(),
                source,
                to_json(&s),
            ),
        )?;
        let query = "SELECT uid FROM TblSong WHERE  source = ?1 AND id = ?2";
        let mut stmt = prepare(&conn, query);
        let uid_song: i32 = stmt.query_row((source, &s.get_id()), |row| row.get(0))?;
        conn.execute(
            "REPLACE INTO TblPlaylistSongs (uidPlaylist, uidSong) VALUES (?1, ?2)",
            (uid_playlist, uid_song),
        )?;
    }
    Ok(())
}

pub fn update_songs<T: SongTrait>(songs: &[T], source: &str) -> Result<()> {
    let conn = Connection::open(get_db_path()).expect("cannot open db");
    for s in songs.iter() {
        conn.execute(
            "REPLACE INTO TblSong (uid, id, source, song) VALUES ((SELECT uid FROM TblSong WHERE source = ?2 AND id = ?1), ?1, ?2, ?3)",
            (
                &s.get_id(),
                source,
                to_json(&s),
            ),
        )?;
    }
    Ok(())
}

pub fn remove_downloaded<T: SongTrait>(songs: &[T], source: &str) -> Result<Vec<T>> {
    let conn = Connection::open(get_db_path())?;
    let mut res = vec![];
    let query = "SELECT song FROM TblSong WHERE source = ?1 AND id = ?2";
    let mut stmt = prepare(&conn, query);
    for s in songs.iter() {
        let s = s.to_song();
        let json = stmt.query_row(rusqlite::params![source, &s.id], |row| {
            row.get::<_, String>(0)
        })?;
        let song: T = from_json(&json);
        let url = song.get_url();
        let path = Path::new(&url);
        if !song.get_downloaded() || !path.try_exists().unwrap_or(false) {
            res.push(song);
        }
    }
    Ok(res)
}

pub fn get_playlist_songs<T: SongTrait>(id: &str, source: &str) -> Result<Vec<T>> {
    let conn = Connection::open(get_db_path())?;
    let query = "SELECT uid FROM TblPlaylist WHERE source = ?1 AND id = ?2";
    let mut stmt = prepare(&conn, query);
    let uid_playlist = stmt.query_row((source, id), |row| row.get::<_, i32>(0))?;
    let query = "SELECT uidSong FROM TblPlaylistSongs WHERE uidPlaylist = ?1";
    let mut stmt = prepare(&conn, query);
    let res = stmt.query_map(rusqlite::params![uid_playlist], |row| row.get(0))?;
    let mut songs_uid: Vec<i32> = vec![];
    for uid in res {
        songs_uid.push(uid?);
    }
    let query = "SELECT song FROM TblSong WHERE uid = ?1";
    let mut stmt = prepare(&conn, query);
    let mut songs: Vec<T> = vec![];
    for uid in songs_uid {
        let json = stmt.query_row(rusqlite::params![uid], |row| row.get::<_, String>(0))?;
        songs.push(from_json(&json));
    }
    Ok(songs)
}

pub fn load_playlist(id: &str, source: &str) -> Result<Playlist> {
    let conn = Connection::open(get_db_path())?;
    let stmt = "SELECT uid, title, size, etag FROM TblPlaylist WHERE source = ?1 AND id = ?2";
    let mut stmt = prepare(&conn, stmt);
    stmt.query_row((source, id), |row| {
        Ok(Playlist {
            title: row.get(1)?,
            tags: Default::default(),
            id: id.into(),
            size: row.get(2)?,
        })
    })
}
