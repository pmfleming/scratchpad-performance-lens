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

## Progress
- Stage 1 foundation checkpoint: frozen project manifest/benchmark metadata and promise membership/scale inventory; corrected noise propagation, opportunity text, uncertain evidence coverage, capacity labels, and unsupported write amplification. Cold-read output includes sample count and nearest-rank position.
- Full lens suite passed: 104 library tests and 11 integration tests. Strict all-target Clippy, formatting and diff checks passed.
- Stage 1 is not yet complete: attributed syscall collection is not implemented (ratios remain null without it); cache configuration is observed only where probes request storage configuration; sampling minima are unchanged; complete behavioral gate inventory/version-change tooling remains to be extended.
- Scratchpad companion exposes actual cache limits and app-default comparison without modifying policy. Library check and focused cache-configuration test passed. Committed as Scratchpad 0727bec.
- Stages 2–7 have not been implemented. No new performance audit or optimization experiment has run.
- Baseline lens library suite: 99 tests passed.

Foundation commits: lens d100a10; Scratchpad 0727bec. The follow-up commit corrects uncertain-evidence advice and tests covered-but-at-risk behavior. These are checkpoints within stage 1, not completion of the seven-stage plan.
