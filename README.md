# scratchpad-performance-lens

Scratchpad-specific measurement JSON producers for the Overview, Telemetry, and
Performance dashboard tabs.

This repository owns producer logic only. It runs against a configured
Scratchpad checkout and writes JSON artifacts into that checkout's
`target/analysis` directory.

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
python -m scratchpad_performance_lens.cli measure search --config splens.toml
python -m scratchpad_performance_lens.cli measure performance-review --config splens.toml
python -m scratchpad_performance_lens.cli measure project-code --config splens.toml
```

Run the standard producer set:

```powershell
python -m scratchpad_performance_lens.cli measure all --config splens.toml
```

Telemetry payloads are generated on demand:

```powershell
python -m scratchpad_performance_lens.cli telemetry --config splens.toml
```

## Boundary

Included here:

- performance and capacity report producers
- overview code metrics producer
- flamegraph index producer
- telemetry payload helpers
- shared performance metadata

Owned by `project-management-board`:

- dashboard UI and local web server
- task catalog and run orchestration
- calls into `scratchpad-performance-lens` and `rust-quality-lens`

Still in Scratchpad:

- Rust probe binaries and Criterion benches that compile against the Scratchpad crate
- packaging and app runtime code
- `target/analysis` as the local artifact destination
