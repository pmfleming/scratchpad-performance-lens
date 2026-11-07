# Plan: precise promises, resident-text performance, residency vs durability

Date: 2026-11-07. Follows the cycle-25 report (`report-cycle25/report.md`) and
the database/memory review (`report-cycle25/database-and-memory-ideas.txt`).

Turso and the relational session/text store remain the storage layer
throughout. Nothing here replaces them or relaxes a gate.

## Ground rules

- **Gates stay fixed.** Every gate in `benches/measurement_manifest.json`
  keeps its metric, budget, minimum workload, launch minimum and
  code-path contract.
- **Changes are additions.** This plan adds description, reference
  measurements and new metrics alongside the gates.
- **Diagnostics cannot prove a promise.** A measurement with text held
  only in process memory (`text_backing: "owned"`) stays ineligible for a
  storage-backed promise, as `measurement_contract::eligible` enforces
  today. Residency variants are diagnostics or reference floors.
- **No moving the stopwatch.** A faster run that stops timing at an
  earlier point does not count as a speedup.
- **Comparisons are paired and deterministic where possible.** The host
  varied 1.31–1.91 s for the same first save on one day. Arms are
  interleaved launch by launch, and instruction counts are recorded where
  wall time is too noisy (callgrind; `perf` is unavailable without
  `perf_event_paranoid` changes).

---

## Part 1 — Make each promise precise while keeping the existing gates

### Problem

- **Generic basis text.** 29 of 36 resource budgets share one boilerplate
  `basis` string ("Existing interaction/capacity budgets applied to the
  declared operation…"). None of them says what the user is waiting for.
- **Ambiguous endpoints.** Gates use outcomes that the review never names:
  first visible, all searchable, published, committed. Readers cannot tell
  whether a 180 ms gate means "the user can continue" or "the work is
  durable".
- **Shared gates count more than once.** `session_persist_cost` and
  `session_restore_cost` sit under three promises, so one failure appears
  three times.
- **Known lens defects distort the result.**
  - Empty top-level `noisy_sources`.
  - The Many Views opportunity text contradicts its `at_risk` health.
  - Aged-store forget is reported as "missing" when it is actually
    "uncertain".
  - Two capacity rows share a display label.
  - Write amplification is computed from kernel `write_bytes`, which is
    roughly 3× the syscall and device bytes (see ideas.txt §2.1).
  - The probes use a 16 MiB / 4 MiB chunk cache; the app's derived budget
    is 512 MiB / 64 MiB on the test host.

### 1.1 Freeze the gate inventory (do first)

- Add a lens test that snapshots every gate: id, promise membership,
  metric pointer, budget, unit, minimum workload, required launches,
  required code path and completion endpoint.
- Any change must edit the snapshot and bump `version` in the manifest,
  with a recorded rationale.
- This is the mechanism behind "keeping the existing gates".

Files: `src/producers/review/registry.rs`, `src/producers/resource_budgets.rs`,
`src/producers/capacity.rs`, new `tests/gate_inventory.rs`.

### 1.2 Add an outcome declaration to every gate

Extend each `resource_budgets` and `capacity_budgets` entry, and each
search `probe_measurements` entry, with an `outcome` object. Existing
fields are unchanged.

```json
"outcome": {
  "promise": "search",
  "user_outcome": "all matches in a 1 GiB open file are listed",
  "endpoint": "all_ranges_returned",
  "blocking": "background_completion",
  "residency": "store_cold",
  "durability_at_endpoint": "not_required",
  "workload": "1 GiB non-repeating UTF-8, one expected match"
}
```

| Field | Values |
|---|---|
| `blocking` | `foreground_interaction` (user waits; e.g. first render, paste publication, visible restore)<br>`background_completion` (user continues; job must still finish in budget; e.g. indexing, full search, first save)<br>`interaction_during_background` (frame/typing/cold-read p99 while work runs) |
| `residency` | `store_cold`<br>`store_warm` (OS page cache hot, app cache empty)<br>`store_resident` (committed chunks retained in the app cache)<br>`owned`<br>`not_applicable` |
| `durability_at_endpoint` | `not_required`<br>`published_ram` (visible and editable, may be lost on crash)<br>`committed_normal`<br>`committed_full` |

Rules enforced by the lens:

- A gate with a missing or unknown `outcome` field is a manifest error.
- Each probe event reports the endpoint, residency and durability it
  actually observed. A mismatch with the declaration makes the evidence
  invalid, extending today's contract checks. `production_paste` already
  reports publication separately from flush; extend that pattern to the
  other gates.
- Choose the `outcome` values from what each gate measures today. Write
  them down without changing any gate.

### 1.3 Promise outcome matrix in the review

- **New table.** `performance_review.json` gains a per-promise list of
  outcomes. Each outcome carries its gates, verdicts and declared fields.
- **Shared gates.** A gate that belongs to several promises appears once,
  with a `shared_with` list. The failure is not counted again in each
  promise's miss total. Keep the existing per-promise totals for
  continuity, and add a separate count of unique failing gates.
- **Generated document.** Generate `docs/promise-outcomes.md` from the
  manifest: one table per promise listing outcome, endpoint, blocking
  class, budget and current verdict. This is the human-readable definition
  of each promise.

### 1.4 Fix the known lens defects

1. Populate top-level `noisy_sources` from scenario and launch quality.
2. Generate opportunity text from structured health. No "passing" text
   when health is `at_risk`.
3. Distinguish `uncertain` (complete evidence with an outlier) from
   `missing`. Thin coverage only for missing evidence.
4. Give capacity rows unique labels by including the endpoint.
5. Write amplification: use syscall bytes (`wchar`, or per-file counts
   from the probe) or device bytes, net of `cancelled_write_bytes`. Keep
   `write_bytes` only as a labelled raw counter.
6. Record the app-equivalent chunk-cache budget (from
   `TextHistoryBudget::derive_from_available_memory`) beside the probe's
   actual limit. Flag any gate whose probe uses a smaller cache than the
   app would.
7. Cold-read p99 from 128 samples is essentially the second-worst sample.
   Report the sample count and order statistic, and raise the minimum
   samples or launches. The 4 ms threshold is unchanged.

### 1.5 Deterministic companion metric

- Add an optional callgrind run, at roughly 50× slowdown, for the session
  save/restore and search probes.
- Record instruction counts per named phase.
- Use it as a regression signal, not a gate. It detected the per-batch
  `PRAGMA synchronous` re-prepare problem (41,772 re-prepares), which wall
  time hid in noise.

**Part 1 is done when:**

- every gate declares an outcome;
- the gate-inventory test is green with no gate changes;
- `promise-outcomes.md` is generated;
- the review shows unique failing gates alongside the existing counts;
- the seven defects above are covered by tests.

---

## Part 2 — Establish, then optimise, how fast the editor runs with text already resident

### Question

For each storage-sensitive operation, how much time is pure computation
(matcher, layout, mutation) and how much comes from the data structures
and the store?

- **If the resident floor misses a budget,** no storage work can meet that
  gate. Optimise computation first.
- **If the floor passes,** the gap to the store-backed number is the target
  for storage work.

### 2.1 Residency ladder (probe work in Scratchpad, artifact in lens)

Run the same fixture, production code path and result checks at five
rungs:

| Rung | Text location | Eligible for store promises? |
|---|---|---|
| R0 `owned_contiguous` | One `&str` | No (reference floor) |
| R1 `owned_piece_tree` | Piece tree with in-memory chunks | No (reference) |
| R2 `store_resident` | Store-backed, chunks retained in app cache | Yes, if the retention policy is one the app actually uses (Part 3) |
| R3 `store_warm` | Store-backed, app cache empty, OS page cache hot | Yes |
| R4 `store_cold` | Store-backed, physically evicted (existing verified-eviction path) | Yes |

The rungs divide the time:

- matcher cost = R0;
- traversal and window assembly = R1 − R0;
- app-cache retrieval and verification = R3 − R2;
- device reads = R4 − R3.

Operations in scope:

| Operation | Workloads |
|---|---|
| Search, file size (`search_file_size_ceiling`) | 64 MiB – 1 GiB |
| Search, target count | 1k, 10k files |
| Random window / scroll extraction | 1 GiB file |
| Paste preparation and publication | 64, 128 MiB |
| Metadata refresh / inspection | 1 GiB |
| Undo/redo replay | 100k-edit history |
| First visible paint | 2 GiB |

Session save/restore is out of scope here; it is a database-representation
problem, not a residency one.

Each rung must:

- assert the same result count;
- match the R0 output exactly, including Unicode and CRLF fixtures;
- honour cancellation;
- record `residency` in the measurement contract, verified by
  `ReadEpoch::read_count` and the chunk-cache hit counters (R2 must
  perform zero store reads).

Lens side:

- new artifact `residency_ladder.json`;
- a review panel per operation showing the rungs, their differences, and
  whether R0 and R1 fall within the gate's budget;
- `eligible()` is unchanged, so R0/R1 never satisfy a store promise.

### 2.2 Decision rule per operation

1. **R0 > budget:** the computation itself misses. Optimise the matcher,
   parallelism or algorithm.
2. **R0 passes, R1 > budget:** piece-tree traversal or window assembly is
   the problem. Optimise the data path.
3. **R1 passes, R2 > budget:** store-handle overhead or verification is the
   problem.
4. **R2 passes, R3/R4 > budget:** this is a retrieval and residency
   question. Go to Part 3.

### 2.3 Optimisation loop (Scratchpad)

Hypotheses from cycle-25 and the review, to rank once the ladder exists:

- **Search planning is serial.** `DocumentChunks::next` calls
  `next_line_boundary_after` on the coordinating thread, which can load
  chunk text there. Each batch also waits for the slowest of eight
  windows (`next_chunk_batch` takes 8).
  Test: plan windows from chunk metadata (newline counts) or let workers
  own boundary discovery; pipeline batches instead of barrier rounds.
- **Window assembly copies spans.** Check whether
  `SEARCH_FRAGMENT_CHUNK_CHARS` windows borrow chunk bodies or copy them.
  Measure copied bytes per searched byte.
- **The worker cap is min(cores, 8).** Re-test once planning is no longer
  serial; more readers did nothing while the planner was suspected to be
  the limit.
- **Verification runs on every read.** BLAKE3 plus length, character,
  newline and ASCII checks cost 14–18% on a cold 1 GiB search. Verify once
  per epoch per committed chunk and mark it verified. Re-reading a
  retained `Arc<String>` already skips the check; this optimisation is
  about re-reads from the store.
- **Paste.** Separate inspection, normalisation and mutation at R0/R1.
  Cycle-25 attributes most of the time to `prepare_mutation_ns`. Confirm
  how much of that is admission waiting rather than computation.
- **Inspection of large single spans.** This is the existing
  byte-balanced UTF-8 inspection hypothesis from the cycle-25 report.

For every change:

- measure at all rungs, paired against the parent revision;
- keep the change only if the target rung improves and no other rung
  regresses beyond noise;
- run the relevant correctness tests (exact matches, cancellation, worker
  failure, Unicode/CRLF);
- record callgrind instruction deltas (1.5) alongside wall time.

**Part 2 is done when:**

- the ladder exists for every operation above;
- each operation is classified under the 2.2 rule;
- for every operation where R0 or R1 misses its budget, optimisation has
  either closed the gap or shown with evidence why it cannot.

---

## Part 3 — Treat memory residency and durability as independent decisions

### Principle

Keeping durable, committed text in RAM costs memory but does not increase
crash exposure. Accepting text that is not yet committed increases crash
exposure. The cycle-25 work mixed the two:

- Raising the pending limit to 1 GiB cut 128 MiB paste publication from
  1,390 ms to 180 ms, but increased exposure.
- A 4 GiB scan cache cut repeat 1 GiB search from 1,556 ms to 323 ms with
  no change in exposure.

The lens must report the two separately so each decision is explicit.

### 3.1 New metrics (lens contract, emitted by probes)

| Class | Meaning | Metric |
|---|---|---|
| `resident_durable_bytes` | Committed chunks retained in app caches (viewport LRU, scan cache) | current and peak |
| `accepted_not_durable_bytes` | Text visible to the user but not yet committed (pending queue, mutable tails) | peak during the operation and at the endpoint |
| `exposure_window_ms` | Time from publication to durable commit for the operation's text | measured; `production_paste` already reports `post_publication_flush_ns` |
| `scratch_bytes` | Search windows, layout, prepared snapshots | peak |
| `os_page_cache` | Whether R3 vs R4 differed | from Part 2 |

Rules:

- **Report against the declared durability endpoint.** Each probe reports
  these metrics alongside the Part 1 `durability_at_endpoint`.
- **Show exposure beside latency.** The review displays both for every
  foreground gate. A variant that improves latency by raising
  `accepted_not_durable_bytes` above the declared policy limit (today the
  32 MiB `BULK_PENDING_BYTES` invariant) is labelled **durability policy
  change** and cannot be presented as a pure optimisation.
- **Declare the limits.** Add a `durability_policy` section to the manifest
  holding the pending limit, required commit modes (manifest FULL, bodies
  NORMAL) and maximum exposure. The lens checks evidence against it.
  Changing it is an explicit, reviewed manifest change, like a gate.

### 3.2 Memory-profile runs

- **Profile override.** Add `SCRATCHPAD_MEMORY_PROFILE=<bytes>` to the
  probes so derived budgets (`TextHistoryBudget`, failure RAM budget, any
  new scan cache) compute from a declared total instead of the host's
  58 GiB. A cgroup memory limit is optional and closer to reality.
- **Profiles.** Run the residency ladder and memory gates at 4 GiB, 16 GiB
  and host memory.
- **Gate verdicts.** Verdicts stay on the host profile, as today.
  Smaller-profile results are reported alongside so that a policy which
  passes only on large machines is visible.

### 3.3 Retention policy experiments (Scratchpad)

1. **Scan retention without durability change.** Give search its own
   retained-text class within one shared budget. Priority:
   1. visible text is protected;
   2. actively searched documents are retained when the budget allows;
   3. bulk preparation gets bounded headroom.
   Evict under memory pressure, using PSI on Linux and available memory
   elsewhere.
   Measure:
   - repeat-search latency (R2);
   - first-search latency (unchanged at R3/R4);
   - RSS at 4/16/host profiles;
   - eviction behaviour while typing.
2. **Pending headroom as an explicit policy option.** Re-run the paste and
   import sweeps at pending limits of 32 MiB, 256 MiB, 1 GiB and
   memory-derived values. Report latency, exposure and peak RSS together.
   The decision belongs to the project owner; the lens only makes the
   trade-off visible.
   First explain the unexplained background-index regression seen with a
   1 GiB pending limit (1 MiB: 18 → 98 ms; 128 MiB: 520 → 1,377 ms).
3. **Faster durability without changing policy.** These lower exposure
   time or let the writer drain faster within Turso:
   - remove the per-batch `PRAGMA synchronous` toggling with a body
     connection fixed at NORMAL and a manifest connection fixed at FULL.
     This cuts 21% of first-save instructions; the 122 session-store tests
     pass with it;
   - size the capture buffer to the text length;
   - try mimalloc.
   These are already measured (ideas.txt §3.1, 3.4, 3.5). Re-measure them
   with the Part 3 metrics so the exposure effect is recorded.

**Part 3 is done when:**

- every foreground and background gate reports residency and exposure
  metrics;
- `durability_policy` is declared and checked;
- the review separates residency-driven improvements from
  durability-policy changes;
- the retention experiment has results at all three memory profiles.

---

## Order and dependencies

| Step | Work | Depends on | Repos |
|---|---|---|---|
| 1 | Gate-inventory freeze (1.1) and defect fixes (1.4) | — | lens |
| 2 | Outcome declarations (1.2) and outcome matrix (1.3) | 1 | lens + manifest |
| 3 | Residency contract field and R0–R4 probes (2.1) | 2 | Scratchpad probes, lens |
| 4 | Exposure/residency metrics (3.1) and memory profiles (3.2) | 2 | Scratchpad probes, lens |
| 5 | Ladder classification (2.2), then optimisation loop (2.3) | 3 | Scratchpad |
| 6 | Retention and pending policy experiments (3.3) | 4, 5 | Scratchpad |
| 7 | Full promise audit with all of the above | 5, 6 | both |

Steps 3 and 4 can run in parallel. Callgrind support (1.5) can land any
time after step 1.

## Risks

- **Extra runtime.** Five rungs × several operations × three memory
  profiles adds audit time. Run the ladder as a focused producer
  (`splens measure residency --scenario …`), not as part of every
  `measure promises`.
- **Unreachable R2 caches.** A `store_resident` result counts only if the
  retention policy that produced it exists in the app with its real
  budget. A test-forced 4 GiB cache is a diagnostic, not evidence.
- **Hand-written declarations.** Endpoint and residency declarations need
  probe-side verification counters (read counts, cache hits, commit flags).
  Without them they are just labels.
