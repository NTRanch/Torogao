# D-1220 rung 1 — estate-native clean-room Rust bundle reader: REPORT

**Verdict: rung 1 COMPLETE.** A clean-room Rust reader for Unity asset
bundles is built, gated, and censused over the held corpus: UnityFS
container parsing, SerializedFile parsing with exact accounting
closure, a deterministic census, and the first class readers —
TextAsset (native layout) and MonoBehaviour field values via embedded
type trees. The AssetRipper cross-check (D-1219 corpus) RAN and
passed 212/212 byte-identical.

## Clean-room law (as commissioned, and kept)

Implemented ONLY from (a) publicly available format documentation
(HearthSim/UnityPack wiki "Format Documentation"; imbushuo.net "Deep
dive into UnityFS", 2017; Unity's official Class ID reference) and
(b) observed bytes of held titles on primo. No implementation's
source — AssetRipper's, UnityPy's, or any other — was read, fetched,
or consulted at any point. The D-1219 lane's exported OUTPUTS are used
as test fixtures (data, not code). Everything docs + bytes could not
decide is in the REVIEW list below, not guessed. The law and every
byte-derived rule are recorded in the crate's `DESIGN.md`.

## Where it lives

- Crate: primo `~/.local/share/pax/bundle-native-20261110/`
  (`bundle-native 0.1.0`, edition 2021, `#![forbid(unsafe_code)]`,
  deps `lz4_flex 0.14` + `lzma-rs 0.3` — both pure Rust; rustc 1.99.1).
- Local mirror: `~/workspace/bundle-native-20261110/mirror/`.
- Evidence: crate `evidence/` (six censuses, test/clippy/fmt logs,
  two theodolite session transcripts).
- Method packaged as skill `local-bundle-native-reader`.

Modules: `bytes` (bounds-checked endian cursor), `bundle` (UnityFS),
`serialized` (SerializedFile + layout-closure search), `typetree`,
`strings` (class names + derived common-string table), `value`
(type-tree decoder, TextAsset reader), `census`, `main`
(`census` / `read` / `text` / `extract`).

## What parses today

- **UnityFS** format versions 6/7/8; block-info codecs stored + LZ4
  (+LZ4HC as LZ4); data blocks stored + LZ4 + LZ4HC; LZMA arm present
  (pure-Rust) but REVIEW — no corpus bytes exercise it (see below).
  Block info after header (16-aligned iff format ≥ 7) or at EOF
  (flags 0x80); data blocks 16-aligned iff flags 0x200.
- **SerializedFile** versions 21 (Unity 2019.4.40f1) and 22
  (2021.3.13f1, 2022.1.23f1) exercised; structural window 13..=22.
  Header (20-byte and 48-byte forms), type table (incl. embedded type
  trees, 32-byte node stride), object table, script types, externals.
  A parse is *closed* only when metadata ends exactly at
  headerSize+metadataSize AND max object end == fileSize; degraded
  parses are labelled, never hidden.
- **Readers:** TextAsset via the documented native layout (works in
  stripped files too); MonoBehaviour via embedded type trees —
  primitives, strings, byte arrays, vectors, maps, PPtrs, structs,
  the compact primitive-array form, and the SerializeReference shapes
  (named list wrappers, rid items, the registry-as-struct). Outcomes:
  `exact` (consumed exactly byteSize), `partial` (structure closed,
  some leaves opaque-preserved), `review` (reasoned, per object).
  Tree-stripped files route every MonoBehaviour to REVIEW by design.

## Census results (final decoder; evidence files in crate `evidence/`)

| corpus | files | objects | MonoBehaviour exact / partial / review | TextAssets |
|---|---|---|---|---|
| HC1 loose (2019.4, stripped) | 13 SF | 11,427 | 0 / 0 / 2,364 (by design — no trees) | 32 |
| HC1 bundles | 596 | 17,093 | 3,781 / 326 / 0 | 198 |
| HC2 loose (2021.3, stripped) | 13 SF | 8,986 | 0 / 0 / 1,229 (by design) | 25 |
| HC2 bundles | 1,165 | 50,892 | 8,574 / 1,710 / 3 | 4 |
| Time Stop loose (2022.1, stripped) | 7 SF | 16,125 | 0 / 0 / 1,278 (by design) | 2 |
| Addressables title (2021.3, v8 bundles) | 8 | 41,054 | 3,265 / 80 / 0 | 0 |

Census files: `census-hc1-loose.txt`, `census-hc1-bundles.txt`,
`census-hc2-loose.txt`, `census-hc2-bundles.txt`,
`census-ts-loose.txt`, `census-addressables.txt`. The only `status
fail` entries are `asset_bundle_config.bin` in both bundle dirs —
not a bundle, correctly classified `kind=other`. The 3 HC2 bundle
reviews share one script class (script_index 4/5) with an underived
shape. Honest delta: my HC2 MonoBehaviour total is 11,516 vs Fathom's
census 11,517 — one object, unexplained; my loose census covers only
the 13 top-level _Data files (subdirectories excluded), the likely
scope difference. Total objects accounted across the six censuses:
147,481.

## Gates (all run on the final tree; cited from `evidence/`)

- **cargo test: 18/18 pass, 0 fail** (`evidence/test-results.txt`).
  Split per the commission: 6 validation (V1 synthetic bundle
  roundtrip stored+LZ4; V2 SF v21 TextAsset+MonoBehaviour exact
  values; V3 SF v22 header + i64 byteStart; V4 LZMA fixture; V5
  compact primitive-array form; V6 managed-reference shapes),
  8 adversarial (A1 truncated bundle; A2 bad magic; A3 LZ4 size-lie,
  both directions; A4 object table overrunning EOF → degraded parse,
  visibly not closed; A5a type-tree level jump → Structure error;
  A5b 200-deep tree → LimitExceeded at the 128 cap; A6a underived
  variable-size type → Review; A6b underived fixed-size type →
  Partial with bytes preserved — every one fail-closed: typed error,
  no panic, no partial output), 4 corpus (C1 loose-file closure over
  all three titles; C2 determinism; C3 battlegirl bundle census; C4
  R1 cross-check).
- **cargo clippy `--all-targets -- -D warnings`: clean;**
  `cargo fmt --check`: clean (`evidence/clippy-fmt.txt`).
- **Determinism:** census twice over the same corpus files →
  byte-identical (C2 gate; census format carries no timestamps).
- **Theodolite (D-1124 cured protocol):** producer first —
  `bacon --headless -j bacon-ls` wrote a current `.bacon-locations`
  before each session. Session A (planted positive control
  `src/control_scratch.rs`, deliberate E0308): the control reported
  2 error diagnostics (severity 1) in-session while all ten real
  `src/*.rs` files reported 0 (`evidence/theodolite-session-a.txt`).
  Session B (control removed, locations regenerated empty): all ten
  files 0 diagnostics (`evidence/theodolite-session-b.txt`). The
  zero is therefore verified, not silent.
- **Cross-check vs D-1219 corpus: RAN, not pending.** Test C4 matches
  this reader's extraction against AssetRipper's exports in
  `~/.local/share/pax/assetripper-r1-20261110/exports/` (their data
  output): multiset (name, bytes) match over HC1's 212 exported
  TextAssets against my TextAsset extraction from HC1's loose files +
  all 596 bundles — **compared=212, mismatches=0**.

## Bugs found and cured during the build (in DESIGN.md)

1. **Object-table alignment** — entries are 4-aligned after the count
   (undocumented; proven by all 33 loose files closing exactly).
2. **LZ4 silent short reads** — decompress into an exact-size buffer
   and verify the written count both ways.
3. **Compact primitive-array form** — array node carrying its element
   type with only a size child.
4. **SerializeReference shapes** — named list wrappers, rid items,
   and the array-flagged registry that serializes as a struct. Found
   by tracing 80 review objects in the Addressables title; all 80 now
   decode (partial — underived leaves stay opaque), reviews there: 0.

## REVIEW list (what clean-room could not derive)

- **LZMA codec**: implemented via pure-Rust `lzma-rs`, but a census
  of all 9,357 held UnityFS files shows LZMA nowhere (LZ4/LZ4HC/
  stored only). Synthetic fixture only; unproven against real bytes.
- **Stripped-file MonoBehaviour values**: unrecoverable from the
  SerializedFile alone — 4,871 objects across the three titles' loose
  files route to REVIEW per object, by design. (Recovery via
  MonoScript → assembly metadata is a rung-2 question.)
- **Underived common-string offsets** (#934, #907, #800, #155, #778,
  collapsed #86): leaves decode as preserved opaque bytes (`partial`),
  never labelled. Admission to the derived table requires decisive
  structural evidence (see DESIGN.md rule 10).
- **SerializeReference payloads**: rids and the registry decode;
  joining rids to `ReferencedObjectData` payloads does not (the tree
  describes the payload as 0 bytes with no children — it is not
  tree-derivable).
- **Type-tree strides 24/28 and the v22 byteSize-i64 variant**:
  closure-gated fallbacks, never observed (stride 32 always won).
- **refTypes-vs-userInfo order**: undecidable on zero-count files
  (the whole corpus); documented order is primary.
- SF versions outside 13..=22, UnityFS format outside 5..=8, unknown
  block codec ids: typed `Unsupported` errors.
- The 1-object HC2 MonoBehaviour delta vs Fathom (11,516 vs 11,517),
  noted above.

## Rung 2 would add

More class readers in demand order (MonoScript identity resolution;
Material/Shader values; Sprite/Texture2D metadata incl. .resS stream
offsets; AnimationClip curves); stripped-tree MonoBehaviour recovery
via script metadata (MonoScript → assembly); SerializeReference
payload joining; streaming census for very large bundles without
materializing the whole decompressed stream; more derived
common-string entries as new evidence accrues; LZMA corpus validation
if a title exercising it is ever held.
