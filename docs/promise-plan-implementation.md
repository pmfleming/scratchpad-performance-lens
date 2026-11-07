# Experiment review and implementation record

Starting point: e4dd0e6. Keep the history and correct defects forward.
The range includes setup and the final report, in addition to the 25 cycles.
Failure to meet application budgets is not evidence that measurement validation should be removed.
Reviewed changed-file inventories, cycle evidence, contract diffs, and current validation/aggregation paths. This is an architectural retention review, not a line-by-line correctness certification.

| Commit | Decision |
|---|---|
| e4dd0e6 Report all 25 performance cycles with audited measurements and graphs | Keep historical evidence; corrections do not rewrite recorded measurements |
| 5aa2d08 Require production search completion evidence and clarify capacity limits | Keep |
| 1cb022c Require retained restore evidence and distinguish measured no-op saves | Keep |
| fea16dd Surface the largest validated database phase in resource summaries | Keep |
| e9809c9 Expose validated database phase subdivisions for save and restore | Keep |
| 121bbf6 Validate completed search-result storage at its prepared operation boundary | Keep |
| 52bf983 Require completed retained history instead of declared edit workload | Keep |
| 74566cf Report observed storage configuration without inferring engine defaults | Keep |
| 972b564 Reuse explicitly declared probe fixtures with complete launch coverage | Keep |
| 146cb7d Scope performance noise to complete independent probe launches | Keep dependency-local noise; fix top-level propagation and stale opportunity text |
| df2ecc5 Expose observed resource workload scaling without changing budgets | Keep |
| fc85b01 Require incremental edit fidelity for maintenance evidence | Keep |
| b67c7e5 Support focused capacity sweeps with complete workload contracts | Keep |
| 71a0df5 fix: require complete declared independent probe launches | Keep |
| 8069962 fix: require explicit paste publication evidence | Keep |
| 1b099b3 fix: attribute measurement noise to affected promises | Keep dependency-local noise; fix top-level propagation and stale opportunity text |
| 09f94da fix: validate reader-scoped cold samples and failed resource outcomes | Keep |
| 70606dc fix: preserve unknown resource timings and statuses | Keep |
| 789cfc3 feat: expose persistence write amplification with explicit scope | Keep denominator/scoping work; replace process-counter amplification inference |
| a68f405 feat: support validated focused resource measurement cycles | Keep |
| d204f2b feat: summarize validated resource phases by workload | Keep |
| 331d8dc fix: require declared resource measurement scopes | Keep |
| 139ee12 feat: report complete-profile background lane diagnostics | Keep |
| 8b248d1 feat: diagnose search reflow and non-layout render work | Keep |
| 51dcaeb fix: prevent reduced search fixtures from proving performance promises | Keep |
| a36005d feat: attribute search first-render latency to measured work | Keep |
| 1f3a2bb feat: add promise-focused measurement cycles and runner cost reports | Keep |

## Implementation qualifications
- Keep all numerical gates, workloads, scopes and launch minima. A cold-read sampling increase is a separately versioned strengthening, not silently part of freezing gates.
- Never subtract cancelled_write_bytes from syscall or device bytes: those counters have different meanings. Process wchar is not attributable store traffic either. Without attributed store I/O, amplification is unknown.
- Residency timing differences are paired diagnostic contrasts, not exclusive causal phase times.
- Retaining a chunk committed at NORMAL is not proof of FULL power-loss durability. Publication, body commit and manifest durability need distinct observations.
- A bounded pending queue is not a maximum bound on all user-visible uncommitted text; mutable tails and uncommitted references also matter.
- A profile override changes budgeting inputs, not the physical RAM/cache capacity of the machine.

## Foundation checkpoints
- Stage 1 foundation checkpoint: frozen project manifest/benchmark metadata and promise membership/scale inventory; corrected noise propagation, opportunity text, uncertain evidence coverage, capacity labels, and unsupported write amplification. Cold-read output includes sample count and nearest-rank position.
- Full lens suite passed: 104 library tests and 11 integration tests. Strict all-target Clippy, formatting and diff checks passed.
- At that checkpoint, attributed syscall collection was not implemented (ratios remain null without it); cache configuration was observed only where probes request storage configuration; sampling minima were unchanged; endpoint inventory/versioning remained to be extended.
- Scratchpad companion exposes actual cache limits and app-default comparison without modifying policy. Library check and focused cache-configuration test passed. Committed as Scratchpad 0727bec.
- Stages 2–7 had not been implemented at the foundation checkpoint.
- Baseline lens library suite: 99 tests passed.

Foundation commits: lens d100a10; Scratchpad 0727bec. The follow-up commit corrects uncertain-evidence advice and tests covered-but-at-risk behavior. These are checkpoints within stage 1, not completion of the seven-stage plan.

## Outcome and diagnostic tooling checkpoint

Scratchpad companion commit: `d92bb47`. Existing budgets, workload sweeps,
measurement scopes, required launches and code-path contracts are unchanged.
The gate inventory test verifies that against cycle 25. A separate versioned
snapshot freezes the new outcome, diagnostic and durability-policy declarations.

| Plan work | Delivered | Remaining |
|---|---|---|
| Outcome declarations and matrix (step 2) | 62 resource/capacity/workflow/frame declarations; observed endpoint validation; shared membership; unique failing-gate count; generated document | Criterion gates retain their existing benchmark metadata and are included in unique failures, rather than receiving a new probe outcome object |
| Residency ladder (step 3) | Seven-operation manifest, five-rung runner/importer, source binding, result/cancellation/residency contracts, coverage and noise checks | Scratchpad's actual seven-operation `residency_probe` and measured trials |
| Memory and exposure (step 4) | Separate policy declaration; current/peak metric validation; FULL exposure calculation; missing-metric reporting; 4/16/host profile protocol; raw pending/cache/tail observations where available | Application budget override plumbing, complete memory ownership and FULL-exposure instrumentation |
| Classification and optimization (step 5) | Diagnostic paired contrasts and classification for complete, stable imported/measured ladders | Actual classifications and evidence-driven editor changes |
| Retention/pending experiments (step 6) | Explicit policy-change labeling, preserving NORMAL/FULL distinction | Retention, pressure eviction and pending-headroom experiments |
| Instruction companion | Opt-in Callgrind runner, raw archives, validated whole-process Ir counts | Named-phase client instrumentation and paired application instruction results |
| Audit (step 7) | Fresh existing-gate audit completed against committed companion; all eight producers completed | An audit including residency/retention optimization evidence cannot be completed before those experiments exist |

The original seven-stage plan is not complete. This checkpoint completes the
lens tooling and its companion observations; it does not claim to make all
performance promises pass. Keep application changes driven by the missing
measurements rather than choosing speculative optimizations in the lens.

Validation: 117 lens library tests and 12 integration tests passed, including
both inventory snapshots, negative outcome/residency cases, source-bound import,
missing-probe behavior and CLI schema exports. Strict all-target Clippy and
formatting passed. Scratchpad library/binaries checked successfully; three
focused outcome tests passed. `splens validate` accepts the committed manifest.

The real `measure residency` smoke check exited nonzero as intended: it wrote
66 incomplete workload/fixture/profile groups, zero trials, and no inferred
timings. This confirms the absence is visible. A local Valgrind format smoke
check produced an unambiguous Ir summary; no application phase counts are
claimed from that check.

See [the diagnostic protocol](residency-diagnostics.md) and
[generated declaration matrix](promise-outcomes.md) for the supported commands
and artifact semantics. Raw audit output is under Scratchpad's ignored
`target/promise-outcomes-audit/`. The [audit notes](promise-outcomes-audit.md)
record the result and the matrix-only correction made during the run without
replacing historical cycle-25 evidence.

Implementation commits: lens `cf51363` (outcomes and diagnostic tooling),
`13de9ed` (preserve aggregate uncertainty); Scratchpad `d92bb47` (probe companion).
