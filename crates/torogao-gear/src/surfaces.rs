//! The gear surfaces (§3), as headless view-models.
//!
//! The estate's testable-surface pattern: the Creator gear
//! editor and the Playroom gear panel are state + behaviour
//! objects whose rendering is data (refusal panels render the
//! refusal *verbatim*, the dashboard renders the state it
//! reads), so every surface behaviour is gated headlessly.
//! The egui registration of these view-models in the Persona
//! is the composition step when the T4.1/T4.2 branches merge;
//! nothing here invents engine internals — the dashboard binds
//! to the §5.2 interface ([`crate::engine_iface`]) with the
//! labelled stub behind it.
//!
//! THE REMOVE-ALL CONTROL (§3.4) is part of this module's
//! contract: it is always present, never disabled by gear
//! state, engine phase, culmination state, mirror state, or a
//! modal surface, and never behind a confirmation dialog.

use crate::anchors::{ContentStamps, RigTemplate};
use crate::document::GearDocument;
use crate::engine_iface::{ArousalEngineView, ArousalStateVector, StubEngine};
use crate::payloads::{MemoryPayloads, PayloadLookup};
use crate::refusal::GearResult;
use crate::solve::{MotionTrack, SolveTrace, anchor_position, run_solve};
use crate::validate::{ValidationContext, validate_document};
use crate::worn::{OpEvent, WornSet};

/// The standing control's exact label (§3.4).
pub const REMOVE_ALL_LABEL: &str = "Remove All Gear & Reset State";

/// One anchor's deterministic-preview readout (§2.9 step 3):
/// the anchor site, the solve over a repertoire motion track,
/// and the attachment's current preview variant.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewAnchor {
    /// Canonical anchor name.
    pub anchor: String,
    /// Anchor site x on the 1024 canvas.
    pub site_x_px: f64,
    /// Anchor site y.
    pub site_y_px: f64,
    /// Contact radius of the site.
    pub site_radius_px: f64,
    /// The solve trace over the preview track.
    pub trace: SolveTrace,
    /// The preview variant currently stepped to.
    pub variant: String,
}

/// The deterministic preview of one document (§2.9 step 3):
/// not a static mock-up — the item is solved on the selected
/// rig through repertoire-class motion.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewReport {
    /// Document id previewed.
    pub id: String,
    /// Rig character previewed on.
    pub character: String,
    /// Motion track used.
    pub track: MotionTrack,
    /// Per-anchor readouts, in document order.
    pub anchors: Vec<PreviewAnchor>,
    /// Worst post-solve residual across anchors, px.
    pub max_residual_px: f64,
    /// Worst measured lag across anchors, steps.
    pub max_lag_steps: u32,
}

/// The Creator gear editor (§§2.9 steps 1–3, 3.1): file view,
/// validator panel, deterministic preview, plus the pack +
/// install actions (delegated to [`crate::pack`]).
#[derive(Debug)]
pub struct CreatorGearEditor {
    document_path: String,
    text: String,
    rig: RigTemplate,
    payloads: MemoryPayloads,
    materials: Vec<String>,
    pack_fixture_ids: Vec<String>,
    variant_step: usize,
}

impl CreatorGearEditor {
    /// An editor over one document text, one rig, and a
    /// payload store.
    pub fn new(
        document_path: &str,
        text: String,
        rig: RigTemplate,
        payloads: MemoryPayloads,
        materials: Vec<String>,
    ) -> Self {
        Self {
            document_path: document_path.to_string(),
            text,
            rig,
            payloads,
            materials,
            pack_fixture_ids: Vec::new(),
            variant_step: 0,
        }
    }

    /// Sets the pack-companion fixture ids this document may
    /// resolve `fixture.<id>` anchors against (§2.4).
    pub fn set_pack_fixture_ids(&mut self, ids: Vec<String>) {
        self.pack_fixture_ids = ids;
    }

    /// Replaces the file view's text (the file is the
    /// project, §2.9 step 1).
    pub fn set_text(&mut self, text: String) {
        self.text = text;
    }

    /// The current file-view text.
    pub fn text(&self) -> &str {
        &self.text
    }

    fn context(&self) -> ValidationContext<'_> {
        ValidationContext {
            document_path: self.document_path.clone(),
            rig: self.rig.clone(),
            payloads: &self.payloads,
            materials: &self.materials,
            pack_fixture_ids: &self.pack_fixture_ids,
            stamps: self.rig.stamps,
        }
    }

    /// Runs the §2.7 validator over the current text — the
    /// same code path the corpus gates consume.
    pub fn validate(&self) -> GearResult<GearDocument> {
        validate_document(&self.text, &self.context())
    }

    /// The validator panel's content: the refusal rendered
    /// verbatim, or `None` when the document validates.
    pub fn refusal_panel(&self) -> Option<String> {
        self.validate().err().map(|r| r.verbatim())
    }

    /// The deterministic preview (§2.9 step 3): solves the
    /// validated document on the editor's rig over the
    /// Greeting track, with anchor/contact readouts and the
    /// currently stepped state variant per anchor.
    pub fn preview(&self, ticks: u64) -> GearResult<PreviewReport> {
        let doc = self.validate()?;
        let mut anchors = Vec::new();
        let mut max_residual_px = 0.0_f64;
        let mut max_lag_steps = 0_u32;
        for (i, attachment) in doc.anchors.iter().enumerate() {
            let Some(site) = self.rig.resolve(&attachment.anchor) else {
                continue; // validation already refused this
            };
            let threshold = if doc.physics.displaceable {
                doc.physics.displacement_threshold_px as f64
            } else {
                f64::INFINITY
            };
            let trace = run_solve(
                attachment,
                site.x_px + attachment.offset_x_px,
                site.y_px + attachment.offset_y_px,
                MotionTrack::Greeting,
                ticks,
                threshold,
            );
            max_residual_px = max_residual_px.max(trace.max_residual_px);
            max_lag_steps = max_lag_steps.max(trace.lag_steps);
            let variant = doc
                .state_variants
                .get((self.variant_step + i) % doc.state_variants.len().max(1))
                .map(|v| v.name().to_string())
                .unwrap_or_default();
            anchors.push(PreviewAnchor {
                anchor: attachment.anchor.clone(),
                site_x_px: site.x_px,
                site_y_px: site.y_px,
                site_radius_px: site.contact_radius_px,
                trace,
                variant,
            });
        }
        Ok(PreviewReport {
            id: doc.id.clone(),
            character: self.rig.character.clone(),
            track: MotionTrack::Greeting,
            anchors,
            max_residual_px,
            max_lag_steps,
        })
    }

    /// Steps the preview's state variants (§2.9 step 3: each
    /// state variant is shown).
    pub fn step_variant(&mut self) {
        self.variant_step += 1;
    }
}

/// The dashboard's data (§3.2): read from the engine through
/// the §5.2 interface, never estimated.
#[derive(Clone, Debug, PartialEq)]
pub struct DashboardView {
    /// The engine's §5.2 state vector.
    pub state: ArousalStateVector,
    /// Floor contributions per worn item (id, permille).
    pub gear_floor_contributions: Vec<(String, i64)>,
    /// The worn set's computed floor (max rule, §5.4).
    pub gear_floor_permille: i64,
    /// Per-device effective input from the engine view.
    pub per_device_effective_input: Vec<(String, u32)>,
    /// Items currently worn.
    pub worn_ids: Vec<String>,
}

impl DashboardView {
    /// Renders the dashboard as text lines (the monitoring-
    /// screen archetype of Addition C, §3.2).
    pub fn render(&self) -> String {
        let s = &self.state;
        let mut out = String::new();
        out.push_str(&format!("phase {}\n", s.phase.name()));
        out.push_str(&format!("arousal {}\n", s.arousal));
        out.push_str(&format!("pleasure_charge {}\n", s.pleasure_charge));
        out.push_str(&format!("sensitivity {}\n", s.sensitivity));
        out.push_str(&format!(
            "floor {} (gear {})\n",
            s.floor, self.gear_floor_permille
        ));
        out.push_str(&format!("peak_input {}\n", s.peak_input));
        out.push_str(&format!("dose {}\n", s.dose));
        out.push_str(&format!(
            "culmination_latch {} episode {}\n",
            s.culmination_latch, s.episode_id
        ));
        for (id, input) in &self.per_device_effective_input {
            out.push_str(&format!("input {id} {input}\n"));
        }
        for (id, floor) in &self.gear_floor_contributions {
            out.push_str(&format!("floor-contribution {id} {floor}\n"));
        }
        out.push_str(&format!("worn {}\n", self.worn_ids.join(",")));
        out
    }
}

/// The Playroom gear panel (§3.2): gear library + worn-set
/// panel + device/dash readouts + the standing Remove All
/// control. The engine behind the dashboard is the §5.2
/// interface; in T4.1 that is the labelled [`StubEngine`].
pub struct PlayroomGearPanel {
    library: Vec<GearDocument>,
    worn: WornSet,
    engine: Box<dyn ArousalEngineView>,
    tick: u64,
}

impl PlayroomGearPanel {
    /// A panel over a validated library, with the stub engine
    /// behind the dashboard (STUB — T4.2 replaces it).
    pub fn new(library: Vec<GearDocument>) -> Self {
        Self {
            library,
            worn: WornSet::new(),
            engine: Box::new(StubEngine::new()),
            tick: 0,
        }
    }

    /// A panel with a caller-provided engine view (the
    /// composition seam for T4.2's real engine).
    pub fn with_engine(library: Vec<GearDocument>, engine: Box<dyn ArousalEngineView>) -> Self {
        Self {
            library,
            worn: WornSet::new(),
            engine,
            tick: 0,
        }
    }

    /// Sets the panel's session tick (op-log timestamps).
    pub fn set_tick(&mut self, tick: u64) {
        self.tick = tick;
    }

    /// Library item ids, sorted.
    pub fn library_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.library.iter().map(|d| d.id.clone()).collect();
        ids.sort();
        ids
    }

    /// Applies a library item to the body (a session
    /// operation; the op log records it).
    pub fn apply(&mut self, id: &str) -> bool {
        let Some(doc) = self.library.iter().find(|d| d.id == id).cloned() else {
            return false;
        };
        self.worn.apply(doc, self.tick);
        true
    }

    /// Removes one worn item.
    pub fn remove(&mut self, id: &str) -> bool {
        self.worn.remove(id, self.tick)
    }

    /// Removes one side of a paired item (§2.3 B).
    pub fn remove_side(&mut self, id: &str, anchor: &str) -> bool {
        self.worn.remove_side(id, anchor, self.tick)
    }

    /// Re-seats a displaced item (user operation, §4.5).
    pub fn reseat(&mut self, id: &str) -> bool {
        self.worn.reseat(id, self.tick)
    }

    /// The worn set (read access for surfaces and gates).
    pub fn worn(&self) -> &WornSet {
        &self.worn
    }

    /// The dashboard's current data, read live from the
    /// engine view + the worn set.
    pub fn dashboard(&self) -> DashboardView {
        let mut contributions: Vec<(String, i64)> = Vec::new();
        for id in self.worn.ids() {
            if let Some(entry) = self.worn.get(&id)
                && entry.fully_seated()
                && let Some(floor) = entry.document.arousal_floor_permille()
            {
                contributions.push((id, floor));
            }
        }
        DashboardView {
            state: self.engine.state_vector(),
            gear_floor_contributions: contributions,
            gear_floor_permille: self.worn.arousal_floor_permille(),
            per_device_effective_input: self.engine.per_device_effective_input(),
            worn_ids: self.worn.ids(),
        }
    }

    /// Whether the Remove All control is available. It is
    /// ALWAYS available (§3.4): never disabled by gear state,
    /// engine phase, culmination state, mirror state, or a
    /// modal surface, and never behind a confirmation.
    pub fn remove_all_available(&self) -> bool {
        true
    }

    /// THE STANDING CONTROL (§3.4): "Remove All Gear & Reset
    /// State". Its act, in the spec's exact order:
    /// (1) latch local stimulation to zero, synchronously,
    /// before any bookkeeping; (2) issue the ache stop if a
    /// mirror session exists; (3) remove every worn item;
    /// (4) reset the arousal engine to baseline; (5) append
    /// the completed act to the session op log.
    pub fn remove_all_and_reset(&mut self) -> Vec<OpEvent> {
        let tick = self.tick;
        let mut events = Vec::new();
        // Each act event is recorded into the session op log
        // as its step happens, so the log itself carries the
        // §3.4 order (latch, stop, removals, reset, completed).
        // (1) Latch first, in the calling path.
        self.engine.latch_stimulation_zero();
        let latch = OpEvent::StimulationLatchedZero { tick };
        self.worn.record(latch.clone());
        events.push(latch);
        // (2) Mirror stop only when a session exists.
        if self.engine.mirror_active() {
            let stop = OpEvent::MirrorStopIssued { tick };
            self.worn.record(stop.clone());
            events.push(stop);
        }
        // (3) Remove every worn item (records its own events).
        self.worn.remove_all_items(tick);
        // (4) Reset the engine.
        self.engine.reset_to_baseline();
        let reset = OpEvent::EngineReset { tick };
        self.worn.record(reset.clone());
        events.push(reset);
        // (5) Append the completed act.
        let completed = OpEvent::RemoveAllCompleted { tick };
        self.worn.record(completed.clone());
        events.push(completed);
        events
    }
}

/// Helper for tests and previews: a validation context over a
/// rig with the rig's own stamps.
pub fn context_for<'a>(
    document_path: &str,
    rig: &RigTemplate,
    payloads: &'a dyn PayloadLookup,
    materials: &'a [String],
    pack_fixture_ids: &'a [String],
) -> ValidationContext<'a> {
    ValidationContext {
        document_path: document_path.to_string(),
        rig: rig.clone(),
        payloads,
        materials,
        pack_fixture_ids,
        stamps: ContentStamps::GOVERNED,
    }
}

/// The anchor position helper re-exported for preview
/// consumers that animate outside the solve.
pub fn preview_anchor_position(
    track: MotionTrack,
    site_x_px: f64,
    site_y_px: f64,
    tick: u64,
) -> (f64, f64) {
    anchor_position(track, site_x_px, site_y_px, tick)
}

impl std::fmt::Debug for PlayroomGearPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayroomGearPanel")
            .field("library_ids", &self.library_ids())
            .field("worn_ids", &self.worn.ids())
            .field("tick", &self.tick)
            .finish_non_exhaustive()
    }
}
