use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen}, event::{DisableMouseCapture, EnableMouseCapture},
};
use music_server::request::{get_answer, Answer};
use std::{error::Error, io, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio::{
    io::AsyncReadExt,
    net::{
        tcp::OwnedReadHalf,
        TcpStream,
    },
};
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, BorderType, Borders, Gauge, ListState}, 
    Frame, Terminal,
};

mod app;
mod dbus;
mod player;
mod event;
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
    loop {
        stream.readable().await?;

        // an answer is preceded by its size
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
    let stream = match TcpStream::connect("127.0.0.1:8080").await {
        Ok(stream) => stream,
        Err(err) => {
            println!("Cannot connect to the server {}", err);
            return Err(err.into());
        }
    };
    let (mut rx, tx) = stream.into_split();
    let app = Arc::new(Mutex::new(App::new(tx)));
    let app_clone = Arc::clone(&app);
    tokio::spawn(async move { listen(&app_clone, &mut rx).await });
    let app_clone = Arc::clone(&app);
    tokio::spawn(async move { dbus::start_dbus(app_clone).await.unwrap() });
    start_ui(&app).await;
    Ok(())
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &Arc<Mutex<App>>) -> io::Result<()> {
    loop {
        let mut app = app.lock().await;
        let player_state = app.player.get_state();
        terminal.draw(|f| ui(f, &app, player_state))?;

        // avoid to block refresh
        if crossterm::event::poll(Duration::from_millis(50))? {
            if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
                if let Some(event) = event::translate_event(&key) {
                    if event == event::Event::Quit {
                        return Ok(())
                    }
                    app.handle_event(event).await;
                }
            }
        }
    }
}

fn ui<B: Backend>(
    f: &mut Frame<B>,
    app: &tokio::sync::MutexGuard<App>,
    player_state: player::State,
) {
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

    // Bottom chunk: Player info
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Max(900), Constraint::Length(3)])
        .split(f.size());

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(20), Constraint::Percentage(80)].as_ref())
        .split(main_chunks[0]);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Max(5),
            Constraint::Max(10),
            Constraint::Max(6),
            Constraint::Max(7),
            Constraint::Max(0),
        ])
        .split(chunks[0]);

    let source_widget = app.get_sources_widget();
    let mut source_state = match app.current_panel {
        app::Panel::Sources => app.state.clone(),
        _ => ListState::default(),
    };
    f.render_stateful_widget(source_widget, left_chunks[0], &mut source_state);

    let playlist_widget = app.get_playlists_widget();
    let mut playlist_state = match app.current_panel {
        app::Panel::Playlists => app.get_playlists_state(),
        _ => ListState::default(),
    };
    f.render_stateful_widget(playlist_widget, left_chunks[1], &mut playlist_state);

    let option_widget = app.get_options_widget();
    f.render_widget(option_widget, left_chunks[2]);

    let info_widget = app.get_info_widget();
    f.render_widget(info_widget, left_chunks[3]);

    let songs_widget = app.get_songs_widget();
    let mut songs_state = match app.current_panel {
        app::Panel::Songs => app.get_songs_state(),
        _ => ListState::default(),
    };
    f.render_stateful_widget(songs_widget, chunks[1], &mut songs_state);

    let percentage = if player_state.duration == 0 {
        0
    } else {
        (player_state.time_pos * 100) / player_state.duration
    };
    let percentage = std::cmp::min(std::cmp::max(percentage, 0), 100);
    let player_info = format!(
        "{} - {}/{}",
        player_state.title,
        duration_to_string(Duration::from_secs(player_state.time_pos as u64)),
        duration_to_string(Duration::from_secs(player_state.duration as u64))
    );

    let player_widget = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(player_info))
        .gauge_style(Style::default().fg(Color::White).bg(Color::Black))
        .percent(percentage as u16);
    f.render_widget(player_widget, main_chunks[1])
}

fn duration_to_string(dur: Duration) -> String {
    let secs = dur.as_secs();
    let (minutes, secs) = (secs / 60, secs % 60);
    let (hours, minutes) = (minutes / 60, minutes % 60);
    let min_sec_str = format!("{:0width$}:{:0width$}", minutes, secs, width = 2);
    if hours == 0 {
        min_sec_str
    } else {
        format!("{:0width$}:{}", hours, min_sec_str, width = 2)
    }
}
