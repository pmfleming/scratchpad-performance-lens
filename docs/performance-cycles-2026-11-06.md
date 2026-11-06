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

## Cycle 7: bounded body preparation and focused lens runs

Persistence queues up to 64 small document captures before draining body writes,
stopping sooner at 1 MiB pending text. Existing transaction and commit behavior
is retained. The serial database actor also keeps a bounded cache of reusable
statement execution objects. Scalar queries exhaust their cursors; any query
decoding or execution failure evicts its statement before recovery proceeds.
A 130-document regression injects manifest failure, retries, restores every
document, and verifies initial-content undo and redo.

The lens accepts repeatable `measure resources --scenario ID` selections. It
runs each selected scenario's full sweep with normal contracts, adaptive repeats,
controls and provenance. Selection is recorded as focused; unknown names fail
before launch, and scenario filtering cannot narrow `measure promises`. Missing
operations remain missing coverage. Runner cost metadata records the selection.

The final focused run has stable controls but still fails both session budgets:
10,000-tab initial persistence 4,406.867 ms and full restore 344.056 ms, versus
180 ms each. The persistence sample records roughly 7.3 GB cumulative allocation
and 822 MB process writes for its 40 MiB text fixture; storage work remains the
next target. Do not interpret this as a completed promise. Evidence is in
Scratchpad `target/analysis/co-development/cycle-7-vm/`; the body-batching-only
diagnostic is in `cycle-7/`. Validation: 701 app tests passed (seven ignored),
then all 96 database tests passed after final failure cleanup (three ignored).
87 lens tests and strict Clippy passed in both repositories.

## Cycle 8: bound chunk transactions by bytes and order document writes

Small bodies now share a transaction up to 1 MiB, retaining the four-millisecond
scheduling limit. A next chunk that would exceed the byte limit is returned to
the front of the queue. A mixed-size regression checks FIFO order, per-drain
bytes and restored contents. Capture preparation, document metadata and content
writes follow document-key order to avoid scattered B-tree updates.

Persistence probes report captured live-text bytes. The lens exposes process
write bytes per captured byte, explicitly distinguishing this denominator from
changed text. Missing/zero denominators remain unknown. The console surfaces
the maximum observed ratio alongside latency and allocation measurements.

The focused run had stable controls. At 10,000 tabs, initial save was
3,267.207 ms with 6.96 process-write bytes per captured text byte; full restore
was 336.645 ms. Both still exceed 180 ms. Production paste also fails 150 ms.
Cold reads during concurrent saves straddled 4 ms across independent runs
(p99 observations 3.943, 4.561 and 3.557 ms), so the lens correctly reports
uncertain rather than pass. Evidence: Scratchpad
`target/analysis/co-development/cycle-8/`. Validation: 702 app tests passed
(seven ignored), 88 lens tests passed, and strict Clippy passed in both repos.

## Cycle 9: retain original allocations and widen metadata scanning

Original text storage shares the supplied String without copying its body into
an Arc<str>. Snapshot tests verify allocation retention and isolation after an
edit. Metadata inspection skips printable ASCII in 32-byte blocks, with
exhaustive classifier and scalar-reference boundary tests. The edited-preview
sweep now includes the required 10,000 pieces without removing smaller cases.

The lens preserves missing resource latency and status as unknown rather than
inventing zero milliseconds or success. The final focused evidence is in
Scratchpad `target/analysis/co-development/cycle-9-final/`: edited previews
complete in at most 5.9 ms; 10,000-tab build and installation still miss their
140 ms budgets (239.8 and 257.8 ms). Buffer construction accounts for 231.5 ms
of the targeted build, while workspace construction accounts for 4.2 ms.
The allocation change removes the full-body copy, but does not resolve latency.
Validation: 704 app tests passed (seven ignored), 89 lens tests passed, and
strict Clippy passed in both repositories.

## Full promise check after cycle 9

All eight producers completed in 884.2 seconds. The review reports six failing
promises and Many Views at risk, with 16 budget misses and six unmet scale
checks. Search UI workflows all passed their workload contracts and budgets.
Capacity met the 10,000-file visibility/completion targets and tab/view targets;
GB indexing, store-backed search and paste remain below promised scale. Twelve
resource scenarios still exceed their budgets. Prepared preview and fragmented
mutation rows now pass with the required workloads.

Evidence: Scratchpad `target/analysis/co-development/promises-after-cycle-9/`.
Capacity controls were stable. The search-workflow control was noisy (within-run
max/min 1.562), so this is not an accepted all-promise baseline. The review also
propagates noise from the derived speed report to every promise; this warrants
more precise attribution rather than weakening measurement thresholds.

## Cycle 10: attribute cold reads to the reader and reuse its statement

The dedicated foreground reader exposes its Linux thread I/O counters for
profiling. Normal reads do not sample counters. Cold-read verification now
excludes concurrent writer and background-reader I/O; unsupported counters stay
unknown. The reader reuses its body-query statement, drains each cursor, and
recreates the statement after a failed query. Regression coverage exercises
missing rows, rebinding, and separation of foreground/background counters.

The lens requires reader-scoped raw samples and recomputes the verified count
and p99 before closing the cold-read measurement gap. Process-scoped or
inconsistent claims cannot prove it. Failed/missing resource statuses also
cannot pass a budget; malformed observations are invalid, not unbudgeted.

The focused run had stable controls. All 128 cold reads were physically verified
on the foreground reader during 362 saves; observed p99 was 3.547 ms against
4 ms. This is one independent run, not a population-tail guarantee. The 64 MiB
store-backed search resource workload also passed its declared five-second
budget; the stricter capacity search promise remains unresolved. Evidence:
Scratchpad `target/analysis/co-development/cycle-10/`. Validation: 705 app tests
passed (seven ignored), 89 lens tests passed, and strict Clippy passed in both.

## Cycle 11: fast ordinary-text inspection and relevant noise attribution

Text inspection uses independent vectorizable passes for ordinary UTF-8, tabs
and line feeds. CR and possible artifact lead bytes retain the existing decoder.
Tests compare scalar character spans across control/Unicode boundaries and check
CRLF spanning a short prefix and a long fast-path span. Temporary construction
diagnostics were removed from source before the final measurement.

Both 10,000-tab resource paths now pass 140 ms with stable controls: targeted
construction 101.453 ms and tab-count installation 118.074 ms. Targeted buffer
construction accounts for 91.315 ms. Production paste still fails and requires
separate publication/flush attribution. Evidence: Scratchpad
`target/analysis/co-development/cycle-11/`.

The lens now applies noisy measurement controls only to promises using the
associated rows. Derived speed-report generation is not treated as an independent
performance measurement. Relevant noisy sources still prevent a passing verdict
and whole-run baseline acceptance; real failures remain failures. Tests cover
search noise leaving unrelated view evidence unaffected, resource dependencies,
and preservation of failure status. Validation: 706 app tests passed (seven
ignored), 90 lens tests passed, and strict Clippy passed in both repositories.

## Cycle 12: measure paste publication separately from durability

Paste publication ends after the new text and queued typing have reached a
rendered/tessellated frame. The full workflow and subsequent chunk flush remain
separate metrics. Both resource and capacity probes use the publication endpoint;
the existing 150 ms budget is unchanged. Process-wide preparation diagnostics
record store binding, normalization, mutation (including storage waits), and
inspection wall time. They are not CPU measurements or an exclusive partition.

The lens requires the declared publication endpoint, preserved queued input,
no injected writer pause, and consistent publication/flush/workflow intervals.
Missing or legacy endpoint claims cannot close the paste measurement gap.

Three independent launches observed 64 MiB publication at 137.711–148.501 ms.
128 MiB still failed at 474.890–2163.860 ms; mutation dominated, and inspection
added 70–131 ms by scanning new storage-backed text. CPU controls were noisy
(max/min 1.751), so this is diagnostic evidence rather than an accepted baseline.
Evidence: Scratchpad `target/analysis/co-development/cycle-12/`. Validation:
706 app tests passed (seven ignored), 91 lens tests passed, and strict Clippy
passed in both repositories.

## Cycle 13: inspect replacement inputs without storage read-back

Prepared replacement metadata now streams unchanged original spans and the
normalized input already owned by the request. It avoids re-reading newly sealed
bodies. Differential tests cover Unicode, control characters, CR/LF boundaries,
multiple replacements and same-position insertions. A 40 MiB store-backed test
checks reader requests, exact metadata, and undo/redo after preparation.

Project budgets can now declare a minimum number of independent launches.
Cold reads require three, and the lens rejects missing or inconsistent launch
coverage instead of accepting a surviving subset. Adaptive repeats still apply
above the declared floor. Validation: 708 app tests passed (seven ignored),
93 lens tests passed, and strict Clippy passed in both repositories.

The focused evidence in Scratchpad `target/analysis/co-development/cycle-13/`
remains failing: verified cold-read p99 values were 4.109, 3.177 and 4.242 ms
against 4 ms; 128 MiB paste publication was 1111.366 ms. Paste inspection took
13.104 ms, while mutation/storage waits consumed 1067.051 ms. Controls were
noisy (max/min 2.285). Subsequent host inspection also found sustained high
Linux I/O pressure, so these timings are not an accepted clean baseline.

## Cycle 14: bounded search planning and focused capacity sweeps

Scratchpad now produces line-aligned search windows lazily. Production search
plans at most eight windows ahead of matching; single-thread probes consume the
same iterator. This avoids planning the whole file before matching and preserves
line boundaries, context overlap, match ordering and cancellation. Tests cover
store-backed planning read counts and full-result equivalence for Unicode,
nonoverlapping plain text and regex with one and four workers.

The lens accepts repeatable `--scenario` selection for capacity runs. It validates
IDs before launching, preserves complete size sweeps and existing budgets, and
retains focused-selection metadata through the typed artifact. Promise runs
cannot be narrowed. Validation: 710 app tests passed (seven ignored), 94 lens
tests passed, and strict Clippy passed in both repositories.

Evidence: Scratchpad `target/analysis/co-development/cycle-14/`. Focused resources
completed in 3.76 seconds and capacity in 30.35 seconds. Store-backed search still
fails the 180 ms capacity budget: observed medians were 247.475 ms at 64 MiB,
925.218 ms at 256 MiB and 3481.982 ms at 1 GiB. The 64 MiB resource scenario stayed
within its separate 5-second budget. These are targeted observations, not a full
promise baseline; other host builds and storage pressure remain relevant.

## Cycle 15: incremental fixture fidelity and indexed history batches

Scratchpad restores history positions, entries and spans in batches of 64
documents. Row validation still attributes corrupt payloads to their document,
checks packed/row-span conflicts and orphans, and preserves exact replay. The
queries cast `json_each` values to TEXT before joining the document indexes.
A query-plan test requires indexed searches for all three history queries.

The first two candidates were rejected: `IN (SELECT value FROM json_each(?))`
scanned the history tables, and an untyped cross join still scanned them once
per requested document. The 10,000-tab observations were 12.401 and 54.362 seconds.
These failed runs remain in `cycle-15/` and `cycle-15-query-fix/`; neither query
form is in the final implementation. The final indexed query measured 312.5 ms
at 10,000 tabs and still fails the unchanged 180 ms restore budget.

Incremental-save preparation now records a one-character insertion instead of
replacing the whole document. This preserves original storage and exact undo/redo
across persistence. Maintenance fixtures declare revision 2 and their edit
preparation; the lens checks that declaration and rejects old replacement
fixtures as forget evidence. Final aged-forget samples stayed within the existing
180 ms budget (maximum 122.6 ms) on the seeded 3 GiB store.

Validation: 713 app tests passed (seven ignored), 96 lens tests passed, and strict
Clippy passed in both repositories. Final targeted evidence is in Scratchpad
`target/analysis/co-development/cycle-15-indexed/`. Full promise verification is
still outstanding; restore, search and other known failures remain open.

## Cycle 16: batched content manifests and scaling diagnostics

Restore now reads piece manifests, backing references and RAM tails in indexed
batches alongside history. Shared decoders retain the single-document checks for
checksums, contiguous positions, UTF-8, lengths and missing references. Bodies
remain lazy, cache owners remain distinct, and decoded history moves into the
restored document instead of being cloned. Query-plan tests require index seeks
for each content table. Existing recovery, quarantine and exact-replay tests
exercise the new batch path.

The lens reports observed workload medians, time per unit and adjacent size/time
multipliers. It separates units and measurement scopes and abstains from a
comparison when any observed trial is invalid. The CLI includes the final step's
per-unit change. This is diagnostic evidence, not a fitted complexity guarantee,
capacity extrapolation or budget override.

Validation: 714 app tests passed (seven ignored), the affected 102 database tests
passed after the final move-only refinement, 97 lens tests passed, and strict
Clippy passed in both repositories. Focused evidence in Scratchpad
`target/analysis/co-development/cycle-16/` completed in 8.54 seconds. Full restore
at 10,000 tabs measured 294.5 ms and still fails 180 ms; startup-visible restore
measured 25.5 ms and passes. Full restore's last step had a 1.04x time-per-tab
multiplier, rather than the rejected candidates' sharply increasing cost.
