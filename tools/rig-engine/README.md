# torogao-rig-engine — the primo rig adapter

The builder-service behind the mobile facade's rig routes
(ARCHITECTURE §5, route B1). It wraps the estate's gated
fleet builders and the A10 instruments; gate verdicts are
computed facade-side by `torogao-mobile-rig`'s gate module
over the raw instrument output this service returns.

## Deploy (primo)

Deployed copy: `~/.local/share/pax/torogao-rig-engine-20261127/`
with `tools/` staged beside it — the two cured builder
copies of record (`build_character_party.py` c694ffb0…,
`build_character.py` b50faeb2…), the instruments
(`measure_battery.py`, `census_glb.py`, `face_stills.py`
from the MoonDeep lip-cure instrument set), `pose_render.py`,
and `MANIFEST.sha256` pinning every one of them. The service
refuses to start when a tool's hash drifts from the manifest.

```sh
systemctl --user enable --now torogao-rig-engine.service
curl 127.0.0.1:8193/health
```

The facade wakes it on demand (`--rig-engine-service`,
default `torogao-rig-engine.service`); it binds loopback
only — the facade is the only door.

## Contract

`GET /health` → `{"engine": "torogao-rig-engine/1", "model_loaded": true}`;
`POST /jobs` takes a rig job document (see
`torogao-mobile-rig`'s `jobs` module) and returns `{"id"}`;
`GET /jobs/<id>` returns `{"state", ...}` — for a derive:
the raw battery and census JSON, both GLB hashes, the
builder hash, and base64 artefacts (GLB + stills); for a
pose-test: per-morph measured displacements + stills.
`POST /jobs/<id>/interrupt` kills the running step.

Artefacts of a derivation are filed under
`artefacts/<glb sha256>/` (`char.glb`, `char.blend`,
`spec.json`, `record.json`) — pose tests and previews find
the gated blend there by the record's `gated_glb_sha256`.

Heavy-compute discipline: one job at a time (a single worker
drains the queue), the OFFICIAL Blender build only (the Arch
build segfaults on IK), every step bounded by a timeout.
