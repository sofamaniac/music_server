mod source;
use std::io;

pub use crate::source::youtube::Client;
use source::Source;
use tokio::net::tcp::{OwnedWriteHalf, OwnedReadHalf};
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

    acceptor_runtime.block_on(async { // this is certainly overkill
        let listener = TcpListener::bind("127.0.0.1:8080").await.unwrap();

        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let _g = request_runtime.enter();
            request_runtime.spawn(stream_handler(socket));
        }
    })
}

async fn stream_read(stream_rx: OwnedReadHalf, broad_tx: broadcast::Sender<String>) -> Result<(), std::io::Error> {
    loop {
        stream_rx.readable().await?;
        // Creating the buffer **after** the `await` prevents it from
        // being stored in the async task.
        let mut buf = [0; 1024];
        let n = match stream_rx.try_read(&mut buf) {
            // socket closed
            Ok(n) if n == 0 => break,
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                // Readable might generate false positives
                continue;
            }
            Err(e) => {
                eprintln!("failed to read from socket; err = {:?}", e);
                return Err(e);
            }
        };
        let message = std::str::from_utf8(&buf[0..n]).expect("some utf8 issue");
        println!("{}", message);
        broad_tx.send(message.to_string()).unwrap();
    }
    Ok(())
}

async fn stream_write(stream_tx: OwnedWriteHalf, mut mpsc_rx: mpsc::Receiver<String>) -> Result<(), std::io::Error> {
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

async fn stream_handler(mut stream: TcpStream) -> Result<(), std::io::Error> {
    println!("Beg");
    let (rx, tx) = stream.into_split();
    let (broad_tx, mut broad_rx) = broadcast::channel::<String>(16);
    let (mpsc_tx, mut mpsc_rx) = mpsc::channel(100);
    tokio::spawn(stream_read(rx, broad_tx.clone()));
    tokio::spawn(stream_write(tx, mpsc_rx));
    let youtube_client = Client::new("Youtube".to_string(), broad_tx.subscribe(), mpsc_tx.clone()).await.unwrap();
    println!("Client created");
    let playlists = youtube_client.get_all_playlists().await;
    let serialized = serde_json::to_string(&playlists).unwrap();
    mpsc_tx.send(serialized).await;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    start_server();
    Ok(())
}
