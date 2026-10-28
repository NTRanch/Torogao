# Persona area previews — how these were made

One short, silent MP4 per Persona area, recorded from the app
itself at the Rust port's landing (2026-10-28). They are
**stage previews**: each video shows that area's Persona stage
(the rail, title, host strip, and command stage) with the
host repertoire loaded. They are not per-area feature demos —
an area's tools are listed on its stage, not exercised.

## Route (all 12 areas)

- The windowed app (`torogao-app`, Persona stage, default UI)
  ran on a virtual X display (Xvfb, 1280x900) with the
  Phase F gate fixture `yui-repertoire.inx` loaded (the Yui
  repertoire: Greeting + Daydream performances).
- The screen was captured with ffmpeg's x11grab (h264,
  30 fps, CRF 18) in one continuous app session: the
  Playroom segment first, then a rail tour through the other
  eleven areas in rail order, one segment per area.
- The capture is **screen recording, not a deterministic
  render**: timings are wall-clock, so files are not expected
  to be byte-identical across runs. The stage itself is
  deterministic (fixed-step clock); the Phase F gate's
  still-frame evidence is the determinism record.
- **Playroom only**: the transport's Stop → Play
  choreography runs the Daydream performance three times
  (Daydream is the repertoire's first animation; Play picks
  the first animation, Stop → Play restarts it from frame 0).
  The puppet in this fixture carries no textures, so the
  performance reads as moving geometry — that is the
  fixture's honest look, the same one the Phase F eye
  sheets show. The other eleven areas' host strips are
  register panels (glyph + text), so no puppet performance
  exists on those stages to record; their motion is the
  stage's own (area-switch wipe on non-reading areas, host
  pull-forward).

## Silence

The app ran with its audio aimed at a null sink, and the
session's stream list was checked before and after: no app
audio streams existed at any point. The files carry **no
audio track at all** (verified per file with ffprobe: one
h264 video stream, zero audio streams).

## Verification

Every video was machine-checked against the Phase F gate's
verified per-area eye sheets: frames sampled at 50% and 85%
of each video were classified by nearest mean-absolute
difference against all twelve sheets (puppet viewport
masked). All 24 sampled frames classified as their own
named area, each with a 3–8x margin over the runner-up
area — the check exists because an earlier screenshot run
once landed one area high; these previews were required to
prove they did not. The Playroom's viewport additionally
shows measurable frame-to-frame motion during the Daydream
runs.

## Specs

h264 MP4, 1280x900 (the app's windowed size on the capture
display), 30 fps, no audio. Playroom 14 s; the other eleven
12 s each.
