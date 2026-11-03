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

/// Writes a solid-colour RGBA PNG using only std (stored
/// deflate blocks), so media fixtures need no encoder crate.
fn write_test_png(path: &Path, width: u32, height: u32, rgba: [u8; 4]) {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc: u32 = 0xFFFF_FFFF;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xEDB8_8320 & (0u32.wrapping_sub(crc & 1)));
            }
        }
        !crc
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut crc_input = kind.to_vec();
        crc_input.extend_from_slice(data);
        out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    }
    // Raw scanlines: filter byte 0 + RGBA per pixel.
    let mut raw = Vec::new();
    for _ in 0..height {
        raw.extend_from_slice(&[0]);
        for _ in 0..width {
            raw.extend_from_slice(&rgba);
        }
    }
    // zlib: header + stored blocks + adler32.
    let mut zlib = vec![0x78u8, 0x01];
    let mut offset = 0;
    while offset < raw.len() {
        let take = (raw.len() - offset).min(65_535);
        let last = offset + take == raw.len();
        zlib.push(u8::from(last));
        zlib.extend_from_slice(&(take as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(take as u16)).to_le_bytes());
        zlib.extend_from_slice(&raw[offset..offset + take]);
        offset += take;
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut png = vec![137u8, 80, 78, 71, 13, 10, 26, 10];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &zlib);
    chunk(&mut png, b"IEND", &[]);
    std::fs::write(path, png).expect("write test png");
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

#[test]
fn media_screen_previews_still_steps_video_and_refuses_model_by_name() {
    let root = fresh_root("media");
    let project_dir = build_fixture(&root);
    write_test_png(&project_dir.join("a-still.png"), 8, 8, [255, 46, 136, 255]);
    std::fs::write(project_dir.join("c-model.inx"), b"not a real puppet").expect("write model");

    // A short clip, when ffmpeg is at hand; the video leg of
    // this test skips silently where it is not.
    let have_ffmpeg = std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if have_ffmpeg {
        let made = std::process::Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=0xFF2E88:size=64x64:rate=10:duration=0.5",
                "-y",
            ])
            .arg(project_dir.join("b-video.mp4"))
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        assert!(made, "ffmpeg made the fixture clip");
    }

    let mut app = App::new(root);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::Browse);
    ch(&mut app, '8');
    assert_eq!(app.screen, Screen::Media);
    // The still is first in sorted order and is already loaded.
    assert_eq!(app.media.items.len(), if have_ffmpeg { 3 } else { 2 });
    assert!(
        app.media.raster.is_some(),
        "the still decoded: {:?}",
        app.media.error
    );
    let screen = render(&app);
    assert!(
        screen.contains("PREVIEW"),
        "media screen renders:\n{screen}"
    );
    assert!(
        screen.contains("graphics:"),
        "the graphics mode + reason are stated:\n{screen}"
    );
    if app.media.mode == torogao_tui::media::GraphicsMode::HalfBlock {
        assert!(
            screen.contains('▀'),
            "half-block pixels render in the buffer:\n{screen}"
        );
    }

    if have_ffmpeg {
        // Step onto the video: probed, framed, and steppable.
        ch(&mut app, 'j');
        let video = app.media.video.as_ref().expect("video loaded");
        assert!(video.info.frames >= 4, "probe counted frames");
        assert_eq!(video.frame, 0);
        key(&mut app, KeyCode::Right);
        let video = app.media.video.as_ref().expect("video still loaded");
        assert_eq!(video.frame, 1, "Right steps exactly one frame");
        key(&mut app, KeyCode::Left);
        let video = app.media.video.as_ref().expect("video still loaded");
        assert_eq!(video.frame, 0, "Left steps back");
        ch(&mut app, ' ');
        let video = app.media.video.as_ref().expect("video still loaded");
        assert!(video.playing, "space plays");
        ch(&mut app, ' ');
        let video = app.media.video.as_ref().expect("video still loaded");
        assert!(!video.playing, "space pauses");
        let screen = render(&app);
        assert!(
            screen.contains("visual only"),
            "the silence rule is stated on the transport:\n{screen}"
        );
        assert!(
            app.media.info[1].contains("no audio is decoded or played"),
            "the transport's second line carries the full silence statement: {:?}",
            app.media.info
        );
    }

    // The model: either a renderer produced a raster, or the
    // preview refused with a named TG-TUI-MEDIA code — never a
    // silent blank.
    let target = if have_ffmpeg { 2 } else { 1 };
    while app.media.sel < target {
        ch(&mut app, 'j');
    }
    match &app.media.error {
        Some(line) => assert!(
            line.starts_with("TG-TUI-MEDIA-"),
            "model refusal is named: {line}"
        ),
        None => assert!(app.media.raster.is_some(), "model rendered a raster"),
    }
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.screen, Screen::Browse);
}

#[test]
fn media_screen_opens_a_path_without_a_project() {
    let root = fresh_root("mediapath");
    let stray = root.join("stray.png");
    write_test_png(&stray, 4, 4, [80, 200, 120, 255]);
    let mut app = App::new(root);
    ch(&mut app, '8');
    assert_eq!(app.screen, Screen::Media);
    ch(&mut app, 'o');
    assert!(app.prompt.is_some(), "path prompt opens");
    type_text(&mut app, &stray.display().to_string());
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.media.items.len(), 1);
    assert!(
        app.media.raster.is_some(),
        "the prompted still previews: {:?}",
        app.media.error
    );
}
