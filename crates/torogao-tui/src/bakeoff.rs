//! Route A bake-off shell over the existing ratatui front-end.
//!
//! The shell renders one measured [`ScenarioReport`] in four
//! panels and owns no workload semantics of its own.

use std::io;
use std::path::Path;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use torogao_tui_bakeoff_core::scenario::{ScenarioReport, run_scenario};

use crate::termcap::TerminalProfile;

/// Presentation state for the bake-off view.
pub struct BakeoffState {
    /// The measured scenario report.
    pub report: ScenarioReport,
    /// Detected terminal profile.
    pub profile: TerminalProfile,
    /// Set when the operator asks to quit.
    pub quit: bool,
    /// Last ordinary key seen, for shell diagnostics.
    pub last_key: Option<char>,
}

impl BakeoffState {
    /// Runs the shared scenario and prepares the shell.
    ///
    /// # Errors
    ///
    /// Returns the workload error rendered as an I/O error so
    /// the existing binary error surface stays unchanged.
    pub fn run(root: &Path) -> io::Result<Self> {
        let report = run_scenario(root)
            .map_err(|error| io::Error::other(format!("bake-off scenario failed: {error}")))?;
        Ok(Self {
            report,
            profile: TerminalProfile::from_env(),
            quit: false,
            last_key: None,
        })
    }

    /// Routes one terminal event. Resize and release events are
    /// ignored by state; the shell redraws after resize itself.
    pub fn handle_event(&mut self, event: Event) {
        if let Event::Key(key) = event {
            self.handle_key(key);
        }
    }

    /// Routes one key press.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.quit = true;
            }
            KeyCode::Char(character) => self.last_key = Some(character),
            _ => {}
        }
    }
}

/// Draws the four-panel bake-off view into one frame.
pub fn render(frame: &mut Frame<'_>, state: &BakeoffState) {
    let root = frame.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(root);
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " TOROGAO TUI BAKE-OFF — ROUTE A (ratatui) ",
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" profile={}", state.profile.protocol.name())),
    ]))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(header, rows[0]);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);
    let left = column_pair(columns[0]);
    let right = column_pair(columns[1]);
    render_panel(frame, left[0], " Session ", &state.report.session_lines);
    render_panel(frame, right[0], " Scene ", &state.report.scene_lines);
    render_panel(frame, left[1], " Rig ", &state.report.rig_lines);
    render_panel(frame, right[1], " Verdict ", &state.report.verdict_lines);

    let footer = Paragraph::new(" q / Esc quit · panels are read-only · no mouse capture ");
    frame.render_widget(footer, rows[2]);
}

fn column_pair(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area)
}

fn render_panel(frame: &mut Frame<'_>, area: Rect, title: &str, lines: &[String]) {
    let text: Vec<Line<'_>> = lines.iter().map(|line| Line::from(line.as_str())).collect();
    let widget = Paragraph::new(text)
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use torogao_tui_bakeoff_core::data::{OperatorData, RigData, SceneData};

    fn dummy_state() -> BakeoffState {
        let data = OperatorData {
            rig: RigData {
                project_name: "bakeoff".to_string(),
                project_sha_prefix: "000000000000".to_string(),
                parameter_count: 0,
                parameter_ids: Vec::new(),
                part_count: 0,
                deformer_count: 0,
                expression_count: 0,
                vertex_count: 0,
                triangle_count: 0,
            },
            scene: SceneData {
                name: "studio".to_string(),
                title: "The Studio".to_string(),
                format: "torogao-scene/1".to_string(),
                character_count: 0,
                fixture_count: 0,
                prop_count: 0,
            },
        };
        BakeoffState {
            report: ScenarioReport {
                data,
                session_lines: Vec::new(),
                scene_lines: Vec::new(),
                rig_lines: Vec::new(),
                verdict_lines: Vec::new(),
                steps: Vec::new(),
            },
            profile: TerminalProfile::detect(Some("screen-256color"), None),
            quit: false,
            last_key: None,
        }
    }

    #[test]
    fn q_and_escape_quit_but_release_events_do_not() {
        let mut state = dummy_state();
        state.handle_key(KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert!(!state.quit);
        state.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(state.quit);

        let mut state = dummy_state();
        state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(state.quit);
    }

    #[test]
    fn ordinary_and_oversized_input_events_do_not_quit_or_panic() {
        let mut state = dummy_state();
        for character in ['x', '界', '✓'] {
            state.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        state.handle_event(Event::Paste("界".repeat(100_000)));
        state.handle_event(Event::Resize(0, 0));
        state.handle_event(Event::Resize(u16::MAX, u16::MAX));
        assert!(!state.quit);
        assert_eq!(state.last_key, Some('✓'));
    }
}
