# Torogao plate production spec — v2 (D-1224)

**Status:** normative from 2026-11-10. Every future plate render of
the Torogao cast follows this record. It supersedes the implicit
v1 spec (the settings anim.py + the girl blends happened to carry).
Chosen by measurement on the estate's A6000, per D-1224: highest
tier whose full 360-frame arc render is practical.

## What did not change

Scene content is frozen. The scene of record is still built by the
anim.py code path — set pieces, props, camera (52 mm, fixed pose),
the light rig (Sun 3.0 @ (55°, −25°), Fill 1.1 @ (70°, 140°), world
(0.16, 0.15, 0.19) strength 1.0), the show application, and the
outfit/visibility schedules are byte-identical code. The production
driver is `anim_v2.py`, derived verbatim from anim.py (sha256
`a7f2ce5d…`) with four enumerated changes: a JSON render profile,
an applied-settings read-back dump, per-frame timing, and an OK
sentinel. Only render settings moved.

## Production configuration (normative)

| item | value |
|---|---|
| Blender | official build, `~/.cache/azoth/blender-official/blender-5.2.3-linux-x64/blender` (5.2.3 LTS, e13b728c6880) |
| GPU backend | Vulkan (`--gpu-backend vulkan`) |
| GPU device | RTX A6000, pinned: `--gpu-device 10de/2230/2 --gpu-device-no-fallback` |
| Threads | `-t 8` |
| Invocation | canonical argv: `-b -Y --factory-startup --offline-mode -noaudio --python-exit-code 1`, timeout-bounded |
| Driver | `anim_v2.py` (primo `~/.local/share/pax/torogao-plate-spec-20261110/scripts/`, sha256 `286e660a…`) |
| Profile | `plate-spec-v2.json` (sha256 `9f8cbd7e…`) — the machine-readable form of the settings below |

Why Vulkan + a pinned device: headless EEVEE otherwise lands on
the first enumerated GPU — on primo that is the RTX 4070 Laptop
(the desktop GPU, 8 GB), which is both the wrong bench device and
the Owner's interactive GPU. Measured backend cost of the switch:
OpenGL vs Vulkan at identical settings differ by mean |Δ| ≈
0.0001/255, max 1 (8-bit) LSB, 0.03 % of pixels — pixel-neutral.
What the pin buys is the A6000's 46 GB and the heavy-slot rule.

## Render settings (normative)

| setting | v2 value | v1 (audited) |
|---|---|---|
| resolution | **6144×7680** (8K-class, 4:5) @ 100 % | 512×640 |
| engine | BLENDER_EEVEE | same |
| `taa_render_samples` | **256** | 64 |
| `shadow_pool_size` | **'2048'** | '512' |
| `shadow_ray_count` | **4** (property max) | 1 |
| `shadow_step_count` | **16** (property max) | 6 |
| `shadow_resolution_scale` | 1.0 | 1.0 |
| `use_shadows` | true | true |
| fast GI `quality` | **1.0** | 0.25 |
| fast GI `resolution` | **'1'** | '2' |
| fast GI `ray_count` | **16** (clamps above) | 2 |
| fast GI `step_count` | **64** (property max) | 8 |
| `use_raytracing` | false — the v1 shading model; raytracing would change the model the renderer parity is measured against, not just its fidelity | false |
| `use_taa_reprojection` | true | true |
| `light_threshold` | 0.01 | 0.01 |
| colour management | AgX, look None, exposure 0.0, gamma 1.0, display sRGB | same |
| `film_transparent` | false | false |
| output | PNG, RGBA, **color_depth 16**, compression 15 (lossless at any level; 15 is the write-speed point) | PNG RGBA 8-bit, compression 15 |

Production rule: **an arc renders in one process, frames in
order.** TAA reprojection makes a frame's last-bit rounding
depend on process history; the sequence is the unit of
reproduction.

## Determinism (measured, stated honestly)

EEVEE pixel output at this spec is deterministic to within
1 × 16-bit LSB on 11 of 47,185,920 pixels (2.3×10⁻⁷ of pixels;
mean |Δ| ≈ 1×10⁻⁷ in 16-bit units) across independent processes
and across process histories — GPU floating-point at the last
bit, nothing larger. PNG **file bytes are not reproducible**:
Blender embeds `Date` and `RenderTime` text chunks per run.
Plate manifests therefore hash the **decoded RGBA16 pixel
buffer** as the determinism carrier and record the file sha256
alongside for transfer integrity.

## Audit of the v1 spec (2026-11-10)

All six cast blends (prima, alma, lux, piper, sloane, yui) were
opened headless and dumped (`audit_settings.py`): **every
render-relevant setting is identical across the six** — EEVEE
block, view/display settings, image settings; the only differing
key in the six dumps is the file path. No blend carries lights, a
camera, or a world: anim.py supplies all three. The v1 numbers in
the table above are therefore the cast-wide v1 spec, not Prima's
alone. Audit dumps: primo
`~/.local/share/pax/torogao-plate-spec-20261110/audit/<girl>.json`.

## Benchmark evidence (A6000, Vulkan, steady-state = 2nd render)

Frames: 135 (BUILD II, representative mid-arc) and 225 (PEAK,
most complex staging). Per-frame time includes PNG write.

| config | frame | s/frame | file size | A6000 peak VRAM |
|---|---|---|---|---|
| v1 quality, 4K (3072×3840) | 135 | 2.69 (OpenGL/4070 calibration) | 11.6 MB (8-bit) | — (4070: 2,932 MiB) |
| v1 quality, 8K | 225 | 6.19 | 42.9 MB (8-bit) | 6,059 MiB |
| MAX, 4K | 135 | 6.99 | 19.3 MB (16-bit) | 4,227 MiB |
| MAX, 4K | 225 | 6.78 | 17.8 MB | 4,229 MiB |
| MAX, 8K | 135 | 27.82 | 71.4 MB (16-bit) | 8,857 MiB |
| MAX, 8K | 225 | 27.57 | 67.2 MB | 8,138 MiB |
| MAX + TAA 512, 8K | 225 | 50.68 | 67.2 MB | 8,186 MiB |

Projections from the MAX rows (360 frames/arc, 6 arcs/cast):

| tier | per arc | full cast time | plates storage / arc | full cast |
|---|---|---|---|---|
| 4K MAX | ≈ 41 min | ≈ 4.1 h | ≈ 6.7 GB | ≈ 40 GB |
| **8K MAX (chosen)** | **≈ 2.8 h** | **≈ 16.6 h** | **≈ 25 GB** | **≈ 149 GB** |
| 8K MAX, TAA 512 | ≈ 5.1 h | ≈ 30.4 h | ≈ 25 GB | ≈ 149 GB |

## Decision

**8K-class (6144×7680), MAX profile, TAA 256.** The 8K arc is
2.8 h on an idle A6000 with 8.9 GB peak VRAM of 46 GB and 25 GB
of plates — practical by every measured axis; nothing failed,
nothing is near a limit. TAA 512 is the rejected step: +83 %
frame time (cast: 30 h) for accumulation depth no viewing size
can recover — its price is recorded here so the step is a
decision, not an unknown. 4K remains the fallback tier if a
future constraint (disk, schedule) binds; it is the same spec
at 3072×3840, nothing else changes.

## Provenance

Benchmarks, audit dumps, profiles, and bench renders: primo
`~/.local/share/pax/torogao-plate-spec-20261110/`. Plates v2:
primo `~/.local/share/pax/torogao-renderer-rung1-20261110/inputs/plates-v2/`
with `manifest.json` (per-frame file + pixel hashes, settings
hash). Plates v1 are untouched and remain the rung-1 evidence of
record.
