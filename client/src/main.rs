use crossterm::{
    event::{self, poll, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use music_server::request::{get_answer, Answer};
use std::{error::Error, io, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio::{
    io::AsyncReadExt,
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
};
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders, List, ListState},
    Frame, Terminal,
};

mod app;
use app::App;

async fn start_ui(app: &Arc<Mutex<App>>) -> Result<(), Box<dyn Error>> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let res = run_app(&mut terminal, app).await;

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

async fn listen(app: &Arc<Mutex<App>>, stream: &mut OwnedReadHalf) -> Result<(), std::io::Error> {
    app.lock().await.request_sources().await;
    app.lock().await.state.select(Some(0));
    loop {
        stream.readable().await?;

        let mut size = [0; 8];
        stream.read_exact(&mut size).await;
        let size = usize::from_be_bytes(size);
        if size == 0 {
            break Ok(());
        }
        let mut buf = vec![0; size];
        stream.read_exact(&mut buf).await;
        let message = match std::str::from_utf8(&buf) {
            Ok(val) => val,
            Err(_) => continue,
        };
        let answer: Answer = match get_answer(message.to_string()).await {
            Ok(val) => val,
            Err(err) => {
                eprintln!("error while parsing answer {}", err);
                continue;
            }
        };
        app.lock().await.handle_answer(answer).await;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let stream = TcpStream::connect("127.0.0.1:8080").await.unwrap();
    let (mut rx, mut tx) = stream.into_split();
    let app = Arc::new(Mutex::new(App::new(tx)));
    let app_clone = Arc::clone(&app);
    tokio::spawn(async move { listen(&app_clone, &mut rx).await });
    start_ui(&app).await;
    Ok(())
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &Arc<Mutex<App>>) -> io::Result<()> {
    loop {
        let mut app = app.lock().await;
        terminal.draw(|f| ui(f, &app))?;

        // avoid to block refresh
        if poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('j') => app.handle_event(app::Direction::Down).await,
                    KeyCode::Char('J') => app.handle_event(app::Direction::DownPanel).await,
                    KeyCode::Char('k') => app.handle_event(app::Direction::Up).await,
                    KeyCode::Char('K') => app.handle_event(app::Direction::UpPanel).await,
                    KeyCode::Char('L') => app.handle_event(app::Direction::RightPanel).await,
                    KeyCode::Char('H') => app.handle_event(app::Direction::LeftPanel).await,
                    _ => (),
                }
            }
        }
    }
}

fn ui<B: Backend>(f: &mut Frame<B>, app: &tokio::sync::MutexGuard<App>) {
    // Wrapping block for a group
    // Just draw the block and the group on the same area and build the group
    // with at least a margin of 1
    let size = f.size();

    // Surrounding block
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Music Client")
        .title_alignment(Alignment::Center)
        .border_type(BorderType::Rounded);
    f.render_widget(block, size);

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .margin(1)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)].as_ref())
        .split(f.size());

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(chunks[0]);

    let source_widget = app.get_sources_widget();
    let mut source_state = app.state.clone();
    f.render_stateful_widget(source_widget, left_chunks[0], &mut source_state);

    let playlist_widget = app.get_playlist_widget();
    let mut playlist_state = app.get_playlists_state();
    f.render_stateful_widget(playlist_widget, left_chunks[1], &mut playlist_state);

    let songs_widget = app.get_songs_widget();
    let mut songs_state = app.get_songs_state();
    f.render_stateful_widget(songs_widget, chunks[1], &mut songs_state);
}
