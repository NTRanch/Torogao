# Porting a diffusion stack: verify semantics at the tensor level, not the API level

Date: 2026-11-04. Agent: pax (torogao-gen Stage 1, D-867).
origin_lane: ntranch
content_label: sfw
label_evidence: every lesson below is inference-engineering mechanics (checkpoint
conversion, sampler verification, GPU build traps); no NSFW content, assets, or
prompts are recorded. The checkpoint involved is named only as a weights hash.
evidence: `~/workspace/torogao-gen-takeover-20261103/PROOF-REPORT.md` §Debug log;
`~/workspace/torogao-gen-takeover-20261103/ARCH-EVAL.md` §1/§3; primo scratch
`~/.local/share/pax/gen-takeover-20261103/` (`comfy-cond.safetensors` reference
conditioning, `dbg*.log`, battery records); branch `pax/torogao-gen-stage1`,
crate `crates/torogao-gen`.

## Lesson

A diffusion pipeline can load, run, and produce structured output while being
semantically wrong in three independent places. The Stage 1 port (ComfyUI →
candle/Rust, SDXL-class checkpoint, weights SHA-256 `536863e9…c`) produced a
flat pastel field on first success, and every defect was an incumbent-side
convention invisible from the API surface:

1. The denoiser contract includes preconditioning: the UNet consumes
   `x / sqrt(sigma^2 + 1)` and the denoised conversion uses the unscaled state.
   Feeding raw state at sigma 14.6 scales the input ~14.65x; GroupNorms keep the
   failure structured instead of NaN, so the output looks "almost like a bug in
   the sampler" — it was not.
2. The conditioning contract is layer- and norm-specific: SDXL consumes
   penultimate-layer hidden states (pre-final-norm) from both text encoders,
   while the pooled vector comes from the final layer's EOS through a
   projection the stock library did not expose.
3. The tokenization contract is asymmetric per encoder: one encoder pads with
   the EOS id, the other with token 0 — and padding positions are attended, so
   the asymmetry changes outputs.

What found them, in order of value: (a) verify the sampler FIRST, standalone,
against the incumbent's own function on a stub denoiser (agreement 3.8e-7
exonerated the math and indicted the contract); (b) obtain the incumbent's
intermediate tensors (its conditioning, saved from an in-process reference run)
and bisect per-component, per-layer, per-position — the padding defect localized
to positions after EOS within one comparison; (c) cross-check with a third
implementation (transformers) to prove the converter innocent before blaming
the incumbent's semantics. Endpoint statistics are the tripwire throughout:
a healthy eps prediction has std ~1.0 at sigma_max; 2.08 at every sigma was the
first hard signal.

## Corollaries

- Checkpoint converters must prove consumption: every source tensor accounted
  for (UNet 1,680/1,680, VAE 248/248, zero unused) plus spot byte-checks. A
  converter that "loads" can still silently drop a stage.
- GPU build provenance is part of the artifact: the kernel-compilation target
  followed the build machine's device 0 (sm_89) and produced PTX the target
  card (sm_86) rejected at runtime. Pin the compute capability explicitly
  (`CUDA_COMPUTE_CAP=86`) and record the backend string in every run record.
- Feature flags are per-crate, not per-workspace: the same library needed its
  `cuda` feature enabled on three separate crates before a LayerNorm kernel
  existed at runtime.
- Background-exec completion handoffs are not evidence (D-567, reconfirmed):
  one delivered "results" for a battery run that had never launched — the
  files did not exist. Every claim in the proof report traces to a foreground
  read of disk.
