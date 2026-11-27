# Bundle reader rung 2 — IL2CPP stripped-tree MonoBehaviour recovery (D-1222)

**Verdict: COMPLETE.** The rung-1 reader now recovers stripped
MonoBehaviours by joining MonoScript identity to the estate
`il2cpp-index` field layouts, synthesizing type trees, and
decoding through the rung-1 value decoder. Proof on both held
titles: **PES 1,822 exact / 0 partial / 1 review of 1,823**
(99.95% exact) and **Natsuki 17,535 exact / 8 partial / 127
review of 17,670** (99.24% exact). Cross-check vs AssetRipper
fixtures: every MonoBehaviour value on PES matches exactly;
on Natsuki there are **zero unexplained value mismatches**.
All numbers below cite run artifacts in the rung-2 workdir
(primo `~/.local/share/pax/bundle-native-rung2-20261110/`).

Clean-room law held throughout: implementation from public
format documentation, observed bytes of the held titles, and
estate artifacts (il2cpp-index outputs) only. AssetRipper was
used as a tool on RE material; its exports are test fixtures
(data), never consulted as code. Both titles are S2c
EXCLUDED: values only, no assets/text/character material
extracted or shipped. Local work only; no pushes, no commits
to shared repos.

## What was built

- **`layout` verb for il2cpp-index** (`LayoutBuilder.cs` +
  CommandLine dispatch): emits an `il2cpp-layout 2` document —
  per-type kind / serializable bit / enum underlying / base
  expression, and per-field F/S/R records (serialized /
  skipped-with-reason / undecidable-with-reason). Index JSONs
  regenerated with the patched tool are byte-identical to the
  pre-patch runs (the verb is purely additive). The decisive
  detail: per-field Cpp2IL attribute analysis must run before
  [SerializeField]/[SerializeReference] queries return ground
  truth.
- **Rust join layer** in bundle-native: `src/layout.rs`
  (fail-closed layout parser + metadata stamp verification —
  the join refuses a metadata file whose magic, version, or
  size does not match the layout document), `src/monoscript.rs`
  (MonoScript layout B decoder, exact-consumption or None),
  `src/synth.rs` (type-tree synthesis: inheritance flattening
  with generic-environment substitution, `ref`/`blk` generic
  arguments, evidence-derived engine-native shapes, preamble
  byte-verification, missing-script preamble-only trees),
  `src/join.rs` (`join-census` / `join-values` verbs). Design
  record: mirror `DESIGN.md`, rung-2 section.
- **Crate fixes along the way:** Unity 6000.3 zero-node
  type-tree blobs are legal "no tree"; reference-type entries
  measure 39 bytes vs the documented 35 (parser accepts both
  paddings; the ambiguity is recorded as a REVIEW note in
  DESIGN.md — the pad sits in an all-zero region and its
  position is unidentifiable); the value decoder's budget is
  now `clamp(4 × object bytes, 2M, 2^28)` after an 8.7 MB
  cloth object died at exactly the old flat 2M cap.

## Per-title results

### Personality Excretion Simulator (QUEUE #28, JinkakuHaisetsu)

Unity 2022.3.22f1, SerializedFile v22, metadata v29.1. One
bundle (`data.unity3d`), 7 serialized files, 1,823
MonoBehaviours, 1,253 MonoScripts; il2cpp-index: 7,101 types /
63,672 methods / 41,868 fields.

- Before (rung 1): all 1,823 stripped MonoBehaviours REVIEW
  (`evidence/census-pes-before.txt`).
- After: **1,822 exact / 0 partial / 1 review**
  (`evidence/det-pes-1.txt`, determinism pair byte-identical).
- Cross-check (`evidence/differ-pes-v3.txt`): compared 1,847
  (1,822 MB + 25 TextAssets) — **MonoBehaviours 1,822/1,822
  exact, zero name diffs, zero value mismatches.** The 5
  non-exact records are TextAsset length diffs proven to be a
  fixture-side artifact: AssetRipper renders binary `m_Script`
  payloads as lossy strings, inflating its lengths; our side
  is byte-proven by exact object consumption
  (`evidence/textasset-length-proof.txt`). The 1 theirs_only
  record is the GUISkin review below.

### Complicated Natsuki and 3D Lewd Touching (QUEUE #26, v1.25)

Unity 6000.3.14f1, SerializedFile v22, metadata v39. Extracted
to the rung-2 workdir (held original untouched). 31 serialized
files, 73,357 objects, 17,670 MonoBehaviours, 3,777
MonoScripts; il2cpp-index: 17,445 types / 131,007 methods /
80,074 fields.

- Before: all 17,670 REVIEW (`evidence/census-natsuki-before.txt`).
- After: **17,535 exact / 8 partial / 127 review**
  (`evidence/det-natsuki-1.txt`, determinism pair byte-identical).
- Cross-check (`evidence/differ-natsuki-v2.txt`): compared
  17,551 — exact 15,678; name_diff 1,869 records, all
  presentation-only (Color32: fixtures render one packed
  `rgba` uint32, we split r/g/b/a channels; Matrix4x4 element
  names m00.. vs e00..; the 8 partials carry an opaque marker
  where fixtures split a byte block; 2 StyleSheets where we
  decode MORE named fields than the fixture export shows,
  values agreeing wherever both exist); theirs_only 127 =
  exactly our 127 reviews; **value_mismatch 4, all
  `m_AsteriskChar` in UnityEngine.UI.InputField: ours 42,
  theirs "*" — the same character, presentation only. Zero
  unexplained value mismatches.**

## Gates

- **cargo test: 36/36** — rung-1's 18 kept green (validation
  8, corpus 4, adversarial 6) + 18 new join tests
  (join_validation 7, join_adversarial 11): layout parse +
  lookups, MonoScript identity, end-to-end synthesis through
  generic bases (ref-to-Object → PPtr; blocked argument unused
  by the definition decodes — the UnityEvent shape), missing-
  script preamble-only; adversarial: bad header/meta, truncated
  or trailing layout, out-of-range type indices, metadata
  stamp mismatch (magic/version/size), truncated/dirty
  MonoScript, R-field refusal, non-Object ref refusal, blocked
  argument in field position refusal, trailing-byte review.
- **clippy** `--all-targets -- -D warnings`: 0 errors.
  **fmt** `--check`: clean.
- **Determinism:** join-census run twice per title;
  byte-identical outputs (`evidence/det-pes-{1,2}.txt`,
  `evidence/det-natsuki-{1,2}.txt`), totals matching the
  recorded final censuses.
- **Theodolite (D-1124 cured protocol).** Rust: producer
  (`bacon --headless -j bacon-ls`) run before each session so
  a current single-line `.bacon-locations` existed at session
  start. Session A with a planted E0308 control
  (`src/control_scratch.rs`): control reported 2 severity-1
  diagnostics in-session; `src/layout.rs`, `src/monoscript.rs`,
  `src/synth.rs`, `src/join.rs`, `src/value.rs` all 0
  (`evidence/theodolite-rung2-session-a.txt`). Session B
  (control removed, locations regenerated): all five files 0
  (`evidence/theodolite-rung2-session-b.txt`); lib.rs restored
  md5-identical. C# (`LayoutBuilder.cs`, csharp-language-server):
  the route serves a pre-load snapshot first, so single-shot
  probes are not evidence — a planted CS0029 in a scratch file
  surfaced as severity-1 at 23 s in-session once the solution
  loaded (`evidence/theodolite-rung2-csharp-poll.txt`), and
  LayoutBuilder.cs read post-load in-session shows **zero
  error diagnostics**, stable across ten post-load reads over
  ~200 s: 2 × IDE0057 (severity-3 style) only
  (`evidence/theodolite-rung2-csharp-baseline-poll.txt`). The
  severity-4 IDE0005 hint seen pre-load was a snapshot
  artifact — post-load it is gone, and the compiler agrees:
  every using directive proved load-bearing under removal
  attempts, and `dotnet build` of il2cpp-index succeeds with
  0 errors.
- **Mirror:** primo crate ↔ `~/workspace/bundle-native-20261110/mirror/`
  md5-verified file-for-file (src, tests, Cargo.toml/lock,
  DESIGN.md, bacon.toml).

## REVIEW list — what still cannot be decoded, and why

PES (1):
- **GUISkin** ("unity default resources" pathid 11000): a
  field is typed UnityEngine.GUIStyle, a native-backed class
  whose serialized shape is not derivable from metadata.
  AssetRipper fails it too (it is the single theirs_only
  record) — parity.

Natsuki (127 + 8 partial):
- **112 × UnityEngine.RenderingLayerMask underived** —
  field type inside UniversalAdditionalLightData and kin.
  The fixture export emits an EMPTY structure for the
  containing classes too, so there is no fixture evidence for
  a shape; honest REVIEW beats an invented struct.
- **11 × SerializedDictionary`2 cyclic type graph**
  (ProbeVolumeBakingSet): class → dictionary → scenario data
  → streamable asset → dictionary. Unity depth-caps such
  trees; the cap is not derivable from metadata alone.
- **1 × GUISkin → GUIStyle** (parity with PES).
- **1 × RenderPipelineGraphicsSettingsCollection**
  (UniversalRenderPipelineGlobalSettings): native collection
  with interface-typed elements.
- **1 × ScriptCommand recursive inline** (ScriptEngine):
  cyclic by-value type graph.
- **1 × VisualTreeAsset**: UIElements asset with proprietary
  inline data.
- **8 partial × MetaXRAcousticGeometry**: one
  underived-but-fixed-size field preserved as opaque bytes
  (by design — structure exact, bytes preserved verbatim).

Carried notes: 246 of PES's 1,252 MonoScript objects belong
to editor/test assemblies absent from player metadata and do
not resolve in the index (breakdown in DESIGN.md). A
MonoBehaviour referencing one would route REVIEW with that
reason; PES's final census shows the case did not arise among
its 1,823 MonoBehaviours (the sole review is GUISkin). Also
carried openly: the Legacy
Gradient shape (163 B) is a single-data-point gate (PES has
zero Gradient fields); the ref_pad position ambiguity; LZMA
still unproven on real bytes (absent from all 9,357 held
UnityFS files); the rung-1 HC2 census delta (ours 11,516 vs
Fathom 11,517 MonoBehaviours, one object, likely scope).

## Payloads (values only)

Under `~/workspace/bundle-native-20261110/rung2-payloads/`
(uniform projection: scalars/enums/PPtrs whole; scalar lists
≤ 64 kept; scalar-only struct lists ≤ 32 kept; every other
array reduced to `{"$list_length": n}` — nothing silently
dropped):

- `pes-device-stage-climax.json` (52,123 B, 20 records):
  SexStageManager, SexStageAttackClassificationProfile,
  GenitalMosaicSettings, and 17 Codex.* records — climax
  audio bank, receiver stage/motion profiles, post-orgasm
  bridge, OrgasmMilk ×3, OrgasmSquirt ×3, wet-sound bank,
  Serena voice bank, nipple-attachment and breast/thigh
  contact profiles, FinalIK restraints ×3.
- `natsuki-interaction-physics.json` (610,974 B, 377
  records): the PBD solver/constraint/data classes (Bind,
  Cloth, Collision, Inner, Pin, SelfCollision, ShapeMatching,
  UnderHair, DeltaMush solvers; PBD_Solver, PBD_Cloth,
  PBD_ClothData), NS_XRPhysicalHand.XRPhysicalHand,
  PhysicalBodyController, JointPhysicsController,
  LocomotionPhysicsController, XR haptics/interactor classes.

## Cleanup

Primo crate `target/` and `.bacon-locations` deleted;
rung-2 throwaway debug examples (7) and local probe scripts
(7) deleted. Kept: deliverables, layouts, fixtures, census /
values / differ evidence, gate transcripts, the extracted
Natsuki working copy (downstream study target). Apparent
space removed ~23.7 MB; actual free space unchanged at df
granularity (255,430,197,248 B available before and after —
btrfs zstd:3 shared extents).
