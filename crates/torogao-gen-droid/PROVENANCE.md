# Provenance — torogao-gen-droid

This crate is estate code (BSD-2-Clause, the workspace licence). It
**links** a third-party native library; it vendors none of it.

## stable-diffusion.cpp (the linked engine)

- Upstream: `leejet/stable-diffusion.cpp` (ggml-based SD/Flux
  inference in C/C++).
- Licence: **MIT** (LICENSE read in the clone, 2026-11-26).
- Pinned build of record for this lane: commit
  `e637c7bccd35aa8841d67fb942ba72918176826c` (2026-11-27), cloned
  at `~/.local/share/pax/droidgen-20261126/sd.cpp` on primo, built
  with the Vulkan backend for host (x86_64), Android arm64-v8a and
  Android x86_64. `build.rs` links `libstable-diffusion` from
  `SDCPP_LIB_DIR`; the library is staged beside the app, never
  copied into this repository.
- ggml (its substrate) is likewise MIT.

## Weights (never in this repository)

- Generator of record (D-1525): `waiIllustriousSDXL_v170`
  (wai), sha256
  `f116b0c78ff441467b0cdc8f1936e1ed18ea31e9997c7b132b1b8db533f0bd04`,
  GGUF-quantized by the lane (q8_0 / q5_1 / q4_0) with the pinned
  build's own convert mode.
- Fast modes: SDXL-Lightning LoRAs (ByteDance; **OpenRAIL++** per
  the Hugging Face model card) merged into wai at strength 1.0 —
  wai remains the generator; the LoRA is an adapter over it.
  4-step LoRA sha256
  `bf56cf2657efb15e465d81402ed481d1e11c4677e4bcce1bc11fe71ad8506b79`,
  8-step LoRA sha256
  `5aa30d94cdf7950a8b53a693682650ee061eddb128c5435a8f88defea73b2ac6`.
