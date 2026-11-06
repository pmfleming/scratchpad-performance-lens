# RustQualityLens refactoring review

## Evidence

Baseline: `565555851f342530efe4d219ebc4ee79a51ee336`.
Used the local checkout at `../rust-quality-lens` (not `../rustqualitylens`),
revision `d28f3d936fa3c0d4c0a7ea753f436517077cb37d`, with its
`target/campaign-tool-build/debug/rqlens` executable. Same executable,
configuration and Rust 1.97.0 / LLVM 21.1.8 were used before and after.

`rqlens.toml` includes the embedded metadata as a fingerprint input.
Generated evidence is retained locally, outside version control:

- `target/quality-review/baseline/`
- `target/quality-review/current/`

## Results

| Measurement | Before | After |
| --- | ---: | ---: |
| Production function cognitive complexity, sum | 517 | 455 |
| Production function cyclomatic complexity, sum | 825 | 813 |
| Production function Halstead effort, sum, rounded | 4,863,163 | 4,444,042 |
| Maximum function cognitive complexity, including tests | 30 | 14 |
| Maximum function cyclomatic complexity, including tests | 34 | 19 |
| RQLens source nonblank lines | 7,978 | 7,765 |
| Duplicated lines | 633 | 440 |
| Duplication percentage | 7.93% | 5.67% |
| AST clone groups | 1 | 0 |
| Explicit `.clone()` / `.cloned()` calls in Rust source and tests | 131 | 111 |
| Escape-hatch records | 8 | 0 |
| Production panic-path findings | 4 | 1 |
| Executed tests | 66 | 56 |
| Test/helper function lines, summed spans | 1,528 | 1,416 |
| LLVM line coverage | 83.35% | 88.49% |
| Covered executable lines | 5,336 | 5,458 |
| LLVM function coverage | 78.49% | 85.22% |
| LLVM region coverage | 80.34% | 86.76% |
| Mean module locality score | 96.14 | 96.21 |
| Mean module observed-reuse leverage score | 36.25 | 35.50 |

Complexity sums use function rows, not file rows, to avoid double counting.
Production excludes `::tests::`, integration-test paths, test-only modules,
and the three removed cfg-test fixture helpers (`fallback_capacity_events`,
`fallback_resource_events`, `workload_label`). Test/helper line volume sums
these excluded function spans, including fixture helpers, once each.

Not every aggregate improves: all-function cyclomatic complexity increases
913 → 928 because the smaller suite exercises more error paths (`?` is counted
as a decision). All-function cognitive complexity decreases 545 → 487 and
Halstead effort decreases 5,600,242 → 5,340,553. Halstead effort is a syntax
heuristic, not developer hours. Architecture averages are likewise advisory:
removing accidental dependencies on report internals reduces their observed
consumer counts, which can lower the leverage score despite better ownership.

## Changes and findings

- **Criterion validation:** separated fresh-result collection, project-owned
  expectations and completeness checking. `record_run` cognitive complexity
  falls 30 → 4; cyclomatic complexity falls 29 → 14. Confidence-interval budget
  classification uses checked bounds instead of invariant-dependent unwraps.
- **Resource contracts:** separated generic sample requirements from special
  save-history/cache requirements. `gap_evidence` cognitive complexity falls
  26 → 5 and cyclomatic complexity 34 → 19. Cache compliance has one shared
  implementation for validation, summaries and samples.
- **Ownership/locality:** frame validation belongs in `common/frames.rs`, not
  the performance review. Its presentation adapter remains with its sole
  consumer in `review.rs`. Schema export belongs to `artifacts/mod.rs`, not
  CLI routing. CLI locality improves 66.25 → 69.25. Artifact-helper leverage
  improves 40 → 50; shared frame-contract leverage is 60. Capacity locality
  decreases 88 → 85 because it now reuses the safe artifact-slice helper.
- **Cloning and redundant code:** reports borrow artifact arrays, capacity
  lookups, available-profile IDs and repetition samples. Raw repetitions move
  into their result. Resource samples copy evolving probe fields once instead
  of separately cloning known fields and merging the originals. Removed the
  duplicate CLI constructor and unused synthetic fallback production paths.
  Consumers still reject legacy synthetic evidence.
- **Test value:** replaced large synthetic workload inventories with focused
  boundary matrices. Added real manifest read/write/freshness/content-binding
  coverage and an artifact → provenance → speed-report → console path. CLI
  tests cover both executables, schema contracts and partial-run continuation.
  The Git-history CLI test now creates its own repository instead of silently
  skipping or writing into the neighboring Scratchpad checkout.
- **Escape hatches:** removed glob imports and the reference-to-Option return.
  No lint suppressions were added. The remaining production `expect` validates
  the compile-time embedded metadata JSON; it is not probe-input recovery.

## Validation and limitations

Passed formatting, workspace/all-target compilation, strict Clippy, all 56
tests, doctests and rustdoc. `rqlens verify` reports six passed gates, no
error-level failures, eight project-policy warnings and fourteen skipped
optional checks. Warnings concern MSRV, license file, repository metadata,
contributing/code-of-conduct/security/changelog files and toolchain policy;
these were not silently waived or treated as passing.

Hotspot, clone, coverage, locality and leverage artifacts report complete
inputs. Branch coverage is unavailable; no branch-coverage claim is made.
Coverage includes test code, so both percentages and absolute covered-line
counts are shown. These runs exercise fixture probes, not full Scratchpad
benchmarks, flamegraph generation or production performance. Remaining large
JSON presentation functions are candidates for further work; no artifact
schema or measurement-budget semantics were intentionally changed.

Reproduce with `cargo-llvm-cov`, matching `llvm-cov` / `llvm-profdata`, and
rust-analyzer available:

```sh
RQLENS=../rust-quality-lens/target/campaign-tool-build/debug/rqlens
"$RQLENS" measure all --config rqlens.toml
"$RQLENS" verify --config rqlens.toml
cargo clippy --workspace --all-targets --locked -- -D warnings
```

If the LLVM tools are outside PATH, set `LLVM_COV` and `LLVM_PROFDATA` explicitly.
The RQLens binary can instead be built from the local checkout with Cargo.

## Follow-up review: baseline `1c5bdad`

This is a fresh comparison against `1c5bdad2c81c990c3cfdda49b7bdeab0b1b9b17e`,
not a reuse of the older measurements above. The baseline includes session-tool
and measurement tests that were not present in that older report. Evidence is in
`target/quality-review/round2-before/` and `target/quality-review/current/`.
Both runs used the same local `target/campaign-tool-build/debug/rqlens` binary,
SHA-256 `622b2c793a574781cd40a190a457f58af4b8c222ee5efce04c4e77488c50929a`.
The tool checkout was at `aa0d6dc358d1717b0cdba446cc771136bf3c3461`; this is
checkout provenance, not a claim that the existing binary was built from it.

| Measurement | Before | After |
| --- | ---: | ---: |
| Production function cognitive complexity, sum | 474 | 458 |
| Production function cyclomatic complexity, sum | 890 | 881 |
| Production function Halstead effort, sum, rounded | 4,713,939 | 4,504,569 |
| Maximum function cognitive complexity | 14 | 13 |
| Maximum function cyclomatic complexity | 25 | 22 |
| Rust source lines, including blanks and tests | 8,897 | 8,865 |
| RQLens source nonblank lines | 8,452 | 8,429 |
| Duplicated lines | 450 | 284 |
| Explicit `.clone()` / `.cloned()` calls | 115 | 85 |
| Escape-hatch records | 2 | 0 |
| Executed tests | 68 | 61 |
| Test/helper function lines, summed spans | 1,748 | 1,747 |
| LLVM line coverage | 89.09% | 92.01% |
| Covered executable lines | 5,874 | 6,026 |
| LLVM function coverage | 85.39% | 87.81% |
| LLVM region coverage | 87.38% | 90.71% |
| Module dependency edges, summed outbound counts | 159 | 156 |
| Mean module locality score | 96.42 | 96.42 |
| Mean observed-reuse leverage score | 33.18 | 32.50 |

### Findings addressed

- **Duplicated benchmark orchestration:** Criterion collection, invalid-row
  construction, budget/provenance annotation and post-publication failure checks
  now have one implementation in `common/criterion.rs`. Search and slowspots
  retain their own presentation and sorting. Their entrypoint cognitive scores
  fall 11 → 1 and 9 → 1; production-wide sums above include the extracted helpers.
- **Ownership:** moved benchmark execution out of statistical helpers, eliminating
  the stats/criterion dependency cycle. Cargo target-directory resolution lives
  in `shared.rs`; probe archiving no longer derives a target path from a Criterion
  directory. Resolve that directory once when invalidating benchmark manifests.
  Internal Criterion helpers are private rather than exposed to every producer.
- **Avoidable copies:** telemetry borrows tabs, modern buffers, views and JSON
  fields; only legacy buffers require an owned compatibility projection. Baseline
  comparisons borrow latency rows, and realistic-frame rows move into their
  destination. Removed the allocating, single-use sort-key helper and the JSON
  round-trip capacity fixture builder.
- **Input robustness:** search-workload multiplication is checked; overflow becomes
  unknown rather than panicking/wrapping. Malformed realistic-frame rows remain
  visible for validation instead of panicking during annotation. Diagnostic-line
  parsing remains streaming, preserves valid prefixes and retains the newest 500
  objects while counting all valid objects.
- **Test value:** added isolated legacy/modern/corrupt session coverage, bounded
  diagnostics, overflow and malformed frame cases. The existing Criterion manifest
  fixture now runs both producers with passing and failing CLI-budget policies.
  Boundary matrices replace repeated setup. The CLI dirty-save test now owns
  independent-process, SHA-256, stderr, summary and no-overwrite assertions formerly
  duplicated in a unit test; timeout/failure unit tests remain distinct.

### Trade-offs and validation

Test count falls appreciably, but test/helper code volume is essentially flat
(one line smaller): the savings funded new behavior coverage, not just deletion.
All-function cyclomatic complexity rises 1,031 → 1,096, largely from explicit
fallible test setup and additional boundary cases. All-function cognitive
complexity falls 518 → 516 and effort falls 5,847,420 → 5,829,917.

Architecture scores do not show a universal improvement. Locality is unchanged;
observed-reuse leverage decreases as accidental consumers of stats and Criterion
internals disappear. The concrete improvement is fewer dependencies and removal
of the cycle, not a claimed leverage-score win. The largest remaining effort
hotspots are JSON report construction in `speed_report.rs` and `review.rs`.

No artifact schemas or measurement-budget policy were intentionally changed.
Shared benchmark errors now consistently include the producer label; unmapped
Criterion results are skipped silently by both collectors. This pass does not
remove the remaining production `expect` for compile-time embedded metadata.

Formatting, strict workspace/all-target Clippy, tests, doctests and rustdoc pass.
RQLens verification reports six passed gates, eight project-policy warnings,
fourteen skipped optional checks and no error-level failures. Hotspot, clone,
coverage, locality and leverage inputs are complete. Coverage includes test code;
branch coverage remains unavailable. Fixture probes are not full Scratchpad
benchmark or GUI validation. See the reproduction commands above; the same
`cargo-llvm-cov` and matching LLVM tools are required for both snapshots.

## Review of the current lens: 2026-11-06, baseline `0cfba9f`

This campaign measures the lens after the ten Scratchpad measurement-accuracy
changes. Earlier campaign numbers above describe older implementations.
The local tool is `../rust-quality-lens`, corresponding to the requested
`rustqualitylens` project. Both snapshots use its existing
`target/campaign-tool-build/debug/rqlens` executable, SHA-256
`3e3704d2731ab916f6c64c6c44b39b6feaaa5a73df9de4b7a9b7193aef629915`.
The tool checkout is at `d137d8d8f219b4ee788935aff27115374626d2f8`;
the existing executable was not rebuilt from that checkout for this campaign.
Both runs use Rust 1.95.0 and LLVM 21.1.8 from that project's Nix shell.

Evidence is retained under `target/quality-review/refactor-2026-11-06/`:
`before/`, `after/`, the two resolved TOML configurations, measurement logs,
`verify.log`, `refactor-tests.log`, and `before-summary.json` /
`after-summary.json`. Both configurations retain this project's metadata
fingerprint inputs and automatic identity resolution.

| Measurement | Before | After |
| --- | ---: | ---: |
| Production function cognitive complexity, sum | 708 | 623 |
| Production function cyclomatic complexity, sum | 1,170 | 1,162 |
| Production function Halstead effort, sum, rounded | 6,277,501 | 5,730,517 |
| Maximum production cognitive complexity | 41 | 13 |
| Maximum production cyclomatic complexity | 30 | 23 |
| Total Rust lines, including blanks, source and tests | 11,341 | 11,334 |
| RQLens source nonblank lines | 10,806 | 10,788 |
| Duplicated lines | 450 | 421 |
| Duplication percentage | 4.16% | 3.90% |
| Explicit `.clone()` / `.cloned()` calls, source and tests | 108 | 50 |
| Escape-hatch records | 6 | 0 |
| Production panic-path findings | 15 | 8 |
| Executed tests | 85 | 71 |
| Test/helper function lines, summed spans | 2,446 | 2,498 |
| LLVM line coverage | 84.58% | 90.21% |
| Covered executable lines | 7,191 / 8,502 | 7,411 / 8,215 |
| LLVM function coverage | 82.85% | 87.33% |
| LLVM region coverage | 83.62% | 88.70% |
| Mean module locality score, all modules | 96.160 | 96.135 |
| Mean module observed-reuse leverage, all modules | 33.600 | 33.846 |
| Mean production module locality | 95.867 | 95.851 |
| Mean production module observed-reuse leverage | 37.333 | 37.447 |

Complexity sums count function records once. Production excludes names containing
`::tests::` or `registry_tests::`, integration-test paths, and files ending in
`_tests.rs`. Test/helper volume sums the excluded function spans. Panic findings
use the same test spans after normalizing the tool's relative and absolute paths.
LOC and clone-call counts include every `.rs` file under `src/` and `tests/`,
including the two added modules. Halstead effort is a syntax heuristic, not
estimated developer hours.

### Findings addressed

- Baseline extraction mixed three evidence shapes, cloned capacity reports into
  a typed round trip, and used fallible serialization through `unwrap`/`expect`.
  Borrowed measurement views now keep eligibility, metric selection, and
  observation identity together. Baseline fingerprint serialization borrows its
  content and preserves the existing canonical field order. Legacy fingerprint
  compatibility is covered explicitly. Snapshot cognitive complexity falls
  41 to 3; this also distributes work into smaller named operations.
- Probe sweeps mixed scenario execution, retries, launch identity, and error
  accumulation. These responsibilities now have separate functions; the manifest
  is read once per sweep. Planned launch counts remain fixed when a retry fails,
  so missing repetitions cannot reduce the required sample count. The sweep's
  cognitive complexity falls 37 to 0; the helpers still carry the actual work.
- Probe output was serialized and reparsed to validate each event. Validation
  now reads the parsed value directly. Probe builds share one Cargo invocation
  and resolve the executable from Cargo's artifact messages.
- Producer probes and session measurements duplicated deadline and child cleanup
  behavior. A shared process module now handles deadlines, process groups on
  Unix, and reaping after timeout or wait failure. Environment and provenance
  commands also share successful-output decoding.
- Promise definitions contained filesystem inventory validation. Inventory now
  owns those checks; validation cognitive complexity falls 31 to 3. Named gap
  requirements are declarative data, with identical contracts grouped together;
  gap-evidence cyclomatic complexity falls 30 to 7.
- Resource-budget groups cloned samples and repeatedly decoded already checked
  numbers. Groups now borrow samples and share checked observations. Workload
  ordering, uncertainty, and authoritative-budget rules are retained.
- Report projections cloned JSON subtrees before serializing them again. Speed
  and capacity projections now borrow their fields, including raw capacity
  repetitions. Mean-latency conversion is shared by its consumers. All six glob
  imports and unused artifact `Clone` implementations were removed.
- Capacity specs retained unused fallback timing thresholds. Those fields and
  their constant were deleted; project-supplied thresholds remain authoritative.
  Code-history reporting now derives current counts and commit metadata from
  the already computed latest history entry, removing repeated Git commands and
  a second count of the same revision.
- Thin parser and summary tests were replaced by broader lifecycle and CLI
  assertions. The baseline lifecycle covers acceptance, regression, overlapping
  observations, environment/evidence mismatch, rejected evidence, archival,
  fingerprint/version corruption, and current-artifact tampering. Self-contained
  Rust probe fixtures cover relocated Cargo output, repeated resource/capacity
  launches, counters, timeouts, truncated/invalid output, omitted scenarios,
  successful publication, and continuation after failure. Session summary
  assertions now exercise the actual CLI artifact instead of a duplicate unit
  success test.

### Trade-offs and validation

The total source reduction is small: seven physical lines and eighteen nonblank
lines. The executed suite is smaller by fourteen tests, but test/helper source
grows by 52 lines to cover the previously untested baseline lifecycle and real
probe processes. This is a reduction in test count, not test/helper source volume.
All-function cyclomatic complexity rises 1,484 to 1,520 as these error paths gain
coverage; all-function cognitive complexity falls 791 to 717 and effort falls
8,355,329 to 8,235,553.

Observed reuse improves slightly. Mean locality is essentially flat with a small
decline: review orchestration now depends on the extracted inventory and shared
statistics; evidence checks reuse the common array accessor. Responsibility
placement and shared process ownership improve, but the aggregate locality score
does not show a numerical win. These advisory scores are not proof of maintenance
cost or runtime performance.

`cargo fmt`, `cargo check --all-targets`, strict all-target Clippy, and all 71 tests
pass. `rqlens verify` reports six passed gates, eight project-policy warnings,
fourteen skipped optional checks, and no error-level failures. The warnings cover
missing MSRV, license text, repository metadata, contribution instructions, code
of conduct, Rust toolchain file, security instructions, and changelog.

Static syntax metrics remain partial because the existing `thread_local!` in
`provenance.rs` is not expanded by this tool. Identity uses Cargo metadata; no
source files are missing or unparsable. Coverage includes test code and does not
provide branch coverage. The fixture probes do not run Scratchpad's full workload
suite or GUI. Artifact schemas and project budget authority are preserved; no
Scratchpad or RustQualityLens source was changed.

Reproduce from this repository, retaining the baseline directory:

```sh
nix develop ../rust-quality-lens --command cargo check --all-targets
nix develop ../rust-quality-lens --command cargo clippy --all-targets -- -D warnings
nix develop ../rust-quality-lens --command cargo test
nix develop ../rust-quality-lens --command \
  ../rust-quality-lens/target/campaign-tool-build/debug/rqlens measure all \
  --config target/quality-review/refactor-2026-11-06/after.toml
nix develop ../rust-quality-lens --command \
  ../rust-quality-lens/target/campaign-tool-build/debug/rqlens verify \
  --config target/quality-review/refactor-2026-11-06/after.toml
```
