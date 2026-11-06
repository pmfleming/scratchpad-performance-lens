# Cycle-25 report artifacts

Start with [the report](report.md). Open [interactive graphs](graphs.html) directly in a browser; no server or network dependency is required. [trends.svg](trends.svg) is a standalone export. [audit-tables.md](audit-tables.md) contains every final resource scenario at its largest workload, complete capacity sweeps, all search workflows and producer costs.

`data.json` is a committed extraction of 90 local artifacts / 881 workload observations across cycles 0–25 and four full audits. It retains intermediate/rejected trials, source SHA-256 hashes, relevant provenance, all observed values, contracts and phase details. `observations.csv` is the flat spreadsheet version. These snapshots remain usable if ignored `target/` directories are cleaned. Original full artifacts, raw probe logs and early cycle 3–6 diagnostics remain local under the paths cited in the report and cycle log; they are not all duplicated here.

Regenerate with Node.js, from the lens repository root:

```sh
node docs/report-cycle25/collect.mjs ../scratchpad/target/analysis/co-development docs/report-cycle25
node docs/report-cycle25/render.mjs docs/report-cycle25
node docs/report-cycle25/tables.mjs docs/report-cycle25
```

The collector reads existing evidence only; it does not run probes. The renderer and table generator need only committed `data.json`. `report.md` is the reviewed narrative, maintained separately so regeneration cannot overwrite its interpretation.

Chart points are source observations, not interpolated cycle scores. The default view selects the final focused artifact at each cycle plus full audits. The checkbox reveals intermediate/rejected artifacts. Per-point details identify the exact file; `scenario_index` and `sample_indices` in the data locate its raw rows. Resource values follow their declared budget JSON pointer, including publication and cold-read p99 rather than outer elapsed time. Capacity/workflow medians follow their producer statistic. Whiskers show observed ranges. Multiple cycles changed measurement contracts; boundaries and caveats must remain attached to exported charts.

Source host: AMD Ryzen 7 PRO 8840HS, 16 logical CPUs, Linux, NVMe ext4, performance power profile. These details describe the recorded environment, not controlled isolation. Final measured Scratchpad source was clean at `b964a137af24119adaefac693fc7faaa90013763`; lens implementation was `5aa2d08`. Reporting commits follow those measured implementation commits.
