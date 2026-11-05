//! Route A bake-off binary: ratatui 0.29 + crossterm 0.28.
//!
//! Usage: `torogao-tui-bakeoff-a [WORKSPACE_ROOT]`. The scenario
//! runs before the terminal is claimed, so a workload failure
//! never leaves the operator in raw mode.

use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use torogao_tui::bakeoff::{BakeoffState, render};

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

fn main() -> io::Result<()> {
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut state = BakeoffState::run(&root)?;

    enable_raw_mode()?;
    let _guard = TerminalGuard;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    run_loop(&mut terminal, &mut state)
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut BakeoffState,
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| render(frame, state))?;
        if state.quit {
            return Ok(());
        }
        if event::poll(Duration::from_millis(250))? {
            let event = event::read()?;
            if matches!(event, Event::Resize(_, _)) {
                terminal.autoresize()?;
            }
            state.handle_event(event);
        }
    }
}
