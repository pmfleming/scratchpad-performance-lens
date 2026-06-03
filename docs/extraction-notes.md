# Extraction Notes

This is the Scratchpad-specific counterpart to `rust-quality-lens`.

`rust-quality-lens` handles reusable Rust quality/correctness/map JSON.
`scratchpad-performance-lens` handles Scratchpad-specific overview,
performance, and telemetry JSON.

The lens itself is now Rust-only. It keeps Scratchpad probes and benches in
Scratchpad because they depend directly on the `scratchpad` crate. This crate
runs from the configured Scratchpad project root and invokes those Cargo targets
there.

The dashboard-facing contract remains the same:

- `splens catalog --config splens.toml`
- `splens measure <producer|all> --config splens.toml`
- `splens telemetry --config splens.toml`

No Python package metadata, Python module entrypoint, or Python tests are part
of this repository anymore.
