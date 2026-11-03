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
use std::io::{self, Stdout, Write};
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
    let mut blasted: Option<u64> = None;
    loop {
        app.tick();
        app.refresh_blast();
        terminal.draw(|frame| torogao_tui::view::draw(frame, app))?;
        // Direct-graphics protocols (kitty/sixel) draw outside
        // ratatui's cell buffer: the app reserves the viewport
        // rect in the buffer and hands the shell a blast to
        // write after the draw, keyed by a signature so an
        // unchanged image is never re-sent. When the blast goes
        // away (screen change), kitty images are deleted so
        // none linger over other screens.
        match app.graphics_blast() {
            Some(blast) if blasted != Some(blast.signature) => {
                let mut stdout = io::stdout();
                stdout.write_all(blast.text.as_bytes())?;
                stdout.flush()?;
                blasted = Some(blast.signature);
            }
            None if blasted.is_some() => {
                let mut stdout = io::stdout();
                if app.media.mode == torogao_tui::media::GraphicsMode::Kitty {
                    stdout.write_all(torogao_tui::media::kitty_delete().as_bytes())?;
                    stdout.flush()?;
                }
                blasted = None;
            }
            _ => {}
        }
        if app.quit {
            torogao_tui::media::cleanup_preview_tmp();
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
