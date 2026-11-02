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
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(frame.area());
    draw_header(frame, rows[0], app, theme);
    match app.screen {
        Screen::Projects => draw_projects(frame, rows[1], app, theme),
        Screen::Browse => draw_browse(frame, rows[1], app, theme),
        Screen::Detail => draw_detail(frame, rows[1], app, theme),
        Screen::Console => draw_console(frame, rows[1], app, theme),
        Screen::Validate => draw_validate(frame, rows[1], app, theme),
        Screen::Replay => draw_replay(frame, rows[1], app, theme),
        Screen::Preview => draw_preview(frame, rows[1], app, theme),
        Screen::Files => draw_files(frame, rows[1], app, theme),
        Screen::Editor => draw_editor(frame, rows[1], app, theme),
        Screen::Settings => draw_settings(frame, rows[1], app, theme),
        Screen::Help => draw_help(frame, rows[1], theme),
    }
    draw_status(frame, rows[2], app, theme);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let project = match &app.open {
        Some(open) => format!("{} · sha {}", open.project.header.name, short(&open.sha256)),
        None => "no project open".to_string(),
    };
    let line = Line::from(vec![
        Span::styled(
            " TOROGAO TUI ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(project, Style::default().fg(theme.data)),
        Span::raw("   "),
        Span::styled(
            "1 Browse  2 Verbs  3 Validate  4 Replay  5 Preview  6 Files  7 Settings  ? Help",
            theme.text(),
        ),
    ]);
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
    let line = Line::from(vec![
        Span::styled(
            format!(" {mode} "),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(app.status.clone(), theme.text()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_projects(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
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
        .highlight_style(theme.selected())
        .highlight_symbol("> ");
    let mut state = ListState::default();
    state.select(if app.projects.is_empty() {
        None
    } else {
        Some(app.project_sel)
    });
    frame.render_stateful_widget(list, area, &mut state);
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
        .highlight_style(theme.selected())
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
        .highlight_style(theme.selected())
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
