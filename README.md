# scratchpad-performance-lens

Scratchpad-specific measurement JSON producers for the Overview, Telemetry, and
Performance dashboard tabs.

This repository owns producer logic only. It runs against a configured
Scratchpad checkout and writes JSON artifacts into that checkout's
`target/analysis` directory.

The lens is implemented entirely in Rust. There is no Python package, module
runner, or Python test harness in this repository.

The public dashboard contract is JSON, but the highest-risk
`performance_review.json` contract is backed by Rust artifact structs and a
generated JSON Schema. The producer still embeds raw evidence rows from other
artifacts, but branch-on fields such as promise health and scale-check status
are typed in Rust before serialization.

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

```powershell
cargo run --bin splens -- measure search --config splens.toml
cargo run --bin splens -- measure performance-review --config splens.toml
cargo run --bin splens -- measure project-code --config splens.toml
```

Run the standard producer set:

```powershell
cargo run --bin splens -- measure all --config splens.toml
```

Export artifact schemas:

```powershell
cargo run --bin splens -- schema export --output schemas
```

Telemetry payloads are generated on demand:

```powershell
cargo run --bin splens -- telemetry --config splens.toml
```

After installation, the same CLI is exposed as both `splens` and
`scratchpad-performance-lens`.

```powershell
cargo install --path .
splens catalog --config splens.toml
```

## Artifact Contracts

Generated schemas are written with:

```powershell
splens schema export --output schemas
```

Current schema output:

- `schemas/performance_review.schema.json`
- `schemas/index.json`

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

```powershell
cargo fmt
cargo check --all-targets
cargo clippy --all-targets
cargo test
```

`measure slowspots` and `measure search` run Scratchpad Criterion benches by
default. Pass `--skip-bench` to read existing `target/criterion` output only.

```powershell
cargo run --bin splens -- measure search --skip-bench --config examples/scratchpad.toml
```

For repository-quality metrics, run `rust-quality-lens` from this workspace:

```powershell
cargo run --manifest-path C:\Code\rust-quality-lens\Cargo.toml --bin rqlens -- catalog
cargo run --manifest-path C:\Code\rust-quality-lens\Cargo.toml --bin rqlens -- measure all
```

RQL writes quality, correctness, locality, leverage, and architecture-map
artifacts into this repository's `target/analysis` directory. The architecture
map treats `slowspots.json` as optional; if this Scratchpad-specific performance
artifact has not been produced, map-level `performance_risk` and `total_score`
remain unknown rather than being scored as zero.
