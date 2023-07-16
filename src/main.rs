mod db;
mod request;
mod source;
use std::{io, vec};

pub use crate::source::youtube::Client;
use request::{handle_request, Request};
use source::Source;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Builder;
use tokio::sync::broadcast;
use tokio::sync::mpsc;

fn start_server() {
    println!("Starting server");
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
        let listener = TcpListener::bind("127.0.0.1:8080").await.unwrap();

        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let _g = request_runtime.enter();
            request_runtime.spawn(stream_handler(socket));
        }
    })
}

async fn stream_read(
    stream_rx: OwnedReadHalf,
    broad_tx: broadcast::Sender<Request>,
) -> Result<(), std::io::Error> {
    loop {
        stream_rx.readable().await?;
        // Creating the buffer **after** the `await` prevents it from
        // being stored in the async task.
        let mut buf = [0; 1024];
        match stream_rx.try_read(&mut buf) {
            // socket closed
            Ok(n) if n == 0 => break,
            Ok(n) => {
                let message = std::str::from_utf8(&buf[0..n]).expect("some utf8 issue");
                let request: Request = match handle_request(message.to_owned()).await {
                    Ok(req) => req,
                    Err(e) => {
                        println!("Error while handling request : {}", e);
                        continue;
                    }
                };
                broad_tx.send(request);
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                // Readable might generate false positives
                continue;
            }
            Err(e) => {
                eprintln!("failed to read from socket; err = {:?}", e);
                return Err(e);
            }
        };
    }
    Ok(())
}

async fn stream_write(
    stream_tx: OwnedWriteHalf,
    mut mpsc_rx: mpsc::Receiver<String>,
) -> Result<(), std::io::Error> {
    let clients = vec!["Youtube"];
    stream_tx.writable().await?;
    stream_tx.try_write(serde_json::to_string(&clients).unwrap().as_bytes())?;
    loop {
        match mpsc_rx.recv().await {
            None => continue,
            Some(message) => {
                println!("{}", message);
                stream_tx.writable().await?;
                stream_tx.try_write(message.as_bytes())?;
            }
        }
    }
}
async fn client_spawning(broad_tx: broadcast::Sender<Request>, mpsc_tx: mpsc::Sender<String>) {
    // We assume that the API are always up
    if online::tokio::check(None).await.is_ok() {
        let mut youtube_client =
            Client::new("Youtube".to_string(), broad_tx.subscribe(), mpsc_tx.clone())
                .await
                .unwrap();
        println!("Client created");
        //youtube_client.init().await;
        youtube_client.listen().await;
    }
}

async fn stream_handler(stream: TcpStream) -> Result<(), std::io::Error> {
    let (rx, tx) = stream.into_split();
    let (broad_tx, _) = broadcast::channel::<Request>(16);
    let (mpsc_tx, mpsc_rx) = mpsc::channel(100);
    tokio::spawn(stream_read(rx, broad_tx.clone()));
    tokio::spawn(stream_write(tx, mpsc_rx));
    tokio::spawn(client_spawning(broad_tx, mpsc_tx));
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    start_server();
    Ok(())
}
