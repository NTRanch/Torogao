//! modapp-smoke: a deterministic end-to-end day-loop run of
//! the Modification App systems on the measured tables —
//! open the hub, watch the menu gate by flag and time, play
//! two paths to the store unlock, spend the catch-up, and
//! quick-travel. Prints the outcome; exit 0 on a completed
//! run, 1 on refusal or a broken expectation. This is the
//! skeleton's Linux-native proof artifact, not a game.

use torogao_modapp::{Game, Menu, measured_dynamics};

fn main() {
    let code = run();
    std::process::exit(code);
}

fn fail(message: &str) -> i32 {
    eprintln!("modapp-smoke: {message}");
    1
}

fn run() -> i32 {
    let dynamics = match measured_dynamics() {
        Ok(d) => d,
        Err(e) => return fail(&e.to_string()),
    };
    let mut game = Game::new(dynamics);

    // The hub at Default on day 0, slot 0: the flag-gated
    // entry node is excluded, the Afternoon-gated node is
    // shown disabled, the Morning node is enabled.
    let menu = match game.menu() {
        Ok(m) => m,
        Err(e) => return fail(&e.to_string()),
    };
    let start_options = match &menu {
        Menu::Options(options) => options.clone(),
        other => return fail(&format!("expected options at Default, got {other:?}")),
    };
    if start_options.len() != 2
        || start_options.iter().any(|o| o.node_id == "AssExpansion-1")
        || !start_options
            .iter()
            .any(|o| o.node_id == "BreastExpansion-1" && o.enabled)
        || !start_options
            .iter()
            .any(|o| o.node_id == "MindControl-1" && o.wrong_time)
    {
        return fail("Default menu gating does not match the measured rules");
    }

    // Path one: four Morning nodes in the Kitchen. Each
    // completion rolls to the next day's Morning; the TP
    // economy is still gated off, so wins grant nothing.
    for node in [
        "BreastExpansion-1",
        "BreastExpansion-2",
        "BreastExpansion-3",
        "BreastExpansion-4",
    ] {
        match game.play_node(node, true, 1.0) {
            Ok(outcome) if outcome.completed && outcome.reward_granted == 0 => {}
            Ok(outcome) => {
                return fail(&format!("unexpected outcome at {node}: {outcome:?}"));
            }
            Err(e) => return fail(&e.to_string()),
        }
    }
    if game.day() != 4 || game.current_node() != "BreastExpansion-4" || game.points() != 0 {
        return fail("state after the first path does not match the measured loop");
    }
    let state = match game.current_state() {
        Ok(s) => s,
        Err(e) => return fail(&e.to_string()),
    };
    if state.body != "BE3" {
        return fail("BreastExpansion-4 must resolve to body BE3");
    }

    // Path two: four Afternoon nodes. The subject's slot-1
    // room is the LivingRoom; wait one slot and move there.
    // The fourth completion is the second level-4 event, so
    // the store unlocks and pays the retroactive catch-up:
    // 9 seen nodes * 105 TP.
    if game.set_room(4).is_err() {
        return fail("set_room refused a known room");
    }
    for node in ["Bimbo-1", "Bimbo-2", "Bimbo-3", "Bimbo-4"] {
        game.wait();
        match game.play_node(node, true, 1.0) {
            Ok(outcome) if outcome.completed => {}
            Ok(outcome) => {
                return fail(&format!("unexpected outcome at {node}: {outcome:?}"));
            }
            Err(e) => return fail(&e.to_string()),
        }
    }
    if !game.store_unlocked() || game.points() != 945 {
        return fail("store unlock catch-up does not match the measured economy");
    }

    // Spend: the console (150) and the quick-travel item
    // (500) come to 295 TP left.
    for item in ["console", "path-tp"] {
        if let Err(e) = game.buy(item) {
            return fail(&e.to_string());
        }
    }
    if game.points() != 295 {
        return fail("balance after purchases does not match the measured prices");
    }

    // Quick travel back across the DAG to Growth-1: the
    // route walks from Bimbo-4 back to BreastExpansion-1 and
    // forward to Growth-1 — 9 nodes at 20 TP = 180 TP —
    // and the goal state is applied directly (body back to
    // DEFAULT, position delta only).
    let trip = match game.teleport("Growth-1") {
        Ok(t) => t,
        Err(e) => return fail(&e.to_string()),
    };
    if trip.route.len() != 9 || trip.price != 180 || trip.state.body != "DEFAULT" {
        return fail("teleport outcome does not match the measured route and state");
    }

    println!(
        "modapp-smoke: success=true day={} time={} points={} current={} body={} store={} teleport_route_len={} teleport_price={} seen_nodes={}",
        game.day(),
        game.time(),
        game.points(),
        game.current_node(),
        trip.state.body,
        game.store_unlocked(),
        trip.route.len(),
        trip.price,
        game.seen_nodes().len()
    );
    0
}
