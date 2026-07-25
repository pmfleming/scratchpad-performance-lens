# scratchpad-performance-lens

Scratchpad-specific measurement JSON producers for the Overview, Telemetry, and
Performance dashboard tabs.

This repository owns producer logic only. It runs against a configured
Scratchpad checkout and writes JSON artifacts into that checkout's
`target/analysis` directory.

The lens is implemented entirely in Rust. There is no Python package, module
runner, or Python test harness in this repository.

The public dashboard contract is JSON. The highest-risk
`performance_review.json` contract and its scale-critical
`capacity_report.json` input are backed by Rust artifact structs and generated
JSON Schemas. The review still embeds raw evidence rows from other artifacts,
but branch-on fields such as promise health, scale-check status, capacity
ceilings, and capacity workloads are typed in Rust before serialization.

The dashboard is no longer owned by Scratchpad or by this lens. The sibling
`project-management-board` repository owns the React/TypeScript dashboard, task
catalog, local run API, and UI workflow that calls this lens. Scratchpad is now
only the Rust editor project under measurement: it provides app source, Cargo
targets, probe binaries, benches, and the artifact output directory.

## Quick Start

Create a config:

```toml
project_name = "scratchpad"
project_root = "../scratchpad"
output_dir = "target/analysis"
```

Run one producer:

```console
cargo run --bin splens -- measure search --config splens.toml
cargo run --bin splens -- measure performance-review --config splens.toml
cargo run --bin splens -- measure project-code --config splens.toml
```

Run the standard producer set:

```console
cargo run --bin splens -- measure all --config splens.toml
```

Export artifact schemas:

```console
cargo run --bin splens -- schema export --output schemas
```

Telemetry payloads are generated on demand:

```console
cargo run --bin splens -- telemetry --config splens.toml
```

After installation, the same CLI is exposed as both `splens` and
`scratchpad-performance-lens`.

```console
cargo install --path .
splens catalog --config splens.toml
```

## Artifact Contracts

Generated schemas are written with:

```console
splens schema export --output schemas
```

Current schema output:

- `schemas/performance_review.schema.json`
- `schemas/capacity_report.schema.json`
- `schemas/index.json`

`capacity_report.json` separates synthetic workload setup from the measured
operation. `samples[].setup_elapsed_ms` reports fixture construction, while
`samples[].elapsed_ms` is the value compared with the capacity threshold;
`samples[].measurement_scope` identifies prepared operations versus end-to-end
workloads. Capacity thresholds use the median of three repetitions; each sample
also reports `elapsed_min_ms`, `elapsed_max_ms`, and `repetition_count`. Staged
workflows can additionally report `background_completion_ms`, keeping the
first-visible threshold separate from eventual hydration or shell installation.

`performance_review.json` carries a typed contract for the dashboard-facing
scenario matrix:

- `promise_health`: `pass`, `at_risk`, `fail`, or `unmeasured`
- `coverage_status`: `covered`, `thin`, or `missing`
- `scale_checks[].status`: `met`, `missing`, `unmeasured`, or
  `failed_before_promise`
- `scale_checks[].ceiling_status`: `not_reached`, `failed_after_promise`,
  `failed_before_promise`, or `unmeasured`
- `synthetic_evidence`: whether fallback/synthetic evidence contributed to the
  scenario health decision

Criterion budget thresholds are treated as authoritative only when they come
from Scratchpad-owned benchmark metadata JSON. Built-in metadata can still
classify older or missing benchmark rows, but built-in thresholds are marked
`stale_budget_risk: true` and do not drive budget pass/fail judgment.

`frame_metrics.json` separates two frame concepts:

- Existing `frame_metrics` rows are annotated as `measurement_scope:
  measured_frame_path_cpu` and `metric_role:
  theoretical_frame_production_capacity`. Their `theoretical_fps_p99` value is
  `1000 / p99_ms`, useful for saying how much isolated frame-path work fits in
  a frame budget.
- If Scratchpad defines a `realistic_frame_metrics` binary target, `splens
  measure frame-metrics` also runs it and preserves its explicit scope. The
  current Scratchpad probe reports `end_to_end_event_to_tessellation`: wheel
  event construction, app update, layout, paint generation, and egui
  tessellation. It explicitly reports `present_included: false`; GPU upload,
  render submission, compositor, present, and vsync must not be inferred from
  that result. A future real-window probe should set `present_included: true`
  only when the stop point is an actual present/frame callback.

## Boundary

Included here:

- performance and capacity report producers
- overview code metrics producer
- flamegraph index producer
- telemetry payload helpers
- shared performance metadata
- generated schema export for typed artifact contracts
- Rust integration tests for the CLI contract

Owned by `project-management-board`:

- dashboard UI and local web server
- task catalog and run orchestration
- calls into `scratchpad-performance-lens` and `rust-quality-lens`

Still in Scratchpad:

- Rust probe binaries and Criterion benches that compile against the Scratchpad crate
- packaging and app runtime code
- `target/analysis` as the local artifact destination

## Development

```console
cargo fmt
cargo check --all-targets
cargo clippy --all-targets
cargo test
```

A Nix development shell is also available through `nix develop`; with direnv
installed, `direnv allow` loads the checked-in flake automatically.

Measure producers write their artifact before returning a probe failure. A
single-tool run exits nonzero when its probe fails. Multi-producer runs continue
through all selected tools, including `speed-report` and `performance-review`,
then return one combined nonzero result. `--fail-on-slow` also makes slowspots
or search threshold violations nonzero. Artifact write/configuration failures
remain nonzero as usual.

`measure slowspots` runs Scratchpad's `search_speed`, `frame_budget`, and
`promise_latency` Criterion suites; `measure search` runs `search_speed`.
`measure all` reuses suites already run by an earlier producer instead of
running `search_speed` twice. Pass `--skip-bench` to read existing
`target/criterion` output only.
`--fail-on-slow` applies only to slowspots/search, and `--index-only` applies
only to flamegraphs; the CLI rejects either flag when no selected producer can
use it.

```console
cargo run --bin splens -- measure search --skip-bench --config examples/scratchpad.toml
```

For repository-quality metrics, run a sibling `rust-quality-lens` checkout
from this workspace (adjust the relative path if needed):

```console
cargo run --manifest-path ../rust-quality-lens/Cargo.toml --bin rqlens -- catalog
cargo run --manifest-path ../rust-quality-lens/Cargo.toml --bin rqlens -- measure all
```

RQL writes quality, correctness, locality, leverage, and architecture-map
artifacts into this repository's `target/analysis` directory. The architecture
map treats `slowspots.json` as optional; if this Scratchpad-specific performance
artifact has not been produced, map-level `performance_risk` and `total_score`
remain unknown rather than being scored as zero.
