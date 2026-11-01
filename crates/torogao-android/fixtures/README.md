# A-1 embedded fixtures

`prima-repertoire.inx` is a **byte-identical copy** of the
canonical Phase F repertoire fixture, embedded into the
Android shell with `include_bytes!` so the phone build
carries Prima's rig and her Greeting/Daydream repertoire
without a filesystem the APK does not have.

- Canonical source:
  `~/workspace/torogao-rust-port-20261027/phase-f-gates/fixtures/prima-repertoire.inx`
  (the Phase F gate corpus; the canonical cast files are
  READ, never modified — AMENDMENT-ANDROID §11 / BRIEF-A1
  standing rules).
- SHA-256:
  `f0c3b8395d5e4ce9f31661705af386049dd6c84b2eefb5125f47e5eb551e656d`
  (verified identical for this copy at A-1 build time).
- Size: 31,916 bytes.

The copy exists so the determinism smoke (BRIEF-A1 §Scope
5) evaluates the *same bytes* on desktop and on Android.
If the canonical fixture ever changes, this copy must be
re-taken and the hash above re-recorded in the same commit.
