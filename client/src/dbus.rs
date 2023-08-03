use std::collections::HashMap;
use std::{sync::Arc, future::pending};

use tokio::sync::Mutex;
use zbus::{ConnectionBuilder, dbus_interface};

use crate::app::{App, Event};

struct BaseInterface {
}

#[dbus_interface(name = "org.mpris.MediaPlayer2")]
impl BaseInterface {
    
    fn identity(&self) -> String {
        "yauma".to_string()
    }

    #[dbus_interface(property)]
    fn can_raise(&self) -> bool {
        false
    }

    fn raise(&self) {}

    fn quit(&self) {}

    #[dbus_interface(property)]
    fn can_quit(&self) -> bool {
        false
    }

    #[dbus_interface(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[dbus_interface(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Default::default()
    }

    #[dbus_interface(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Default::default()
    }
}
struct PlayerInterface{
    app: Arc<Mutex<App>>
}

#[dbus_interface(name = "org.mpris.MediaPlayer2.Player")]
impl PlayerInterface {
    async fn next(&self) {
        self.app.lock().await.handle_event(Event::Next).await;
    }
    async fn previous(&self) {
        self.app.lock().await.handle_event(Event::Prev).await;
    }
    async fn pause(&self) {
        self.app.lock().await.set_pause_val(true);
    }
    async fn unpause(&self) {
        self.app.lock().await.set_pause_val(false);
    }
    async fn play_pause(&self) {
        self.app.lock().await.player.playpause();
    }
    async fn play(&self) {
        self.unpause().await;
    }
    async fn stop(&self) {
        self.app.lock().await.player.stop();
    }
    async fn seek(&self) {}
    async fn set_position(&self) {}
    async fn open_uri(&self) {}

    
    #[dbus_interface(property)]
    async fn playback_status(&self) -> String {
        let app = self.app.lock().await;
        if app.player.is_stopped() {
            "Stopped".to_string()
        } else if app.player.paused() {
            "Paused".to_string()
        } else {
            "Playing".to_string()
        }
    }

    #[dbus_interface(property)]
    async fn loop_status(&self) -> String {
        if self.app.lock().await.is_auto() {
            "Playlist".to_string()
        } else {
            "None".to_string()
        }
    }

    #[dbus_interface(property)]
    fn rate(&self) -> f32 {
        1.0
    }
    #[dbus_interface(property)]
    async fn position(&self) -> u64 {
        let app = self.app.lock().await;
        let state = app.player.get_state();
        (state.time_pos * 1000000) as u64
    }
    #[dbus_interface(property)]
    async fn metadata(&self) -> HashMap<&str, zbus::zvariant::Value> {
        use zbus::zvariant::Value;
        let app = self.app.lock().await;
        let mut res = HashMap::new();
        if let Some(song) = app.get_playing_song_info() {
            res.insert("mpris:trackid", Value::Str(song.id.clone().into()));
            res.insert("mpris:length", Value::U64(song.duration.as_micros() as u64));
            res.insert("xesam:title", Value::Str(song.title.clone().into()));
            res.insert("xesam:artist", Value::Str(song.artists.join(", ").into()));
        }
        res
    }


    #[dbus_interface(property)]
    fn can_go_next(&self) -> bool {
        true
    }
    #[dbus_interface(property)]
    fn can_go_previous(&self) -> bool {
        true
    }
    #[dbus_interface(property)]
    fn can_play(&self) -> bool {
        true
    }
    #[dbus_interface(property)]
    fn can_pause(&self) -> bool {
        true
    }
    #[dbus_interface(property)]
    fn can_seek(&self) -> bool {
        true
    }
    #[dbus_interface(property)]
    fn can_control(&self) -> bool {
        true
    }
}

pub async fn start_dbus(app: Arc<Mutex<App>>) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let base = BaseInterface {};
    let player = PlayerInterface { app };
    let _conn = ConnectionBuilder::session()?
        .name("org.mpris.MediaPlayer2.yauma")?
        .serve_at("/org/mpris/MediaPlayer2", base)?
        .serve_at("/org/mpris/MediaPlayer2", player)?
        .build()
        .await?;
    // Do other things or go to wait forever
    pending::<()>().await;
    Ok(())
}
