//! Headless keystroke smoke (D-487 gate): drives the real `App`
//! state machine with key events and renders through ratatui's
//! `TestBackend`, asserting screen state at each step of the
//! core flow — open fixture project → browse records → edit a
//! record through its verb → validate → inspect replay — plus
//! the adversarial cases (bad JSON, a deferred verb, bad RON in
//! the editor, a non-canonical project file).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::{Path, PathBuf};
use torogao_author::Status;
use torogao_tui::App;
use torogao_tui::app::Screen;

/// A fresh temp projects root unique to this test process.
fn fresh_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("torogao-tui-{}-{name}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clear stale dir");
    }
    std::fs::create_dir_all(&dir).expect("create dir");
    dir
}

/// Builds the fixture project `smoke` inside `root` through the
/// authoring pipeline itself: init, two parameters, a pose
/// entry, one expression.
fn build_fixture(root: &Path) -> PathBuf {
    let project = root.join("smoke");
    std::fs::create_dir_all(&project).expect("create project dir");
    let script = r#"
{"verb":"project_init","inputs":{"name":"smoke","stamp":{"basis":"test","lane":"all-ages","pack":"all-ages","rating":"all-ages"},"consent":{"adult":true,"schema":"torogao-consent/1","wants_selected_scenes":true}}}
{"verb":"param_define","inputs":{"id":"param_a","class":"angle","min":0.0,"default":0.5,"max":1.0}}
{"verb":"param_define","inputs":{"id":"param_b","class":"angle","min":-1.0,"default":0.0,"max":1.0}}
{"verb":"param_pose_default_set","inputs":{"parameter":"param_a","value":0.25}}
{"verb":"expression_define","inputs":{"id":"smile","blend":"additive","values":[{"parameter":"param_a","value":0.1}],"visibility":[]}}
"#;
    let requests = torogao_author::parse_script(script).expect("script parses");
    let answer = torogao_author::run_requests(&project, &project, &requests);
    assert_eq!(answer.status, Status::Ok, "fixture: {}", answer.doc);
    project
}

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ch(app: &mut App, c: char) {
    key(app, KeyCode::Char(c));
}

fn ctrl(app: &mut App, c: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        ch(app, c);
    }
}

fn render(app: &App) -> String {
    let backend = TestBackend::new(110, 32);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| torogao_tui::view::draw(frame, app))
        .expect("draw");
    let buffer = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn file_sha(path: &Path) -> String {
    torogao_author::intake::sha256_hex(&std::fs::read(path).expect("read file"))
}

#[test]
fn core_flow_open_browse_edit_validate_replay() {
    let root = fresh_root("core");
    let project_dir = build_fixture(&root);
    let sha_before = file_sha(&project_dir.join("project.ron"));

    let mut app = App::new(root.clone());
    let screen = render(&app);
    assert!(
        screen.contains("PROJECTS"),
        "home lists projects:\n{screen}"
    );
    assert!(screen.contains("smoke"), "fixture listed:\n{screen}");

    // Open the fixture.
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::Browse);
    let screen = render(&app);
    assert!(screen.contains("FAMILIES"), "browser renders:\n{screen}");
    assert!(screen.contains("parameters"), "families listed:\n{screen}");

    // Browse to the parameters family, then to its records.
    for _ in 0..5 {
        ch(&mut app, 'j');
    }
    assert_eq!(
        app.current_family().map(|family| family.name),
        Some("parameters")
    );
    key(&mut app, KeyCode::Tab);
    let screen = render(&app);
    assert!(screen.contains("param_a"), "records listed:\n{screen}");

    // Detail view of param_a.
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::Detail);
    let screen = render(&app);
    assert!(
        screen.contains("neutral (pose) value: 0.25"),
        "detail:\n{screen}"
    );
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.screen, Screen::Browse);

    // Move to the pose family (Tab back to families, two rows
    // down, Tab to records) and edit param_a's neutral value
    // through its set-verb — the core's in-place edit path.
    key(&mut app, KeyCode::Tab);
    ch(&mut app, 'j');
    ch(&mut app, 'j');
    assert_eq!(app.current_family().map(|family| family.name), Some("pose"));
    key(&mut app, KeyCode::Tab);
    ch(&mut app, 'e');
    assert_eq!(app.screen, Screen::Console);
    assert!(app.console.editing, "prefill opens the inputs editor");
    assert!(
        app.console.inputs.text().contains("\"value\": 0.25"),
        "prefill carries the record: {}",
        app.console.inputs.text()
    );
    ctrl(&mut app, 'u');
    type_text(&mut app, "{\"parameter\":\"param_a\",\"value\":0.6}");
    key(&mut app, KeyCode::F(5));
    let (status, _) = app.console.answer.clone().expect("answer recorded");
    assert_eq!(status, Status::Ok, "edit admitted");
    let open = app.open.as_ref().expect("project still open");
    let parameter = open.project.parameter("param_a").expect("parameter");
    assert_eq!(
        open.project.neutral_value(parameter),
        0.6,
        "the edit landed in the model"
    );
    assert_ne!(
        file_sha(&project_dir.join("project.ron")),
        sha_before,
        "the edit landed on disk"
    );

    // Validate: the edited project is clean.
    key(&mut app, KeyCode::Esc);
    ch(&mut app, '3');
    assert_eq!(app.screen, Screen::Validate);
    assert!(app.validate.ran);
    assert!(
        app.validate.findings.is_empty(),
        "findings: {:?}",
        app.validate.findings
    );
    let screen = render(&app);
    assert!(
        screen.contains("CLEAN"),
        "validate renders clean:\n{screen}"
    );

    // Replay: six admitted ops (5 fixture + 1 edit), no divergence.
    ch(&mut app, '4');
    assert_eq!(app.screen, Screen::Replay);
    assert!(app.replay.ran);
    assert_eq!(app.replay.log.len(), 6, "log: {:?}", app.replay.log);
    assert!(!app.replay_diverged(), "replay ends at the on-disk hash");
    let screen = render(&app);
    assert!(screen.contains("REPLAY"), "replay renders:\n{screen}");
    ch(&mut app, 'j');
    assert_eq!(app.replay.sel, 1, "stepping moves through the log");

    // Preview: the structured readout names the pose state.
    ch(&mut app, '5');
    assert_eq!(app.screen, Screen::Preview);
    let screen = render(&app);
    assert!(
        screen.contains("STRUCTURED READOUT"),
        "preview is labelled for what it is:\n{screen}"
    );
    assert!(screen.contains("param_a = 0.6"), "pose readout:\n{screen}");
    // Expression test from the preview screen.
    key(&mut app, KeyCode::Enter);
    assert!(
        !app.preview.tested.is_empty(),
        "expression_test produced a state readout"
    );

    // Files: author a content.ron sidecar through the editor.
    ch(&mut app, '6');
    assert_eq!(app.screen, Screen::Files);
    ch(&mut app, 'j');
    assert_eq!(app.files[app.file_sel].name, "content.ron");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::Editor);
    type_text(&mut app, "\"hello\"");
    ctrl(&mut app, 's');
    let content_path = project_dir.join("content.ron");
    assert!(content_path.is_file(), "sidecar saved through the gate");
    assert_eq!(
        std::fs::read_to_string(&content_path).expect("read sidecar"),
        "\"hello\""
    );
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.screen, Screen::Files);
}

#[test]
fn bad_json_is_a_diagnostic_and_writes_nothing() {
    let root = fresh_root("badjson");
    let project_dir = build_fixture(&root);
    let sha_before = file_sha(&project_dir.join("project.ron"));
    let mut app = App::new(root);
    key(&mut app, KeyCode::Enter);
    ch(&mut app, '2');
    assert_eq!(app.screen, Screen::Console);
    key(&mut app, KeyCode::Enter);
    assert!(app.console.editing);
    ctrl(&mut app, 'u');
    type_text(&mut app, "{bad json");
    key(&mut app, KeyCode::F(5));
    let (status, doc) = app.console.answer.clone().expect("answer recorded");
    assert_eq!(status, Status::Refused);
    assert!(doc.contains("not valid JSON"), "diagnostic: {doc}");
    assert_eq!(
        file_sha(&project_dir.join("project.ron")),
        sha_before,
        "nothing ran, nothing was written"
    );
}

#[test]
fn deferred_verb_refuses_by_name_and_writes_nothing() {
    let root = fresh_root("deferred");
    let project_dir = build_fixture(&root);
    let sha_before = file_sha(&project_dir.join("project.ron"));
    let mut app = App::new(root);
    key(&mut app, KeyCode::Enter);
    ch(&mut app, '2');
    let index = app
        .console
        .verbs
        .iter()
        .position(|row| row.name == "physics_bake")
        .expect("deferred verb listed");
    assert!(app.console.verbs[index].deferred.is_some());
    app.console.sel = index;
    app.console.inputs = torogao_tui::textbuf::TextBuf::from_text("{}");
    ch(&mut app, 'x');
    let (status, doc) = app.console.answer.clone().expect("answer recorded");
    assert_eq!(status, Status::Refused);
    assert!(
        doc.contains("physics_bake"),
        "refusal names the verb: {doc}"
    );
    assert_eq!(file_sha(&project_dir.join("project.ron")), sha_before);
}

#[test]
fn bad_ron_in_editor_is_refused_at_the_save_gate() {
    let root = fresh_root("badron");
    let project_dir = build_fixture(&root);
    let project_file = project_dir.join("project.ron");
    let bytes_before = std::fs::read(&project_file).expect("read project");
    let mut app = App::new(root);
    key(&mut app, KeyCode::Enter);
    ch(&mut app, '6');
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::Editor);
    ch(&mut app, 'x');
    ctrl(&mut app, 's');
    let editor = app.editor.as_ref().expect("editor still open");
    assert!(
        editor.diagnostic.is_some(),
        "the gate's refusal is the diagnostic"
    );
    assert!(editor.dirty, "a refused save leaves the buffer dirty");
    assert_eq!(
        std::fs::read(&project_file).expect("read project"),
        bytes_before,
        "the file is byte-identical after the refused save"
    );
    // Esc asks; y discards.
    key(&mut app, KeyCode::Esc);
    assert!(app.editor.as_ref().expect("editor").confirm_discard);
    ch(&mut app, 'y');
    assert_eq!(app.screen, Screen::Files);
}

#[test]
fn noncanonical_project_text_is_a_validate_finding() {
    let root = fresh_root("finding");
    let project_dir = build_fixture(&root);
    // A prose-class edit a text editor would make: an extra blank
    // line. The file still parses; it is no longer the canonical
    // emission, and validate must say so.
    let project_file = project_dir.join("project.ron");
    let mut text = std::fs::read_to_string(&project_file).expect("read project");
    text.push('\n');
    std::fs::write(&project_file, text).expect("rewrite project");

    let mut app = App::new(root);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::Browse, "the file still loads");
    ch(&mut app, '3');
    assert!(app.validate.ran);
    assert!(
        !app.validate.findings.is_empty(),
        "the non-canonical text is a finding"
    );
    let screen = render(&app);
    assert!(screen.contains("finding(s)"), "findings render:\n{screen}");
    // And replay now diverges from the on-disk bytes — surfaced.
    ch(&mut app, '4');
    assert!(
        app.replay_diverged(),
        "text edit vs op log divergence is flagged"
    );
}

#[test]
fn create_project_from_the_home_screen() {
    let root = fresh_root("create");
    let mut app = App::new(root.clone());
    ch(&mut app, 'n');
    assert!(app.prompt.is_some(), "new-project prompt opens");
    type_text(&mut app, "fresh");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::Browse, "created and opened");
    assert!(root.join("fresh").join("project.ron").is_file());
    assert_eq!(
        app.open
            .as_ref()
            .map(|open| open.project.header.name.as_str()),
        Some("fresh")
    );
}
