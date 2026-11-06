# Measurement integrity and migration

This change set addresses the eight measurement-review findings and five
additional reporting defects. Implementations are split between this lens
(reporting, validation, execution) and Scratchpad (production-dependent probes).
Each primary finding and each additional reporting improvement has a separate
local Git commit; probe changes are committed in Scratchpad too.

## Evidence is not the same as a passing promise

The review distinguishes available measurements, valid measurements, required
operations, and successful demonstration at scale. A failure can be real even
when coverage is incomplete. A catalog entry or synthetic row is not proof.

The registry in `src/producers/review/registry.rs` declares required operation
IDs. A sibling operation or broad family match can associate evidence with a
promise, but cannot satisfy a different required operation. Profiles are optional
diagnostics, counted as available only when the profile artifact is available.

Scale checks require within-budget, successful capacity samples for every
capacity operation associated with the target. Resource-only targets require
explicit timing budgets and successful samples for each required resource
operation. Workload sizes from arbitrary results (for example nanoseconds per
frame) are not successful scale evidence. An observed failure above a target
with the last success below it leaves the target unproven. Headroom is based on
demonstrated success, not the first failed workload.

`covered` is coverage of the declared matrix, not proof of every possible
workload, hardware configuration, or population tail. Missing required probes or
budgets intentionally keep current results thin/unmeasured rather than green.

## Freshness and validity

Producer sidecars (`<artifact>.provenance.json`) record a run ID, start/end time,
artifact content fingerprint, checkout revision and dirty/source fingerprint,
host/toolchain/environment, and lens executable fingerprint. The fingerprint is
a deterministic change detector, not a security signature. Artifacts must match
the current context and be no older than 24 hours to contribute evidence.

A producer returning nonzero for a measured budget failure does not turn valid
measurements into missing data. Partial capacity artifacts retain real samples;
completeness and status are evaluated per sample. Execution/configuration errors,
corrupted JSON, or unverified cached results are not passing measurements.

Scratchpad's `benches/measurement_manifest.json` enumerates expected Criterion
suite/group/parameter combinations. Fresh executions invalidate old suite
manifests, validate every newly written estimate, and require the declared rows.
`--skip-bench` reuses only matching, unexpired, content-bound suite manifests.
Standalone probes are not skipped by that option.

NDJSON must consist of valid measurement objects with scenario, step, workload,
positive elapsed time, and status. Invalid/truncated lines cause a failure rather
than silent sample deletion. Probe stdout/stderr is archived beneath the actual
Cargo target directory in `splens-probes`, including failed NDJSON executions.
Cargo-reported executable paths are used; guessed `target/release` executables
are never launched after a build.

## Statistical contracts

### Criterion

- `mean_confidence_interval_ns` preserves the Criterion mean interval.
- `budget_probe_label` is `mean_confidence_interval_upper_bound`.
- `budget_status` is `pass`, `fail`, `uncertain`, or `unmeasured`.
- The whole interval must lie at/below the budget to pass, or above it to fail.
  Overlap is uncertainty, not proof of a tail violation.
- `dispersion_upper_ns` / `dispersion_lower_ns` are mean ± 2 standard deviations,
  clipped at zero for the lower diagnostic. They are **not** p95/p05.
- `tail_latency_measured: false` prevents microbenchmark estimates from implying
  individual user-request latency percentiles.

### Capacity

At least three complete repetitions are required. Repetition identities,
workload, units and scopes must agree. Duplicate, missing or inconsistent
repetitions produce incomplete evidence. Failed repetitions are never hidden by
a successful median. The median/min/max describe successful trials; if none
succeeded, those fields are null. Even-sized medians average the middle values.

Top-level RSS/handle peaks consider all repetitions. `raw_repetitions` preserves
all timing, failure, application-counter, memory-budget, and process-counter
evidence, including failed trials. The median is coarse observed-run screening,
not a population p95/p99 guarantee.

### Frames and production search workflows

The CPU frame histogram reports its bucket resolution and a conservative
percentile upper bound. An overflowing last bucket uses the observed maximum,
not the nominal last bucket edge. The realistic frame probe retains 2,000 raw
chronological frame durations after warmup and reports nearest-rank percentiles.
These samples are not 2,000 independent executions or a confidence guarantee.

Search workflow samples time query command through dispatch, first result
applied to app state, first result rendered/tessellated, and observed completion.
Fixtures are independently constructed outside the operation interval and match
counts are checked before reporting success. Thirty repetitions are the default;
a passing budget requires all observed trials within budget. A median over budget
fails; otherwise insufficient count or mixed outcomes are uncertain. A reduced
smoke run is for validation, not performance certification.

The prepared Criterion scanner's historical `*_first_response` names are retained
for identity compatibility, but their scope/description now says first-target
**full-scan completion**. Throughput uses only bytes actually scanned, not all
prepared targets. Real dispatch/current-workspace/all-tabs behavior comes from
the separate `search-workflows` producer.

## Resource boundaries

Process snapshots preserve CPU time, RSS, I/O bytes, and open-file counts where
supported; Linux adds peak RSS and minor/major page faults. Counter deltas are
between boundaries outside the operation timer and include background activity
and sampling overhead. Unsupported or reset counters are null. Process-lifetime
RSS high-water must not be read as a resettable per-operation peak.

The tracking allocator never resets absolute live bytes while prepared objects
remain alive. Interval allocation traffic, baseline live bytes, absolute live
bytes, live growth and absolute heap peak are separate values. Retained provenance
heap is additionally sampled before document teardown. Reallocation traffic does
not increment the count of allocation/deallocation calls.

Application capacity counters and memory-budget objects are preserved rather than
discarded. Increased memory usage or absent OS counters do not establish a CPU
bottleneck: without instrumented saturation evidence the limiting resource is
`unknown`.

Gap closure requires successful scoped samples, the declared minimum workload,
and required metrics. Shared persistence/restore gaps require every contributing
scenario. Numeric snapshot, serialization, write, reconstruction and initial
paint phase timings are preserved in resource events. Cache compliance requires
observed retained chunks and the actual limit, not just low heap use.

## Reporting and consumer migration

- Repeated Criterion rows from search and slowspots are deduplicated by benchmark
  identity, not presentation labels; newer valid measurements are preferred.
  Frame and workflow scopes remain distinct evidence.
- Unknown/synthetic capacity ceilings remain null. Unknown latency budgets are
  not `over_budget: false`. Triage has a separate `unknown` count.
- Probe failure produces an empty failed artifact rather than fabricated timing
  numbers. Diagnostic source statuses remain available to explain missing data.
- Code history counts each revision's blobs with the same nonblank-line and
  embedded-test classification as the current snapshot. Content/category caching
  handles blank edits, root commits, renames and first-parent merges. The selected
  snapshot remains the upstream tracking ref (or HEAD), not the working tree;
  historical `latest_push.date` means commit time, not actual push time.
- Re-export the capacity/review schemas. Consumers must tolerate nullable timing
  values and ceiling booleans and render unknown/uncertain/invalid explicitly.

## Remaining scope limits

No headless result proves GPU submission, compositor latency, vsync, dropped
presented frames, or input-to-photon latency. First-visible probes end at
headless tessellation; resource-only first-paint phases are layout/paint
preparation, not display presentation. Files created immediately before reads
are warm-cache workloads, not cold-disk performance.

The matrix does not yet cover all combinations of cold/warm storage, hardware,
regex/Unicode query semantics, cancellation, mixed long-session workflows,
autosave pressure, durable filesystem sync, or low-memory recovery. Add those as
separately scoped workload contracts, not inferred passes from existing rows.
No automatic claim of a statistically significant revision-to-revision speedup is
made without a controlled baseline experiment.

## Validation

The lens tests cover the reproduced false scale pass, corrupt Criterion inputs,
missing profiles/budgets, failed/missing/duplicate repetitions, percentile budget
semantics, frame budget propagation, deduplication, gap closure, unknown triage,
relocated Cargo targets, and exact Git history classification. Scratchpad tests
cover histogram overflow, process-counter units, allocator baselines and actual
search dispatch/delivery/render preparation. A reduced-size workflow smoke run
checks the standalone probe protocol; it is not a full performance sweep.

## Probe failure isolation

Capacity sweeps run in separate processes selected by `capacity_probe --scenario ID`.
Successful rows survive crashes and malformed trailing output. Probes emit a
`step_started` record before timing; a process failure is attached to that workload
as an error, never a successful latency. Missing repetitions remain incomplete.
All probe executions have a 180-second limit, configurable with
`SPLENS_PROBE_TIMEOUT_SECS`. Raw stdout and stderr remain in `target/splens-probes`.

## Production-path evidence

Storage-sensitive capacity samples and editor-scroll frames carry a
`measurement_contract` with `text_backing`, `production_path`, and `code_path`.
Frames also declare `document_bytes`. A large workload label cannot substitute
for store backing; scroll evidence must exceed the 16 MiB storage threshold.
Owned-text component rows remain diagnostics. Search uses snapshot chunk windows;
paste measures the app input/preparation/publication workflow without an injected
writer pause. GPU submission and presentation remain outside frame scope.

## Fixture identity and deduplication

Store workloads use deterministic non-repeating UTF-8 fixtures (revision 2).
The fixture kind and revision travel with the measurement contract. Setting
`SCRATCHPAD_FIXTURE_KIND=dedup` selects a repeating fixture for diagnostics;
such samples cannot prove a storage-sensitive promise. Fresh load probes use
an isolated store and report logical bytes and database growth. Growth below
half the logical body flags the fixture as invalid, independently of latency.

## Resource scale budgets

A completed resource workload proves scale only when its exact operation also
passes a project-owned `resource_budgets` entry in the measurement manifest.
Entries name a JSON-pointer metric, a maximum and its unit. Missing or invalid
budgets cannot prove scale. Budget misses feed promise health, including the
initial save; an unchanged steady save has its own scenario. Initial limits reuse
existing interaction, capacity and storage-plan targets rather than fitting the
observed runtime. New limits should be calibrated through accepted baselines.
