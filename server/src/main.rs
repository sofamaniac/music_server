use std::io;

mod config;
mod db;
mod lastfm;
mod source;
mod utils;

use crate::source::{spotify, youtube, Source};
use music_server::request::{self, handle_request, Answer, Request};
use tokio::io::AsyncReadExt;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Builder;
use tokio::sync::broadcast;
use tokio::sync::mpsc;

use log::{debug, error, info, trace, warn};

fn start_server() {
    info!("Starting server");
    let acceptor_runtime = Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("acceptor-pool")
        .thread_stack_size(3 * 1024 * 1024)
        .enable_time()
        .enable_io()
        .build()
        .unwrap();
    let request_runtime = Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("request-pool")
        .thread_stack_size(3 * 1024 * 1024)
        .enable_time()
        .enable_io()
        .build()
        .unwrap();
    let utility_runtime = Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("utility-pool")
        .thread_stack_size(3 * 1024 * 1024)
        .enable_time()
        .enable_io()
        .build()
        .unwrap();

    acceptor_runtime.block_on(async {
        let config = config::get_config();
        let port = config.port;
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port))
            .await
            .unwrap();

        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let _g = request_runtime.enter();
            request_runtime.spawn(stream_handler(socket));
        }
    })
}

async fn stream_read(
    mut stream_rx: OwnedReadHalf,
    broad_tx: broadcast::Sender<Request>,
) -> Result<(), std::io::Error> {
    loop {
        stream_rx.readable().await?;
        // Creating the buffer **after** the `await` prevents it from
        // being stored in the async task.
        let mut size = [0; 8];
        stream_rx.read_exact(&mut size).await;
        let size = usize::from_be_bytes(size);
        if size == 0 {
            // socket was closed
            break Ok(());
        }
        let mut buf = vec![0; size];
        stream_rx.read_exact(&mut buf).await;
        let message = match std::str::from_utf8(&buf) {
            Ok(mes) => mes,
            Err(err) => {
                error!("Error while reading {}", err);
                continue;
            } // TODO inform client
        };
        let request: Request = match handle_request(message.to_owned()).await {
            Ok(req) => req,
            Err(e) => {
                error!("Error while handling request : {} {}", e, message);
                // TODO inform client
                continue;
            }
        };
        info!("{:?}", request);
        broad_tx.send(request);
    }
}

async fn stream_write(
    stream_tx: OwnedWriteHalf,
    mut mpsc_rx: mpsc::Receiver<Answer>,
) -> Result<(), std::io::Error> {
    loop {
        match mpsc_rx.recv().await {
            None => break Ok(()),
            Some(message) => {
                let json = serde_json::to_string(&message).unwrap();
                let message = request::prepare_message(json);
                stream_tx.writable().await?;
                stream_tx.try_write(&message)?;
            }
        }
    }
}
async fn client_spawning(broad_tx: broadcast::Sender<Request>, mpsc_tx: mpsc::Sender<Answer>) {
    // We assume that the API are always up
    if online::tokio::check(None).await.is_ok() {
        let mut youtube_client =
            youtube::Client::new("Youtube", broad_tx.subscribe(), mpsc_tx.clone())
                .await
                .unwrap();
        let mut spotify_client =
            spotify::Client::new("Spotify", broad_tx.subscribe(), mpsc_tx.clone()).await;
        tokio::spawn(async move {
            spotify_client.authenticate().await;
            spotify_client.fetch_all_playlists().await;
            spotify_client.listen().await;
        });
        tokio::spawn(async move {
            youtube_client.init().await;
            youtube_client.listen().await;
        });
    }
}

async fn stream_handler(stream: TcpStream) -> Result<(), std::io::Error> {
    let (rx, tx) = stream.into_split();
    let (broad_tx, _) = broadcast::channel::<Request>(16);
    let (mpsc_tx, mpsc_rx) = mpsc::channel::<Answer>(100);
    tokio::spawn(client_spawning(broad_tx.clone(), mpsc_tx)).await;
    tokio::spawn(stream_write(tx, mpsc_rx));
    tokio::spawn(stream_read(rx, broad_tx));
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    db::init().expect("Failed to initialize db");
    start_server();
    Ok(())
}
