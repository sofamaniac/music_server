use crossterm::event::{KeyCode, KeyEvent};

#[derive(PartialEq, Clone, Debug)]
pub enum Event {
    Move(Direction),
    Play,
    Pause,
    Stop,
    Shuffle,
    Volume(i64),
    Download,
    Auto,
    Next,
    Prev,
    Seek(SeekMode),
    GoToCurrent,
    Repeat,
    Quit,
    Enter,
}

#[derive(PartialEq, Clone, Debug)]
pub enum Direction {
    Up,
    Down,
    RightPanel,
    LeftPanel,
    UpPanel,
    DownPanel,
}

#[derive(PartialEq, Clone, Debug)]
pub enum SeekMode {
    Absolute(i64),
    Relative(i64),
    Percent(usize),
}

pub fn translate_event(key: &KeyEvent) -> Option<Event> {
    match key.code {
        KeyCode::Char('q') => Some(Event::Quit),
        KeyCode::Char('T') => Some(Event::Download),
        // Movement
        KeyCode::Char('j') => Some(Event::Move(Direction::Down)),
        KeyCode::Char('J') => Some(Event::Move(Direction::DownPanel)),
        KeyCode::Char('k') => Some(Event::Move(Direction::Up)),
        KeyCode::Char('K') => Some(Event::Move(Direction::UpPanel)),
        KeyCode::Char('L') => Some(Event::Move(Direction::RightPanel)),
        KeyCode::Char('H') => Some(Event::Move(Direction::LeftPanel)),
        KeyCode::Char(' ') => Some(Event::Pause),
        KeyCode::Enter => Some(Event::Enter),

        // Player control
        KeyCode::Char('d') => Some(Event::Volume(-5)),
        KeyCode::Char('f') => Some(Event::Volume(5)),
        KeyCode::Char('<') => Some(Event::Prev),
        KeyCode::Char('>') => Some(Event::Next),
        KeyCode::Char('a') => Some(Event::Auto),
        KeyCode::Char('y') => Some(Event::Shuffle),
        KeyCode::Char('g') => Some(Event::GoToCurrent),
        KeyCode::Char('r') => Some(Event::Repeat),

        // Seeking
        KeyCode::Right => Some(Event::Seek(SeekMode::Relative(5))),
        KeyCode::Left => Some(Event::Seek(SeekMode::Relative(-5))),
        KeyCode::Char('&') => Some(Event::Seek(SeekMode::Percent(10))),
        KeyCode::Char('é') => Some(Event::Seek(SeekMode::Percent(20))),
        KeyCode::Char('"') => Some(Event::Seek(SeekMode::Percent(30))),
        KeyCode::Char('\'') => Some(Event::Seek(SeekMode::Percent(40))),
        KeyCode::Char('(') => Some(Event::Seek(SeekMode::Percent(50))),
        KeyCode::Char('-') => Some(Event::Seek(SeekMode::Percent(60))),
        KeyCode::Char('è') => Some(Event::Seek(SeekMode::Percent(70))),
        KeyCode::Char('_') => Some(Event::Seek(SeekMode::Percent(80))),
        KeyCode::Char('ç') => Some(Event::Seek(SeekMode::Percent(90))),
        KeyCode::Char('à') => Some(Event::Seek(SeekMode::Percent(0))),
        _ => None,
    }
}
