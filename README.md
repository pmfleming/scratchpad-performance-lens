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
cargo run --bin splens -- measure search-workflows --config splens.toml
cargo run --bin splens -- measure performance-review --config splens.toml
cargo run --bin splens -- measure project-code --config splens.toml
```

Run the standard producer set:

```console
cargo run --bin splens -- measure all --config splens.toml
```

For development cycles focused on the seven promises, use `measure promises`.
It runs the same Criterion suites, frames, production search workflows, and
reports, with capacity/resource scenarios selected from the union of the promise
coverage and scale requirements. All required workload sizes and repetitions
remain unchanged. It omits unrelated capacity/resource diagnostics and code
history analysis. `measure all` retains the complete diagnostic suite. Use separate
output directories to retain both runs; each command replaces selected artifacts.

For a targeted resource cycle, keep the full workload sweep and select scenarios:

```console
splens measure resources --scenario session_persist_cost --scenario session_restore_cost --config cycle.toml
```

This preserves budgets, validation, adaptive repetitions and provenance, and
records a `focused` selection in the artifact and requested scenarios in
`measure_run.json`. Unknown scenario IDs fail before launching a probe. Use a
separate output directory: the resulting resource artifact contains only the
selected scenarios and cannot establish coverage for omitted operations.
`--scenario` is rejected for `promises` and other producers.

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

## Session diagnostics

The Rust CLI also owns the session migration helpers formerly kept as Python
scripts in Scratchpad:

```console
splens compare-session-baseline BASELINE_RUN CURRENT_RUN --require-all-promises
splens measure-large-dirty-save --binary /absolute/path/to/resource_probe --output NEW_RUN_DIR --launches 3
splens measure-session-maintenance --binary /absolute/path/to/resource_probe --output NEW_RUN_DIR --launches 3
```

These are opt-in diagnostics, not part of `measure all`. They compare archived
reports or run an explicitly selected, already-built Scratchpad probe. See
[Session diagnostics](docs/session-diagnostics.md) for archive formats, failure
semantics, and migration examples.

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
workloads. Capacity thresholds use the successful-trial median, with at least
three complete, uniquely identified repetitions required to demonstrate success.
Each sample also reports `elapsed_min_ms`, `elapsed_max_ms`, `repetition_count`,
`expected_repetition_count`, `successful_repetition_count`, `failure_count`,
`complete`, and `raw_repetitions`. Timing fields are nullable when no successful
trial exists. Any failed repetition remains a failure regardless of the median;
resource peaks include all repetitions. Staged
workflows can additionally report `background_completion_ms`, keeping the
first-visible threshold separate from eventual hydration or shell installation.
Such samples require `completed_workload_value` to match the requested workload;
a timed-out wait is not background completion.

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
`stale_budget_risk: true` and do not drive budget pass/fail judgment. Criterion
budgets use the mean confidence interval: wholly below the budget is `pass`,
wholly above is `fail`, overlap is `uncertain`, and missing authority/intervals
is `unmeasured`. `dispersion_upper_ns` and `dispersion_lower_ns` are explicitly
mean ± two standard deviations, not latency percentiles.

`resource_profiles.json` separates prepared setup from measured work where a
scenario requires expensive fixture construction or indexing. Cache-oriented
rows expose `retained_file_chunks`, `file_chunk_cache_limit`, and
`cache_bound_held` per sample, plus scenario-level `cache_bound_violations`, so
a low peak heap is not the only evidence that eviction is working. Explicit
resource-contract violations and failed resource samples count as review budget
misses instead of remaining passive diagnostics.

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

## Measurement Integrity

See [Measurement integrity and migration](docs/measurement-integrity.md) for the
full contracts, validation rules, and remaining measurement boundaries.
The [end-to-end Scratchpad investigation](docs/scratchpad-performance-investigation.md)
records actual full runs, measured improvements, discovered probe failures, and
remaining budget violations.

- Every producer writes a `.json.provenance.json` sidecar binding its artifact
  contents to the measured checkout, toolchain, host, relevant environment,
  lens executable, and run time. Evidence expires after 24 hours. Missing,
  changed, failed, or stale evidence does not prove a promise. The selected
  producer's previous artifact and sidecar are removed before invocation, so an
  early failure cannot refresh old partial evidence. Each producer run shares a
  lazy context snapshot, captured after any build/benchmark work; the next producer
  gets a fresh snapshot because builds can change inputs such as `Cargo.lock`.
- Scratchpad owns `benches/measurement_manifest.json`, listing expected Criterion
  suites and workload parameters. A fresh run must produce every declared row.
  `--skip-bench` requires matching, unexpired lens-generated Criterion manifests;
  arbitrary pre-existing Criterion caches are unverified, not zero-latency data.
  Only manifest-listed entries are collected. Unlisted leftovers are ignored;
  missing, modified or corrupt listed entries still fail validation.
- Required operation IDs are evaluated individually. `available_count`,
  `valid_count`, `expected_count`, and `missing_evidence` explain coverage.
  Diagnostic profiles are optional and must actually be available to count.
- Scale checks use successful, within-budget samples for **every required
  capacity operation**, never failed attempts or an unrelated resource row.
  `headroom_workload` is demonstrated success minus the promised minimum.
- `search-workflows` measures production query dispatch, first result delivered,
  first result tessellated, and completion using independent prepared fixtures.
  It defaults to 30 repetitions, with all-tabs coverage up to 10,000 targets.
  `SCRATCHPAD_WORKFLOW_REPETITIONS` and `SCRATCHPAD_WORKFLOW_MAX_TARGETS` can
  reduce a smoke run; fewer than 30 repetitions cannot produce a passing budget.
  Budgets and minimum fixture sizes come from Scratchpad's measurement manifest.
  Reduced fixtures remain diagnostic, with an `unmeasured` promise budget even
  when their observed timings pass. The lens checks raw match counts, byte counts,
  operation timings, and summaries against that contract before reporting.
  First-render rows also summarize per-trial delivery-to-tessellation intervals,
  editor layout time/input bytes, and rendered/skipped search-result groups when
  supplied by the probe. Missing counters remain unknown; these diagnostics do
  not alter budgets. The render interval is calculated within each trial before
  aggregation, rather than subtracting unrelated percentile or median values.
- Process CPU, RSS, I/O, faults, and open-file counters are preserved where
  supported. Unsupported counters remain null. RSS high-water is explicitly
  process-lifetime; allocator live bytes are absolute with an interval baseline.
  Missing saturation evidence produces `unknown`, never an assumed CPU bottleneck.
- Resource probe IDs are declared by Scratchpad in
  `benches/measurement_manifest.json` under `probes.resource_probe`. Missing
  scenarios fail the run but preserve valid samples in a partial artifact.
  Resource gaps close only with successful, correctly scoped measurements at
  the required workload and with all required metrics. Session phase timings
  are numeric data, not just formatted labels. Resource-backed scale targets use
  contract-complete successful workloads, not an invented resource latency budget.
  Resource rows expose `demonstrated_workload` and `scale_evidence_basis`; separate
  authoritative latency measurements still gate the session promise. A fresh
  capacity artifact with an incompatible schema fails review explicitly rather
  than being silently interpreted as an empty report.
- Frame p95 and p99 budgets both affect promise health. Histogram resolution,
  overflow and percentile method remain visible; headless work never implies
  display presentation. Speed-report triage counts unknown evidence separately.
- Probe executables come from Cargo compiler-artifact messages, respecting
  workspace, `.cargo/config.toml`, and `CARGO_TARGET_DIR` configuration.

**Migration:** regenerate measurements and export schemas after upgrading. Old
artifacts without provenance are intentionally unverified. Dashboard consumers
must support nullable timing/ceiling fields and explicit unknown/uncertain
statuses; they must not coerce those states to zero or passing.

## Boundary

Included here:

- performance and capacity report producers
- archived session comparisons and independent dirty-save probe orchestration
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
running `search_speed` twice after a successful producer. Pass `--skip-bench`
to read existing, provenance-validated Criterion output only. It does not skip
standalone probes such as `search-workflows`, capacity, resources, or frames.
`--fail-on-slow` applies only to slowspots/search, and `--index-only` applies
only to flamegraphs; the CLI rejects either flag when no selected producer can
use it.

Each measurement invocation writes `measure_run.json` with per-producer wall
time, execution status, errors, and whether Criterion cache reuse was requested.
The report is updated between producers, so an interrupted run retains its
completed costs and identifies the active producer. These costs include build,
fixture setup, probes, and reporting; they are diagnostics for improving the
lens, not application latency or promise evidence. A completed execution still
does not imply that performance budgets passed.

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
