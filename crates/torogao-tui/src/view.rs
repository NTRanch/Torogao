//! Rendering: ratatui widgets over the app state.
//!
//! The palette is Addition U translated to the terminal: black
//! ground, hot-pink borders and text family (#FF2E88 accent,
//! #FF9EC4 body, #FF5CA8 headings, #7A4A61 disabled mauve). Hues
//! that carry data — hashes, statuses, refusal codes — stay
//! distinguishable per Addition U's data carve-out: hashes in
//! violet, ok in green, refusals in red. The Mono register is the
//! same layout in grayscale (the Nerd presentation).

use crate::app::{Accent, App, Screen, pretty};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

/// The terminal translation of the Addition U palette.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    /// Frame borders.
    pub border: Color,
    /// Body text.
    pub body: Color,
    /// Headings and labels.
    pub heading: Color,
    /// Active/focus accent.
    pub accent: Color,
    /// Disabled/secondary text.
    pub dim: Color,
    /// Hashes and machine data.
    pub data: Color,
    /// Success status.
    pub good: Color,
    /// Refusal/finding status.
    pub bad: Color,
}

/// The theme for a palette register.
pub fn theme(accent: Accent) -> Theme {
    match accent {
        Accent::Pink => Theme {
            border: Color::Rgb(255, 46, 136),
            body: Color::Rgb(255, 158, 196),
            heading: Color::Rgb(255, 92, 168),
            accent: Color::Rgb(255, 46, 136),
            dim: Color::Rgb(122, 74, 97),
            data: Color::Rgb(167, 139, 250),
            good: Color::Rgb(80, 200, 120),
            bad: Color::Rgb(255, 80, 80),
        },
        Accent::Mono => Theme {
            border: Color::Gray,
            body: Color::White,
            heading: Color::White,
            accent: Color::White,
            dim: Color::DarkGray,
            data: Color::Gray,
            good: Color::White,
            bad: Color::Gray,
        },
    }
}

impl Theme {
    fn block(self, title: &str, focused: bool) -> Block<'_> {
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if focused { self.accent } else { self.border }))
            .title(Span::styled(
                format!(" {title} "),
                Style::default()
                    .fg(self.heading)
                    .add_modifier(Modifier::BOLD),
            ))
            .style(Style::default().bg(Color::Black))
    }

    fn text(self) -> Style {
        Style::default().fg(self.body).bg(Color::Black)
    }

    fn selected(self) -> Style {
        Style::default()
            .fg(Color::Black)
            .bg(self.accent)
            .add_modifier(Modifier::BOLD)
    }
}

/// Draws the whole frame: header, the screen's body, status bar.
pub fn draw(frame: &mut Frame, app: &App) {
    let theme = theme(app.accent);
    let size = frame.area();
    // The companion pane redraws its viewport record each
    // frame; screens without the pane leave it cleared, so a
    // stale rect can never place a blast (D-639).
    app.companion_viewport.set(None);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(size);
    draw_header(frame, rows[0], app, theme);
    match app.screen {
        Screen::Projects => draw_projects(frame, rows[1], app, theme, size),
        Screen::Browse => draw_browse(frame, rows[1], app, theme),
        Screen::Detail => draw_detail(frame, rows[1], app, theme),
        Screen::Console => draw_console(frame, rows[1], app, theme),
        Screen::Validate => draw_validate(frame, rows[1], app, theme),
        Screen::Replay => draw_replay(frame, rows[1], app, theme),
        Screen::Preview => draw_preview(frame, rows[1], app, theme),
        Screen::Media => draw_media(frame, rows[1], app, theme, size),
        Screen::Files => draw_files(frame, rows[1], app, theme),
        Screen::Editor => draw_editor(frame, rows[1], app, theme),
        Screen::Settings => draw_settings(frame, rows[1], app, theme),
        Screen::Help => draw_help(frame, rows[1], theme),
    }
    draw_status(frame, rows[2], app, theme);
}

/// DEC-2's sparkle cycle (D-639): the glyph at position
/// `pos` advances every 3 ticks; positions are phase-offset
/// so they never blink in unison.
const SPARKLE_CYCLE: [char; 4] = ['✦', '·', '✧', '·'];

fn sparkle(tick: u64, pos: usize) -> char {
    SPARKLE_CYCLE[((tick / 3) as usize + pos) % SPARKLE_CYCLE.len()]
}

/// DEC-1's breath factor (D-639): the selection background
/// steps toward the black ground over a 20-tick (2.0 s)
/// cycle — accent, 75%, 55%, 75%.
fn breath_factor(tick: u64) -> u8 {
    [100, 75, 55, 75][((tick % 20) / 5) as usize]
}

/// Scales an RGB colour toward black by `factor` percent;
/// non-RGB colours pass through (the breath reads on the
/// register's accent either way).
fn breath(color: Color, factor: u8) -> Color {
    match color {
        Color::Rgb(r, g, b) => Color::Rgb(
            (u16::from(r) * u16::from(factor) / 100) as u8,
            (u16::from(g) * u16::from(factor) / 100) as u8,
            (u16::from(b) * u16::from(factor) / 100) as u8,
        ),
        other => other,
    }
}

/// The list selection style: DEC-1's breathing background
/// on the two list screens (Projects, Browse) while
/// decorations are on; the theme's flat selection anywhere
/// else.
fn selected_style(theme: Theme, app: &App) -> Style {
    if app.decorations_on && matches!(app.screen, Screen::Projects | Screen::Browse) {
        Style::default()
            .fg(Color::Black)
            .bg(breath(theme.accent, breath_factor(app.tick_count)))
            .add_modifier(Modifier::BOLD)
    } else {
        theme.selected()
    }
}

/// DEC-3's pulse phase (D-639): the status bar's mode
/// segment is dim for 5 ticks out of every 10.
fn status_dimmed(tick: u64) -> bool {
    (tick / 5) % 2 == 1
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let project = match &app.open {
        Some(open) => format!("{} · sha {}", open.project.header.name, short(&open.sha256)),
        None => "no project open".to_string(),
    };
    let mut spans = vec![
        Span::styled(
            " TOROGAO TUI ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(project, Style::default().fg(theme.data)),
        Span::raw("   "),
        Span::styled(
            "1 Browse  2 Verbs  3 Validate  4 Replay  5 Preview  6 Files  7 Settings  8 Media  ? Help",
            theme.text(),
        ),
    ];
    // DEC-2 (D-639): three sparkles on the title rule,
    // Projects and Media headers only.
    if app.decorations_on && matches!(app.screen, Screen::Projects | Screen::Media) {
        spans.push(Span::raw("  "));
        for pos in 0..3 {
            spans.push(Span::styled(
                sparkle(app.tick_count, pos).to_string(),
                Style::default().fg(theme.heading),
            ));
            spans.push(Span::raw(" "));
        }
    }
    let line = Line::from(spans);
    frame.render_widget(
        Paragraph::new(line).block(theme.block("torogao — terminal front-end", false)),
        area,
    );
}

fn draw_status(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    if let Some((_, buf)) = &app.prompt {
        let line = Line::from(vec![
            Span::styled(
                " PROMPT (Enter confirm · Esc cancel) > ",
                Style::default().fg(theme.accent),
            ),
            Span::styled(buf.text(), theme.text()),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        let (row, col) = buf.cursor();
        let _ = row;
        frame.set_cursor_position((area.x + 40 + u16::try_from(col).unwrap_or(0), area.y));
        return;
    }
    let mode = if app.console.editing {
        "EDITING INPUTS"
    } else if app.screen == Screen::Editor {
        "EDITOR"
    } else {
        "NORMAL"
    };
    // DEC-3 (D-639): the mode segment pulses bold/dim on
    // the work screens — the only animation there. Projects
    // and Media carry the companion and sparkles instead,
    // and Browse is DEC-1's screen.
    let dimmed = app.decorations_on
        && !matches!(
            app.screen,
            Screen::Projects | Screen::Media | Screen::Browse
        )
        && status_dimmed(app.tick_count);
    let line = Line::from(vec![
        Span::styled(
            format!(" {mode} "),
            Style::default().fg(theme.accent).add_modifier(if dimmed {
                Modifier::DIM
            } else {
                Modifier::BOLD
            }),
        ),
        Span::styled(app.status.clone(), theme.text()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_projects(frame: &mut Frame, area: Rect, app: &App, theme: Theme, size: Rect) {
    // D-639: with the companion pane, the front door splits
    // [30% list | Min detail | 28 cols companion]. Below the
    // threshold (or with the toggle off) the list takes the
    // whole body, exactly the pre-slice rendering.
    if app.companion_on && crate::companion::pane_visible(Screen::Projects, size.width, size.height)
    {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Min(0),
                Constraint::Length(crate::companion::PANE_COLS),
            ])
            .split(area);
        draw_project_list(frame, cols[0], app, theme);
        draw_project_detail(frame, cols[1], app, theme);
        draw_companion_pane(frame, cols[2], app, theme);
    } else {
        draw_project_list(frame, area, app, theme);
    }
}

fn draw_project_list(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let items: Vec<ListItem> = app
        .projects
        .iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            ListItem::new(Line::from(vec![
                Span::styled(name, theme.text()),
                Span::styled(
                    format!("  {}", path.display()),
                    Style::default().fg(theme.dim),
                ),
            ]))
        })
        .collect();
    let title = format!(
        "PROJECTS under {} — Enter open · n new · o open path · r refresh · q quit",
        app.projects_root.display()
    );
    let list = List::new(items)
        .block(theme.block(&title, true))
        .highlight_style(selected_style(theme, app))
        .highlight_symbol("> ");
    let mut state = ListState::default();
    state.select(if app.projects.is_empty() {
        None
    } else {
        Some(app.project_sel)
    });
    frame.render_stateful_widget(list, area, &mut state);
}

/// The middle column of the pane layout: the selected
/// project's detail — the name and the full path the list
/// row truncates.
fn draw_project_detail(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let lines = match app.projects.get(app.project_sel) {
        Some(path) => {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            vec![
                Line::from(Span::styled(
                    name,
                    Style::default()
                        .fg(theme.heading)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(path.display().to_string(), theme.text())),
                Line::from(Span::styled(
                    "project.ron present — Enter opens it".to_string(),
                    Style::default().fg(theme.dim),
                )),
            ]
        }
        None => vec![Line::from(Span::styled(
            "(no projects under the root — n creates one)".to_string(),
            Style::default().fg(theme.dim),
        ))],
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(theme.block("PROJECT", false))
            .wrap(Wrap { trim: false }),
        area,
    );
}

/// The companion pane (D-639): Prima's current frame in a
/// bordered pane with the graphics mode named at its foot —
/// the same honesty line the Media screen shows. In
/// direct-graphics modes the pixels arrive as a post-draw
/// blast over the recorded rect; in half-block mode the
/// frame is sampled into the buffer here.
fn draw_companion_pane(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let block = theme.block("PRIMA — companion", false);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);
    let image_area = rows[0];
    match app.media.mode {
        crate::media::GraphicsMode::HalfBlock => {
            if let Some(raster) = &app.companion.raster {
                let lines = halfblock_grid_lines(raster, image_area.width, image_area.height);
                frame.render_widget(Paragraph::new(lines), image_area);
            } else {
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        "(companion frame unavailable)".to_string(),
                        Style::default().fg(theme.dim),
                    ))),
                    image_area,
                );
            }
        }
        _ => {
            app.companion_viewport.set(Some((
                image_area.x,
                image_area.y,
                image_area.width,
                image_area.height,
            )));
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(
                "graphics: {} — {}",
                app.media.mode.label(),
                app.media.mode_reason
            ),
            Style::default().fg(theme.dim),
        ))),
        rows[1],
    );
}

/// Samples `raster` into half-block lines centred in a
/// `width` × `height` cell budget (the companion pane's
/// in-buffer rendering; the Media viewport keeps its own
/// zoom-aware path).
fn halfblock_grid_lines(
    raster: &image::RgbaImage,
    width: u16,
    height: u16,
) -> Vec<Line<'static>> {
    let grid = crate::media::halfblock_cells(raster, width, height);
    let grid_width = grid.first().map_or(0, |row| row.len());
    let pad_left = usize::from(width).saturating_sub(grid_width) / 2;
    let pad_top = usize::from(height).saturating_sub(grid.len()) / 2;
    let mut lines: Vec<Line> = Vec::new();
    for _ in 0..pad_top {
        lines.push(Line::from(""));
    }
    for row in &grid {
        let mut spans = vec![Span::raw(" ".repeat(pad_left))];
        for (top, bottom) in row {
            spans.push(Span::styled(
                "▀",
                Style::default()
                    .fg(Color::Rgb(top.0, top.1, top.2))
                    .bg(Color::Rgb(bottom.0, bottom.1, bottom.2)),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn two_panes(area: Rect) -> (Rect, Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);
    (cols[0], cols[1])
}

fn draw_browse(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let (left, right) = two_panes(area);
    let Some(open) = &app.open else { return };
    let families: Vec<ListItem> = open
        .families
        .iter()
        .map(|family| {
            ListItem::new(Line::from(vec![
                Span::styled(family.name, theme.text()),
                Span::styled(
                    format!("  ({})", family.records.len()),
                    Style::default().fg(theme.dim),
                ),
            ]))
        })
        .collect();
    let list = List::new(families)
        .block(theme.block(
            "FAMILIES — Tab/h/l switches pane",
            !app.browse_records_focus,
        ))
        .highlight_style(selected_style(theme, app))
        .highlight_symbol("> ");
    let mut state = ListState::default();
    state.select(Some(app.family_sel));
    frame.render_stateful_widget(list, left, &mut state);

    let records: Vec<ListItem> = app
        .current_family()
        .map(|family| {
            family
                .records
                .iter()
                .map(|record| {
                    ListItem::new(Line::from(vec![
                        Span::styled(record.id.clone(), Style::default().fg(theme.heading)),
                        Span::styled(format!("  {}", record.summary), theme.text()),
                    ]))
                })
                .collect()
        })
        .unwrap_or_default();
    let title = format!(
        "RECORDS — Enter detail · e edit via verb · {}",
        app.current_family().map_or("", |family| family.name)
    );
    let list = List::new(records)
        .block(theme.block(&title, app.browse_records_focus))
        .highlight_style(selected_style(theme, app))
        .highlight_symbol("> ");
    let mut state = ListState::default();
    state.select(if records_len(app) == 0 {
        None
    } else {
        Some(app.record_sel)
    });
    frame.render_stateful_widget(list, right, &mut state);
}

fn records_len(app: &App) -> usize {
    app.current_family()
        .map_or(0, |family| family.records.len())
}

fn draw_detail(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let Some(record) = app.current_record() else {
        return;
    };
    let mut lines = vec![Line::from(Span::styled(
        record.id.clone(),
        Style::default()
            .fg(theme.heading)
            .add_modifier(Modifier::BOLD),
    ))];
    for detail in &record.detail {
        lines.push(Line::from(Span::styled(detail.clone(), theme.text())));
    }
    let paragraph = Paragraph::new(lines)
        .block(theme.block("RECORD — j/k scroll · e edit via verb · Esc back", true))
        .wrap(Wrap { trim: false })
        .scroll((u16::try_from(app.detail_scroll).unwrap_or(0), 0));
    frame.render_widget(paragraph, area);
}

fn draw_console(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let (left, right) = two_panes(area);
    let verbs: Vec<ListItem> = app
        .console
        .verbs
        .iter()
        .map(|row| {
            let tags = match &row.deferred {
                Some(slice) => format!(" [deferred: {slice}]"),
                None if row.mutating => " [mutating]".to_string(),
                None => " [read-only]".to_string(),
            };
            ListItem::new(Line::from(vec![
                Span::styled(row.name.clone(), theme.text()),
                Span::styled(tags, Style::default().fg(theme.dim)),
            ]))
        })
        .collect();
    let list = List::new(verbs)
        .block(theme.block("VERBS — Enter edit inputs · x run", !app.console.editing))
        .highlight_style(theme.selected())
        .highlight_symbol("> ");
    let mut state = ListState::default();
    state.select(Some(app.console.sel));
    frame.render_stateful_widget(list, left, &mut state);

    let halves = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(right);
    let verb_name = app
        .console
        .verbs
        .get(app.console.sel)
        .map_or("", |row| row.name.as_str());
    let inputs_title = format!(
        "INPUTS (JSON) for {verb_name} — {}",
        if app.console.editing {
            "editing: F5/Ctrl+R run · Ctrl+U clear · Esc done"
        } else {
            "Enter to edit"
        }
    );
    let text = app.console.inputs.text();
    let inputs = Paragraph::new(text)
        .block(theme.block(&inputs_title, app.console.editing))
        .style(theme.text());
    frame.render_widget(inputs, halves[0]);
    if app.console.editing {
        let (row, col) = app.console.inputs.cursor();
        let inner = halves[0].inner(ratatui::layout::Margin {
            horizontal: 1,
            vertical: 1,
        });
        let scroll = row.saturating_sub(usize::from(inner.height.saturating_sub(1)));
        frame.set_cursor_position((
            inner.x + u16::try_from(col).unwrap_or(0),
            inner.y + u16::try_from(row - scroll).unwrap_or(0),
        ));
    }
    let (answer_title, answer_body, answer_style) = match &app.console.answer {
        Some((status, doc)) => (
            format!("ANSWER — {}", crate::app::status_label(*status)),
            doc.clone(),
            if matches!(
                status,
                torogao_author::Status::Refused | torogao_author::Status::Unloadable
            ) {
                Style::default().fg(theme.bad)
            } else {
                theme.text()
            },
        ),
        None => (
            "ANSWER".to_string(),
            "(no verb run yet)".to_string(),
            theme.text(),
        ),
    };
    let answer = Paragraph::new(answer_body)
        .block(theme.block(&answer_title, false))
        .style(answer_style)
        .wrap(Wrap { trim: false });
    frame.render_widget(answer, halves[1]);
}

fn draw_validate(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    if !app.validate.ran {
        frame.render_widget(
            Paragraph::new("press r to run project_validate")
                .block(theme.block("VALIDATE", true))
                .style(theme.text()),
            area,
        );
        return;
    }
    if app.validate.findings.is_empty() {
        let line = Line::from(vec![
            Span::styled(
                "CLEAN",
                Style::default().fg(theme.good).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" — no findings · sha {}", short(&app.validate.sha256)),
                theme.text(),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(line).block(theme.block("VALIDATE — r re-run · Esc back", true)),
            area,
        );
        return;
    }
    let items: Vec<ListItem> = app
        .validate
        .findings
        .iter()
        .map(|(code, message)| {
            ListItem::new(Line::from(vec![
                Span::styled(code.clone(), Style::default().fg(theme.bad)),
                Span::styled(format!("  {message}"), theme.text()),
            ]))
        })
        .collect();
    let title = format!(
        "VALIDATE — {} finding(s) · sha {} · r re-run",
        app.validate.findings.len(),
        short(&app.validate.sha256)
    );
    let list = List::new(items)
        .block(theme.block(&title, true))
        .highlight_style(theme.selected())
        .highlight_symbol("> ");
    let mut state = ListState::default();
    state.select(Some(app.validate.sel));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_replay(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let halves = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);
    let (summary, diverged) = match (&app.replay.report, &app.replay.refusal) {
        (Some(report), _) => (
            format!(
                "records {} · denials {} · replayed sha {}",
                report["records"],
                report["denials"],
                short(report["sha256"].as_str().unwrap_or(""))
            ),
            app.replay_diverged(),
        ),
        (None, Some((code, message))) => {
            (format!("REFUSED {code} — {message}"), app.replay_diverged())
        }
        (None, None) => ("press r to run project_replay".to_string(), false),
    };
    let items: Vec<ListItem> = app
        .replay
        .log
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let verb = entry["verb"].as_str().unwrap_or("?");
            let tail = if entry["result"].as_str() == Some("denied") {
                format!("DENIED {}", entry["refusal"]["code"].as_str().unwrap_or(""))
            } else {
                format!(
                    "sha {}",
                    short(entry["project_sha256_after"].as_str().unwrap_or(""))
                )
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:>4} ", index + 1), Style::default().fg(theme.dim)),
                Span::styled(verb.to_string(), theme.text()),
                Span::styled(format!("  {tail}"), Style::default().fg(theme.data)),
            ]))
        })
        .collect();
    let title = if diverged {
        format!("REPLAY — {summary} · !! DIVERGED from on-disk project !!")
    } else {
        format!("REPLAY — {summary} · j/k step · r re-run")
    };
    let list = List::new(items)
        .block(theme.block(&title, true))
        .highlight_style(theme.selected())
        .highlight_symbol("> ");
    let mut state = ListState::default();
    state.select(if app.replay.log.is_empty() {
        None
    } else {
        Some(app.replay.sel)
    });
    frame.render_stateful_widget(list, halves[0], &mut state);

    let detail = app
        .replay
        .log
        .get(app.replay.sel)
        .map_or_else(|| "(no log record selected)".to_string(), pretty);
    frame.render_widget(
        Paragraph::new(detail)
            .block(theme.block("LOG RECORD", false))
            .style(theme.text())
            .wrap(Wrap { trim: false }),
        halves[1],
    );
}

fn draw_preview(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let Some(open) = &app.open else { return };
    let project = &open.project;
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "STRUCTURED READOUT — this is the companion/persona state as data, not a rendering.",
            Style::default().fg(theme.dim),
        )),
        Line::from(Span::styled(
            format!("project: {}", project.header.name),
            theme.text(),
        )),
        Line::from(Span::styled(
            format!(
                "lane {} · rating {} · consent {}",
                project.header.stamp.lane,
                project.header.stamp.rating,
                project.header.consent.schema
            ),
            theme.text(),
        )),
        Line::from(Span::styled(
            format!(
                "assets {} · parts {} · deformers {} · parameters {} · expressions {}",
                project.assets.len(),
                project.parts.len(),
                project.deformers.len(),
                project.parameters.len(),
                project.expressions.len()
            ),
            theme.text(),
        )),
        Line::from(Span::styled(
            "neutral pose:",
            Style::default().fg(theme.heading),
        )),
    ];
    for parameter in &project.parameters {
        lines.push(Line::from(Span::styled(
            format!(
                "  {} = {} (default {}, range {}..{})",
                parameter.id,
                project.neutral_value(parameter),
                parameter.default,
                parameter.min,
                parameter.max
            ),
            theme.text(),
        )));
    }
    lines.push(Line::from(Span::styled(
        "expressions — Enter runs expression_test:",
        Style::default().fg(theme.heading),
    )));
    for (index, expression) in project.expressions.iter().enumerate() {
        let marker = if index == app.preview.sel { "> " } else { "  " };
        lines.push(Line::from(Span::styled(
            format!("{marker}{}", expression.id),
            if index == app.preview.sel {
                theme.selected()
            } else {
                theme.text()
            },
        )));
    }
    for tested in &app.preview.tested {
        lines.push(Line::from(Span::styled(
            tested.clone(),
            Style::default().fg(theme.data),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(theme.block(
                "PREVIEW — j/k pick expression · Enter test · Esc back",
                true,
            ))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_media(frame: &mut Frame, area: Rect, app: &App, theme: Theme, size: Rect) {
    let (left, right) = two_panes(area);
    let tag = |kind: crate::media::MediaKind| match kind {
        crate::media::MediaKind::Image => "IMG",
        crate::media::MediaKind::Model => "MDL",
        crate::media::MediaKind::Video => "VID",
    };
    let items: Vec<ListItem> = app
        .media
        .items
        .iter()
        .map(|item| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("[{}] ", tag(item.kind)),
                    Style::default().fg(theme.data),
                ),
                Span::styled(item.label.clone(), theme.text()),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(theme.block(
            "MEDIA — j/k pick · Enter reload · o open path · Esc back",
            true,
        ))
        .highlight_style(theme.selected())
        .highlight_symbol("> ");
    let mut state = ListState::default();
    state.select(if app.media.items.is_empty() {
        None
    } else {
        Some(app.media.sel)
    });
    frame.render_stateful_widget(list, left, &mut state);

    let label = app
        .media
        .items
        .get(app.media.sel)
        .map_or("MEDIA PREVIEW", |item| item.label.as_str());
    let title = format!(
        "PREVIEW {label} — {} · space play · ←/→ step · +/− zoom",
        app.media.mode.label()
    );
    let halves = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(6)])
        .split(right);
    // D-639: the companion pane docks right of the preview
    // area, splitting the viewport region [Min preview |
    // 28 cols companion]; the list and the INFO pane below
    // are untouched. Below the width threshold the preview
    // takes the whole region, exactly the pre-slice layout.
    let (preview_area, companion_area) =
        if app.companion_on && crate::companion::pane_visible(Screen::Media, size.width, size.height)
        {
            let split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Min(0),
                    Constraint::Length(crate::companion::PANE_COLS),
                ])
                .split(halves[0]);
            (split[0], Some(split[1]))
        } else {
            (halves[0], None)
        };
    let viewport = preview_area.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 1,
    });
    let graphics = app.media.mode != crate::media::GraphicsMode::HalfBlock;
    match (&app.media.raster, &app.media.error) {
        (Some(raster), _) if graphics => {
            // The pixels arrive as a post-draw blast over this
            // reserved rect; the buffer holds only the frame.
            app.media_viewport.set(Some((
                viewport.x,
                viewport.y,
                viewport.width,
                viewport.height,
            )));
            frame.render_widget(
                Paragraph::new("").block(theme.block(&title, true)),
                preview_area,
            );
            let _ = raster;
        }
        (Some(raster), _) => {
            app.media_viewport.set(None);
            let cols = ((f32::from(viewport.width) * app.media.zoom) as u16).max(1);
            let rows = ((f32::from(viewport.height) * app.media.zoom) as u16).max(1);
            let grid = crate::media::halfblock_cells(raster, cols, rows);
            let grid_width = grid.first().map_or(0, |row| row.len());
            let pad_left = usize::from(viewport.width).saturating_sub(grid_width) / 2;
            let pad_top = usize::from(viewport.height).saturating_sub(grid.len()) / 2;
            let mut lines: Vec<Line> = Vec::new();
            for _ in 0..pad_top {
                lines.push(Line::from(""));
            }
            for row in &grid {
                let mut spans = vec![Span::raw(" ".repeat(pad_left))];
                for (top, bottom) in row {
                    spans.push(Span::styled(
                        "▀",
                        Style::default()
                            .fg(Color::Rgb(top.0, top.1, top.2))
                            .bg(Color::Rgb(bottom.0, bottom.1, bottom.2)),
                    ));
                }
                lines.push(Line::from(spans));
            }
            frame.render_widget(
                Paragraph::new(lines).block(theme.block(&title, true)),
                preview_area,
            );
        }
        (None, Some(error)) => {
            app.media_viewport.set(None);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    error.clone(),
                    Style::default().fg(theme.bad),
                )))
                .block(theme.block(&title, true))
                .wrap(Wrap { trim: false }),
                preview_area,
            );
        }
        (None, None) => {
            app.media_viewport.set(None);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "no media selected — j/k pick an item, or o opens a file by path",
                    Style::default().fg(theme.dim),
                )))
                .block(theme.block(&title, true)),
                preview_area,
            );
        }
    }
    if let Some(pane) = companion_area {
        draw_companion_pane(frame, pane, app, theme);
    }
    let mut info_lines = vec![Line::from(Span::styled(
        format!(
            "graphics: {} — {}",
            app.media.mode.label(),
            app.media.mode_reason
        ),
        Style::default().fg(theme.dim),
    ))];
    for line in &app.media.info {
        info_lines.push(Line::from(Span::styled(line.clone(), theme.text())));
    }
    frame.render_widget(
        Paragraph::new(info_lines)
            .block(theme.block("INFO / TRANSPORT", false))
            .wrap(Wrap { trim: false }),
        halves[1],
    );
}

fn draw_files(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let items: Vec<ListItem> = app
        .files
        .iter()
        .map(|row| {
            ListItem::new(Line::from(vec![
                Span::styled(row.name.clone(), theme.text()),
                Span::styled(
                    format!(
                        "  {} · {}",
                        if row.exists { "present" } else { "absent" },
                        if row.editable {
                            "editable (parse-gated save)"
                        } else {
                            "read-only (append-only)"
                        }
                    ),
                    Style::default().fg(theme.dim),
                ),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(theme.block(
            "FILES — Enter open · project files are text (Addition V); content.ron prose is English (D-472)",
            true,
        ))
        .highlight_style(theme.selected())
        .highlight_symbol("> ");
    let mut state = ListState::default();
    state.select(if app.files.is_empty() {
        None
    } else {
        Some(app.file_sel)
    });
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_editor(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let Some(editor) = &app.editor else { return };
    let mut title = format!(
        "EDIT {}{} — Ctrl+S save (parse-gated) · Esc {}",
        editor.name,
        if editor.dirty { " *" } else { "" },
        if editor.dirty { "asks" } else { "back" }
    );
    if editor.readonly {
        title.push_str(" · READ-ONLY");
    }
    if editor.confirm_discard {
        title.push_str(" · DISCARD CHANGES? y/n");
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(2)])
        .split(area);
    let text = editor.buf.text();
    let paragraph = Paragraph::new(text)
        .block(theme.block(&title, true))
        .style(theme.text())
        .scroll((
            u16::try_from(
                editor
                    .buf
                    .cursor()
                    .0
                    .saturating_sub(usize::from(rows[0].height.saturating_sub(3))),
            )
            .unwrap_or(0),
            0,
        ));
    frame.render_widget(paragraph, rows[0]);
    let (row, col) = editor.buf.cursor();
    let inner = rows[0].inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 1,
    });
    frame.set_cursor_position((
        inner.x + u16::try_from(col).unwrap_or(0),
        inner.y
            + u16::try_from(row.saturating_sub(usize::from(rows[0].height.saturating_sub(3))))
                .unwrap_or(0),
    ));
    let diagnostic = match &editor.diagnostic {
        Some(diagnostic) => Line::from(Span::styled(
            format!("SAVE GATE: {diagnostic}"),
            Style::default().fg(theme.bad),
        )),
        None => Line::from(Span::styled(
            format!(
                "line {} col {} · {} lines",
                row + 1,
                col + 1,
                editor.buf.lines().len()
            ),
            Style::default().fg(theme.dim),
        )),
    };
    frame.render_widget(Paragraph::new(diagnostic), rows[1]);
}

fn draw_settings(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let lines = vec![
        Line::from(Span::styled(
            format!(
                "accent register: {}  (a toggles Pink/Mono)",
                match app.accent {
                    Accent::Pink => "pink (Persona)",
                    Accent::Mono => "mono (Nerd)",
                }
            ),
            theme.text(),
        )),
        Line::from(Span::styled(
            format!(
                "projects root: {}  (p changes)",
                app.projects_root.display()
            ),
            theme.text(),
        )),
        Line::from(Span::styled(
            format!(
                "companion pane: {}  (c toggles) — Prima on the front door and Media",
                if app.companion_on { "on" } else { "off" }
            ),
            theme.text(),
        )),
        Line::from(Span::styled(
            format!(
                "decorations: {}  (d toggles) — selection breath, title sparkle, status pulse",
                if app.decorations_on { "on" } else { "off" }
            ),
            theme.text(),
        )),
        Line::from(Span::styled(
            "settings persist to ~/.config/torogao/tui-settings.txt".to_string(),
            Style::default().fg(theme.dim),
        )),
        Line::from(Span::styled(
            "mouse: not used anywhere — every action is keyboard-reachable".to_string(),
            Style::default().fg(theme.dim),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(theme.block("SETTINGS — Esc back", true))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_help(frame: &mut Frame, area: Rect, theme: Theme) {
    let lines: Vec<Line> = crate::KEYMAP
        .iter()
        .map(|(keys, action)| {
            Line::from(vec![
                Span::styled(
                    format!("{keys:<22}"),
                    Style::default()
                        .fg(theme.heading)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(*action, theme.text()),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines)
            .block(theme.block("KEYMAP — Esc/q back", true))
            .wrap(Wrap { trim: false }),
        area,
    );
}

/// The first 12 chars of a hash, for display.
fn short(hash: &str) -> String {
    hash.chars().take(12).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breath_factor_steps_the_twenty_tick_cycle() {
        assert_eq!(breath_factor(0), 100);
        assert_eq!(breath_factor(4), 100);
        assert_eq!(breath_factor(5), 75);
        assert_eq!(breath_factor(9), 75);
        assert_eq!(breath_factor(10), 55);
        assert_eq!(breath_factor(14), 55);
        assert_eq!(breath_factor(15), 75);
        assert_eq!(breath_factor(19), 75);
        assert_eq!(breath_factor(20), 100);
    }

    #[test]
    fn breath_scales_rgb_toward_black() {
        assert_eq!(breath(Color::Rgb(255, 46, 136), 100), Color::Rgb(255, 46, 136));
        assert_eq!(breath(Color::Rgb(255, 46, 136), 55), Color::Rgb(140, 25, 74));
        assert_eq!(breath(Color::White, 55), Color::White);
    }

    #[test]
    fn sparkle_advances_every_three_ticks_with_phase_offsets() {
        // Same position, one full glyph cycle per 12 ticks.
        assert_eq!(sparkle(0, 0), '✦');
        assert_eq!(sparkle(2, 0), '✦');
        assert_eq!(sparkle(3, 0), '·');
        assert_eq!(sparkle(6, 0), '✧');
        assert_eq!(sparkle(9, 0), '·');
        assert_eq!(sparkle(12, 0), '✦');
        // Positions never blink in unison: at any tick the
        // three positions are not all the same glyph phase.
        for tick in 0..24 {
            let phases: Vec<usize> = (0..3)
                .map(|pos| ((tick / 3) as usize + pos) % SPARKLE_CYCLE.len())
                .collect();
            assert!(
                phases[0] != phases[1] && phases[1] != phases[2],
                "tick {tick}: {phases:?}"
            );
        }
        // The glyph at a position is its cycle entry.
        assert_eq!(sparkle(0, 1), SPARKLE_CYCLE[1]);
        assert_eq!(sparkle(0, 2), SPARKLE_CYCLE[2]);
    }

    #[test]
    fn status_pulse_alternates_every_five_ticks() {
        assert!(!status_dimmed(0));
        assert!(!status_dimmed(4));
        assert!(status_dimmed(5));
        assert!(status_dimmed(9));
        assert!(!status_dimmed(10));
    }
}
