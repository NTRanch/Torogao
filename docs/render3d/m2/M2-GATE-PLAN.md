# Rung 2 / M2 — gate plan (DRAFT for Pax gate review)

**This is a draft. Nothing in it is built, run, or ratified.**
Every numeric bound below is a **proposal** (brief Appendix B
item 7): it becomes a stated-before-the-run bound only when the
module's first gate is opened under it. Building starts on a
later, explicit gate pass. No Owner PASS is declared anywhere.

Module order as ruled (D-1250): **M2.1 Vivienne first**, then
**M2.2 the scene system**. Standing rulings, not reopened here:
D-1247 (Prima bust weight 1, applied at render time), D-1248
(residual bounds unrevised; a crossing is stop-and-report).

This file is Markdown: **not-LSP-validated**.

## 1. M1 state as gated (read from disk 2026-11-10)

Evidence root `~/.local/share/pax/torogao-renderer-rung2-20261110/m1/`
(`m1/`). Branch tip at drafting: `b6944af`.

| Item | State | Evidence |
|---|---|---|
| M1 report | committed `9ec6542` — **stale on one point**: it says the GPU gates are unread; the table below supersedes it | `docs/render3d/m1/M1-REPORT.md` |
| Cast loader + gates | committed `d3f1d4a` | — |
| Clippy fix (debt from `7b9297b`) | committed `b5815db`; all-targets `-D warnings` passes | — |
| `registry_data.rs` | committed by Pax, `b6944af` | — |
| Proving suite, all 17 (`--ignored`) | **17 passed** on the A6000 | `m1/proving-suite.log`, `m1/PROVING-DONE` |
| GPU queue | complete, no stop | `m1/gpu-queue.log`, `m1/GPU-QUEUE-DONE` |
| Theodolite | **REVIEW** — control SEEN, final reads still carry incompleteness markers | `~/.local/share/pax/trio-m1-20261110/theo-verdict.md` |

Per-girl GPU gates (bounds: positions ≤ 1e-4 m, normals ≤ 1e-3,
frames 0/90/180/225/270/359; twins 360/360):

| Girl | Bust weight | Max position Δ m | Max normal Δ | Twin frames identical | Distinct frames | `GPU-DONE` |
|---|---|---|---|---|---|---|
| Prima | 1 (D-1247) | 2.384e-7 | 1.788e-7 | 360/360 | 346 | yes |
| Alma | as saved (0) | 2.384e-7 | 1.788e-7 | 360/360 | 335 | yes |
| Lux | as saved (0) | 2.384e-7 | 1.788e-7 | 360/360 | 352 | yes |
| Piper | as saved (0) | 2.384e-7 | 1.788e-7 | 360/360 | 350 | yes |
| Sloane | as saved (0) | 2.384e-7 | 1.788e-7 | 360/360 | 346 | yes |
| Yui | as saved (0) | 2.384e-7 | 1.788e-7 | 360/360 | 349 | yes |

Source: `m1/<girl>/crosscheck.json`, `twin-compare.json`.

One line of scrutiny, stated rather than explained away: the two
maxima are **the same value for all six girls**. 2.384e-7 is
2⁻²², one f32 step for a coordinate between 1 and 2 m, so a
shared quantum is a plausible reading — but it is a reading, not
a measurement. It is listed in §2 as carried over.

### 2. Carried over from M1 (open — never assumed closed)

1. **Theodolite verified zero** on the M1 Rust files: REVIEW.
2. **Identical crosscheck maxima across six girls**: confirm the
   f32-quantum reading with a per-vertex histogram, or find the
   real cause. Cheap; belongs at the start of M2.1.
3. **Yui clip provenance**: cause not fully determined (ruled
   exception in force).
4. **No visual verdict pack** was assembled in M1; the twin
   renders are determinism evidence only.
5. **Rung-1 path renders Prima at bust weight 0**
   (`arc.rs` `PLATE_BUST_WEIGHT`), while D-1247 applies 1 on the
   cast path. Named for the gate; not reopened by this plan.
   M2.2's regression gate (§4, G2) fixes which path it compares.
6. **M1 report's GPU section is stale** (item above). Needs a
   one-paragraph addendum or this table accepted as the record.
7. **Family B intake** — was not started in M1; it is M2.1.

## 3. M2.1 — Vivienne (Family B enters the renderer)

### 3.1 Her census, re-read from the artifact

`~/.local/share/pax/strobe-smoothing-20261110/vivienne/vivienne.glb`,
sha256 `94abe339…`, 30,201,216 bytes (the `gate/` copy and
`vivienne_twin.glb` are byte-identical to it). Read from the glTF
JSON at drafting:

| Fact | Value |
|---|---|
| Nodes / skins / joints / animations | 59 / 1 / 54 / 0 |
| Vertices | Body 93,806 · Hair 31,906 · Outfit 23,053 · OutfitEnd 20,716 = **169,481** |
| Morph targets | Body **36**; Hair, Outfit, OutfitEnd **0** |
| Attributes, every primitive | POSITION, NORMAL, TEXCOORD_0, COLOR_0, COLOR_1, JOINTS_0, WEIGHTS_0 |
| Images | 4 embedded PNG atlases (Body, Hair, Outfit, OutfitEnd), one per material |
| Materials | 4 × baseColorTexture, metallic 0, roughness 0.75, double-sided, no alphaMode |
| Sampler | one: mag LINEAR (9729), min LINEAR_MIPMAP_LINEAR (9987) |
| Sparse accessors | 72 of 105 |

Differences from Family A that the M1 code does not cover — each
is a design point, not an assumption:

- **No `Body_Bust_Size`** and **no outfit morph targets** (the
  cast carries 37 + 1 + 1). The loader's bust and outfit-target
  expectations must not be required for her.
- **COLOR_1 is present beside a texture.** M1's rule
  (`paint_from_color1` when COLOR_1 exists) must be re-measured
  for atlas materials; the brief forbids assuming it transfers.
- **The sampler asks for mipmaps.** Mip generation and filtering
  are where a GPU can stop being bit-reproducible. This is the
  main determinism risk of the module.
- **The D-1206 pack says "37 keys"; the GLB carries 36 targets.**
  Presumably the 37th is the Blender Basis key. To be confirmed
  from her blend before any morph gate cites a count — REVIEW.

### 3.2 Supplies M2.1 needs before its first gate (clarification)

Not found on disk at drafting — **each is a question for the
gate, not something to improvise**:

- **A Vivienne clip + skeleton def + channels** (the M1 loader
  reads `<slug>.clip.json`, `-skeleton.json`, `-channels.json`).
  No such files were found under her evidence tree or the cast
  reference tree. Either they are generated (which show, which
  generator version?) or M2.1's pose gates run on **rest pose +
  morph states only**. This plan is written for the second
  reading where it matters and marks the gates that need a clip.
- **Her Blender witness set**: which blend (her cured blend is
  named in `input_census.json`), which vertices, which states.
  Rung 1 used four strain-witness vertices; for 93,806 Body
  vertices the plan proposes a full-mesh witness (G4).
- **The plate rig for her** — plate generation named (v1 512×640
  or plate-spec v2), and whether her D-1206 compositions carry
  camera data or must be matched by eye (the latter is not a
  gate).

### 3.3 Gates (proposed bounds; fresh, not inherited from M1)

| # | Gate | Proposed bound | Basis / status |
|---|---|---|---|
| G1 | Census and loader parity: loader counts vs an independent glTF-JSON read | **exact**: 54 joints, 59 nodes, 4 primitives, per-mesh vertex and target counts of §3.1, 169,481 total | same instrument as M1 (`m1/census_parity.py`) |
| G2 | Atlas identity, file level: SHA-256 of each embedded PNG's bytes equals the texture-cure file of record | **exact**, 4 of 4 | hashes recorded at the gate; mismatch = provenance stop |
| G3 | Atlas identity, decoded level: SHA-256 of decoded RGBA8 (2048×2048×4) from the renderer's decoder equals an independent decoder's | **exact**, 4 of 4 | proves the decode, not just the bytes |
| G4 | Morph parity vs Blender, rest pose: for each of the 36 targets at weight 1, renderer CPU reference vs Blender evaluated mesh, all Body vertices | **≤ 1e-5 m** max | proposal. Motivated by the pack's "morph fidelity worst 7.2e-07 m" (a different comparison — subdivided deltas — so it informs, it does not transfer). REVIEW until first measured |
| G5 | Skinning parity vs Blender (needs a pose: clip or a stated test pose) | **≤ 1e-4 m** max over all vertices of all four meshes | proposal; rung-1's witness bound class. Needs §3.2 supply |
| G6 | GPU-vs-CPU crosscheck, all four meshes (M1 checked Body only) | positions **≤ 1e-4 m**, normals **≤ 1e-3** | proposal: same numbers as M1 but **re-proposed**, on 12× the vertices and with 72 sparse accessors |
| G7 | UV path: GPU-sampled base colour vs CPU reference sampling the same decoded atlas at the same UV, at a fixed set of texel-centre probes, mip 0 | **exact** RGBA8 at texel centres | proposal. Texel-centre probes remove filtering from the question, so exact is achievable |
| G8 | Filtered sampling: twin-run identity of full frames (see G9); CPU agreement of filtered samples is **not gated** | — | filtering arithmetic is implementation-defined; gating it against a CPU model would manufacture a bound |
| G9 | Twin-render determinism under the plate rig | **100% frame SHA-256 identical**, two fresh processes; plus a third run after a device re-create | the third run is new: it targets mip generation, the risk named in §3.1 |
| G10 | Paint rule for atlas materials: measured, written down, then tested | a written rule + a test that fails if COLOR_1 is applied where the measurement says it must not be | brief M1 item 3 ("measure per family") |
| G11 | Verdict pack reproducing her D-1206 compositions in-engine, beside the smoothed sheets of record | prepared for the Owner's eye; **no agent verdict** | expected-vs-seen, crops from the final frames at viewing size, an honest FAIL exemplar per item |
| G12 | Adversarial loader set: missing image, truncated PNG, non-PNG mime, texture index out of range, wrong atlas size, UV accessor of wrong type | each a typed error, no partial model | half of the module's tests, per the estate split |

Memory bound to state before G6/G9 run: decoded atlases
4 × 2048² × 4 B = 67.1 MB at mip 0 (≈ 89.5 MB with a full mip
chain), matching the brief's ≈ 67 MB figure; a proposed upload
budget is set at the gate from a measured allocation, not here.

### 3.4 Design decisions the first gate must rule on

- **Mipmaps**: (a) generate on CPU with a fixed, documented box
  filter and upload every level — deterministic by construction,
  recommended; (b) GPU blit generation — simpler, reproducibility
  unproven; (c) mip 0 only — deterministic but aliases at plate
  scale. Recommendation (a); cost is one small CPU routine and
  its test.
- **Colour space**: glTF base-colour textures are sRGB-encoded;
  the decision is an sRGB image format (hardware decode) versus
  decoding in the shader. Either is fine; it must be one, stated,
  and G7 must probe through it.
- **Backend boundary**: atlases cross as plain-data packets
  (dimensions + RGBA8 bytes + per-material index). The only
  `unsafe` stays in `src/backend/vulkan.rs`.

### 3.5 Stop conditions (M2.1)

Any proposed bound crossed once ratified; atlas hash mismatch
(G2/G3); census mismatch (G1); twin or third-run hash difference
(G9); a supply of §3.2 missing when its gate is reached; the
paint-rule measurement contradicting itself between meshes.
Stop, land the evidence, report as measured.

## 4. M2.2 — scene system (brief §4 M2)

New crate `torogao-scene` (`#![forbid(unsafe_code)]`). Order:

1. **Format first.** `DESIGN.md` for `torogao-scene/1` (RON,
   versioned from birth, fail-closed on unknown fields) lands and
   is reviewed **before** any loader code.
2. Loader + typed errors. 3. Timeline evaluation at 60 Hz into
   the existing packets, extended to N characters. 4. Camera
   paths, with a plate-camera preset. 5. Lights beyond the plate
   pair and the SH ambient packet.

What M1 already supplies, to be reused rather than rebuilt:
`plate_rig_scene(camera_world)` (camera as data),
`DrawFrame.include_set` (set flag gating both the shadow-depth
and colour passes), `cast::load_cast_inputs` and `OutfitPlan`,
and the per-girl replay hashes pinned in `tests/proving.rs`.

Gates — the brief's five, restated with their numbers:

| # | Gate | Bound |
|---|---|---|
| S1 | Two-character scene file: both posed by their own clips; twin runs hash-identical; each character's replay hash equals her solo M1 hash at shared frames | exact, both |
| S2 | Prima arc as a scene file reproduces the rung-1 replay hash | exact: `1bd9dd534a0ea37f8e827e5d9e7fcdcac3d8cebfd53401c3cd05b9be46e986eb` |
| S3 | Camera path: keyframe poses reproduced at keys; interpolated frames deterministic across twin runs | f32-exact at keys; 100% twin identity between |
| S4 | Adversarial loader: unknown version, unknown field, dangling character/clip reference, empty timeline | each a typed error, no partial scene |
| S5 | `DESIGN.md` precedes the loader | commit order shows it |

Proposed additions (proposals, for the gate to accept or strike):

- **S6 — frame regression.** S2 fixes *evaluation* (the replay
  hash). A rendered-frame comparison of scene-file Prima against
  the hardcoded arc path (frame SHA-256, 360/360) would fix
  *drawing* too. It must name the bust weight it uses, because of
  carried-over item 5.
- **S7 — SH ambient reduces to the plate world colour at l = 0**:
  frames hash-identical to the flat-colour path. The brief states
  the reduction; this makes it a test.
- **S8 — N-character packet bound**: a stated maximum character
  count and joint-matrix budget, rejected with a typed error
  above it.

Remaining M2 scope in the brief's order after the format and
loader: timeline, camera paths, lighting rigs. Crate pins (RON,
serde) are taken from docs.rs at pickup and cited (Appendix B
item 5).

Stop conditions (M2.2): S2 hash miss; any composition effect on
a solo replay hash (S1); a format question `DESIGN.md` does not
answer; a need for `unsafe` outside the backend file.

## 5. Clauses in force for any build step

- **Theodolite clause** (brief §7): producer first —
  `.bacon-locations` current when the session starts; a planted
  control in the same session; a zero counts only from a read
  without incompleteness markers. M1 ended at REVIEW on exactly
  this; M2 should open by settling how a quiescent read is
  obtained, so the same REVIEW is not reproduced.
- **Bloch clause**: every lesson carries `origin_lane` +
  `content_label` + `evidence` from birth; this tree is the
  NTRanch lane; unclassified defaults to the restrictive label.
- Determinism (D-212), the backend boundary, no clip evaluation
  in the renderer, tests half validation / half adversarial,
  local commits only.
- Clean-room law for anything informed by held titles.

## 6. Close-out shape, per sub-module

One report each for M2.1 and M2.2: gate table against every
stated bound with the artifact path per number; hashes; what
remains unproven; kept files; skill outcomes; tree state.
Anything unverifiable is REVIEW.

## 7. Questions for the gate (specific)

1. Does Vivienne get a clip and def for M2.1 (which show and
   generator), or do G5/G9 run on stated test poses?
2. Which blend and which states form her Blender witness?
3. Which plate generation and camera for her pack (G11)?
4. Mipmap route: is recommendation (a) accepted?
5. Is this document's §1 table accepted as M1's GPU record, or
   is an addendum to `M1-REPORT.md` wanted?
