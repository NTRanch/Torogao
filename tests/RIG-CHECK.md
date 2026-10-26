# The rig-check helper (not vendored)

The P3 and P4 harnesses invoke an external validator binary,
`rig-check`, as:

    rig-check <model.json>

It prints a `VALID` line with the model's census (parameters,
parts, textures, deformers, meshes, physics) and exits 0 when the
model is valid; a non-zero exit means the model failed validation.

The binary is a compiled artifact of the D build, so it is **not
vendored into this repository** — source repos carry source. The
gate runner (`tests/run_gates.sh`) takes its path as a parameter.
During the Rust port the D build provides the binary; the ported
engine's own validator takes over the role when it lands (RP2),
at which point this note is retired.
