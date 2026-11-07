# Charge completion is a condition, not a new source — 2026-11-08

The Scratchpad three-cycle campaign exposed a provenance defect. During a full
11-minute promise audit, BAT0 changed from `Charging` to `Not charging` while
AC remained online. Source/dirty fingerprints, toolchain, lens executable,
CPU governors/frequency limits, power profile and filesystems were identical.
All eight producers completed, but exact context equality discarded five earlier
artifacts as stale. Fixed-control medians remained ~206–207 microseconds.
The original audit and sidecars remain untouched at
`../scratchpad/target/ux-performance-20261107/final-complete/`.

[Linux's power-supply documentation](https://docs.kernel.org/power/power_supply_class.html)
distinguishes battery operating status from external-supply online status.
The measurement-policy decision here is to treat Charging, Full and Not charging
as the same **non-discharging externally powered identity only when a named
non-battery supply is positively online**. Discharging, unknown/missing status,
unknown external power and disconnected AC remain distinct. Supply types and
online state, governors, frequency limits and power profile remain identity
fields. Tests exercise those boundaries, not just the observed happy case.

Raw supply status is now included in before/after transient conditions beside
CPU/IO/memory pressure and temperatures. Charge/thermal effects can still affect
performance: fixed-control noise checks and all latency gates are unchanged.
`power_identity_revision: 2` explicitly invalidates old contexts. No historical
sidecars are normalized, restamped or promoted; the application campaign reruns
the complete audit with the new lens executable. This fixes false stale-source
classification without making a performance result pass.
