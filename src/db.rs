#![warn(clippy::unwrap_used)]
use rusqlite::{params, Connection, Result};

use crate::source::{Playlist, Song};

pub fn init() -> Result<()> {
    let conn = Connection::open("data/db.db3")?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS TblSong (
            uid INTEGER PRIMARY KEY,
            id TEXT NOT NULL,
            title TEXT NOT NULL,
            artists TEXT NOT NULL,
            tags TEXT NOT NULL,
            duration INTEGER NOT NULL,
            uri TEXT NOT NULL, 
            downloaded INTEGER NOT NULL, 
            source TEXT NOT NULL,
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
        " CREATE TABLE IF NOT EXISTS TblPlaylistSongs (
            uidPlaylist INTEGER NOT NULL,
            uidSong INTEGER NOT NULL,
            unique (uidPlaylist, uidSong))",
        (),
    )?;

    Ok(())
}

pub fn playlist_needs_update(id: String, source: String, etag: String) -> bool {
    let conn = Connection::open("data/db.db3").unwrap();
    let mut stmt = conn
        .prepare("SELECT etag FROM TblPlaylist WHERE source = ?1 AND id = ?2")
        .unwrap();
    let mut rows = stmt.query(rusqlite::params![source, id]).unwrap();
    match rows.next().unwrap() {
        Some(row) => row.get::<usize, String>(0).unwrap() == etag,
        None => false,
    }
}

pub fn add_playlist(
    source: String,
    playlist: Playlist,
    songs: Vec<Song>,
    etag: String,
) -> Result<()> {
    let conn = Connection::open("data/db3").unwrap();
    conn.execute(
        "REPLACE INTO TblPlaylist (id, title, size, etag, source) (?1, ?2, ?3, ?4, ?5)",
        (
            playlist.id,
            playlist.title,
            playlist.size,
            etag,
            source.clone(),
        ),
    )?;
    for s in songs {
        conn.execute(
            "REPLACE INTO TblSong (id, title, artists, tags, duration, uri, downloaded, source) (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            (s.id, s.title, serde_json::to_string(&s.artits).unwrap(), serde_json::to_string(&s.tags).unwrap(), s.duration.as_millis() as i64, s.url, s.downloaded, source.clone()))?;
    }
    Ok(())
}

pub fn add_song(source: String, song: Song) {}
