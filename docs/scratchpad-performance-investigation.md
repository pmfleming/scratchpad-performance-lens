# Scratchpad performance investigation — 2026-09-12

## Execution

Ran the actual release-mode lens against `/home/laufan/Projects/scratchpad`.
Neither `~/Documents/scratchpad` nor `~/documents/scratchpad` exists on this host.
No synthetic fallback results were substituted for failed probes.

Host: Linux x86-64, AMD Ryzen 7 PRO 8840HS, 16 logical CPUs, approximately
58 GiB RAM; rustc 1.97.0. These are warm-cache/headless measurements, not display
presentation results. Timed suites were run serially, without concurrent test
builds or other lens suites. CPU affinity/frequency were not pinned.

Three complete `measure all` invocations were attempted:

1. **Baseline:** lens `de289a8`, Scratchpad `5289cf3`. Exit 1: capacity crashed;
   the lens rejected the otherwise successful resource run because of a stale
   scenario ID.
2. **First corrected run:** lens `3f9de9c`, Scratchpad `3426365`. Exit 0, but raw
   capacity inspection uncovered a false background-completion claim.
3. **Final:** lens `97b6d0b`, Scratchpad `019fa4a`. Exit 0; all nine standard
   producers completed, including actual capacity and resource sweeps.

Final coverage comprises 34 Criterion measurements, 12 production workflow rows
(30 trials per mode), four frame rows, 12 capacity scenarios with 56 steps × three
repetitions, and 22 resource scenarios with 68 samples. Flamegraphs are an optional
extra and were not collected.

**Exit 0 means measurement execution succeeded, not that every budget passed.**

## Measured search improvement

All-tabs workload: 10,000 independently prepared tabs, 16 KiB per tab, one match
per tab. The same correctness assertions and timing boundaries were used before
and after. Values below are observed medians in milliseconds.

| Query-command boundary | Baseline | Final | Final budget status |
| --- | ---: | ---: | --- |
| Dispatch | 280.283 | 12.657 | pass: 50 ms budget |
| First result delivered | 284.915 | 17.084 | pass: 100 ms budget |
| First result tessellated | 349.009 | 100.415 | uncertain: maximum 175.959 ms exceeds 120 ms |
| Completion observed | 428.830 | 164.202 | pass: 500 ms budget |

Dispatch was approximately **22.1× faster** in the full runs. The production UI
thread previously scanned every tab for duplicate names while naming each search
target: O(N²) work before submitting the request. It now constructs duplicate-name
counts once per request. Regression tests preserve duplicate-name labels,
active-tab-first ordering and label freshness after a rename.

A separate confirmation reused the saved baseline executable followed by the
final executable, with no intervening compilation and 30 trials each. Dispatch
was **293.201 → 12.785 ms**; completion was **447.553 → 159.393 ms**. These are
repeated observations on one host, not a population-tail or hardware-independent
speedup guarantee. Other benchmark changes are not attributed to this fix.

## Defects exposed and corrected

### First-visible measurement raced asynchronous file opening

The baseline capacity probe crashed on its first 1 MiB file. It tessellated an
untitled frame before the asynchronous file read finished, then asserted that the
active document was nonempty. Library tests had not exposed this because their
staged-open threshold is reduced to 16 bytes.

The probe now polls the production I/O handler until the requested path is the
active document, then tessellates that document. Waiting remains inside the
measurement interval, with a 30-second timeout and 1 ms polling backoff. Empty
files are valid; a different document is not. Integration tests use the actual
production thresholds and exercise a 1 MiB asynchronous open and an empty file.

### Background timeout was mislabeled as completion

In the first corrected run, each 50,000-file sample claimed completion at roughly
1.34 seconds, but its note reported only **2 tabs installed**. The helper used a
best-effort one-second wait without checking whether the background work ended.

Scratchpad now exposes a timeout-returning wait API. The profile requires actual
completion and verifies the number of installed files. The final observations:

| Requested files | First tessellation, median ms | Background completion, median ms | Verified installed files |
| ---: | ---: | ---: | ---: |
| 2,048 | 19.051 | 149.502 | 2,048 |
| 10,000 | 70.295 | 700.743 | 10,000 |
| 50,000 | 334.052 | 3,551.712 | 50,000 |

The extra untitled tab explains why total tab counts are one higher than file
counts. The 3.55-second result corrects a false measurement; it is not an app
performance regression. Lens aggregation now requires `completed_workload_value`
to match the requested workload whenever background-completion timing is supplied,
and requires completion not to precede first visibility. Mismatches are incomplete
evidence, never a successful capacity sample. The typed capacity schema preserves
the completion count as well as raw repetitions.

### Resource evidence was discarded because of an obsolete ID

The actual resource probe produced 68 successful samples, including
`large_file_first_visible_paint`. The lens expected the old
`file_backed_open_first_visible_paint` name from synthetic fallback definitions
and discarded all resource rows.

Expected scenario IDs now come from Scratchpad's
`benches/measurement_manifest.json` → `probes.resource_probe`. Missing scenarios
produce a nonzero result and a partial artifact with `missing_scenarios`, while
valid samples from other scenarios survive. Synthetic fixtures are test-only.
The final run retained all 22 scenarios with no missing IDs.

### Console and triage hid useful workflow information

The original console printed detailed component scanner results but only a static
description for production workflows. It now prints workflow medians, maxima,
budgets and statuses. Missing timings print `unknown`, not zero. Component tables
also expose budget status instead of allowing “nominal” to imply a verified pass.
The speed report now places the three production dispatch rows in its dispatch
section rather than reporting zero dispatch scenarios.

Workflow triage preserves the observed budget-failure reason instead of saying
“budget status unknown.” Missing timing values remain null, and absent bottleneck
information defaults to unknown. Existing component metadata may still contain
CPU classification hints; those are not measurements of system saturation.

## Remaining findings — deliberately not marked green

- **Active-file long-line search rendering:** final median **164.877 ms** against
  a 120 ms budget. Layout instrumentation attributes a median **161.199 ms** to
  layout work over the 1 MiB fixture. The viewport can include the enormous line
  plus short following lines; the current long-line windowing path applies only
  to a single logical line. A correct fix needs multiline clipping/index mapping
  and cursor/selection/Unicode tests, not a looser budget or a smaller fixture.
- **All-tabs first-render variation:** median is below budget, but the maximum
  is about 176 ms. Its status remains uncertain, not pass.
- **50,000-file first visibility:** approximately 334 ms exceeds the 180 ms
  threshold, although 10,000 files were demonstrated below budget.
- **128 MiB paste:** final Criterion mean was 155.396 ms, with a 95% mean interval
  of 150.617–160.621 ms, above its 150 ms budget. Earlier full runs passed this
  budget. This code path was not optimized; attribution requires controlled
  follow-up runs. The final failure is retained rather than selecting an earlier
  passing observation.
- Several required operation budgets or benchmarks remain missing, and session
  scale is not fully demonstrated by the available resource contracts. The final
  review has **one passing, three failing and three at-risk promises**.
- Initial session persistence still has a substantial file-I/O phase. An unchanged
  session Criterion benchmark is not interchangeable with first-time persistence.
- Headless tessellation does not measure GPU submission, compositor latency,
  actual presentation, input-to-photon delay, or cold-storage behavior.

## Evidence and reproduction

Artifacts retained locally under Scratchpad:

- `target/analysis/investigation/baseline/`
- `target/analysis/investigation/after/` (the intermediate run with the diagnosed
  completion defect; not final evidence)
- `target/analysis/investigation/final/` (JSON and provenance sidecars)
- `target/splens-probes/` (raw capacity/resource stdout and stderr)

Lens-side `target/investigation/` retains the three logs/exit codes, baseline and
intermediate Criterion archives, saved baseline executable, confirmation JSON,
validation logs and exported schemas. These generated artifacts are not committed.
Older artifacts are historical observations; provenance intentionally prevents
using them to certify the changed checkout.

To run fresh measurements from the lens checkout without overwriting these
snapshots:

```console
cargo run --release --bin splens -- measure all --config examples/scratchpad-investigation.toml
```

That config writes `target/analysis/investigation/latest/` under Scratchpad.
Use the same release executable/config when reading cached results. Do not use
`--skip-bench` to reuse measurements from a different source revision or executable.

Validation: lens **65 tests**; Scratchpad **475 library tests**, two first-visible
integration tests and the allocator regression; strict Clippy and formatting
checks in both repositories; schema export. Changes are committed locally only.

Implementation commits: Scratchpad `a1c99a4`, `3426365`, `019fa4a`; lens `a8141e8`,
`3f9de9c`, `b6f82a3`, `97b6d0b`.
