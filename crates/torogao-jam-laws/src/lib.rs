//! torogao-jam-laws — the validated laws of the Torogao Jam Program,
//! ported for torogao in its own language.
//!
//! Each module carries one game's validated technique filing (the
//! full prose filings live in docs/jam-port-backs/ in the torogao
//! tree). Everything here is a pure function or a small state machine
//! over plain data: the jam games proved these shapes in play, and
//! this crate is where they become torogao's own. Tiger Style
//! (tiger-style-rust): explicit contracts, bounded work, no hidden
//! behaviour. Content labels per §9: every item in this crate is
//! SFW-labelled technique.

// ---------------------------------------------------------------------------
// W1 Trancefall — F-code face performance (Zig filing)
// ---------------------------------------------------------------------------

pub mod trancefall {
    /// Transition timing between adjacent face states: descending the
    /// F-code ladder is slower than surfacing (measured keeper values
    /// from the W1 build).
    pub const STEP_DOWN_MS: u32 = 900;
    pub const STEP_UP_MS: u32 = 500;

    /// Total transition time between two face-state indices, moving
    /// one step at a time through the intermediates (the table is
    /// total: every pair has a defined path).
    pub fn transition_ms(from: u8, to: u8) -> u32 {
        let steps = from.abs_diff(to) as u32;
        if to > from {
            steps * STEP_DOWN_MS
        } else {
            steps * STEP_UP_MS
        }
    }
}

// ---------------------------------------------------------------------------
// W2 Idle Hands — the idle performer (Odin filing)
// ---------------------------------------------------------------------------

pub mod idle_hands {
    /// The companion idle as data (measured in the W2 build): breath
    /// loop, blink timing modulated by a state scalar, gaze springs.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct IdleParams {
        pub breath_period_ms: u32,
        pub blink_min_ms: u32,
        pub blink_max_ms: u32,
        pub blink_close_ms: u32,
        pub gaze_stiffness_per_s: f32,
        pub hair_stiffness_per_s: f32,
        pub sleeve_stiffness_per_s: f32,
        pub fidget_quiet_ms: u32,
        pub look_settle_ms: u32,
    }

    pub const IDLE_PARAMS: IdleParams = IdleParams {
        breath_period_ms: 3_800,
        blink_min_ms: 2_400,
        blink_max_ms: 5_200,
        blink_close_ms: 130,
        gaze_stiffness_per_s: 6.0,
        hair_stiffness_per_s: 2.6,
        sleeve_stiffness_per_s: 3.2,
        fidget_quiet_ms: 6_000,
        look_settle_ms: 900,
    };

    /// Blink interval scaled by a 0.0..=1.0 state scalar (loneliness
    /// in W2): the sadder the actor, the slower the blink.
    pub fn blink_interval_ms(p: &IdleParams, scalar: f32, pick: f32) -> u32 {
        let base =
            p.blink_min_ms as f32 + pick.clamp(0.0, 1.0) * (p.blink_max_ms - p.blink_min_ms) as f32;
        (base * (1.0 + scalar.clamp(0.0, 1.0))) as u32
    }

    /// One exponential-chase step for a secondary-motion channel.
    /// stiffness is per-second; dt_ms is the frame step.
    pub fn chase(current: f32, target: f32, stiffness_per_s: f32, dt_ms: u32) -> f32 {
        let k = 1.0 - (-stiffness_per_s * (dt_ms as f32 / 1000.0)).exp();
        current + (target - current) * k
    }
}

// ---------------------------------------------------------------------------
// W7 By Heart — the correction ladder + culmination shape (C++ filing)
// ---------------------------------------------------------------------------

pub mod by_heart {
    /// The ladder's window scaling: rung 0 full, rung 1 three
    /// quarters, rung 2 half. Three rungs fail the ORDER, never the
    /// sitting (the caller owns the rung count).
    pub fn window_after_rung(base_ms: i64, rung: u32) -> i64 {
        match rung {
            0 => base_ms,
            1 => base_ms * 3 / 4,
            _ => base_ms / 2,
        }
    }

    /// On-the-beat: an answer inside the first half of its window.
    pub fn on_the_beat(window_left_ms: i64, window_full_ms: i64) -> bool {
        window_full_ms > 0 && window_left_ms * 2 >= window_full_ms
    }

    /// The culmination chain runs its windows at three fifths.
    pub fn chain_window(base_ms: i64) -> i64 {
        base_ms * 3 / 5
    }
}

// ---------------------------------------------------------------------------
// W11 Withheld — the Governor control law + petition pattern (Rust filing)
// ---------------------------------------------------------------------------

pub mod withheld {
    /// One session contract for the Governor: a fixed wall, a ride
    /// band under it, rise/decay rates, a latch. Winning is riding
    /// the band; touching the wall trips the latch.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct GovernorLaw {
        pub wall_milli: i32,
        pub band_milli: i32,
        pub rise_per_step: i32,
        pub decay_per_step: i32,
        pub latch_ms: i64,
        pub latch_bleed_per_step: i32,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum GovernorEvent {
        None,
        Riding,
        Tripped,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Governor {
        pub drive_milli: i32,
        pub latched: bool,
        pub latch_ms_left: i64,
        pub trips: u32,
        pub term_added_ms: i64,
    }

    impl Governor {
        pub fn new() -> Governor {
            Governor {
                drive_milli: 0,
                latched: false,
                latch_ms_left: 0,
                trips: 0,
                term_added_ms: 0,
            }
        }

        pub fn band_lo(&self, law: &GovernorLaw) -> i32 {
            law.wall_milli - law.band_milli
        }

        /// Advance one fixed step under (or without) stimulus.
        pub fn step(&mut self, law: &GovernorLaw, stimulus: bool, dt_ms: i64) -> GovernorEvent {
            if self.latched {
                self.drive_milli = (self.drive_milli - law.latch_bleed_per_step).max(0);
                self.latch_ms_left -= dt_ms;
                if self.latch_ms_left <= 0 {
                    self.latched = false;
                }
                return GovernorEvent::None;
            }
            if stimulus {
                self.drive_milli = (self.drive_milli + law.rise_per_step).min(1000);
            } else {
                self.drive_milli = (self.drive_milli - law.decay_per_step).max(0);
            }
            if self.drive_milli >= law.wall_milli {
                self.latched = true;
                self.latch_ms_left = law.latch_ms;
                self.trips += 1;
                self.term_added_ms += law.latch_ms;
                return GovernorEvent::Tripped;
            }
            if self.drive_milli >= self.band_lo(law) {
                return GovernorEvent::Riding;
            }
            GovernorEvent::None
        }
    }

    impl Default for Governor {
        fn default() -> Self {
            Self::new()
        }
    }

    /// The petition pattern: each banked petition is worth time and
    /// band on the NEXT session. The wall never moves.
    pub const PETITION_CAP: u32 = 3;
    pub const PETITION_TIME_MS: i64 = 8_000;
    pub const PETITION_BAND_MILLI: i32 = 15;

    pub fn petition_schedule(banked: u32) -> (i64, i32) {
        let n = banked.min(PETITION_CAP);
        (
            i64::from(n) * PETITION_TIME_MS,
            n as i32 * PETITION_BAND_MILLI,
        )
    }

    /// A well-formed ask: from at or below wall − 400, unlatched, off
    /// cooldown, under the cap.
    pub fn ask_is_well_formed(
        drive_milli: i32,
        wall_milli: i32,
        latched: bool,
        cooldown_ms: i64,
        banked: u32,
    ) -> bool {
        !latched && drive_milli <= wall_milli - 400 && cooldown_ms <= 0 && banked < PETITION_CAP
    }
}

// ---------------------------------------------------------------------------
// W13 Exposure — development tracks + the exposure ledger (Lua filing)
// ---------------------------------------------------------------------------

pub mod exposure {
    /// The development-system at one-sitting scale: no decay, a drift
    /// cap, stage thresholds on applied totals. Gains stay PENDING
    /// until witnessed at an appraisal — that law is the caller's
    /// loop; the arithmetic lives here.
    pub const TRACK_CAP: i32 = 140;
    pub const STIR_AT: i32 = 40;
    pub const FORM_AT: i32 = 100;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Stage {
        Dormant = 0,
        Stirring = 1,
        Forming = 2,
    }

    pub fn stage_of(total: i32) -> Stage {
        if total >= FORM_AT {
            Stage::Forming
        } else if total >= STIR_AT {
            Stage::Stirring
        } else {
            Stage::Dormant
        }
    }

    /// Apply pending gains at an appraisal; returns (applied,
    /// drifted_off) — what does not fit under the cap is gone, and
    /// the record says so.
    pub fn apply_pending(total: i32, pending: i32) -> (i32, i32) {
        let fits = pending.min(TRACK_CAP - total);
        (fits, pending - fits)
    }

    /// Ledger weight: a context's first logging today counts in
    /// full; a repetition counts a quarter (floored by the caller).
    pub fn ledger_weight(already_logged: bool) -> (i32, i32) {
        if already_logged { (1, 4) } else { (1, 1) }
    }

    /// Attention decay between beats: ×0.88, integer-exact.
    pub fn attention_decay(attention: i32) -> i32 {
        attention * 88 / 100
    }

    /// Praise weighting: ×3/2, applied only while a recorded context
    /// is warm (within two beats) or is the programme itself.
    pub fn attention_gain(base: i32, praise: bool, recorded_warm: bool) -> i32 {
        if praise && recorded_warm {
            base * 3 / 2
        } else {
            base
        }
    }
}

// ---------------------------------------------------------------------------
// W14 Standard Issue — the book of record + T-gate arithmetic (C++ filing)
// ---------------------------------------------------------------------------

pub mod standard_issue {
    /// Certificate bits in the book of record.
    pub const CERT_FIT: u8 = 1;
    pub const CERT_SIGNALS: u8 = 2;

    /// The T-gate arithmetic, computed from the book — never stored:
    /// T1 = any one paper; T2 = Fit + Signals; T3 = two Certified
    /// Standards (a count this two-bit book cannot express, by
    /// design: the gate stays honestly locked at this scale).
    pub fn tier(certs: u8) -> u8 {
        let fit = certs & CERT_FIT != 0;
        let signals = certs & CERT_SIGNALS != 0;
        if fit && signals {
            2
        } else if fit || signals {
            1
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// Fathom RE 8245833 — the build-up envelope (toy-play escalation study)
// ---------------------------------------------------------------------------

pub mod build_up {
    /// The measured build-up of fathom RE 8245833 (1920x1080, 60 fps,
    /// 5923 frames, 98.7 s), as one reusable envelope. The source is a
    /// staircase: each editorial cut raises the motion floor and the
    /// pose opens one step, one landing holds (the plateau), then a
    /// single sustained peak window, then an aftermath that never
    /// returns to baseline inside the file. Intensities are the
    /// per-segment mid-band motion means normalised to the first
    /// segment (x1000). The full filing is
    /// docs/jam-port-backs/build-up-8245833.md in the torogao tree;
    /// the RE of record is the fathom-re-buildup workspace RECIPE.md.
    ///
    /// Arousal-engine/1 mapping, with the divergences kept: the peak
    /// fires exactly once; the source peak is a SUSTAINED window
    /// (8.2 s), not the engine's 3000 ms peak hold, and the source
    /// has NO daze segment — the file ends in aftermath — so the
    /// engine's 6000 ms daze is consumer-synthesised, never
    /// extracted. Crest trains are absent from this source: a
    /// consumer wanting trains repeats the peak window under its
    /// own law.
    /// Segment boundaries (frames at 60 fps) and intensity floors of
    /// the measured staircase: (start_frame, intensity_milli).
    pub const STAIRCASE: [(u32, u32); 8] = [
        (0, 1000),    // held pose, device placed, micro-motion only
        (1212, 2090), // first work: the floor doubles
        (1804, 3830),
        (2457, 5690),
        (3112, 8820),  // sustain: the long working landing
        (4132, 7750),  // plateau: holds BELOW the previous floor
        (5210, 17000), // the peak window (culmination, exactly once)
        (5696, 12600), // aftermath: elevated, no taper in the file
    ];

    /// The source's total length and peak placement.
    pub const TOTAL_FRAMES: u32 = 5923;
    pub const PEAK_START: u32 = 5210;
    pub const PEAK_END: u32 = 5696;

    /// The fast drive cycle resolved in the back half of the source
    /// (15 frames at 60 fps, autocorrelation r = 0.83 in the peak
    /// window): the device's own frequency, under the body's slower
    /// rock (~1.05 s, weakly resolved at the sustain landing).
    pub const DRIVE_CYCLE_FRAMES: u32 = 15;

    /// Arousal-engine/1 reference points the envelope is mapped
    /// against (the engine's law, not the source's measurement).
    pub const ENGINE_PEAK_MS: u32 = 3000;
    pub const ENGINE_DAZE_MS: u32 = 6000;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Phase {
        /// Segments 0..=3: the floor rises every cut.
        Escalate,
        /// Segment 4: the long working landing at the top floor.
        Sustain,
        /// Segment 5: intensity held below the sustain floor —
        /// the held breath before the peak, not a rest.
        Plateau,
        /// Segment 6: culmination. Fires once.
        Peak,
        /// Segment 7: post-peak aftermath; the file ends here,
        /// still elevated. There is no return-to-baseline segment.
        Aftermath,
    }

    /// The phase a staircase segment belongs to.
    pub fn phase_of(segment: usize) -> Phase {
        match segment {
            0..=3 => Phase::Escalate,
            4 => Phase::Sustain,
            5 => Phase::Plateau,
            6 => Phase::Peak,
            _ => Phase::Aftermath,
        }
    }

    /// The staircase segment a frame falls in (the last segment
    /// runs to TOTAL_FRAMES).
    pub fn segment_at(frame: u32) -> usize {
        let mut seg = 0;
        for (i, &(start, _)) in STAIRCASE.iter().enumerate() {
            if frame >= start {
                seg = i;
            }
        }
        seg
    }

    /// The intensity floor (milli, segment 0 = 1000) at a frame.
    pub fn intensity_milli_at(frame: u32) -> u32 {
        STAIRCASE[segment_at(frame)].1
    }

    /// Culmination fires exactly once: true only inside the peak
    /// window, which is a single contiguous segment of the
    /// staircase — the exactly-once property is structural.
    pub fn culmination_active(frame: u32) -> bool {
        (PEAK_START..PEAK_END).contains(&frame)
    }

    /// The peak window's length in ms at a given frame rate,
    /// beside the engine's peak hold: the divergence, as data.
    /// (8245833 at 60 fps: 486 frames = 8100 ms vs the engine's
    /// 3000 ms — a consumer compresses the window or sustains
    /// under its own law; this crate records, it does not decide.)
    pub fn peak_window_ms(fps: u32) -> u32 {
        (PEAK_END - PEAK_START) * 1000 / fps
    }

    /// The quartermaster drill adaptation (ADAPT-8245833, design
    /// filing): the staircase run as called floors under
    /// appraisal. Deviation from the called floor accumulates as
    /// faults against a hard ceiling of 3; the round ratio feeds
    /// per-rank earn gates (0.55 / 0.60 / 0.65 / 0.70 permille),
    /// with level pass bands at 0.60 / 0.65 / 0.70. Rounds append
    /// to the evidence ledger inside its existing 512 cap. The
    /// peak round is called once per sitting and never repeated.
    pub const DRILL_FAULT_CEILING: u32 = 3;
    pub const DRILL_EARN_GATES_PERMILLE: [u32; 4] = [550, 600, 650, 700];
    pub const DRILL_PASS_BANDS_PERMILLE: [u32; 3] = [600, 650, 700];
    pub const DRILL_LEDGER_CAP: usize = 512;

    pub fn drill_earns_step(rank: usize, ratio_permille: u32) -> bool {
        rank < DRILL_EARN_GATES_PERMILLE.len() && ratio_permille >= DRILL_EARN_GATES_PERMILLE[rank]
    }

    pub fn drill_faults_fail(faults: u32) -> bool {
        faults >= DRILL_FAULT_CEILING
    }
}

// ---------------------------------------------------------------------------
// W4 Handfast — coupled-anchor contact + beat grammar (Lua filing)
// ---------------------------------------------------------------------------

pub mod handfast {
    /// A contact is stored as (figure, anchor) ON THE RECEIVER —
    /// never as a world position. The world position is derived
    /// every tick from the receiver's anchor transform (breathing
    /// lift and sway included), so drift is impossible by
    /// construction, not by correction. (The ADAPT-8245833 filing
    /// re-affirms this law for the build-up staircase: a stage
    /// change is a contact broken and re-made at a new anchor,
    /// never a slide.)
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Contact {
        pub receiver_figure: u16,
        pub anchor: u16,
    }

    /// An anchor: a named point on a figure, as a local offset in
    /// milli-units from the figure origin.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Anchor {
        pub local_x_milli: i32,
        pub local_y_milli: i32,
    }

    /// Derive the contact's world position for this tick: receiver
    /// origin + anchor offset + the breathing lift applied to the
    /// whole figure (breath is a layer under everything coupled).
    pub fn derive_world(
        origin_x_milli: i32,
        origin_y_milli: i32,
        anchor: &Anchor,
        breath_lift_milli: i32,
    ) -> (i32, i32) {
        (
            origin_x_milli + anchor.local_x_milli,
            origin_y_milli + anchor.local_y_milli + breath_lift_milli,
        )
    }

    /// Acceptance metric: at least 95% of frames within an intimate
    /// shot show contact in the active zone (the reference's final
    /// shot measured 564/581 = 97%). Integer-exact; an empty shot
    /// never passes.
    pub const ACCEPTANCE_MIN_PERMILLE: u32 = 950;

    pub fn contact_zone_accepted(frames_in_zone: u32, frames_total: u32) -> bool {
        frames_total > 0
            && u64::from(frames_in_zone) * 1000
                >= u64::from(frames_total) * u64::from(ACCEPTANCE_MIN_PERMILLE)
    }

    /// The beat grammar, in order; the held appraisal is the
    /// terminal state and the longest element — a hold with a
    /// filling meter, not a gap between actions.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Beat {
        Approach = 0,
        Contact = 1,
        Guidance = 2,
        HeldAppraisal = 3,
    }

    pub const BEAT_ORDER: [Beat; 4] = [
        Beat::Approach,
        Beat::Contact,
        Beat::Guidance,
        Beat::HeldAppraisal,
    ];

    /// Expression ramps, never snaps: every expressive channel
    /// moves toward its target at a bounded rate (90 milli per
    /// tick in the shipped game), passing through all
    /// intermediates. A snap is a data error.
    pub const RAMP_STEP_MILLI: i32 = 90;

    pub fn ramp_toward(current_milli: i32, target_milli: i32) -> i32 {
        if target_milli > current_milli {
            (current_milli + RAMP_STEP_MILLI).min(target_milli)
        } else {
            (current_milli - RAMP_STEP_MILLI).max(target_milli)
        }
    }

    /// The receiving figure carries higher activity (x1.30,
    /// matching the reference's measured 2.64 vs 2.08).
    pub const RECEIVER_ACTIVITY_MILLI: i32 = 1300;

    /// Breathing is a layer, not an animation: always on, never
    /// gated by beat or screen; amplitude scales per level.
    pub const BREATH_PERIOD_MS_PRIMA: u32 = 4300;
    pub const BREATH_PERIOD_MS_SLOANE: u32 = 4900;

    pub fn breath_scale_milli(level: u32) -> i32 {
        match level {
            0 => 1000,
            1 => 1150,
            _ => 1300,
        }
    }

    /// The glowing iris is a switchable face LAYER, never a model
    /// property (Owner ruling): on only when the beat calls for
    /// it, regard is at or over the scene's threshold, and the
    /// user setting allows it.
    pub fn iris_glow_on(
        in_glow_beat: bool,
        regard_milli: i32,
        regard_threshold_milli: i32,
        user_setting: bool,
    ) -> bool {
        in_glow_beat && user_setting && regard_milli >= regard_threshold_milli
    }
}

// ---------------------------------------------------------------------------
// W8 Creamery — sample-clock sync + the chart ledger (Zig filing)
// ---------------------------------------------------------------------------

pub mod creamery {
    /// Sample-clock sync: animation phase is DERIVED from a stream
    /// sample counter (nominal 48 kHz; one 16 ms step = 768
    /// samples), never from a second timeline. Measured tolerance
    /// in the shipped core: zero steps of drift, because every
    /// cycle duration is an integer number of steps — the sync
    /// invariant is exact integer arithmetic.
    pub const SAMPLE_RATE_HZ: u32 = 48_000;
    pub const STEP_MS: u32 = 16;
    pub const SAMPLES_PER_STEP: u64 = 768;

    pub fn step_at(sample_count: u64) -> u64 {
        sample_count / SAMPLES_PER_STEP
    }

    pub fn samples_at_step(step: u64) -> u64 {
        step * SAMPLES_PER_STEP
    }

    /// Hit-stop freezes the clock with everything else (96 ms);
    /// freezing only the visuals while the stream runs would spend
    /// the sync the design exists to prove. The flash is 240 ms.
    pub const HIT_STOP_MS: u32 = 96;
    pub const FLASH_MS: u32 = 240;

    pub fn clock_advance(samples: u64, hit_stop_active: bool) -> u64 {
        if hit_stop_active { 0 } else { samples }
    }

    /// The chart-entry shape for the book of record: a witnessed
    /// event, the tool configuration it ran under, what landed,
    /// and the measured result.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct ChartEntry {
        pub cycle: u32,
        pub preset: u8,
        pub hits: u32,
        pub strokes: u32,
        pub yield_ml: u32,
        pub full: bool,
    }

    /// Append-only, bounded ledger: 24 entries; overflow drops are
    /// COUNTED, never silently overwritten. The session total is
    /// exactly the sum of the entries.
    pub const CHART_CAP: usize = 24;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Chart {
        entries: [ChartEntry; CHART_CAP],
        len: usize,
        pub dropped: u32,
    }

    impl Chart {
        pub fn new() -> Chart {
            Chart {
                entries: [ChartEntry::default(); CHART_CAP],
                len: 0,
                dropped: 0,
            }
        }

        /// Append one entry; returns false (and counts the drop)
        /// when the ledger is full.
        pub fn push(&mut self, entry: ChartEntry) -> bool {
            if self.len >= CHART_CAP {
                self.dropped += 1;
                return false;
            }
            self.entries[self.len] = entry;
            self.len += 1;
            true
        }

        pub fn len(&self) -> usize {
            self.len
        }

        pub fn is_empty(&self) -> bool {
            self.len == 0
        }

        pub fn total_yield_ml(&self) -> u64 {
            self.entries[..self.len]
                .iter()
                .map(|e| u64::from(e.yield_ml))
                .sum()
        }
    }

    impl Default for Chart {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Queued comfort changes: a preset change made mid-cycle
    /// QUEUES and applies at the cycle boundary; a running cycle
    /// is never re-judged retroactively.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct PresetQueue {
        pub active: u8,
        pub pending: Option<u8>,
    }

    impl PresetQueue {
        pub fn queue(&mut self, preset: u8) {
            self.pending = Some(preset);
        }

        /// At the boundary the queued preset becomes active;
        /// returns the now-active preset either way.
        pub fn apply_at_boundary(&mut self) -> u8 {
            if let Some(p) = self.pending.take() {
                self.active = p;
            }
            self.active
        }
    }
}

// ---------------------------------------------------------------------------
// W3 Blushline — face-channel response curves + gaze scorer (Mojo filing)
// ---------------------------------------------------------------------------

pub mod blushline {
    /// The blush channel as a pure function stack. Saturating
    /// cubic response: 1 - (1 - x)^3 — monotone, exact endpoints,
    /// no transcendentals, bit-reproducible in any language.
    pub fn response(x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        let rest = 1.0 - x;
        1.0 - rest * rest * rest
    }

    /// Session accumulator: gain 0.16 per unit stimulus, clamped
    /// to the channel range.
    pub const ACC_GAIN: f32 = 0.16;

    pub fn accumulate(acc: f32, stimulus: f32) -> f32 {
        (acc + ACC_GAIN * stimulus.max(0.0)).clamp(0.0, 1.0)
    }

    /// Step integrator whose rise rate is 4x its fall rate
    /// (1/32 vs 1/128 per 16 ms step — both exact binary
    /// fractions): colour arrives fast, leaves slowly.
    pub const RISE_PER_STEP: f32 = 1.0 / 32.0;
    pub const FALL_PER_STEP: f32 = 1.0 / 128.0;

    pub fn step_toward(current: f32, target: f32) -> f32 {
        if target > current {
            (current + RISE_PER_STEP).min(target)
        } else {
            (current - FALL_PER_STEP).max(target)
        }
    }

    /// Tier coupling for the arousal engine's low tiers:
    /// thresholds inclusive at the bottom.
    pub fn tier_of(blush: f32) -> u8 {
        if blush >= 0.55 {
            2
        } else if blush >= 0.25 {
            1
        } else {
            0
        }
    }

    /// Gaze scoring under section 1.7. Rule order is contractual:
    /// downcast FIRST on normalised downward offset (>= 0.45),
    /// then toward at magnitude <= 0.30, else away. Unreadable
    /// geometry returns the INVALID sentinel instead of a false
    /// verdict.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum GazeVerdict {
        Downcast,
        Toward,
        Away,
        Invalid,
    }

    pub const DOWNCAST_MIN: f32 = 0.45;
    pub const TOWARD_MAX: f32 = 0.30;

    pub fn gaze_verdict(dx: f32, dy_down: f32, disc_radius: f32) -> GazeVerdict {
        if !dx.is_finite() || !dy_down.is_finite() || !disc_radius.is_finite() || disc_radius <= 0.0
        {
            return GazeVerdict::Invalid;
        }
        let nx = dx / disc_radius;
        let ny = dy_down / disc_radius;
        if ny >= DOWNCAST_MIN {
            return GazeVerdict::Downcast;
        }
        if (nx * nx + ny * ny).sqrt() <= TOWARD_MAX {
            GazeVerdict::Toward
        } else {
            GazeVerdict::Away
        }
    }
}

// ---------------------------------------------------------------------------
// W9 Milkmaid — fluid timeline + attributed culmination (Mojo filing)
// ---------------------------------------------------------------------------

pub mod milkmaid {
    /// The fluid timeline model: yield is a cumulative integer
    /// ledger sampled into a ring (every 30 steps, 56 samples)
    /// and drawn as the round's own curve. Any accumulating
    /// quantity gets a ledger, a sampler, and a curve — never a
    /// bare counter. Growth is booked as capacity, not yield.
    pub const SAMPLE_EVERY_STEPS: u32 = 30;
    pub const RING_SAMPLES: usize = 56;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Timeline {
        ring: [i64; RING_SAMPLES],
        head: usize,
        pub samples: u32,
        pub ledger_ml: i64,
    }

    impl Timeline {
        pub fn new() -> Timeline {
            Timeline {
                ring: [0; RING_SAMPLES],
                head: 0,
                samples: 0,
                ledger_ml: 0,
            }
        }

        /// Book yield into the ledger; on a sampling step the new
        /// cumulative total enters the ring (overwriting the
        /// oldest sample — the ring is bounded by design).
        pub fn book(&mut self, step: u32, delta_ml: i64) {
            self.ledger_ml += delta_ml;
            if step.is_multiple_of(SAMPLE_EVERY_STEPS) {
                self.ring[self.head] = self.ledger_ml;
                self.head = (self.head + 1) % RING_SAMPLES;
                self.samples += 1;
            }
        }

        /// The most recent sampled cumulative total.
        pub fn latest(&self) -> i64 {
            if self.samples == 0 {
                0
            } else {
                self.ring[(self.head + RING_SAMPLES - 1) % RING_SAMPLES]
            }
        }
    }

    impl Default for Timeline {
        fn default() -> Self {
            Self::new()
        }
    }

    /// A culmination event: exactly-once, attributed to the
    /// method active at the step it lands, and impossible to
    /// rewrite after the verdict. A same-step switch captures the
    /// culmination; a late one does not.
    pub const METHOD_COUNT: usize = 4;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct CulminationEvent {
        pub method_id: u8,
        pub counts: [u32; METHOD_COUNT],
        pub yield_split_ml: [i64; METHOD_COUNT],
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct CulminationLatch {
        pub event: Option<CulminationEvent>,
    }

    impl CulminationLatch {
        pub fn new() -> CulminationLatch {
            CulminationLatch { event: None }
        }

        /// Fire the culmination under the method active NOW.
        /// Returns the event on the first firing only; every
        /// later call returns None and the record stands.
        pub fn fire(
            &mut self,
            active_method: u8,
            counts: [u32; METHOD_COUNT],
            yield_split_ml: [i64; METHOD_COUNT],
        ) -> Option<CulminationEvent> {
            if self.event.is_some() {
                return None;
            }
            let ev = CulminationEvent {
                method_id: active_method,
                counts,
                yield_split_ml,
            };
            self.event = Some(ev);
            Some(ev)
        }
    }

    impl Default for CulminationLatch {
        fn default() -> Self {
            Self::new()
        }
    }

    /// The persistence split: settings + lifetime aggregates
    /// persist in the shell; a round is one sitting, held in-core
    /// and never saved.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum SaveItem {
        Setting,
        LifetimeAggregate,
        RoundState,
    }

    pub fn persists_in_shell(item: SaveItem) -> bool {
        matches!(item, SaveItem::Setting | SaveItem::LifetimeAggregate)
    }
}

// ---------------------------------------------------------------------------
// W6 Crescendo — the arousal-engine/1 tier law (GDScript filing)
// ---------------------------------------------------------------------------

pub mod crescendo {
    /// The canon tempos, as (rise_ms, hold_ms) per tier. At 16 ms
    /// fixed steps they resolve to rise+hold steps of 250/250,
    /// 438/375, 688/563, 1000/750 — the non-integer tiers quantise
    /// upward by <1 step. Durations are stated in ms and the
    /// quantisation accepted, never disguised as frame-exactness.
    pub const STEP_MS: u32 = 16;
    pub const TIER_TEMPOS_MS: [(u32, u32); 4] = [
        (4_000, 4_000),   // T1
        (7_000, 6_000),   // T2
        (11_000, 9_000),  // T3
        (16_000, 12_000), // T4
    ];

    pub fn steps_for(ms: u32) -> u32 {
        ms.div_ceil(STEP_MS)
    }

    /// Face coupling: F-03 at edge, F-05 at peak, F-10 in the
    /// daze after. Peak and daze durations as resolved in the W6
    /// divergence filing D-1 (daze = 2x peak; the afterglow is
    /// the longer beat).
    pub const FACE_EDGE: u8 = 3;
    pub const FACE_PEAK: u8 = 5;
    pub const FACE_DAZE: u8 = 10;
    pub const PEAK_MS: u32 = 3_000;
    pub const DAZE_MS: u32 = 6_000;

    /// The edge verb (divergence D-2): edge accrues only while
    /// released; holding at the edge accrues strain — 1200 ms of
    /// strain slips to half rise, and 3 slips fail the run.
    pub const EDGE_STRAIN_LIMIT_MS: u32 = 1_200;
    pub const MAX_SLIPS: u32 = 3;

    pub fn slip(rise_milli: i32) -> i32 {
        rise_milli / 2
    }

    pub fn run_failed(slips: u32) -> bool {
        slips >= MAX_SLIPS
    }

    /// Rise decay on release (divergence D-3): a slow bleed of
    /// dt/4, never punitive.
    pub fn decay_per_step(step_ms: u32) -> u32 {
        step_ms / 4
    }

    /// Session clocks sized at ~2x the perfect run (divergence
    /// D-6): the ratio is the tuning constant, not the absolutes.
    pub const SESSION_CLOCKS_MS: [(u32, u32); 3] =
        [(30_000, 90_000), (50_000, 120_000), (78_000, 150_000)];

    /// Weather mapping (divergence D-5): tier-1 during rise/edge,
    /// a distinct peak state (4) and daze state (5). Cheap,
    /// total, testable.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Phase {
        Rise,
        Edge,
        Peak,
        Daze,
    }

    pub fn weather_index(phase: Phase, tier: u8) -> u8 {
        match phase {
            Phase::Rise | Phase::Edge => tier.saturating_sub(1).min(3),
            Phase::Peak => 4,
            Phase::Daze => 5,
        }
    }

    /// Culmination (divergence D-4): fires on completion of the
    /// session's TARGET tier edge, latched so no path re-fires
    /// it — exactly-once with the trigger named.
    pub fn culmination_fires(completed_tier: u8, target_tier: u8, already_latched: bool) -> bool {
        !already_latched && completed_tier >= target_tier && target_tier > 0
    }
}

// ---------------------------------------------------------------------------
// W12 Second Wave — continuation + rig memory (C# filing)
// ---------------------------------------------------------------------------

pub mod second_wave {
    /// Three-phase structure: Build / Peak (frozen values, timed
    /// decision window) / Continuation, with exactly-one-
    /// continuation enforced structurally.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Phase {
        Build,
        Peak,
        Continuation,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ContinuationLatch {
        pub used: bool,
    }

    impl ContinuationLatch {
        pub fn new() -> ContinuationLatch {
            ContinuationLatch { used: false }
        }

        /// Take the one continuation; a second take fails.
        pub fn take(&mut self) -> bool {
            if self.used {
                return false;
            }
            self.used = true;
            true
        }
    }

    impl Default for ContinuationLatch {
        fn default() -> Self {
            Self::new()
        }
    }

    /// The phase grammar: build -> peak lands in the 72-86% band
    /// of a scene's depth budget, then the held continuation.
    pub fn peak_in_band(depth_milli: i32, budget_milli: i32) -> bool {
        budget_milli > 0
            && i64::from(depth_milli) * 100 >= i64::from(budget_milli) * 72
            && i64::from(depth_milli) * 100 <= i64::from(budget_milli) * 86
    }

    /// Reduced-curve continuation arithmetic: rise x0.55, decay
    /// x0.60, both floored. (An earlier x1.40 decay draft made the
    /// continuation mathematically unsustainable under any duty
    /// cycle; the shipped pair sustains ~50% at the ceiling.)
    pub fn reduced_rise(base_milli: i32) -> i32 {
        base_milli * 550 / 1000
    }

    pub fn reduced_decay(base_milli: i32) -> i32 {
        base_milli * 600 / 1000
    }

    /// Sensitivity meter: seeded at 300 milli on entry (the
    /// second wave starts already tender), +5/step held, -3/step
    /// released, soft cap 1000 (1250 with the assist setting) —
    /// reaching the cap is a settle outcome, not a fail.
    pub const SENSITIVITY_SEED_MILLI: i32 = 300;
    pub const SENSITIVITY_HELD_PER_STEP: i32 = 5;
    pub const SENSITIVITY_RELEASED_PER_STEP: i32 = -3;
    pub const SENSITIVITY_CAP_MILLI: i32 = 1000;
    pub const SENSITIVITY_CAP_ASSIST_MILLI: i32 = 1250;

    pub fn sensitivity_step(current_milli: i32, held: bool, assist: bool) -> i32 {
        let cap = if assist {
            SENSITIVITY_CAP_ASSIST_MILLI
        } else {
            SENSITIVITY_CAP_MILLI
        };
        let delta = if held {
            SENSITIVITY_HELD_PER_STEP
        } else {
            SENSITIVITY_RELEASED_PER_STEP
        };
        (current_milli + delta).clamp(0, cap)
    }

    /// Time-in-band to close, per level: (target_milli, ms).
    pub const BAND_TARGETS: [(i32, u32); 3] = [(650, 4_000), (720, 5_000), (780, 6_000)];

    /// Rig memory as residue + floor: the next scene's start is
    /// the end value times an outcome-typed percentage, plus a
    /// floor that depends on HOW the previous scene ended. The
    /// stopped path's missing floor is the STOP cost made
    /// mechanical.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Outcome {
        ContinuationSuccess,
        Settled,
        Stopped,
        Faded,
    }

    pub fn residue_start_milli(end_milli: i32, outcome: Outcome) -> i32 {
        match outcome {
            Outcome::ContinuationSuccess => end_milli * 40 / 100 + 90,
            Outcome::Settled => end_milli * 25 / 100 + 40,
            Outcome::Stopped => end_milli * 15 / 100,
            Outcome::Faded => end_milli * 10 / 100,
        }
    }

    /// STOP is a priced choice: always available, always
    /// recorded; 150 points, and the Rig remembers less (see the
    /// residue table above).
    pub const STOP_POINTS: i32 = 150;
}

// ---------------------------------------------------------------------------
// W5 Redlight — reaction-set sizing + attention floor (C# filing)
// ---------------------------------------------------------------------------

pub mod redlight {
    /// Reaction-set sizing law: at least 6 lines per (character x
    /// event class), selection = uniform pick with last-line
    /// exclusion. Below ~4 lines per class the repeat window
    /// collapses and repetition shows within one session; 6 is
    /// the floor for a session-length sampler. Coverage is
    /// measured as distinct lines heard per class — never as
    /// total lines recorded.
    pub const MIN_LINES_PER_CLASS: u32 = 6;

    pub fn set_is_session_sized(lines: u32) -> bool {
        lines >= MIN_LINES_PER_CLASS
    }

    /// Uniform pick over the class's lines with the last-heard
    /// line excluded: `roll` is the caller's uniform draw. With
    /// no last line (or a single line) the pick is direct.
    /// Returns None only for an empty set.
    pub fn pick_line(lines: u32, last: Option<u32>, roll: u32) -> Option<u32> {
        if lines == 0 {
            return None;
        }
        match last {
            Some(l) if lines > 1 && l < lines => {
                let idx = roll % (lines - 1);
                Some(if idx >= l { idx + 1 } else { idx })
            }
            _ => Some(roll % lines),
        }
    }

    /// The attention economy, in milli-units (0-1000). Attention
    /// is tracked in hundredths of a milli-unit so the per-step
    /// decays (0.30 / 0.38 / 0.46 by audience size) stay
    /// integer-exact. Gains: pose +55 on a 30-step cooldown,
    /// applause +70, a timed lens answer +95. Decay NEVER crosses
    /// the floor; only praise raises the floor (+50 per
    /// compliment, cap 500) plus +35 immediate — a floor that
    /// only praise can raise turns compliments from flavour into
    /// structure.
    pub const DECAY_PER_STEP_HUNDREDTHS: [i32; 3] = [30, 38, 46];
    pub const POSE_GAIN_MILLI: i32 = 55;
    pub const POSE_COOLDOWN_STEPS: u32 = 30;
    pub const APPLAUSE_GAIN_MILLI: i32 = 70;
    pub const LENS_GAIN_MILLI: i32 = 95;
    pub const PRAISE_FLOOR_STEP_MILLI: i32 = 50;
    pub const PRAISE_FLOOR_CAP_MILLI: i32 = 500;
    pub const PRAISE_IMMEDIATE_MILLI: i32 = 35;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Attention {
        /// Hundredths of a milli-unit.
        pub value_hundredths: i32,
        pub floor_milli: i32,
    }

    impl Attention {
        pub fn new() -> Attention {
            Attention {
                value_hundredths: 0,
                floor_milli: 0,
            }
        }

        pub fn value_milli(&self) -> i32 {
            self.value_hundredths / 100
        }

        /// One decay step for an audience size (0..=2); the value
        /// stops at the floor.
        pub fn decay_step(&mut self, audience: usize) {
            let decay = DECAY_PER_STEP_HUNDREDTHS[audience.min(2)];
            let floor_h = self.floor_milli * 100;
            self.value_hundredths = (self.value_hundredths - decay).max(floor_h);
        }

        pub fn gain(&mut self, gain_milli: i32) {
            self.value_hundredths = (self.value_hundredths + gain_milli * 100).min(100_000);
        }

        /// A compliment: the floor rises (capped) and the value
        /// gains the immediate term.
        pub fn praise(&mut self) {
            self.floor_milli =
                (self.floor_milli + PRAISE_FLOOR_STEP_MILLI).min(PRAISE_FLOOR_CAP_MILLI);
            self.gain(PRAISE_IMMEDIATE_MILLI);
        }
    }

    impl Default for Attention {
        fn default() -> Self {
            Self::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Tests: each law carries the numbers its game measured.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_transition_table_is_total_and_asymmetric() {
        assert_eq!(trancefall::transition_ms(0, 0), 0);
        assert_eq!(trancefall::transition_ms(0, 2), 1800); // down: 2 x 900
        assert_eq!(trancefall::transition_ms(3, 1), 1000); // up: 2 x 500
    }

    #[test]
    fn blink_slows_with_the_scalar() {
        let p = idle_hands::IDLE_PARAMS;
        let calm = idle_hands::blink_interval_ms(&p, 0.0, 0.5);
        let low = idle_hands::blink_interval_ms(&p, 1.0, 0.5);
        assert!(low > calm);
        // The chase converges and never overshoots in one step.
        let v = idle_hands::chase(0.0, 100.0, p.gaze_stiffness_per_s, 16);
        assert!(v > 0.0 && v < 100.0);
    }

    #[test]
    fn ladder_scales_and_beat_is_the_first_half() {
        assert_eq!(by_heart::window_after_rung(4000, 0), 4000);
        assert_eq!(by_heart::window_after_rung(4000, 1), 3000);
        assert_eq!(by_heart::window_after_rung(4000, 2), 2000);
        assert!(by_heart::on_the_beat(2000, 4000));
        assert!(!by_heart::on_the_beat(1999, 4000));
        assert_eq!(by_heart::chain_window(5000), 3000);
    }

    #[test]
    fn governor_trips_latches_and_rides() {
        let law = withheld::GovernorLaw {
            wall_milli: 780,
            band_milli: 180,
            rise_per_step: 5,
            decay_per_step: 3,
            latch_ms: 3000,
            latch_bleed_per_step: 25,
        };
        let mut g = withheld::Governor::new();
        // Ride: oscillate under the wall.
        let mut hold = false;
        let mut rode = false;
        for _ in 0..2000 {
            if g.drive_milli >= law.wall_milli - 25 {
                hold = false;
            } else if g.drive_milli <= g.band_lo(&law) - 30 {
                hold = true;
            }
            if g.step(&law, hold, 16) == withheld::GovernorEvent::Riding {
                rode = true;
            }
        }
        assert!(rode);
        assert_eq!(g.trips, 0);
        // Hold through the wall: trip + latch + the book grows.
        let mut g2 = withheld::Governor::new();
        let mut tripped = false;
        for _ in 0..400 {
            if g2.step(&law, true, 16) == withheld::GovernorEvent::Tripped {
                tripped = true;
            }
        }
        // The latch (3000 ms) expires inside the 400-step window, so
        // assert the event and the book, not the end state.
        assert!(tripped && g2.trips >= 1 && g2.term_added_ms >= 3000);
    }

    #[test]
    fn petitions_move_schedule_never_wall() {
        assert_eq!(withheld::petition_schedule(2), (16_000, 30));
        assert_eq!(withheld::petition_schedule(99), (24_000, 45)); // capped
        assert!(withheld::ask_is_well_formed(300, 780, false, 0, 0));
        assert!(!withheld::ask_is_well_formed(500, 780, false, 0, 0)); // too high
        assert!(!withheld::ask_is_well_formed(300, 780, true, 0, 0)); // latched
    }

    #[test]
    fn track_law_thresholds_cap_and_ledger() {
        assert_eq!(exposure::stage_of(39), exposure::Stage::Dormant);
        assert_eq!(exposure::stage_of(40), exposure::Stage::Stirring);
        assert_eq!(exposure::stage_of(100), exposure::Stage::Forming);
        assert_eq!(exposure::apply_pending(135, 30), (5, 25)); // cap binds
        assert_eq!(exposure::ledger_weight(false), (1, 1));
        assert_eq!(exposure::ledger_weight(true), (1, 4));
        assert_eq!(exposure::attention_decay(1000), 880);
        assert_eq!(exposure::attention_gain(100, true, true), 150);
        assert_eq!(exposure::attention_gain(100, true, false), 100);
    }

    #[test]
    fn build_up_staircase_phases_and_exactly_once_peak() {
        use build_up::Phase;
        // The staircase floors rise through escalation and sustain,
        // dip at the plateau, and jump hardest into the peak.
        let floors: Vec<u32> = build_up::STAIRCASE.iter().map(|&(_, f)| f).collect();
        assert!(floors[0] < floors[1] && floors[1] < floors[2]);
        assert!(floors[2] < floors[3] && floors[3] < floors[4]);
        assert!(floors[5] < floors[4]); // the plateau holds below sustain
        assert!(floors[6] > floors[4] * 19 / 10); // the peak jump, ~x1.9
        assert!(floors[7] < floors[6] && floors[7] > floors[4]); // aftermath stays up
        // Phase map over the whole take.
        assert_eq!(build_up::phase_of(build_up::segment_at(0)), Phase::Escalate);
        assert_eq!(
            build_up::phase_of(build_up::segment_at(4000)),
            Phase::Sustain
        );
        assert_eq!(
            build_up::phase_of(build_up::segment_at(5000)),
            Phase::Plateau
        );
        assert_eq!(build_up::phase_of(build_up::segment_at(5500)), Phase::Peak);
        assert_eq!(
            build_up::phase_of(build_up::segment_at(5900)),
            Phase::Aftermath
        );
        // Exactly-once: culmination is active in one contiguous
        // window and nowhere else in the file.
        assert!(!build_up::culmination_active(build_up::PEAK_START - 1));
        assert!(build_up::culmination_active(build_up::PEAK_START));
        assert!(build_up::culmination_active(build_up::PEAK_END - 1));
        assert!(!build_up::culmination_active(build_up::PEAK_END));
        let active = (0..build_up::TOTAL_FRAMES)
            .filter(|&f| build_up::culmination_active(f))
            .count() as u32;
        assert_eq!(active, build_up::PEAK_END - build_up::PEAK_START);
        // The divergence, as data: the source peak window at its
        // native 60 fps is far longer than the engine's peak hold.
        assert_eq!(build_up::peak_window_ms(60), 8100);
        assert!(build_up::peak_window_ms(60) > build_up::ENGINE_PEAK_MS);
    }

    #[test]
    fn contact_is_anchor_relative_and_derived_per_tick() {
        let anchor = handfast::Anchor {
            local_x_milli: 120,
            local_y_milli: -40,
        };
        let c = handfast::Contact {
            receiver_figure: 2,
            anchor: 7,
        };
        assert_eq!(c.anchor, 7);
        // The receiver breathes/sways: the derived position moves
        // with the origin, the stored record never changes.
        let a = handfast::derive_world(1000, 2000, &anchor, 15);
        let b = handfast::derive_world(1010, 2000, &anchor, 25);
        assert_eq!(a, (1120, 1975));
        assert_eq!(b, (1130, 1985));
        // Acceptance: the reference shot (564/581 = 97%) passes;
        // 94% does not; an empty shot does not.
        assert!(handfast::contact_zone_accepted(564, 581));
        assert!(handfast::contact_zone_accepted(95, 100));
        assert!(!handfast::contact_zone_accepted(94, 100));
        assert!(!handfast::contact_zone_accepted(0, 0));
        // Ramps pass through intermediates and never overshoot.
        assert_eq!(handfast::ramp_toward(0, 1000), 90);
        assert_eq!(handfast::ramp_toward(950, 1000), 1000);
        assert_eq!(handfast::ramp_toward(1000, 0), 910);
        // Beat grammar is the filed order, appraisal last.
        assert_eq!(handfast::BEAT_ORDER[3], handfast::Beat::HeldAppraisal);
        // Glow is a layer: needs beat AND regard AND the setting.
        assert!(handfast::iris_glow_on(true, 600, 500, true));
        assert!(!handfast::iris_glow_on(true, 600, 500, false));
        assert!(!handfast::iris_glow_on(true, 499, 500, true));
        assert!(!handfast::iris_glow_on(false, 600, 500, true));
    }

    #[test]
    fn sample_clock_never_drifts_and_chart_is_a_bounded_ledger() {
        // Zero drift: step boundaries land exactly on sample counts.
        for step in 0..10_000u64 {
            assert_eq!(creamery::step_at(creamery::samples_at_step(step)), step);
        }
        assert_eq!(
            creamery::SAMPLES_PER_STEP * 1000 / u64::from(creamery::STEP_MS),
            u64::from(creamery::SAMPLE_RATE_HZ)
        );
        // Hit-stop freezes the clock outright.
        assert_eq!(creamery::clock_advance(768, true), 0);
        assert_eq!(creamery::clock_advance(768, false), 768);
        // Ledger: cap binds, drops are counted, total is the sum.
        let mut chart = creamery::Chart::new();
        for i in 0..30u32 {
            chart.push(creamery::ChartEntry {
                cycle: i,
                preset: 1,
                hits: 2,
                strokes: 9,
                yield_ml: 10,
                full: i % 3 == 0,
            });
        }
        assert_eq!(chart.len(), creamery::CHART_CAP);
        assert_eq!(chart.dropped, 6);
        assert_eq!(chart.total_yield_ml(), 240);
        // Queued preset applies only at the boundary.
        let mut q = creamery::PresetQueue {
            active: 1,
            pending: None,
        };
        q.queue(3);
        assert_eq!(q.active, 1);
        assert_eq!(q.apply_at_boundary(), 3);
        assert_eq!(q.apply_at_boundary(), 3);
    }

    #[test]
    fn blush_curve_stack_and_gaze_rule_order() {
        // Cubic: exact endpoints, monotone, saturating.
        assert_eq!(blushline::response(0.0), 0.0);
        assert_eq!(blushline::response(1.0), 1.0);
        let mut prev = 0.0;
        for i in 0..=100 {
            let v = blushline::response(i as f32 / 100.0);
            assert!(v >= prev && v <= 1.0);
            prev = v;
        }
        // Accumulator: 0.16 gain, clamped; negative stimulus ignored.
        assert!((blushline::accumulate(0.0, 1.0) - 0.16).abs() < 1e-6);
        assert_eq!(blushline::accumulate(0.95, 10.0), 1.0);
        assert_eq!(blushline::accumulate(0.5, -3.0), 0.5);
        // Rise is 4x fall: from 0 to 1 takes 32 steps up, 128 down.
        let mut v = 0.0;
        let mut steps = 0;
        while v < 1.0 {
            v = blushline::step_toward(v, 1.0);
            steps += 1;
        }
        assert_eq!(steps, 32);
        let mut steps = 0;
        while v > 0.0 {
            v = blushline::step_toward(v, 0.0);
            steps += 1;
        }
        assert_eq!(steps, 128);
        // Tiers inclusive at the bottom.
        assert_eq!(blushline::tier_of(0.24), 0);
        assert_eq!(blushline::tier_of(0.25), 1);
        assert_eq!(blushline::tier_of(0.55), 2);
        // Gaze: downcast is judged FIRST (0.45 down at any sideways
        // offset), toward is inclusive at 0.30, unreadable geometry
        // is INVALID, never a verdict.
        assert_eq!(
            blushline::gaze_verdict(0.9, 0.45, 1.0),
            blushline::GazeVerdict::Downcast
        );
        assert_eq!(
            blushline::gaze_verdict(0.3, 0.0, 1.0),
            blushline::GazeVerdict::Toward
        );
        assert_eq!(
            blushline::gaze_verdict(0.31, 0.0, 1.0),
            blushline::GazeVerdict::Away
        );
        assert_eq!(
            blushline::gaze_verdict(0.0, 0.0, 0.0),
            blushline::GazeVerdict::Invalid
        );
        assert_eq!(
            blushline::gaze_verdict(f32::NAN, 0.0, 1.0),
            blushline::GazeVerdict::Invalid
        );
    }

    #[test]
    fn fluid_timeline_and_exactly_once_attributed_culmination() {
        let mut tl = milkmaid::Timeline::new();
        for step in 0..=300u32 {
            tl.book(step, 2);
        }
        // Samples land every 30 steps: steps 0,30,...,300 = 11.
        assert_eq!(tl.samples, 11);
        assert_eq!(tl.latest(), 602);
        assert_eq!(tl.ledger_ml, 602);
        // Exactly-once: first fire records the active method; a
        // later fire under a switched method changes nothing.
        let mut latch = milkmaid::CulminationLatch::new();
        let ev = latch.fire(2, [1, 2, 3, 4], [10, 20, 30, 40]);
        assert_eq!(ev.map(|e| e.method_id), Some(2));
        assert_eq!(latch.fire(0, [9, 9, 9, 9], [0, 0, 0, 0]), None);
        assert_eq!(latch.event.map(|e| e.counts), Some([1, 2, 3, 4]));
        // Save split: settings + aggregates persist; rounds do not.
        assert!(milkmaid::persists_in_shell(milkmaid::SaveItem::Setting));
        assert!(milkmaid::persists_in_shell(
            milkmaid::SaveItem::LifetimeAggregate
        ));
        assert!(!milkmaid::persists_in_shell(milkmaid::SaveItem::RoundState));
    }

    #[test]
    fn crescendo_tempos_quantisation_and_edge_law() {
        // The filed step table, exactly: 250/250, 438/375, 688/563,
        // 1000/750.
        let steps: Vec<(u32, u32)> = crescendo::TIER_TEMPOS_MS
            .iter()
            .map(|&(r, h)| (crescendo::steps_for(r), crescendo::steps_for(h)))
            .collect();
        assert_eq!(steps, vec![(250, 250), (438, 375), (688, 563), (1000, 750)]);
        assert_eq!(crescendo::DAZE_MS, 2 * crescendo::PEAK_MS);
        // Edge: strain limit, slip halves the rise, 3 slips fail.
        assert_eq!(crescendo::EDGE_STRAIN_LIMIT_MS, 1200);
        assert_eq!(crescendo::slip(800), 400);
        assert!(!crescendo::run_failed(2));
        assert!(crescendo::run_failed(3));
        // Release decay is dt/4.
        assert_eq!(crescendo::decay_per_step(16), 4);
        // Clocks run at roughly 2x the perfect run or better.
        for &(perfect, clock) in crescendo::SESSION_CLOCKS_MS.iter() {
            assert!(clock * 10 >= perfect * 19);
        }
        // Weather: total over phases, peak/daze distinct.
        assert_eq!(crescendo::weather_index(crescendo::Phase::Rise, 1), 0);
        assert_eq!(crescendo::weather_index(crescendo::Phase::Edge, 4), 3);
        assert_eq!(crescendo::weather_index(crescendo::Phase::Peak, 2), 4);
        assert_eq!(crescendo::weather_index(crescendo::Phase::Daze, 2), 5);
        // Culmination: target-tier edge only, latched.
        assert!(crescendo::culmination_fires(2, 2, false));
        assert!(!crescendo::culmination_fires(1, 2, false));
        assert!(!crescendo::culmination_fires(2, 2, true));
    }

    #[test]
    fn second_wave_continuation_curves_and_residue() {
        // Reduced curve, floored.
        assert_eq!(second_wave::reduced_rise(1000), 550);
        assert_eq!(second_wave::reduced_decay(1000), 600);
        assert_eq!(second_wave::reduced_rise(3), 1);
        // Sensitivity: seed, rates, caps (plain and assist).
        let mut s = second_wave::SENSITIVITY_SEED_MILLI;
        s = second_wave::sensitivity_step(s, true, false);
        assert_eq!(s, 305);
        s = second_wave::sensitivity_step(s, false, false);
        assert_eq!(s, 302);
        assert_eq!(second_wave::sensitivity_step(999, true, false), 1000);
        assert_eq!(second_wave::sensitivity_step(1249, true, true), 1250);
        assert_eq!(second_wave::sensitivity_step(2, false, false), 0);
        // Peak band edges are inclusive.
        assert!(second_wave::peak_in_band(720, 1000));
        assert!(second_wave::peak_in_band(860, 1000));
        assert!(!second_wave::peak_in_band(719, 1000));
        assert!(!second_wave::peak_in_band(861, 1000));
        // Residue + floor per outcome; stopped gets NO floor.
        assert_eq!(
            second_wave::residue_start_milli(800, second_wave::Outcome::ContinuationSuccess),
            410
        );
        assert_eq!(
            second_wave::residue_start_milli(800, second_wave::Outcome::Settled),
            240
        );
        assert_eq!(
            second_wave::residue_start_milli(800, second_wave::Outcome::Stopped),
            120
        );
        assert_eq!(
            second_wave::residue_start_milli(800, second_wave::Outcome::Faded),
            80
        );
        // Exactly one continuation.
        let mut latch = second_wave::ContinuationLatch::new();
        assert!(latch.take());
        assert!(!latch.take());
    }

    #[test]
    fn redlight_reaction_sizing_and_attention_floor() {
        assert!(redlight::set_is_session_sized(6));
        assert!(!redlight::set_is_session_sized(5));
        // Last-line exclusion: every roll maps off the last line,
        // and every other line is reachable.
        for last in 0..6u32 {
            let mut seen = [false; 6];
            for roll in 0..5u32 {
                let pick = redlight::pick_line(6, Some(last), roll).unwrap();
                assert_ne!(pick, last);
                seen[pick as usize] = true;
            }
            assert_eq!(seen.iter().filter(|&&s| s).count(), 5);
        }
        assert_eq!(redlight::pick_line(0, None, 0), None);
        assert_eq!(redlight::pick_line(1, Some(0), 7), Some(0));
        // Attention: decay stops at the floor; only praise raises it.
        let mut a = redlight::Attention::new();
        a.gain(200);
        for _ in 0..10_000 {
            a.decay_step(2);
        }
        assert_eq!(a.value_milli(), 0);
        a.praise();
        assert_eq!(a.floor_milli, 50);
        assert_eq!(a.value_milli(), 35);
        for _ in 0..10_000 {
            a.decay_step(0);
        }
        assert_eq!(a.value_milli(), 50); // the floor holds
        for _ in 0..20 {
            a.praise();
        }
        assert_eq!(a.floor_milli, redlight::PRAISE_FLOOR_CAP_MILLI);
    }

    #[test]
    fn build_up_drill_gates_from_the_quartermaster_adaptation() {
        assert!(build_up::drill_earns_step(0, 550));
        assert!(!build_up::drill_earns_step(0, 549));
        assert!(build_up::drill_earns_step(3, 700));
        assert!(!build_up::drill_earns_step(4, 1000)); // no such rank
        assert!(!build_up::drill_faults_fail(2));
        assert!(build_up::drill_faults_fail(3));
    }

    #[test]
    fn t_gate_arithmetic() {
        assert_eq!(standard_issue::tier(0), 0);
        assert_eq!(standard_issue::tier(standard_issue::CERT_FIT), 1);
        assert_eq!(standard_issue::tier(standard_issue::CERT_SIGNALS), 1);
        assert_eq!(
            standard_issue::tier(standard_issue::CERT_FIT | standard_issue::CERT_SIGNALS),
            2
        );
    }
}
