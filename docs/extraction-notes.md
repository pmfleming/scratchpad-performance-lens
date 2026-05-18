# Extraction Notes

This is the Scratchpad-specific counterpart to `rust-quality-lens`.

`rust-quality-lens` handles reusable Rust quality/correctness/map JSON.
`scratchpad-performance-lens` handles Scratchpad-specific overview,
performance, and telemetry JSON.

The first extraction keeps Rust probes and benches in Scratchpad because they
depend directly on the `scratchpad` crate. This package runs from the configured
Scratchpad project root and invokes those Cargo targets there.
