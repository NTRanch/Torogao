# torogao-gen-droid

The on-device generation engine for Torogao on Android: the second
engine behind Torogao Mobile's bounded generation contract
(submit → poll → fetch), running in-process on the phone instead of
against the primo seat.

- Backend: stable-diffusion.cpp (ggml, **Vulkan**) via a small
  hand-written FFI seam — see `PROVENANCE.md` for the pinned
  upstream, its MIT licence, and the weights' provenance.
- Generator: wai (D-1525), GGUF-quantized; fast modes are
  Lightning-LoRA merges of the same checkpoint (adapter over wai,
  not a generator change).
- Build the native backend with
  `SDCPP_LIB_DIR=<dir holding libstable-diffusion.so> cargo build --features native`.
- `examples/droid_smoke.rs` runs the canonical Prima prompt through
  the contract end-to-end (host or device).

Determination, measurements, and the phone bench live in
`~/workspace/droidgen-20261126/` (DETERMINATION.md, RECORD.md).
