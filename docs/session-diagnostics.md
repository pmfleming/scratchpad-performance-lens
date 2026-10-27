# Session diagnostics

These Rust commands replace Scratchpad's `scripts/compare-session-baseline.py`
and `scripts/measure-large-dirty-save.py`. Their ownership is here because they
interpret lens artifacts and orchestrate external measurements. Scratchpad still
owns the `resource_probe` implementation and Criterion benchmarks that depend on
its production code. Neither helper belongs in Scratchpad's `xtask` or runtime.

Both commands are available through `splens` and `scratchpad-performance-lens`.
They are opt-in, independent of `measure all`, and do not write dashboard
artifacts or claim fresh provenance for archived evidence.

## Compare session baselines

```console
splens compare-session-baseline BASELINE_RUN CURRENT_RUN
splens compare-session-baseline BASELINE_RUN CURRENT_RUN --require-all-promises
```

Each directory must contain:

- `check.exit` containing `0`, or, only when absent, legacy `performance.exit`
  containing `0`;
- `performance/performance_review.json`.

This matches archives made by Scratchpad's `scripts/check-session-migration.sh`.
A failing combined marker never falls back to a successful performance marker.

For the `Session Persistence Restore` scenario, the following rows are required:

- `session_persist_latency/10000`
- `session_restore_latency/10000`
- `session_restore_background_completion/10000`

Acceptance preserves the original policy: each current measurement must be
`valid`, any authoritative budget must be `pass`, and neither the mean nor the
mean confidence interval upper bound may exceed its baseline value. A
nonauthoritative budget does not invent a budget failure. The command prints
individual comparisons and all scenario promise-health transitions. Exit status
is nonzero on session regression; `--require-all-promises` additionally requires
every current scenario's promise health to be `pass`.

Malformed/missing data, duplicate scenario or benchmark names, nonpositive or
nonnumeric timings, and differing scenario sets are errors. In particular, a
missing scenario cannot make the all-promises gate pass. This is an archived
comparison, not a new measurement, a current-provenance check, or a claim of
statistical significance.

## Independent large dirty-buffer saves

Build the production-dependent probe in the Scratchpad checkout first, then
select that executable explicitly:

```console
cargo build --release --locked --bin resource_probe
splens measure-large-dirty-save --binary target/release/resource_probe --output target/session-save-runs/new-run --launches 3
```

`--binary` is now required: a lens checkout must not guess another repository's
`target/release` path. Paths are relative to the caller's working directory.
Use the actual executable path for custom Cargo target directories or cross-target
builds, and include `.exe` on Windows. The command does not build the probe or
claim that a selected prebuilt executable corresponds to current source.

`--launches` defaults to 3 and accepts 1–20. Each launch starts a new process with
`--large-dirty-save`, with a 300-second deadline. The Scratchpad probe owns fixture
isolation, setup exclusion, and cleanup. Each successful launch must report
exactly nine successful `large_dirty_buffer_save` samples, uniquely covering
steps 0–8, with positive save durations.

The output directory must not exist. Its archive retains the original layout:

- `metadata.json`: UTC time, resolved binary path, SHA-256 identity, OS/architecture,
  launch count, and I/O scope;
- `run-N.jsonl` and `run-N.stderr`: unmodified process output per launch;
- `summary.json`: array of per-launch `launch`, `median_save_ms`,
  `median_write_bytes`, and `write_bytes_available` fields, also printed to stdout.

The save median converts nanoseconds to milliseconds. Write-byte medians are
available only if all nine samples have the counter; otherwise the median is
`null` and availability is `false`. These are process write-byte deltas including
WAL and automatic checkpoints, **not physical device traffic**.

Raw streams go directly to files, avoiding pipe deadlocks and preserving partial
output on crashes and timeouts. On timeout the child is killed and reaped. Failed
exit, malformed/partial output, missing/duplicate steps, or failed samples return
nonzero and leave diagnostics in place. No successful summary is written unless
all launches succeed. Existing archives are never overwritten.

These summaries describe nine saves per independent launch; they do not establish
a population percentile, confidence interval, or dashboard promise compliance.
