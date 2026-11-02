//! Entry point for the `torogao-tui` binary.
//!
//! Wires the terminal to `torogao_tui::app::App`: raw mode +
//! alternate screen, **without** mouse capture — the workflow is
//! keyboard-only by design (D-487), and mouse events are ignored
//! even if a backend reports them. The terminal is restored on
//! every exit path, including panic (the guard's `Drop`).

#![forbid(unsafe_code)]

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{self, Stdout};
use std::process::ExitCode;
use std::time::Duration;
use torogao_tui::App;

/// Restores the terminal on drop, panic or not.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

fn run() -> io::Result<()> {
    let mut app = App::new(App::default_projects_root());
    app.load_settings();

    enable_raw_mode()?;
    let _guard = TerminalGuard;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let outcome = event_loop(&mut terminal, &mut app);
    drop(_guard);
    outcome
}

fn event_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|frame| torogao_tui::view::draw(frame, app))?;
        if app.quit {
            return Ok(());
        }
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key)
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                {
                    app.handle_key(key);
                }
                // Resize redraws on the next loop turn; mouse and
                // paste events are deliberately unused.
                _ => {}
            }
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("torogao-tui: {err}");
            ExitCode::from(2)
        }
    }
}
