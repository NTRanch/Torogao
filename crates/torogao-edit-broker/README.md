# torogao-edit-broker

The edit-route job broker (Torogao desktop tranche T2): one
primo service that owns the ComfyUI edit queue, so lanes
submit and poll instead of racing waiter scripts for idle
gaps in a shared queue.

## Why

The primo-local edit route (Qwen-Image-2.1 on ComfyUI :8189,
driven by `edit-local.py`) is keeper-grade and CLI-only. Its
record from 2026-11-27 documents the friction this broker
removes: lanes ran waiter scripts that polled for an empty
queue and fired on an idle reading — one lane relaxed its
own idle threshold because other lanes' waiters kept taking
the gaps, and jobs queued behind unrelated lanes' probes.
The de facto protocol was racing pollers, not a queue. The
broker is the queue.

## The contract

The broker implements the estate's facade engine contract —
the surface the torogao-gen seat implements and the governed
facade (`torogao-mobile-facade`) speaks — so it can be
registered as the facade's second engine without changes:

| Route | Meaning |
|---|---|
| `GET /health` | `{engine, model_loaded, driver_present, comfyui_alive, comfyui_url, queue_depth}` |
| `POST /jobs` | submit a job document → `202 {id}`; refusals are `400` with the named reason |
| `GET /jobs/<id>` | job state; when `done`, the delivered frame as `image_png_base64` plus all outputs in `artefacts` (name, kind, sha256, bytes, inline bytes) |
| `GET /jobs` | `{counts, queue}` — counts by state and the FIFO order |
| `POST /jobs/<id>/interrupt` | cancel: a queued job never runs; a running job's driver is killed, ComfyUI is sent `/interrupt`, partial outputs are discarded |

Loopback only. One worker thread runs jobs serially in
submission order; each job is one `edit-local.py` subprocess
under an 1800 s deadline.

### `model_loaded`, honestly

The facade reads `engine` and `model_loaded` from `/health`.
For this engine the "model" is the edit route itself: the
broker reports loaded when the driver of record is in place,
because the driver auto-starts ComfyUI on the first job —
that auto-start is this engine's load path. `driver_present`
and `comfyui_alive` are reported alongside so the mapping
hides nothing.

## The job document

```json
{
  "canvas_png_base64": "<the canvas to edit, PNG>",
  "prompt": "<the instruction, as text>",
  "config": "faithful" | "turbo",
  "seed": 2701,
  "mask": { "kind": "explicit", "mask_png_base64": "<PNG>", "feather": 8 }
        | { "kind": "auto", "feather": 8, "threshold": 28, "dilate": 6 }
}
```

- `config` defaults to **faithful** (no LoRA, 20 steps,
  cfg 4.0) — the proof-gated, chain-safe configuration.
- **turbo** is offered only under its label, which travels
  in every job state the broker reports: *fast, NOT
  chain-safe (small props drift); Qwen Research License
  fence — non-commercial internal use only.*
- **omni is not offered.** Submitting `config: "omni"` is
  refused at submit with its proof verdict named (it
  re-composes the frame instead of editing it —
  primo-local-edit-route RECORD.md, 2026-11-27). The option
  stays in the driver for hand use; the broker is not a way
  around the verdict.
- `mask` asks for compositing (outside the mask the
  delivered frame is the canvas's pixels exactly). The
  explicit and auto kinds' fields are mutually exclusive,
  mirroring the driver's own refusals.
- Unknown fields are refused, everywhere: a silently ignored
  field is a silent no-op.
- Bounds: body ≤ 64 MiB (jobs carry PNGs inline), image
  ≤ 32 MiB decoded, prompt ≤ 16 384 chars, feather ≤ 64 px,
  threshold ≤ 255, dilate ≤ 64 px, job table ≤ 256 entries.

Outputs are separate candidate artefacts, as the mobile
architecture specifies: `raw_edit` (always), `composited`
and `mask_alpha` (when compositing ran). The delivered
frame — the one in `image_png_base64` — is the composited
frame when compositing ran, else the raw edit.

## Running it

```
torogao-edit-broker [--port 8193] \
  [--driver ~/.local/share/pax/primo-local-edit-route-20261127/edit-local.py] \
  [--work-dir ~/.local/share/torogao-edit-broker] \
  [--output-dir <route output dir, default: beside the driver>] \
  [--comfy-url http://127.0.0.1:8189] [--python python3]
```

Job inputs are staged under `<work-dir>/job-N/` (canvas,
prompt, mask, driver log); job ids are seeded past the
highest existing staging directory, so a restart never
reissues an id. A systemd user unit is in `packaging/` —
installing it is a deployment act, not part of the build.

## What is not here (named gaps)

- **Facade registration.** The facade-side second-engine
  registration and the edit recipe bounds in its governance
  list are the mobile architecture's separate "extend" piece.
  This crate is the engine side of that seam, complete; the
  facade does not know about it yet.
- **Live-fire proof.** The contract tests run against a stub
  driver (same CLI, same output discipline) because primo's
  GPUs were wedged for the whole build window. One real
  end-to-end job through the broker against the real route
  is owed when a GPU frees; the stub pins everything except
  ComfyUI itself.
- **The wai img2img mode.** The driver's second mode is a
  labelled nudge tool that failed its edit proof; the
  broker's contract is the instruction-edit route only.
