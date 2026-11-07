# Residency and durability diagnostic protocol

This protocol implements the lens side of the promise/residency plan. It does
not constitute results from the proposed application experiments. Turso and the
relational store remain the storage layer, and existing promise eligibility is
unchanged.

## Available tooling

`splens measure residency --config diagnostic.toml` reads
`diagnostics.residency` in the project manifest. It validates declarations,
builds the declared `residency_probe`, then launches a fresh process for each
operation, workload, fixture, profile, independent launch and rung. Rung order
rotates between launches. Use `--scenario ID` to select complete operation
sweeps. Store diagnostics in a separate output directory from the promise audit.

The current declarations cover search by bytes and target count, random-window
extraction, paste publication, metadata refresh, undo/redo, and first-window
preparation. Each requests ASCII and Unicode/CRLF fixtures, three independent
launches, and 4 GiB, 16 GiB and host budget profiles. A profile override supplies
budgeting inputs; it does not constrain physical RAM or OS cache size.

The application target is not implemented yet. The runner writes an incomplete
`residency_ladder.json` and a readable companion, then exits nonzero when the
target is absent. Review reports missing/incomplete diagnostics independently
of existing promise health. Even complete residency diagnostics have
`diagnostic_only: true` and `promise_eligible: false`.

## Probe input and output

Each process receives:

```text
--scenario ID --workload INTEGER --fixture NAME --rung RUNG --launch INDEX
SCRATCHPAD_MEMORY_PROFILE=BYTES   (unset for host)
SCRATCHPAD_DIAGNOSTIC_REVISION=GIT_REVISION
```

It must emit one JSON `Trial` on stdout; stderr is archived separately. Export
`residency_trial.schema.json` with `splens schema export --output schemas` for
the exact fields. The lens sets the before/after measurement controls itself
when launching probes. Raw stdout, stderr and an importable `trials.json`
envelope are retained in `residency-raw/`. Each trial has a five-minute timeout.

The input identity includes scenario, size and unit, fixture name and SHA-256
digest, revision, rung, launch index, host memory and budget profile. It also
includes the declared code path and completion endpoint, expected and actual
result counts, an exact-result SHA-256 digest, and cancellation-check evidence.
Result digests must encode the full operation output, not just its count.
Paired rungs must agree on input and result digests, counts and host memory.
The producer is responsible for computing those digests and checking cancellation;
the lens checks their consistency, not the underlying editor implementation.

`metric` names the existing gate's metric pointer. `metric_ns` is a native-wall
observation at that endpoint, not Callgrind time. A p99 metric includes at least
128 positive `metric_samples_ns` observations; the lens recomputes nearest rank.
An unavailable rung has `status: "unavailable"`, a nonempty reason and null
timing. Failed results, duplicate identities and mixed revisions are rejected.

| Rung | Required observations |
|---|---|
| `owned_contiguous` | Zero store reads |
| `owned_piece_tree` | Zero store reads |
| `store_resident` | Zero store reads and positive retained reads |
| `store_warm` | App cache empty at start, OS cache primed, positive store reads |
| `store_cold` | App cache empty at start, successful eviction, positive physical read bytes |

Physical-read evidence must describe the measured reader; concurrent writer
activity is not proof of a cold read. A future production gate declaring
`store_resident` additionally requires the app's actual retention policy; an
artificially enlarged cache remains diagnostic.

## Import and classification

`splens import-residency --input trials.json --config diagnostic.toml` accepts
an object with `source_context` and `trials`. The revision and source fingerprint
must match the configured checkout, and each trial must name that revision.
Successful import writes a content-bound provenance sidecar. Source edits
invalidate the imported evidence. This is reproducibility checking, not a
cryptographic attestation of externally supplied measurements.
The recorded source context, including its original host/environment identity,
is preserved in the artifact. An import timestamp is not a collection timestamp;
the envelope does not currently certify when externally supplied trials ran.

The analyzer requires every declared rung and launch before classifying a
workload/profile. Missing evidence is incomplete; absent or noisy before/after
controls yield `uncertain_noise`. Stable complete groups compare rung medians
against the existing reference budget and identify computation, tree/window
assembly, resident-store access, or retrieval as the next area to investigate.
Adjacent-rung differences use paired launch contrasts. They are not exclusive
causal phase costs and can be negative. Reference gate and diagnostic endpoints
are reported separately; metadata refresh and undo/redo are narrower diagnostic
operations, so their budget comparison does not establish the full gate outcome.

## Residency and durability

`durability_policy` separately declares the pending queue limit, NORMAL body
commit and FULL manifest commit modes, and an optional maximum exposure time.
No exposure limit has been specified by the project, so that limit is null.
The lens checks version and values; changing a pending limit is explicitly
classified as `durability_policy_change` rather than a pure speed improvement.

Memory observations distinguish current/peak FULL-durable retained bytes,
accepted-but-not-FULL-durable bytes, and scratch space. Current values cannot
exceed peaks. Exposure is derived only from publication and FULL commit
timestamps. NORMAL body flush duration is reported separately and cannot fill
that field. The 32 MiB bulk queue bound is not a bound on all accepted text:
mutable tails and references awaiting a manifest commit also matter.

The current resource emitter supplies measured pending queue, cache and tail
values where available. It does not yet track total FULL-durable residency,
all accepted uncommitted bytes, scratch peaks or the full exposure interval.
Those fields remain null and appear in `missing_metrics` and the review table.

## Instruction counts

`splens measure-instructions --probe resource_probe --scenario ID` (or
`capacity_probe`) builds the production probe and runs it under Callgrind.
Valgrind must be on PATH. The default timeout is one hour, configurable with
`--timeout-seconds`. Raw profile, stdout and stderr are retained.

`instruction_counts.json` labels `Ir` as whole-process instructions including
setup and teardown. Phase counts remain null until probe-side client requests
delimit them. It carries no native latency and is never promise evidence. The
parser rejects missing or ambiguous instruction summaries; it never chooses
one thread/dump silently. Compare matched source, fixture and instrumentation
configurations, and use native runs for wall-clock decisions.

## Remaining application experiment work

Implement and verify all seven probe paths, apply profile overrides to derived
app budgets, instrument FULL exposure and memory ownership, and collect paired
native/instruction evidence before choosing optimizations. Scan retention and
pending-headroom experiments, pressure eviction and regression comparisons
remain application work. The lens reports their absence rather than declaring
the original seven-stage plan complete.
