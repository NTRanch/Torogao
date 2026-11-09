//! Gate tests for torogao-modapp. Half validation (the
//! measured behavior), half adversarial (corrupt tables,
//! hostile plays, economy attacks), after the estate's
//! 50/50 test split. Every expected value comes from the
//! measured spec (games/modification-app-dynamics.md).

use torogao_modapp::{Dynamics, Game, Menu, ModappError, measured_dynamics, world};

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn new_game() -> Game {
    Game::new(measured_dynamics().expect("measured table loads"))
}

/// Play a full path of Morning nodes from the Kitchen.
fn play_morning_path(game: &mut Game, nodes: &[&str]) {
    for node in nodes {
        let outcome = game
            .play_node(node, true, 1.0)
            .expect("measured path plays");
        assert!(outcome.completed, "{node} completes");
    }
}

/// Reach the store unlock: BE path then Bimbo path, as in
/// the smoke run. Ends on day 8 with 945 TP.
fn unlocked_game() -> Game {
    let mut game = new_game();
    play_morning_path(
        &mut game,
        &[
            "BreastExpansion-1",
            "BreastExpansion-2",
            "BreastExpansion-3",
            "BreastExpansion-4",
        ],
    );
    game.set_room(4).expect("known room");
    for node in ["Bimbo-1", "Bimbo-2", "Bimbo-3", "Bimbo-4"] {
        game.wait();
        let outcome = game.play_node(node, true, 1.0).expect("bimbo path plays");
        assert!(outcome.completed, "{node} completes");
    }
    assert!(game.store_unlocked());
    game
}

// ---------- validation ----------

#[test]
fn measured_table_matches_the_spec_counts() {
    let d = measured_dynamics().expect("measured table loads");
    assert_eq!(d.node_count(), 39);
    assert_eq!(d.item_count(), 4);
    assert_eq!(d.slots_per_day(), 4);
    assert_eq!(d.teleport_difficulty(), 800);
    assert_eq!(d.teleport_price_per_node(), 20);
    assert_eq!(d.new_event_points(), 100);
    assert_eq!(d.store_catchup_per_seen_node(), 105);
    assert_eq!(d.schedule(), &[5, 4, 2, 1]);
    assert_eq!(
        d.flag_event("ASS_UNLOCKED").expect("flag exists"),
        "AssExpansion-1-enter"
    );
    let be1 = d.node("BreastExpansion-1").expect("node exists");
    assert_eq!(
        be1.children,
        vec![
            "BreastExpansion-2".to_string(),
            "Growth-1".to_string(),
            "Shrink-1".to_string()
        ]
    );
    let mc3 = d.node("MindControl-3").expect("node exists");
    assert_eq!(
        mc3.children,
        vec!["Goth-1".to_string(), "MindControl-4".to_string()]
    );
}

#[test]
fn state_resolver_merges_default_and_delta() {
    let d = measured_dynamics().expect("measured table loads");
    // MindControl rows are empty deltas: pure Default.
    let mc = world::resolve_state(&d, "MindControl-2").expect("state resolves");
    assert_eq!(mc.body, "DEFAULT");
    assert_eq!(mc.face, "DEFAULT;DEFAULT;SMILE");
    assert!(approx(mc.scale, 1.0));
    // Growth-1 moves position only; the body stays Default's.
    let g1 = world::resolve_state(&d, "Growth-1").expect("state resolves");
    assert_eq!(g1.body, "DEFAULT");
    assert!(approx(g1.position.1, -5.0));
    // Shrink-3 carries the full delta set including the
    // face placement.
    let s3 = world::resolve_state(&d, "Shrink-3").expect("state resolves");
    assert_eq!(s3.body, "SHRINK3");
    assert!(approx(s3.scale, 0.96));
    assert!(approx(s3.position.1, 94.874));
    assert!(approx(s3.face_position.rotation, -0.2));
    // Goth-2 overrides face and extras but not position.
    let goth = world::resolve_state(&d, "Goth-2").expect("state resolves");
    assert_eq!(goth.extras, "GOTH_BANGS");
    assert!(approx(goth.position.0, 0.0));
}

#[test]
fn clock_wraps_the_four_slot_day() {
    let mut game = new_game();
    assert_eq!((game.day(), game.time()), (0, 0));
    game.wait();
    game.wait();
    game.wait();
    assert_eq!((game.day(), game.time()), (0, 3));
    game.wait();
    assert_eq!((game.day(), game.time()), (1, 0));
}

#[test]
fn menu_gates_by_flag_time_and_presence() {
    let mut game = new_game();
    // Default at Morning in the Kitchen: AE-1 flag-hidden,
    // BE-1 enabled, MC-1 wrong-time.
    match game.menu().expect("menu resolves") {
        Menu::Options(options) => {
            assert_eq!(options.len(), 2);
            assert!(options.iter().all(|o| o.node_id != "AssExpansion-1"));
            assert!(
                options
                    .iter()
                    .any(|o| o.node_id == "BreastExpansion-1" && o.enabled)
            );
            assert!(
                options
                    .iter()
                    .any(|o| o.node_id == "MindControl-1" && o.wrong_time && !o.enabled)
            );
        }
        other => panic!("expected options, got {other:?}"),
    }
    // Wrong room: the menu is replaced by the absence notice.
    game.set_room(0).expect("known room");
    match game.menu().expect("menu resolves") {
        Menu::SubjectAbsent { subject_room } => assert_eq!(subject_room, 5),
        other => panic!("expected absence, got {other:?}"),
    }
    // A leaf node reports end of content before presence.
    let mut done = unlocked_game();
    done.set_room(0).expect("known room");
    match done.menu().expect("menu resolves") {
        Menu::EndOfContent => {}
        other => panic!("expected end of content, got {other:?}"),
    }
}

#[test]
fn node_completion_applies_state_and_rolls_the_day() {
    let mut game = new_game();
    let outcome = game
        .play_node("BreastExpansion-1", true, 0.5)
        .expect("node plays");
    assert!(outcome.completed);
    assert_eq!((outcome.day, outcome.time), (1, 0));
    let state = outcome.state.expect("state applied");
    assert_eq!(state.body, "BE1");
    assert!(game.seen_events().contains("BreastExpansion-1-enter"));
    assert!(game.seen_nodes().contains("BreastExpansion-1"));
    // A failed minigame changes nothing.
    let before = (game.day(), game.time(), game.current_node().to_string());
    let failed = game
        .play_node("BreastExpansion-2", false, 1.0)
        .expect("failure is a valid outcome");
    assert!(!failed.completed);
    assert_eq!(
        (game.day(), game.time(), game.current_node().to_string()),
        before
    );
}

#[test]
fn tp_economy_is_gated_until_the_store_unlock() {
    let mut game = new_game();
    // Before the unlock, AddPoints is a no-op (the gate).
    assert_eq!(game.add_points(500).expect("gate is not an error"), 0);
    assert_eq!(game.points(), 0);
    play_morning_path(
        &mut game,
        &[
            "BreastExpansion-1",
            "BreastExpansion-2",
            "BreastExpansion-3",
            "BreastExpansion-4",
        ],
    );
    // One level-4 event is not enough.
    assert!(!game.store_unlocked());
    assert_eq!(game.points(), 0);
    game.set_room(4).expect("known room");
    for node in ["Bimbo-1", "Bimbo-2", "Bimbo-3"] {
        game.wait();
        game.play_node(node, true, 1.0).expect("node plays");
    }
    game.wait();
    let outcome = game.play_node("Bimbo-4", true, 1.0).expect("node plays");
    // The unlock fires on the second level-4 event and pays
    // seen nodes (9) * 105.
    assert_eq!(outcome.unlock_catchup, 945);
    assert!(game.store_unlocked());
    assert_eq!(game.points(), 945);
    // After the unlock, wins pay: Growth-5's row is 75 + 25.
    assert_eq!(game.add_points(25).expect("gate open"), 25);
    assert_eq!(game.points(), 970);
}

#[test]
fn rewards_follow_the_measured_formulas() {
    let d = measured_dynamics().expect("measured table loads");
    let ae = d.reward("AE").expect("reward row");
    assert_eq!((ae.base, ae.span), (50, 50));
    let growth = d.reward("Growth").expect("reward row");
    assert_eq!((growth.base, growth.span), (75, 25));
    let mc = d.reward("MindControl").expect("reward row");
    assert_eq!((mc.base, mc.span), (100, 0));
    let vampire = d.reward("Vampire").expect("reward row");
    assert_eq!((vampire.base, vampire.span), (66, 0));
    // Setup pays nothing: refused, not zeroed.
    assert!(matches!(
        d.reward("Setup"),
        Err(ModappError::UnknownMinigame { .. })
    ));
    // Tuning merges defaults under the level row.
    let bimbo4 = d.tuning("Bimbo", 4, false).expect("tuning row");
    assert!(approx(bimbo4["time_to_shoot"], 4.0));
    assert!(approx(bimbo4["rows"], 12.0));
    let ae_tp = d.tuning("AE", 0, true).expect("teleport row");
    assert!(approx(ae_tp["required"], 12.0));
}

#[test]
fn teleport_route_price_and_goal_state() {
    let mut game = unlocked_game();
    game.buy("console").expect("buy works");
    game.buy("path-tp").expect("buy works");
    assert_eq!(game.points(), 295);
    // From Bimbo-4 the route walks back up the played path
    // to BreastExpansion-1, then forward to Growth-1:
    // 9 nodes at 20 TP. The price gates affordability; the
    // 0.12.3 sources deduct nothing (spec REVIEW #2), so
    // the balance is unchanged by the trip.
    let price = game.teleport_price("Growth-1").expect("price computes");
    assert_eq!(price, 20 * 9);
    let trip = game.teleport("Growth-1").expect("teleport works");
    assert_eq!(trip.route.len(), 9);
    assert_eq!(trip.route[0], "Bimbo-4");
    assert_eq!(trip.route[8], "Growth-1");
    assert_eq!(trip.state.body, "DEFAULT");
    assert!(approx(trip.state.position.1, -5.0));
    assert_eq!(game.current_node(), "Growth-1");
    assert_eq!(game.points(), 295);
    // Teleport does not mark the node seen or save its event
    // (the source writes the fields directly).
    assert!(!game.seen_nodes().contains("Growth-1"));
    assert!(!game.seen_events().contains("Growth-1-enter"));
}

// ---------- adversarial ----------

#[test]
fn corrupt_tables_are_refused() {
    // Truncated / not RON at all.
    assert!(matches!(
        Dynamics::from_ron("(nodes: ["),
        Err(ModappError::Parse(_))
    ));
    assert!(matches!(
        Dynamics::from_ron("not a table"),
        Err(ModappError::Parse(_))
    ));
    let good = torogao_modapp::DYNAMICS_RON;
    // A child reference to a node that does not exist.
    let bad_child = good.replacen("children: [\"Default\"]", "children: [\"NoSuchNode\"]", 1);
    assert!(matches!(
        Dynamics::from_ron(&bad_child),
        Err(ModappError::Invalid { .. })
    ));
    // A time gate outside the day.
    let bad_gate = good.replacen(
        "\"Shrink-5\", path: \"Shrink\", time_gate: Some(1)",
        "\"Shrink-5\", path: \"Shrink\", time_gate: Some(9)",
        1,
    );
    assert!(matches!(
        Dynamics::from_ron(&bad_gate),
        Err(ModappError::Invalid { .. })
    ));
    // A required flag the flag table does not carry.
    let bad_flag = good.replacen(
        "req_flag: Some(\"ASS_UNLOCKED\")",
        "req_flag: Some(\"NOPE_UNLOCKED\")",
        1,
    );
    assert!(matches!(
        Dynamics::from_ron(&bad_flag),
        Err(ModappError::Invalid { .. })
    ));
    // An incomplete Default state row breaks the merge base.
    let bad_default = good.replacen(
        "(id: \"Default\", body: Some(\"DEFAULT\")",
        "(id: \"Default\", body: None",
        1,
    );
    assert!(matches!(
        Dynamics::from_ron(&bad_default),
        Err(ModappError::Invalid { .. })
    ));
}

#[test]
fn unknown_identifiers_are_refused() {
    let d = measured_dynamics().expect("measured table loads");
    assert!(matches!(
        d.node("Vampire-1"),
        Err(ModappError::UnknownNode { .. })
    ));
    assert!(matches!(
        d.flag_event("NOPE"),
        Err(ModappError::UnknownFlag { .. })
    ));
    assert!(matches!(
        d.item("rocket"),
        Err(ModappError::UnknownItem { .. })
    ));
    assert!(matches!(
        d.reward("Dance"),
        Err(ModappError::UnknownMinigame { .. })
    ));
    assert!(matches!(
        world::resolve_state(&d, "INTRO"),
        Err(ModappError::UnknownNode { .. })
    ));
    let mut game = new_game();
    assert!(matches!(
        game.play_node("Vampire-1", true, 1.0),
        Err(ModappError::UnknownNode { .. })
    ));
    assert!(matches!(
        game.buy("rocket"),
        Err(ModappError::UnknownItem { .. })
    ));
}

#[test]
fn hostile_plays_are_refused_without_state_change() {
    let mut game = new_game();
    let snapshot = (
        game.day(),
        game.time(),
        game.points(),
        game.current_node().to_string(),
    );
    // Not a child of the current node.
    assert!(matches!(
        game.play_node("Goth-3", true, 1.0),
        Err(ModappError::NotAChild { .. })
    ));
    // Flag-gated child: hidden means refused.
    assert!(matches!(
        game.play_node("AssExpansion-1", true, 1.0),
        Err(ModappError::FlagNotMet { .. })
    ));
    // Wrong slot.
    assert!(matches!(
        game.play_node("MindControl-1", true, 1.0),
        Err(ModappError::WrongTime { .. })
    ));
    // Subject absent.
    game.set_room(0).expect("known room");
    assert!(matches!(
        game.play_node("BreastExpansion-1", true, 1.0),
        Err(ModappError::SubjectAbsent { .. })
    ));
    game.set_room(5).expect("known room");
    // Non-finite performance ratio.
    assert!(matches!(
        game.play_node("BreastExpansion-1", true, f64::NAN),
        Err(ModappError::InvalidStep { .. })
    ));
    // Unknown room.
    assert!(matches!(
        game.set_room(99),
        Err(ModappError::InvalidStep { .. })
    ));
    assert_eq!(
        (
            game.day(),
            game.time(),
            game.points(),
            game.current_node().to_string()
        ),
        snapshot
    );
}

#[test]
fn economy_attacks_are_refused() {
    // Buying before the store unlock is refused outright.
    let mut locked = new_game();
    assert!(matches!(
        locked.buy("console"),
        Err(ModappError::StoreLocked)
    ));
    let mut game = unlocked_game();
    // Unique items cannot be bought twice.
    game.buy("console").expect("first buy works");
    assert!(matches!(
        game.buy("console"),
        Err(ModappError::AlreadyOwned { .. })
    ));
    // Insufficient balance: 945 - 150 - 500 = 295, the
    // bat-plush costs 600.
    game.buy("path-tp").expect("buy works");
    assert!(matches!(
        game.buy("bat-plush"),
        Err(ModappError::InsufficientPoints { .. })
    ));
    assert_eq!(game.points(), 295);
    // Non-unique items can repeat while funds last.
    game.buy("name-changer").expect("consumable buys");
    game.buy("name-changer").expect("consumable rebuys");
    assert_eq!(game.inventory_count("name-changer"), 2);
    assert_eq!(game.points(), 195);
}

#[test]
fn teleport_attacks_are_refused() {
    let mut game = unlocked_game();
    // No path-tp item yet.
    assert!(matches!(
        game.teleport("Growth-1"),
        Err(ModappError::TeleportItemMissing)
    ));
    // Unknown target (checked after the item gate in the
    // source's flow: own the item first, then refuse).
    game.buy("console").expect("buy works");
    game.buy("name-changer").expect("buy works");
    game.buy("path-tp").expect("buy works");
    assert_eq!(game.points(), 245);
    assert!(matches!(
        game.teleport("Vampire-1"),
        Err(ModappError::UnknownNode { .. })
    ));
    // Unaffordable target: the Bimbo-4 -> Shrink-5 route is
    // 13 nodes = 260 TP against a 245 TP balance. The trip
    // is refused and nothing changes.
    let before = (game.points(), game.current_node().to_string());
    assert!(matches!(
        game.teleport("Shrink-5"),
        Err(ModappError::InsufficientPoints { .. })
    ));
    assert_eq!((game.points(), game.current_node().to_string()), before);
}

#[test]
fn replay_cannot_farm_the_first_view_bonus() {
    let mut game = unlocked_game();
    game.buy("path-tp").expect("buy works");
    // Teleport back to Default and replay a seen node: the
    // minigame win pays again (50 + 50 * ratio), but the
    // first-view bonus does not (AddPointsNewEvent semantics).
    game.teleport("Default").expect("teleport works");
    game.set_room(5).expect("known room");
    let before = game.points();
    let outcome = game
        .play_node("BreastExpansion-1", true, 1.0)
        .expect("replay is allowed");
    assert!(outcome.completed);
    assert_eq!(outcome.new_event_bonus, 0);
    assert_eq!(outcome.reward_granted, 100);
    assert_eq!(game.points(), before + 100);
}

#[test]
fn points_overflow_is_refused() {
    let mut game = unlocked_game();
    assert!(matches!(
        game.add_points(i64::MAX),
        Err(ModappError::InvalidStep { .. })
    ));
    assert_eq!(game.points(), 945);
}

#[test]
fn grafted_tables_cannot_extend_the_graph() {
    let good = torogao_modapp::DYNAMICS_RON;
    // A tuning row for an unknown class is refused.
    let bad_tuning = good.replacen(
        "(class: \"Setup\", level: 0",
        "(class: \"Dance\", level: 0",
        1,
    );
    assert!(matches!(
        Dynamics::from_ron(&bad_tuning),
        Err(ModappError::Invalid { .. })
    ));
    // A state row for a node that does not exist is refused.
    let bad_state = good.replacen(
        "(id: \"MindControl-4\", body: None",
        "(id: \"Vampire-1\", body: None",
        1,
    );
    assert!(matches!(
        Dynamics::from_ron(&bad_state),
        Err(ModappError::Invalid { .. })
    ));
    // A duplicate node id is refused.
    let bad_dup = good.replacen(
        "(id: \"Goth-3\", path: \"Goth\"",
        "(id: \"Goth-2\", path: \"Goth\"",
        1,
    );
    assert!(matches!(
        Dynamics::from_ron(&bad_dup),
        Err(ModappError::Invalid { .. })
    ));
    // Tuning lookups past the measured levels refuse rather
    // than extrapolate: AE has no level 5.
    let d = measured_dynamics().expect("measured table loads");
    assert!(matches!(
        d.tuning("AE", 5, false),
        Err(ModappError::UnknownMinigame { .. })
    ));
}
