---
name: scratchpad-performance-lens
description: Run and maintain the Rust-only Scratchpad performance lens that emits dashboard JSON artifacts for Scratchpad-specific performance, telemetry, capacity, resource, flamegraph, and code metrics views.
---

# Scratchpad Performance Lens

Use this skill when working in `scratchpad-performance-lens` or when a task needs
Scratchpad-specific performance artifacts from `splens`.

## Contract

- This repository is Rust-only.
- Do not add Python package metadata, Python source files, or Python tests.
- The CLI binaries are `splens` and `scratchpad-performance-lens`.
- Config is TOML with `project_name`, `project_root`, and `output_dir`.
- Artifacts are written under the configured Scratchpad `target/analysis`
  directory.
- The highest-risk dashboard artifact contract, `performance_review.json`, is
  represented by typed Rust structs in `src/artifacts/performance_review.rs`.
- Dashboard branch fields must stay typed, not stringly `json!` plumbing:
  `promise_health`, `coverage_status`, `scale_checks[].status`,
  `scale_checks[].ceiling_status`, and `synthetic_evidence`.
- Export schemas with `splens schema export`; currently this writes
  `performance_review.schema.json` and `index.json`.
- Built-in benchmark metadata is classification fallback only. Do not let
  Rust-side fallback thresholds drive budget pass/fail decisions; thresholds
  are authoritative only when supplied by Scratchpad benchmark metadata JSON.

## Common Commands

```powershell
cargo run --bin splens -- catalog --config examples/scratchpad.toml
cargo run --bin splens -- telemetry --config examples/scratchpad.toml
cargo run --bin splens -- measure all --config examples/scratchpad.toml
cargo run --bin splens -- measure search --skip-bench --config examples/scratchpad.toml
cargo run --bin splens -- schema export --output schemas
```

## Validation

```powershell
cargo fmt
cargo check --all-targets
cargo clippy --all-targets
cargo test
```

Use `--skip-bench` for Criterion-backed producers when you only need to parse
existing Scratchpad benchmark output.

## Quality Lens

Use the separate `rust-quality-lens` repo for project quality metrics:

```powershell
cargo run --manifest-path C:\Code\rust-quality-lens\Cargo.toml --bin rqlens -- catalog
cargo run --manifest-path C:\Code\rust-quality-lens\Cargo.toml --bin rqlens -- measure all
```

The default RQL config is enough for this repo unless a task needs a custom
`rqlens.toml`. RQL writes artifacts to `target/analysis` in this repository.
`slowspots.json` is optional for the architecture map; if it is missing,
`performance_risk` and `total_score` should remain unknown.

## Maintenance Notes

- Keep Scratchpad-specific performance probes in Scratchpad and this lens; do
  not generalize them into `rust-quality-lens`.
- Add typed artifact structs and schema coverage before adding new
  dashboard-critical branch fields.
- Prefer preserving existing JSON shape when typing artifacts so the sibling
  dashboard can migrate incrementally.
