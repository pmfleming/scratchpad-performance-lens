# Paired performance cycles — 2026-11-06

## Working plan

Each cycle follows the same feedback loop:

1. Read Scratchpad's promises and manifest, then collect real release evidence.
2. Use the lens to identify a failing operation and the work responsible for it.
3. Improve the application path while preserving workload and correctness.
4. Improve the lens wherever that investigation exposes inaccurate conclusions,
   missing attribution, or unnecessary measurement cost.
5. Validate both changes, repeat the affected production workload without
   concurrent builds, and commit both repositories at the cycle boundary.
6. Run the complete promise lane periodically to expose regressions and select
   the next operation. Preserve partial and failed evidence.

The seven promises remain the acceptance criteria. A completed measurement
process is not a passing promise, and a targeted improvement is not proof that
all promises pass. GPU submission and display presentation are outside the
headless workflow measurements.

## Cycle 0: runner cost and evidence scope

Lens commit `1f3a2bb` includes the existing uncommitted lens work, as requested,
plus a `measure promises` lane and `measure_run.json` wall-time accounting.
The promise lane derives its capacity/resource selection from the review
registry while retaining the same required workloads and budgets. It avoids
unrelated diagnostic ceilings. The text-mutation scale dependency now names
the production-paste scenario that the registry actually collects.

An initial `measure all` was interrupted after completed Criterion, frame, and
workflow measurements; its unrelated text-layout diagnostic timed out. This
is incomplete evidence, not an accepted baseline. Logs and an explicit
interruption marker are under `target/co-development/initial/` in the lens.

## Cycle 1: search result rendering and attribution

Scratchpad `dec2962` culls offscreen collapsed search-result groups while
preserving scroll geometry, expanded groups, and visible controls. Tests compare
culled and unculled scroll geometry and exercise a visible result button.
The application probe exposes rendered/skipped group counts.

Lens `a36005d` adds per-trial delivery-to-render intervals, editor layout work,
and rendered/skipped group summaries. Missing counters remain unknown. This
separates result-list work from editor layout without changing any budget.

The saved pre-change release executable ran immediately before the changed
probe, with 30 independent fixtures per mode and no intervening compilation.
For all-tabs first render at 10,000 targets × 16 KiB, the observed median was
109.017 ms before and 43.977 ms after; maxima were 150.091 and 54.490 ms.
All 30 changed trials met the 120 ms budget. The post-change control was stable.
These are observations on one host, not a population-tail guarantee.

Active-buffer first render still failed: its changed median was 167.689 ms,
with 164.527 ms spent in editor layout. A long first line followed by the match
line bypasses the existing single-line viewport clipping. Any fix must preserve
document/display offsets, cursor movement, selections, Unicode, and scrolling.

Raw confirmation artifacts are in Scratchpad's
`target/co-development/cycle-1/` and
`target/analysis/co-development/cycle-1/`. These measurements preceded the
cycle commit; their provenance records the parent revision and dirty source.

Validation: Scratchpad's targeted search UI tests and strict library/test Clippy
passed; the lens's full tests and strict all-target Clippy passed.

## Cycle 2: authoritative workflow contracts

The search probe duplicated manifest budgets, and a reduced target fixture
could report passing operation budgets. Scratchpad now owns workflow sizes
alongside budgets in its manifest. The lens validates raw match counts, bytes,
operation times, and statistical summaries before accepting a row. Smaller
fixtures retain observed timings but are diagnostic and cannot prove a promise.

Validation: 696 Scratchpad library tests passed (seven ignored), 80 lens tests
passed, and strict Clippy passed in both repositories. A real 30-trial run capped
at 100 targets preserved all four observed all-tabs passes but reported them as
diagnostic with an unmeasured promise budget. Active/current workloads retained
their normal verdicts because the cap did not reduce those fixtures. Artifacts
are in Scratchpad's `target/analysis/co-development/cycle-2-reduced/`.
The full-workload confirmation uses
`target/analysis/co-development/cycle-2-full/`.

## Follow-up priorities

1. Verify search under stable host controls after the cycle-3 wrapping fix.
2. Investigate background installation timeout in many-file capacity sweeps;
   preserve the distinction between promised scale and larger stress workloads.
3. Use the complete promise review to rank remaining latency/resource failures.
4. Repeat the affected workloads after each fix and commit each paired cycle.

Full promise execution after cycle 1 completed all eight producers in about
17 minutes. Capacity returned partial evidence after the two 50,000-file
background-installation timeouts; all other producers completed. At 10,000
files, both first-visible and background-completion measurements completed
but exceeded their budgets. The review reported six failed promises and one
at risk (many views), with 19 budget misses and seven missing scale targets.
Several producer controls were noisy, so this is not an accepted baseline.
The separately controlled cycle-1 search confirmation remains the evidence for
the attributed search improvement.

The full run is recorded in Scratchpad's
`target/analysis/co-development/promises-after-cycle-1/`; the lens log is
`target/co-development/initial/promises-after-cycle-1.log`. Capacity failures
remain partial evidence and must not be represented as a successful baseline.

## Cycle 3: stable wrapped layout and reflow diagnostics

Tracing the production path corrected the initial diagnosis: the default editor
uses wrapped, proportional text. Its first frame had no scrollbar gutter; the
result frame gained a seven-pixel gutter and rewrapped the unchanged long
paragraph. The editor now reserves the vertical gutter for wrapped text while
keeping scrollbar visibility conditional. Text, selections, cursor mapping and
wrapping semantics are unchanged. A regression test exercises content growing
and shrinking across frames and checks stable viewport width.

The probe records prepared/result wrap widths and wrapping mode. The lens reports
the maximum per-trial width change, layout-job counts, and render time outside
font layout. Missing or inconsistent counters stay unknown.

All twelve workflow operations passed in 30 trials per mode. Active first render
had median 3.746 ms and maximum 5.036 ms against 120 ms; its median layout time
was 0.503 ms. Wrap-width changes were zero in every measured trial. Host controls
flagged noise and unrelated projects were compiling during the comparison, so
these results are not an accepted baseline. A stable full review is still needed.
Artifacts are in Scratchpad's `target/analysis/co-development/cycle-3/`; raw
baseline confirmation and the diagnostic trace are in
`target/co-development/cycle-3/`. Validation: 697 Scratchpad tests passed (seven
ignored); the focused gutter regression and strict Clippy also passed. The lens
has 82 passing tests and strict all-target Clippy coverage.

## Cycle 4: selected-file delivery before inactive batch installation

The path lane now delivers the selected file's loaded text before constructing
inactive cold shells. Once the batch arrives, requested tab order is restored.
Failed selected files retain the existing successful-file fallback. Activity
recording uses a set of installed document IDs instead of repeatedly scanning
the batch. Regression tests cover delivery order, selected-file failure, final
tab ordering, and pending reservations.

First-visible profiling now rejects cold placeholders and checks the selected
fixture's loaded byte count. The lens aggregates path/session/analysis busy wall
time from raw repetitions. Its scope explicitly includes background completion;
overlapping lane times are neither CPU time nor a sum of critical-path work.
Missing or failed trials cannot produce a complete median.

The focused run completed all nine trials, including the previously timing-out
50,000-file workload. At the promised 10,000 files, first-visible observations
were 81.878–83.432 ms against 180 ms, and installation completed in
1,633.273–1,727.777 ms against 5,000 ms. The 50,000-file stress workload still
exceeds both budgets. Normal deferred autosave remains separate from shell
installation; persistence has its own required measurements. Unrelated host
build/test activity was present, so this focused run is not an accepted baseline.
Raw evidence: Scratchpad `target/co-development/cycle-4/many-files.jsonl`.
Validation: 698 app tests passed (seven ignored), 83 lens tests passed, and
strict Clippy passed in both repositories.

## Cycle 5: separate fixture preparation from measured resource operations

Edited-preview and fragmented-mutation probes now prepare the edited document
before timing. Tab construction receives the same unique 48 KiB UTF-8 strings,
prepared before timing, and still constructs the buffers and workspaces itself.
Prepared state survives through resource sampling, excluding fixture teardown.
Tab split/combine preparation now happens only for the selected scenario.
The manifest declares the required prepared-operation scope for these six
probes; the lens rejects missing or mismatched scope without changing limits.

Three focused repetitions per scenario preserve a clear remaining bottleneck:
10,000-tab construction still takes about 1.1 seconds, exceeding 140 ms even
after text generation is excluded. Edited-preview and fragmented-mutation
operations are within 180 ms at every existing workload. Split and combine
operations also meet 140 ms. These are scoped measurement corrections, not
claims that production operations became faster. Evidence is in Scratchpad's
`target/co-development/cycle-5/`. The resource binary passes strict Clippy;
84 lens tests and strict all-target Clippy pass.

## Cycle 6: avoid per-document system-memory queries

Phase isolation found that every TextDocument construction refreshed system
memory for its default undo budget. Construction bursts now share a sample for
one second; explicit automatic-budget resets still sample immediately. A
deterministic test verifies reuse and expiry without sleeping. The metadata
scanner also skips printable ASCII in eight-byte words, preserving scalar
handling for controls, Unicode, and CRLF boundaries. Exhaustive byte/position
tests accompany the existing streaming and parallel-inspection tests.

The tab probe records buffer and workspace construction separately. The lens
summarizes named nanosecond phases per workload, excludes non-time fields, and
leaves failed, missing or impossible observations incomplete. Overlapping phases
are explicitly not a partition of elapsed time.

Focused current observations: 10,000-tab construction 261.832 ms (still over
140 ms), 10,000-file tracking 80.197 ms (within 180 ms), and 10,000-tab full
restore 417.236 ms (still over 180 ms). The 50,000-file stress tracking remains
over budget. Temporary isolation code is archived under ignored target output;
it is not part of the shipped application. Evidence: Scratchpad
`target/co-development/cycle-6/`. Validation: 700 app tests passed (seven
ignored), 85 lens tests passed, and strict Clippy passed in both repositories.
