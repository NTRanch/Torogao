# FINDING — torogao-format canonical writer is not f64 round-trip stable

Filed by the mocap pipeline lane, 2026-11-27. Status: **open**,
owned by the torogao-format crate (motion_clip / jsonfmt), not by
this pipeline. The mocap draft clips validate clean under the
working rules; freezing them produces a document that fails
`TG-CLIP-FREEZE-MISMATCH` on revalidation from disk.

## Minimal repro (no mocap code involved)

A `torogao-motion-clip/2` document with a single parameter channel
whose key value is `-0.9696413881143823`:

```
motion-clip-check --canonical probe.json
  -> the value prints as -0.969641388114382385   (a different f64)
```

The canonical writer (`jsonfmt::to_d_json`, C `%.18g` semantics as
implemented by `fmt_f64`) does not render this f64 in a form that
parses back to itself. Python's correctly-rounded parser reads
`-0.9696413881143823` as the f64 with bits `fd9359604d07efbf`; the
canonical text parses to `fe9359604d07efbf` — one ulp away.

## Measured scope

Running `--canonical` over a mocap draft (14,220 float values)
shifted **2,146 values by one ulp**. Repeated canonical passes
keep shifting values (the map is not idempotent), so no
source-side quantization reaches a fixed point: drafts rounded to
%.18g, 15, or 12 significant digits all still fail revalidation
after freeze. The H0 goldens pass because their values are short
decimals whose renderings are fixed points of the writer.

## Consequence

Any frozen clip whose numbers are not golden-shaped (i.e. any
real captured or computed curve data) cannot currently survive
the freeze → revalidate round trip. `freeze_document`'s internal
self-validation passes because it validates the in-memory
document; the failure appears only when the printed frozen
document is parsed back.

## Suggested direction for the format lane

Bisect inside `jsonfmt::fmt_f64`'s digit assembly (the
`{:.17e}` digit extraction, trailing-zero strip, and decimal
reassembly) against a correctly-rounded %.18g reference, with a
round-trip property test over random f64 bit patterns:
`parse(fmt(x)) == x` and `fmt(parse(fmt(x))) == fmt(x)` for all
finite x. Whether the ulp enters in `fmt_f64` or in serde_json's
float parsing under this workspace's feature set is part of the
same bisection; the probe above isolates it to the
parse/format pair inside torogao-format's canonical path.
