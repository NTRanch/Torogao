//! The persona audit, extended to the phone surface
//! (BRIEF-A1 §Scope 3–4, gate B5).
//!
//! The desktop audit (`torogao_app::persona::audit`) runs
//! over the Persona *model* — areas, commands, accents,
//! parity — and the phone presents that same model, so its
//! lines are re-run here verbatim (extended, not forked:
//! PROGRAM §3.4). What this module adds is the phone half:
//! the §3.3 reflow rules checked against the layout model,
//! the area-claim honesty table (Settings claimed, the
//! other eleven explicitly not), and the Settings
//! enumeration the walk operates.
//!
//! Operability itself is proven by the on-device walk
//! (gate B5's screen recording); this audit is the
//! structural half — it proves the surface the walk walks
//! is the surface the rules describe.

use serde::Serialize;
use torogao_app::persona::{self, Area, CommandKind, StageContext};

use crate::layout::{
    COMMAND_ROW_HEIGHT_DP, RAIL_HEIGHT_DP, REFERENCE_VIEWPORTS, STAGE_CHROME_DP,
    STAGE_COLLAPSED_HEIGHT_DP, STAGE_IMAGE_MIN_DP, STAGE_PANEL_MARGIN_Y_DP, STAGE_ROW_GAP_DP,
    STAGE_TITLE_ROW_DP, STANDING_BAR_HEIGHT_DP, TOUCH_TARGET_MIN_DP, frame_layout,
};

/// The areas this stage claims operable on phone. Every
/// other area renders its frame only. The Settings walk
/// (gate B5) is the evidence for this one claim; A-2 takes
/// up the rest, area by area.
pub const CLAIMED_OPERABLE: [Area; 1] = [Area::Settings];

/// One phone audit line (the desktop `AuditLine` shape).
#[derive(Debug, Clone, Serialize)]
pub struct PhoneAuditLine {
    /// What was checked.
    pub check: String,
    /// Whether it holds.
    pub pass: bool,
    /// The measured evidence.
    pub evidence: String,
}

fn line(check: &str, pass: bool, evidence: String) -> PhoneAuditLine {
    PhoneAuditLine {
        check: check.to_string(),
        pass,
        evidence,
    }
}

/// The Phase F area order, stable ids (PROGRAM §1: the
/// twelve Persona areas in order).
const AREA_IDS_IN_ORDER: [&str; 12] = [
    "playroom",
    "creator",
    "scenes",
    "effects",
    "audio",
    "writer",
    "video",
    "tracking",
    "toolbox",
    "convert",
    "tutorials",
    "settings",
];

/// Runs the full phone audit: the desktop model's audit
/// re-run, then the phone-surface checks.
pub fn phone_audit() -> Vec<PhoneAuditLine> {
    let mut lines = Vec::new();
    // The desktop model audit, re-run verbatim: the phone
    // presents this model, so its rules bind the phone.
    let desktop = persona::audit();
    let desktop_passed = desktop.iter().filter(|entry| entry.pass).count();
    lines.push(line(
        "desktop persona audit (re-run)",
        desktop_passed == desktop.len(),
        format!("{desktop_passed}/{} lines pass", desktop.len()),
    ));
    lines.push(audit_area_order());
    lines.push(audit_settings_enumeration());
    lines.push(audit_claim_honesty());
    lines.push(audit_touch_floor());
    lines.push(audit_stage_header_budget());
    lines.push(audit_standing_controls());
    lines.push(audit_one_column());
    lines.push(audit_pinned_context());
    lines.push(audit_fixture_present());
    lines
}

/// The twelve areas exist in the Phase F order — the
/// rail the frame draws is the census's rail.
fn audit_area_order() -> PhoneAuditLine {
    let ids: Vec<&str> = Area::ALL.iter().map(|area| area.id()).collect();
    line(
        "areas enumerated (Phase F order)",
        ids == AREA_IDS_IN_ORDER,
        format!("{ids:?}"),
    )
}

/// Settings' enumeration is exactly its three native
/// commands (no registry tool homes to the meta register):
/// the toggle, save, load the walk operates.
fn audit_settings_enumeration() -> PhoneAuditLine {
    let commands = persona::commands(Area::Settings);
    let ids: Vec<&str> = commands.iter().map(|command| command.id.as_str()).collect();
    let all_native = commands
        .iter()
        .all(|command| matches!(command.kind, CommandKind::Native(_)));
    let expected = [
        "native:toggle-nerd-mode",
        "native:save-session-state",
        "native:load-session-state",
    ];
    line(
        "settings enumeration",
        all_native && ids == expected,
        format!("{ids:?}"),
    )
}

/// The claim table is honest: exactly Settings is claimed
/// operable; the other eleven areas are named not-claimed.
fn audit_claim_honesty() -> PhoneAuditLine {
    let not_claimed: Vec<&str> = Area::ALL
        .iter()
        .filter(|area| !CLAIMED_OPERABLE.contains(area))
        .map(|area| area.id())
        .collect();
    line(
        "area claims (A-1)",
        CLAIMED_OPERABLE == [Area::Settings] && not_claimed.len() == 11,
        format!(
            "claimed: {:?}; not claimed: {:?}",
            CLAIMED_OPERABLE.map(|area| area.id()),
            not_claimed
        ),
    )
}

/// §3.3 rule 3: the floor itself is Android's 48 dp, and
/// every interactive height the layout model sizes meets
/// it on every reference viewport.
fn audit_touch_floor() -> PhoneAuditLine {
    let mut holds = TOUCH_TARGET_MIN_DP >= 48.0
        && STANDING_BAR_HEIGHT_DP >= TOUCH_TARGET_MIN_DP
        && RAIL_HEIGHT_DP >= TOUCH_TARGET_MIN_DP
        && COMMAND_ROW_HEIGHT_DP >= TOUCH_TARGET_MIN_DP;
    for viewport in REFERENCE_VIEWPORTS {
        let layout = frame_layout(viewport, false);
        holds &= layout.standing_bar.height >= TOUCH_TARGET_MIN_DP;
        holds &= layout.rail.height >= TOUCH_TARGET_MIN_DP;
    }
    line(
        "touch target floor (§3.3 rule 3)",
        holds,
        format!(
            "floor {TOUCH_TARGET_MIN_DP} dp; standing {STANDING_BAR_HEIGHT_DP}, rail {RAIL_HEIGHT_DP}, row {COMMAND_ROW_HEIGHT_DP}"
        ),
    )
}

/// The stage header's row budget (the B1 defect's cure):
/// on every reference viewport the expanded stage fits
/// its full chrome plus the image floor, and the
/// collapsed stage is exactly its title + transport
/// budget — the header can never draw rows the panel
/// has not reserved.
fn audit_stage_header_budget() -> PhoneAuditLine {
    let mut holds = true;
    for viewport in REFERENCE_VIEWPORTS {
        let expanded = frame_layout(viewport, false);
        holds &= expanded.stage.height >= STAGE_CHROME_DP + STAGE_IMAGE_MIN_DP;
        let collapsed = frame_layout(viewport, true);
        holds &= collapsed.stage.height == STAGE_COLLAPSED_HEIGHT_DP;
    }
    holds &= STAGE_COLLAPSED_HEIGHT_DP
        == STAGE_PANEL_MARGIN_Y_DP + STAGE_TITLE_ROW_DP + STAGE_ROW_GAP_DP + COMMAND_ROW_HEIGHT_DP;
    line(
        "stage header budget (B1 cure)",
        holds,
        format!(
            "chrome {STAGE_CHROME_DP} dp + image floor {STAGE_IMAGE_MIN_DP} dp fit every expanded stage; collapsed stage == {STAGE_COLLAPSED_HEIGHT_DP} dp (title + transport) on 3 viewports"
        ),
    )
}

/// §3.3 rule 5: the standing bar is pinned to the bottom
/// edge and the content column excludes it, on every
/// reference viewport, stage collapsed or not.
fn audit_standing_controls() -> PhoneAuditLine {
    let mut holds = true;
    for viewport in REFERENCE_VIEWPORTS {
        for collapsed in [false, true] {
            let layout = frame_layout(viewport, collapsed);
            holds &= layout.standing_bar.bottom() == viewport.height;
            holds &= !layout.content.overlaps(&layout.standing_bar);
            holds &= !layout.content.overlaps(&layout.rail);
        }
    }
    line(
        "standing controls pinned (§3.3 rule 5)",
        holds,
        "standing bar at the bottom edge; content region excludes bar and rail on 3 viewports × 2 stage states".to_string(),
    )
}

/// §3.3 rule 2: one panel column — the content region is
/// the full viewport width under a top-pinned stage.
fn audit_one_column() -> PhoneAuditLine {
    let mut holds = true;
    for viewport in REFERENCE_VIEWPORTS {
        let layout = frame_layout(viewport, false);
        holds &= layout.content.width == viewport.width;
        holds &= layout.stage.y == 0.0;
        holds &= layout.content.height > 0.0;
    }
    line(
        "one panel column, portrait (§3.3 rules 1–2)",
        holds,
        "content width == viewport width; stage pinned at y=0; positive content height on 3 portrait viewports".to_string(),
    )
}

/// The pinned comparison context (persona rule 5) is
/// identical in every area's stage view — the phone frame
/// shows the same context block the desktop does.
fn audit_pinned_context() -> PhoneAuditLine {
    let context = StageContext::default();
    let holds = Area::ALL
        .iter()
        .all(|area| persona::stage_view(*area, &context).pinned == context);
    line(
        "pinned context in every area (rule 5)",
        holds,
        "stage_view pinned context identical across all 12 areas".to_string(),
    )
}

/// The embedded fixture carries the repertoire the shell
/// presents (Prima, Greeting + Daydream).
fn audit_fixture_present() -> PhoneAuditLine {
    match torogao_app::scene::Scene::load_bytes(
        crate::fixture::PRIMA_REPERTOIRE_BYTES,
        crate::fixture::EMBEDDED_MODEL_LABEL,
    ) {
        Ok(scene) => {
            let names: Vec<&str> = scene
                .puppet
                .animations
                .iter()
                .map(|(name, _)| name.as_str())
                .collect();
            line(
                "embedded repertoire fixture",
                names.contains(&"Greeting") && names.contains(&"Daydream"),
                format!("animations: {names:?}"),
            )
        }
        Err(err) => line("embedded repertoire fixture", false, format!("{err}")),
    }
}

/// The audit as one JSON document (the shell writes it as
/// boot evidence; the gate reads it back).
pub fn phone_audit_json() -> serde_json::Value {
    let lines = phone_audit();
    let passed = lines.iter().filter(|entry| entry.pass).count();
    serde_json::json!({
        "kind": "torogao-android-phone-audit/1",
        "checks_passed": passed,
        "checks_total": lines.len(),
        "lines": lines,
    })
}

#[cfg(test)]
mod tests {
    use super::phone_audit;

    /// Every phone audit line passes on the model as
    /// built — the surface the walk walks is the surface
    /// the rules describe.
    #[test]
    fn phone_audit_is_fully_green() {
        let lines = phone_audit();
        assert!(!lines.is_empty());
        for entry in &lines {
            assert!(entry.pass, "{} failed: {}", entry.check, entry.evidence);
        }
    }
}
