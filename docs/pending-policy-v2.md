# Pending-admission policy v2 — 2026-11-07

Scratchpad's authorized UX experiment uses available RAM for bulk preparation.
The NORMAL body and FULL manifest boundaries, outcome contract v1, every gate,
every workload, and the residency diagnostic inventory remain unchanged.

Policy v2 sets the healthy bulk queue threshold to
`min(failure_limit, clamp(available_memory / 32, 32 MiB, 256 MiB))`.
Linux memory PSI `some avg10 >= 1%` reduces the threshold to
`min(failure_limit, 32 MiB)`. This is a producer threshold: one admitted chunk
(up to 256 KiB) can overshoot it; typing and already accepted bodies are never
discarded. A falling budget stops new bulk admission until draining catches up.
It is not a bound on all uncommitted text or a promise of bounded crash exposure.

The lens compares an observed threshold against the formula using the observed
memory, failure budget and pressure flag. Missing inputs produce unknown;
an arbitrary smaller number does not automatically satisfy the policy. Invalid
bounds, divisor and pressure declarations fail validation. V1 continues to use
its exact fixed-threshold check and untouched frozen fixture. The new independent
`outcomes-policy-v2.json` freezes this explicit policy revision; numerical gates
remain checked against `gates-cycle25.json`.

This is policy compatibility work, not a discovered speedup or a threshold fix.
No prior results are relabelled under v2. UI publication and explicit FULL barrier
timestamps may quantify a probe's exposure interval; they do not establish a
maximum exposure policy for normal autosave. Unknown total accepted/uncommitted
and resident-durable bytes remain unknown.
