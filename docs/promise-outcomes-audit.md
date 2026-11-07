# Outcome-contract audit

Date: 2026-11-07. Scratchpad revision: `d92bb47`.

All eight producers in a fresh `measure promises` run completed successfully.
The run took approximately 17 minutes including release compilation. This
validates the measurement integration; it does not mean all promises passed.
Existing budgets, scopes, workloads and launch minima were retained.

| Promise | Observed health |
|---|---|
| Large Files | fail |
| Many Files | fail |
| Search | fail |
| Many Tabs | fail |
| Many Views | pass |
| Large Text Mutation | fail |
| Session Persistence Restore | fail |

All seven promises had covered evidence. Six scale targets remained unmet.
Noise was reported for capacity and resources and retained in dependency-local
health decisions. This was a single integration audit, not a paired optimization
experiment; no speedup over cycle 25 is claimed.

## Contract validation

- Both required storage-backed frame outcomes validated with no mismatch.
- All 12 production search workflow rows completed and met their timing budgets.
- All 150 raw capacity observations validated their declared outcomes.
- All 158 resource observations across 35 scenarios validated their outcomes.
- Capacity paste was within budget through 8 MiB and first exceeded it at 64 MiB.
- Capacity production file-size search was within budget through 64 MiB and first
  exceeded it at 256 MiB.
- Production paste reported the configured 32 MiB admission threshold and measured
  pending/cache/tail observations. FULL exposure and total accepted uncommitted
  bytes remained unknown. NORMAL body-flush duration was not substituted for FULL
  exposure.
- Residency measurements were absent, as expected: Scratchpad still lacks the
  seven-operation probe. The separate missing-probe smoke check produced 66
  incomplete groups, zero trials and a nonzero exit status.

## Final reporting correction

The audit was launched before the final reporting polish and uncertainty fix.
Its lens executable fingerprint, retained in the provenance sidecars, is
`eae78ead474fa446`. Its auxiliary outcome matrix counted individual outliers as
failures for `aged_store_forget` and `cold_reads_during_save`, although their raw
aggregate resource verdicts were correctly **uncertain**. The primary promise
health decisions above used the existing aggregate verdicts and are unaffected.

Commit `13de9ed` makes the outcome matrix preserve those aggregate verdicts. A
regression test verifies that a lone outlier stays uncertain and a failing
aggregate stays failed. Recounting the recorded raw verdicts gives **10 unique
failing gates and two uncertain gates**, rather than the pre-fix auxiliary
count of 12 failures. The original audit JSON and provenance have not been
rewritten or restamped. A future fresh run with the final executable will
generate the corrected matrix directly.

The unique-gate count covers the measured required sweeps, including capacity
steps above promised scale. It is not interchangeable with the legacy total of
10 per-promise budget misses: shared membership and scale semantics differ.

## Final checks and artifacts

The final lens code passed 117 library tests, 12 integration tests, strict
all-target Clippy, formatting and diff checks. Both the cycle-25 numerical gate
snapshot and the new outcome/policy snapshot passed. Scratchpad library and all
binaries checked; its three focused outcome tests passed. CLI validation and
all five schema exports succeeded. A Valgrind format smoke check confirmed the
Ir summary format; no application phase-instruction measurements are claimed.

Raw run and sidecars: `/home/laufan/Projects/scratchpad/target/promise-outcomes-audit/`.
Raw subprocess logs: Scratchpad's ignored `target/splens-probes/` directory.
The standalone [promise declarations](promise-outcomes.md) intentionally show
unmeasured verdicts; they are definitions, not a copy of this audit.

The [implementation record](promise-plan-implementation.md) and
[diagnostic protocol](residency-diagnostics.md) identify the remaining
application probes, full exposure instrumentation and optimization experiments.
