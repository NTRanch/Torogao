# Torogao Bench

The estate's hand-animation bench for Blender 5.3: a Blender
extension whose panel drives the round-trip converter between
Blender Actions and `torogao-motion-clip/2` documents, pose-asset
import/export on the same converter, an evaluated NLA flatten,
and a thin NLA layer front over Blender's native NLA data.

Hand animation on this bench authors the D-1397 scene spec
(`torogao-motion-clip/2`) directly — there is no bypass format.
The bench writes *working* documents; canonicalisation and
freezing belong to the estate gate tool (`motion-clip-check`,
`crates/torogao-format`), whose compiler stamp of record is
`torogao-format motion_clip h0 (2026-11-27)`, band `tier2-1e-6`.

Adopted stack (Owner's tooling ruling, 2026-11-27 00:20 PT):
Blender 5.3 core + the bundled Copy Global Transform + free
AnimAll + free Commotion. No paid animation-layers add-on: in
Blender 5.3 the NLA is the layering surface, and the bench's
strip list is a front over that native data — it adds no layer
format and no evaluation semantics of its own.

## Layout

- `converter.py` — the converter (below) and the evaluated read.
- `clipmath.py` — Appendix C evaluation replicated operation for
  operation from `crates/torogao-runtime/src/clip_eval.rs`,
  including the Tier 2 digest; pure Python, no Blender imports.
  The H2 gate pins it against the Rust evaluator's golden
  digests before trusting it as an instrument.
- `clipdoc.py` — document discipline: rig identity (SHA-256 of
  the hierarchy definition's bytes), the op log, channel
  assembly rules the validator will re-check at freeze time.
- `census.py` — a plain-Python GLB census in the fleet A10.2
  instrument's shape: joints, morph targets, and per-animation
  sampler key counts and time spans.
- `nla_front.py` — the strip list: add / select / mute / solo /
  reorder (track order and strip-to-adjacent-track), blend mode
  and influence editing, all write-through to native strips.
- `panel.py` — the bench panel and file operators.
- `gates/` — the rung gates (headless): `h2_gate.py` (the H2
  gate suite) and `h2_render.py` (the twin-render spot check),
  with the H2 golden clip and gate report under `gates/golden/`.

## Binding law

From the estate binders that froze Lana's clips
(`bind_rung1.py` / `bind_proof.py`, rung-1 lane) and FORMATS.md:

- a joint **rotation** value *is* the pose bone's local rotation
  quaternion (a delta from rest), stored `[x, y, z, w]`;
- the root joint's **translation** channel holds absolute glTF
  node translations; a pose bone location binds as a delta from
  the rest translation, converted glTF → Blender as `(x, -z, y)`
  and rotated by the inverse of the bone's `matrix_local` 3×3.
  The converter inverts exactly that, reading the rest
  translation from the rig's GLB of record. Translation channels
  are bound for the root joint only; keyed locations on any
  other joint are a named refusal, because no estate binder
  reads them;
- **morph** channels are shape key values by target name;
  **parameter** channels are not Blender-authored — they ride
  the round-trip stash untouched.

One discrepancy is named, not hidden: the rung-1 binders bound
Hips translations against an *authored* frame-0 constant
(`[0, 0.98, 0]`), while Lana's GLB node translation for Hips is
`[0, 0.9410714507102966, 0]`. The bench binds against the GLB of
record — self-consistent within the bench and with the runtime,
which applies clip translations as node translations — so a
clip moved between the two conventions carries the constant's
difference (~3.9 cm) in its root translation only. Rotations
and morphs are unaffected.

## Curve law

- Bézier segments convert to cubic Hermite parametrically
  (DESIGN §3.2): `m0 = 3·(P1.y − P0.y)/Δf`,
  `m1 = 3·(P3.y − P2.y)/Δf`. Handle frame placement is not clip
  data; the converter writes handles back at the segment
  thirds, where Blender's graph evaluation is the same curve.
  Blender's AUTO / AUTO_CLAMPED handles already sit at the
  thirds, and `settle_handles_thirds` normalises hand-dragged
  free handles to the same placement.
- Where the two curve languages genuinely differ, the
  converter **samples the segment to per-frame linear keys**
  and names the sampling in the op log. Values at every frame
  stay exact in both languages. The sampled cases:
  - a flat Bézier handle on a rotation channel (clamped end
    keys, held poses, single-axis extrema) yields a zero
    quaternion tangent, which the format refuses
    (`TG-CLIP-ZERO-QUATERNION`);
  - linear rotation segments: the clip's linear is slerp
    (Appendix C), an F-Curve's linear is componentwise —
    sampled in *both* directions;
  - easing-family interpolations (DESIGN §3.4: edit-time
    shapes) and channels whose components key unevenly.
- A missing `key_type` is the default keyframe; non-default
  types (breakdown / moving_hold / extreme) round-trip
  strictly.

## The round-trip rule

A clip imported to an action carries its source document as a
stash on the action. On export, if the regenerated document
matches the stash — same structure, values within the Tier 2
band (an F-Curve stores f32; a round trip rounds at ~1e-7),
linear rotation segments compared after slerp expansion — the
source is re-emitted verbatim, so **clip → action → clip is
byte-identical after canonicalisation**. Any real edit breaks
the match and the export becomes a new revision: the source's
op log plus the converter's entries for the import and the
export.

## Running the H2 gate

```sh
# on primo, official Blender build (the Arch build segfaults
# on IK-carrying files), from the torogao worktree:
cargo build -p torogao-format --bin motion-clip-check
~/.cache/azoth/blender-official/blender-5.3.0-linux-x64/blender \
  -b -Y --offline-mode -noaudio \
  -P tools/blender/torogao_bench/gates/h2_gate.py -- \
  <lana.glb> <lana-reaction-hierarchy.json> \
  crates/torogao-format/tests/golden/motion-clip-2 \
  <workdir> target/debug/motion-clip-check
~/.cache/azoth/blender-official/blender-5.3.0-linux-x64/blender \
  -b -Y --offline-mode -noaudio \
  -P tools/blender/torogao_bench/gates/h2_render.py -- \
  <workdir>/h2_witness.glb <workdir>/render
```

Witness rig: Lana's rung-1 cured rig (`lana.glb`,
sha256 `78ef1c6e…`, A10.1 PASS, A10.2 PASS), hierarchy
definition `lana-reaction-hierarchy.json` (rig identity
`3851eebd…`). The gate's test clip — bend-battery poses plus a
short wave, authored by the gate session as an agent session —
is banked frozen under `gates/golden/`.
