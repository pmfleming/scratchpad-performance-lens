# Scratchpad and the performance lens: 25-cycle report

The 25 paired improvement cycles are complete. Work stops here at the user's request, after the full audit of cycle 25. The original goal—meeting or exceeding every performance promise—has **not** been achieved. Several individual paths are substantially faster, and the lens now makes stronger, more useful distinctions between a fast observation and valid evidence of a promise.

The measured implementation commits are Scratchpad `b964a13` and performance lens `5aa2d08`. The subsequent report commits change documentation and reporting artifacts only. The full chronological engineering record, including test counts and focused artifact paths, is [the cycle log](../performance-cycles-2026-11-06.md).

## Final audit: promises remain unmet

All eight producers completed in **806.440 seconds (13 min 26 s)**. The final review reports **6 fail, 1 at risk, 0 pass**, **11 reported budget misses**, and **6 unmet scale checks**. Six promises have covered evidence; session persistence/restore is marked thin because the aged-store forget result is uncertain. Coverage means available evidence, not a passing result.

| Promise | Final verdict | What prevents completion |
|---|---|---|
| Large Files | Fail | 1 GiB background indexing 7,866.191 ms / 5,000 ms; 2 GiB UTF-8 load 8,919.663 ms / 5,000 ms. Cold-read p99 across three launches: 3.679, 4.068, 4.114 ms / 4 ms. |
| Many Files | Fail | 10k first visibility passes at 84.533 ms / 180 ms, and background hydration at 953.547 ms / 5,000 ms. Lazy-open tracking still takes 1,013.034 ms / 180 ms; 50k construction takes 432.784 ms / 180 ms. Shared session-save/restore gates also fail. |
| Search | Fail | All 12 workflow medians pass; 10k first render 26.813 ms / 120 ms. The production 1 GiB matcher takes 1,445.451 ms / 180 ms; only 64 MiB is demonstrated within that capacity budget. |
| Many Tabs | Fail | 10k construction is 96.326 ms / 140 ms (noisy control), installation 116.048 ms / 140 ms, and manipulation passes through 20k tabs. Required session save/restore still fails. |
| Many Views | At risk | Capacity passes at 1,000 views and 1,000 splits, with no reported budget misses, but view-resource controls are noisy. |
| Large Text Mutation | Fail | 128 MiB paste publication: resource observation 1,404.355 ms / 150 ms; capacity median 353.238 ms / 150 ms. Different probe fixtures/contexts are kept separate. |
| Session Persistence Restore | Fail; thin evidence | At 10k tabs, first-save launches 1,280.092–1,322.585 ms / 180 ms; full retained restore 284.833–311.192 ms / 180 ms. Aged-store forget has one 215.650 ms outlier / 180 ms and is uncertain. |

Fresh confirmation of the major targeted gains: 100k retained edits **93.560 ms / 180 ms**, retained production results over 1k targets **16.245 ms / 180 ms**, unchanged 10k-tab saves **97.277–101.827 ms / 180 ms**, and visible startup restore **17.557–19.024 ms / 180 ms**. The session group includes a noisy launch, so its passing observations are not clean baseline proof. These targeted passes do not override failed promise dependencies.

The audit-checkpoint sequence is:

| Audit after cycle | Reported budget misses | Unmet scale checks | Promise health |
|---|---:|---:|---|
| 1 | 19 | 7 | 6 fail, 1 at risk |
| 9 | 16 | 6 | 6 fail, 1 at risk |
| 16 | 12 | 4 | 6 fail, 1 at risk |
| 25 | 11 | 6 | 6 fail, 1 at risk |

The later increase in unmet scale checks reflects cold-read/aged-store failures or uncertainty as well as stricter evidence requirements; it is not a monotonic progress score. The budget-miss count is the review's aggregation, with shared evidence across promises and capacity failures represented separately. See [all current resource rows, capacity sweeps, workflows and producer costs](audit-tables.md).

## Lens limitations exposed by the final audit

These remain unfixed at the requested stopping point:

- Top-level `meta.noisy_sources` is empty even though scenario-level gaps and raw launch controls identify noisy resource/capacity evidence. Read scenario and launch quality; an empty top-level list is not a clean-baseline certificate.
- Many Views correctly has `promise_health: at_risk`, but its generated opportunity text says the promise is passing. The structured health and noise evidence take precedence; the recommendation wording needs correction.
- Aged-store forget has complete measured evidence with one over-budget outlier and an `uncertain` verdict, but the aggregate review describes it as “Missing valid resource evidence” and makes session coverage thin. That wording conflates uncertainty with absent measurements.
- The background-hydration capacity row shares a first-visible display label. This report uses scenario IDs and the correct endpoint to distinguish them.
- Named phase and completion contracts now improve attribution, but there is still no clean accepted all-promise baseline, automatic causal attribution of speedups, or proof of display-present performance. Fresh flamegraphs were excluded from this audit because their provenance was unverified.

## Read the graphs

Open [the interactive graphs](graphs.html) or the [standalone SVG](trends.svg). The [extracted data](data.json) contains the extracted observations from every discovered cycle artifact under `target/analysis/co-development`, including intermediate and rejected trials, source hashes, measurement scopes, and control status. The chart displays the final focused run available for a cycle plus periodic full audits; rejected candidates remain in the data and experiment table below.

The horizontal axis spans cycles 1–25. Empty positions mean no comparable measurement was collected in that cycle. Dots are medians; whiskers are observed minima/maxima, **not** confidence intervals. The vertical axis is logarithmic latency divided by the original budget: below 1× is faster than the budget. Hollow points indicate noisy or unknown CPU controls. Separate marks show full audits. There is no interpolation and no assumption that an unmeasured cycle passed.

Measurement changes are marked explicitly. In particular, excluding fixture setup or teardown, fixing result retention, and switching a probe to the production matcher must not be counted as application speedups. CPU-control stability does not establish storage or scheduler stability. All results are observations on this host, not population-tail guarantees.

## What improved in the application

- **Search rendering:** offscreen result-group culling reduced the controlled 10,000-target first-render median from 109.017 to 43.977 ms in cycle 1, with maximum 150.091 → 54.490 ms against 120 ms. Reserving the wrapped editor gutter then eliminated a seven-pixel width change that forced expensive reflow. Cycle 3's active-buffer median fell from the previously observed 167.689 ms to 3.746 ms, but that comparison had host noise; the final audit provides fresh confirmation.
- **Large tab sets:** after separating fixture work, caching the default undo-budget memory sample and accelerating ordinary UTF-8 inspection, 10,000-tab construction reached 101.453 ms in cycle 11 against 140 ms. The cycle-9 prepared-operation observation was 239.835 ms. Earlier setup-inclusive results are not a like-for-like speedup denominator.
- **Repeated editing:** correcting temporary pin ownership avoided copying a growing text tail on every append. At 100,000 edits, cycle 19 → 20 latency fell 1,314.543 → 91.603 ms against 180 ms; cumulative allocation fell approximately 20.246 GB → 0.230 GB. The revised probe also proves that the real 100,000-entry history is retained.
- **Search result publication:** after first correcting the probe, the same 1,000-target production workload fell 1,151.317 → 16.261 ms in cycle 21 against 180 ms. Publishing increasingly large prefixes, instead of cloning the accumulated result every 64 matches, removed quadratic copying. Allocation fell approximately 1.452 GB → 22.13 MB; all 49,110 matches remained retained.
- **Session storage:** bounded body batches, statement reuse, indexed batched restore queries, indexed cache eviction, and batched manifest/history/layout writes reduced work while keeping durability. At 10,000 tabs, first-save observations moved from 4,406.867 ms in cycle 7 to a cycle-24 median of 1,209.525 ms. This is still far above 180 ms, and the cycle-24 launches included a 2,570.381 ms outlier. History writes fell from 1,359.099 ms in cycle 22 to 299.108 ms in cycle 23; layout writes fell from 261.394 ms in cycle 23 to roughly 128–134 ms in cycle 24. These named phases support the optimization claims more directly than attributing all wall-time movement to SQL batching.
- **Store-backed search:** lazy bounded planning avoids reading the entire file ahead of matching. Cycle 25 reused a bounded worker cohort. Under the newly corrected production-matcher contract, 1 GiB completion changed from a 1,410.018 ms baseline median to 1,363.165 ms in the focused final run; median-trial process CPU fell 2,760 → 2,480 ms. This is a modest improvement and remains above 180 ms. Earlier serial-probe timings are a different measurement method.

Decimal GB/MB above describe allocator byte counts; KiB/MiB/GiB describe binary workload sizes. Cumulative allocation is traffic, not peak resident memory.

## What improved in the performance lens

**Accuracy.** Workload sizes and thresholds now come from the application manifest. Raw bytes, result counts, retained histories, production code paths, completion endpoints, repetition counts and scope are validated before evidence can prove a promise. Missing values stay unknown. Failed or incomplete trials cannot become successful medians. Cold-read evidence is attributed to the foreground reader thread and recomputed from physically verified samples, rather than trusting process-wide writer activity. Unchanged saves may legitimately write zero bytes, but only with explicit proof that no transaction committed.

**Utility.** Search reports separate dispatch, first result, first rendered result and completion, with editor layout and culling counters. Resource reports expose named phases, validated nonoverlapping database subdivisions, unattributed remainder, observed scaling, allocation, process I/O, and write amplification with a stated denominator. The capacity report now distinguishes a demonstrated within-budget workload from the first failing step; it no longer presents a failed step as a useful capacity limit. Actual store page/cache configuration is recorded rather than assumed.

**Efficiency.** The promise lane selects required evidence and records producer wall time. Focused resource and capacity runs keep complete sweeps and contracts while selecting the operation under investigation. Shared fixture groups reuse expensive session and aged-store preparation without relaxing launch minima. For the aged-store group, three shared 3 GiB fixtures replaced seven separately prepared fixtures, while keeping three cold-read launches and nine maintenance trials. Search-speed reporting reuses the freshly produced Criterion data. Focused session runs became practical in seconds; full audits remain much more expensive and are not replaced by focused passes.

**Confidence.** Noise is attributed to relevant sources rather than contaminating unrelated promises through derived reports. Warmed, launch-local CPU controls and minimum independent launch counts improve interpretation. They do not certify I/O isolation. The final tables preserve raw gate failures even when noise prevents an accepted baseline.

## Every paired cycle

Each row describes the final retained work. The linked chronological record contains detailed validation. Commits are local; nothing was pushed.

| Cycle | Scratchpad work | Lens work and observation | App / lens commits |
|---|---|---|---|
| 0, setup | Captured existing work and probe contracts. | Added promise-selected lane and wall-time accounting. Initial broad run stopped on an unrelated diagnostic timeout; not a baseline. | `ad488e7` / `1f3a2bb` |
| 1 | Cull offscreen collapsed search groups, preserve scroll geometry and controls. | Delivery/render/layout attribution. Controlled 10k-target first render 109.017 → 43.977 ms; active-buffer reflow still slow. | `dec2962` / `a36005d` |
| 2 | Read workflow fixture sizes/budgets from the manifest. | Validate raw bytes, matches and summaries. A reduced 100-target run remains diagnostic even when fast. | `946f31f` / `51dcaeb` |
| 3 | Reserve wrapped-editor gutter to prevent needless full-paragraph reflow. | Report prepared/result widths and layout work. Active first render 3.746 ms; noisy host, not accepted baseline. | `d9dd4d4` / `8b248d1` |
| 4 | Deliver selected loaded file before inactive shells; preserve order/fallback and avoid repeated ID scans. | Reject placeholders; expose overlapping background-lane busy times. 10k files first visible 81.878–83.432 ms; 50k stress still slow. | `ce0c0d1` / `139ee12` |
| 5 | Move preview/mutation/tab fixture preparation outside operation timers; retain measured objects. | Enforce prepared-operation scope on six probes. Measurement correction; tab construction still about 1.1 s. | `aa48a99` / `331d8dc` |
| 6 | Cache default undo-budget memory sampling for one second; accelerate printable ASCII inspection. | Attribute tab buffer/workspace construction. 10k tabs 261.832 ms, still above 140 ms. | `1ec8417` / `d204f2b` |
| 7 | Batch small body captures (64 documents / 1 MiB); reuse bounded SQL execution objects with failure cleanup. | Focused resource selection with full contracts/provenance. First save 4,406.867 ms; restore 344.056 ms. | `efc5933` / `a68f405` |
| 8 | Bound body transactions by 1 MiB and four milliseconds; preserve FIFO; order document writes. | Expose process writes per captured text byte. First save 3,267.207 ms; cold reads straddle 4 ms. | `a253250` / `789cfc3` |
| 9 | Retain original String allocation; widen printable ASCII scans; include 10k-piece preview workload. | Keep missing latency/status unknown. Prepared previews pass; tab build 239.835 ms still fails. Full audit: 16 misses. | `9a9f405` / `70606dc` |
| 10 | Reuse foreground reader statements; expose reader-thread I/O counters. | Require and recompute physical cold-read proof. One launch p99 3.547 ms / 4 ms; stronger repeated proof still needed. | `80f319a` / `09f94da` |
| 11 | Vectorize ordinary UTF-8 metadata scans, preserving control/CRLF behavior. | Attribute noise only to dependent promises. 10k tab build 101.453 ms / 140 ms. | `17ad6ec` / `1b099b3` |
| 12 | Separate paste publication from later flush; record preparation subphases. | Require rendered publication, queued input and consistent intervals. 64 MiB 137.711–148.501 ms; 128 MiB fails; noisy. | `50af326` / `8069962` |
| 13 | Inspect replacement inputs without rereading newly stored bodies. | Enforce minimum independent launches; cold reads require three. Paste inspection ~13 ms, storage/mutation still dominates. | `536a959` / `71a0df5` |
| 14 | Plan line-aligned search windows lazily with bounded eight-window lookahead. | Focused capacity sweeps preserve thresholds/full sizes. 1 GiB serial diagnostic search still ~3.482 s. | `96c45fb` / `b67c7e5` |
| 15 | Batch history restore using indexed, typed query joins; fix aged-store incremental-edit fixture. | Require fixture revision and incremental history preservation. Rejected two scanning query forms; final restore 312.481 ms. | `6b7d513` / `fc85b01` |
| 16 | Batch indexed content-manifest reads; lazy bodies and ownership-preserving decoders. | Observed scaling, time per unit, adjacent-step growth without extrapolation. Focused restore 294.480 ms; full audit 12 misses. | `00fc1da` / `df2ecc5` |
| 17 | Index bounded LRU eviction instead of scanning every retained chunk. | Warm and localize CPU controls. Save preparation ~1,436 → 550 ms across observations; total still 2,418.706 ms. | `450b9e8` / `146cb7d` |
| 18 | Allow combined resource scenarios and declare shared fixture groups. | Validate groups, launch minima, order and missing/extra rows. Reuse expensive stores; retain all required repetitions. | `68c7d08` / `972b564` |
| 19 | Reject 16 KiB pages; retain verified newline scan optimization and expose actual store settings. | Validate observed engine/page/cache configuration. Final cold p99 3.244/3.848/3.893 ms; history baseline 1,314.543 ms. | `0c8a6d2` / `74566cf` |
| 20 | Keep owned extraction pins, avoid mutation-only Arc copies; validate real retained history. | Require actual history count/bytes and revision. 100k edits 91.603 ms; ~88× less cumulative allocation. | `5d469a9` / `52bf983` |
| 21 | Prepare/retain production search fixture; geometrically space partial result publication. | Validate complete retained target/match counts and endpoint. Corrected baseline 1,151.317 → 16.261 ms at 1k targets. | `ac731f4` / `121bbf6` |
| 22 | Instrument session prepare/write/commit and restore database phases. | Validate child intervals and remainder; withhold inconsistent fractions. Diagnostic cycle, not an optimization claim. | `e58e6e1` / `e9809c9` |
| 23 | Batch proven-new document/history rows in the existing durable transaction. | Surface largest validated database child phase. History writes 1,359.099 → 299.108 ms; save still 1,321.565 ms. | `569ba43` / `fea16dd` |
| 24 | Batch proven-new layouts; retain restored workspace beyond timing/sampling; expose no-op transaction flag. | Validate restore contents/endpoint, three session launches and legitimate zero-write saves. Save median 1,209.525 ms; retained restore 263.534 ms. | `a709316` / `1cb022c` |
| 25 | Reuse bounded fragment-search worker cohort; capacity probe uses the actual production matcher and completion count. | Enforce code path, exact bytes, all returned ranges and one expected match; show true within-budget capacity. 1 GiB focused search 1,363.165 ms / 180 ms. | `b964a13` / `5aa2d08` |

## Experiments, corrections and unsuccessful approaches

| Attempt | Evidence or failure | Decision |
|---|---|---|
| Broad initial `measure all` | Unrelated text-layout diagnostic timed out after useful producers had completed. | Preserve partial evidence; use the required promise lane. Do not call the interrupted run a baseline. |
| Unwrapped viewport-projection and font diagnostics, cycle 3 | Drafted long-line projection with cursor/display mapping and tried named/proportional font smoke probes. The production default was wrapped text, so this did not target the actual cause. Tracing identified the seven-pixel gutter change. | Discard the projection patch; keep wrapping and fix stable layout width. Patch, font smoke data and layout trace are archived under `target/co-development/cycle-3/`. |
| Reduced search workload, cycle 2 | 100 targets could look fast while failing the 10,000-target contract. | Deliberate lens negative check, not an optimization; excluded from promise evidence. |
| Selected-file reordering, cycle 4 | Fixed promised 10k first visibility and prior background timeout behavior; 50k stress still exceeded limits. | Retain benefit and remaining stress failures. |
| Body batching and allocation retention, cycles 7–9 | Reduced some work but session save, restore and tab construction still missed budgets. | Retain correctness-tested changes; continue attribution rather than declare success. |
| Ordinary scan prototypes, cycle 11 | An archived combined 32-byte scan prototype was explored before the independent vectorizable passes. Controls, artifact lead bytes, tabs and CRLF still required exact decoder semantics. | The draft was not shipped; retain only the scalar-equivalence-tested fast path. |
| Paste endpoint correction, cycle 12 | Original elapsed time also included later durability work. Correct publication still fails at 128 MiB. | Report publication and flush separately, retain 150 ms and durability behavior. |
| History SQL `IN (SELECT value FROM json_each(?))`, cycle 15 | 10k restore about 12.401 s; query plan scanned. | Rejected. |
| Untyped cross-join history query, cycle 15 | 10k restore about 54.362 s; still did not use the intended indexed seek. | Rejected. Typed `CAST` plus ordered `CROSS JOIN` produced indexed lookups and ~312 ms restore; query-plan regression tests added. |
| 16 KiB database pages, cycle 19 | First save 3,397.763 ms, allocation ~12.322 GB, writes ~484 MB; import writes improved but elapsed remained noisy and CPU similar. | Rejected; retain 4 KiB defaults and measured-configuration reporting. |
| Drop all owned preflight pins, cycle 20 | Broke 36 tests because extraction still needs owned pin bodies. | Rejected before commit; distinguish mutation-only pins from extraction ownership. Full corrected suite passed. |
| Search probe reset/teardown behavior, cycle 21 | Old probe included fixture/reset work and cleared results before sampling. Corrected baseline was still 1,151.317 ms. | Fix probe first, then measure prefix-copy optimization on the same corrected workload. |
| Restore teardown in timer, cycle 24 | Old timer included dropping reconstructed state; heap snapshot came after drop. | Retain full restored state through timing and sampling; mark a method boundary. Do not call removed teardown a restore optimization. |
| Zero-write unchanged saves, cycle 24 | Lens treated legitimate zero `file_write_ns` as missing evidence. | Accept only explicit no-transaction, all-zero write subdivisions with complete counts. Slow/missing first saves still fail. |
| Serial capacity matcher, cycle 25 | Did not exercise production intra-buffer worker policy, and lacked an enforced final result count. | Replace with production matcher; separate new baseline from worker-reuse result. The 1 GiB promise still fails. |
| Noisy or slow repeats throughout | CPU noise and substantial I/O pressure; even stable CPU controls coexist with multi-second storage outliers. | Keep repeats and uncertainty; no selecting only the best run, no relaxed threshold. |

## How the workstreams fed each other

The loop was concrete: lens render attribution exposed reflow, so Scratchpad fixed width stability and the lens gained width diagnostics. Allocation and phase evidence exposed repeated tail copying and prefix cloning, so the app changed ownership/publication and the lens gained retained-result contracts. Session phase subdivisions identified history/layout writes, leading to batched SQL and more actionable dominant-phase reporting. Production search investigation exposed a serial capacity probe, so the probe and lens contract were corrected before evaluating worker reuse. Expensive cold-read preparation motivated shared fixtures, while correctness concerns required stricter launch and I/O verification.

No latency threshold was loosened. Manifest durability remains FULL; immutable body durability remains NORMAL. Pending text stays bounded at 32 MiB, and the chunk cache stays bounded at 16 MiB globally / 4 MiB per owner. New batching preserves ordering, cancellation and transaction/retry behavior covered by the relevant tests. Headless rendered results mean CPU layout/tessellation; GPU upload, compositor and display presentation are outside these measurements.

## Validation and practical limits

Cycle 25 passed **726 Scratchpad library tests (7 ignored)**, **109 lens tests**, and strict Clippy checks in both repositories. Production capacity, resource and workflow probes were built in release mode. Earlier cycles additionally exercised rollback/retry/reopen, query plans, Unicode and CRLF boundaries, exact matches, cancellation, worker failure/panic, bounded-cache model behavior and malformed evidence. Per-cycle counts and the scope of targeted versus full suites are in the chronological log; not every cycle reran every test.

The report is based on committed cycle-25 sources, earlier structured artifacts, raw trials and the cycle log. Many focused measurements were made before their cycle commit and record a parent revision plus dirty source. Full audits after cycles 1, 9, 16 and 25 are the only complete-review checkpoints; 25 cycles do **not** mean 25 full independent audits. The first audit had partial capacity evidence. Earlier contracts differ, so historical verdict counts describe what the lens concluded then, not a retroactive evaluation under today's stricter contract.

Fresh flamegraphs were not collected for every failing operation. Some time attributions are wall intervals, not CPU samples or exclusive critical-path partitions. Current large-file fixtures and headless workflows do not cover all real files, scripts, encodings, devices or display behavior. Stable controls do not eliminate storage contention. Aggregate promise coverage is evidence availability, not a claim that all measurements or promises pass.

## Remaining hypotheses, not executed

No cycle 26 was started. The following are leads from inspection, not measured fixes:

- Instrument the bounded reader queue's one-millisecond full-queue polling and distinguish enqueue wait from database service time before changing wakeups.
- Investigate byte-balanced UTF-8 inspection work: a large single span can defeat span-count parallelism. Preserve character boundaries, control handling and bounded pins. The final audit's dense multilingual paste benchmark still shows a large metadata phase.
- Examine serial storage-reader pressure and line planning for 1 GiB production search. An alternative pool or matcher would need exact-result, overlap, cancellation and memory-bound validation.
- Reconcile the lazy-open resource endpoint with the separately budgeted first-visible and background-hydration workflows. The current 180 ms gate remains binding; changing scope requires explicit fidelity evidence, not simply choosing the faster endpoint.
- Continue reducing first-save content preparation and batched content-reference work, then restore reconstruction/validation. Current phase data makes those investigations more specific.
- Investigate the outer parallel-target search failure path: same-generation read failure may still be treated as partial completion. Cycle 25's fragment-worker failure handling does not establish that the outer path is correct.

These leads are deliberately distinguished from shipped or tested work. The continuous optimization goal is paused at the user's requested stopping point.
