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
