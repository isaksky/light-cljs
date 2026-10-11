# Canonical sorted collections — 2026-10-11

`light-cljs-119` extends the canonical JavaScript runtime shared by QuickJS
applications, compiled macros and emitted Node applications with sorted maps,
sorted sets, custom comparators, and ordered traversal. The implementation and
local public arity/protocol contract are in `src/js_runtime.js` and
`tests/compiler_sorted_contracts.rs`.

## Complete upstream source evidence

The pinned references are Maven ClojureScript **1.12.134** and clean checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. `execution.clj` independently verifies
reference identity and effective `:optimizations :none`, `:static-fns false` and
`:def-emits-var false` settings.

The historical 18-file inventory classified **104 sorted assertion sites** in
**16 complete bodies** as excluded from its earlier selected helper scope. That
historical accounting stays reproducible. The intended-language ledger executes
all 104 through complete-body witnesses; its sorted category supplies no runtime
exclusion. Fifteen bodies are preserved in `compiler-sorted-collections.tsv`,
and the remaining `test-multimethods-2` body retains its existing complete
namespace/setup adapter in `compiler-multimethods/test-multimethods-2`.

The fixture additionally preserves the independently inventoried
`other_functions_test` update-keys/update-vals bodies and the full
`set_equiv_test` body, including incomparable numeric/keyword sorted sets and
hash/sorted equality in both directions. The fixture contains eighteen complete
body workflows. Every fixture carries the original body identity and SHA-256;
ledger witnesses retain each original assertion SHA-256. A provenance test
rejects a partial or empty fixture selection, structurally reproduces every
fixture from its complete original body, and checks all 104 historical sorted
sites have original-body witnesses. The Rust engine test also executes the
complete reused multimethod namespace in both engines.

Harness adaptations are limited to `deftest`/`testing` becoming `do` and `is`/
`are` becoming executable assertions, including every original `are` row.
The original record declaration `(defrecord CLJS2079 [a b])` is included where
used. Ordinary `LITE_MODE` is bound to false while keeping its original
`^boolean` hint and both original branch bodies. Setup, iteration, saved
constructor calls, comparator callbacks, effect paths, neighboring assertions
and expected values remain intact. The reused multimethod adapter explicitly
aliases `obj-map` to `array-map` for the excluded private storage representation;
it retains every original constructor call and does not claim ObjMap identity.

Seven public source/arity mappings use these original bodies: `sorted-map`,
`sorted-map-by`, `sorted-set`, `sorted-set-by`, `subseq` at three and five
arguments, and `-comparator`. All seventeen public declaration/arity rows have hashed phase witnesses bound
to the same contract, which executes both application and macro phases. The
public TreeMap, TreeSet and TreeMapSeq constructors execute their original
arities with ordinary empty/singleton setup and meaningful operations. Their
private red-black tree/node storage representation is not a dialect promise.
Additional local contracts exercise reverse
ranges, saved callbacks, custom ISorted methods, immutable updates, metadata,
key representatives, ordinary failure paths, reused sessions and macro phase
behavior. Local execution alone does not invent an upstream source witness.

The expanded ledger also places sorted-dependent hashing and standard library
bodies in this family. Their `hash-ordered-coll`/`hash-unordered-coll`,
`clojure.set`, and `clojure.walk` prerequisites belong to `light-cljs-120` and
`light-cljs-123`. Missing source-specific mappings remain visible. Neither
these dependencies nor an absent upstream public witness is counted as a
passing assertion, or described as proof that sorted collection runtime
semantics are absent.

## Verification

All eighteen fixture workflows independently matched both pinned references.
The complete local public API contract was also measured against both references
using `/tmp/sorted-contract-oracle.tsv` and `tests/reference/execution.clj`; each
returned exactly `true`. This independent check corrected map `-entry-key`
to require IMapEntry and retained the upstream empty return from applying
`sorted-map-by`/`sorted-set-by` to zero arguments. Adding multiple keys with that
absent comparator remains a visible error path.

```sh
cargo test --locked --test compiler_sorted_collections --test compiler_sorted_contracts
cargo test --locked --test compiler_sorted_collections sorted_fixtures_match_pinned_release_and_checkout -- --ignored --exact --nocapture
python3 -B tests/reference/compiler-sorted-collections.py
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B tests/reference/compiler-language/report.py --run --feature sorted-collections --output /tmp/sorted-completion-final.json --artifacts /tmp/sorted-completion-final-artifacts
```

The strengthened complete-body engine test passes: eighteen fixture bodies plus
the reused multimethod namespace execute in QuickJS and Node. The fixture
provenance test, `compiler_language` accounting, both provenance tests,
structural reproduction, and generated ledger reproducibility pass. The ignored
pinned oracle test was explicitly executed: all eighteen fixture bodies matched
both references; the complete local public contract independently returned true
against each reference.

The final focused live report exits successfully with no semantic mismatch. All
104 historical sorted sites are covered. Across the broader sorted family it
covers 139 source assertions and seven original public source/arity obligations;
all seventeen sorted public phase witnesses pass. Thirty-one source sites remain
missing because their complete hashing/library bodies have no faithful adapter
(one hash helper site, four walk sites, two union sites and twenty-four
intersection sites). Eleven source-specific obligation mappings remain missing: ten public rows
(ISorted; -sorted-seq, -sorted-seq-from, -entry-key; the three tree collection
constructors; both rsubseq arities; comparator) and the local sorted-map
registration row. The independent ordinary/macro contract executes their actual
API behavior; that does not satisfy the source-witness schema by itself. The strict full
language gate remains incomplete. All 27 supplemental boundary contracts verify.

Regression commands (all pass except the separately accounted existing SCI gaps):

```sh
node tests/support/js_runtime.test.js
node tests/support/js_core_helpers.test.js
cargo test --test compiler_protocols --test compiler_transients --test compiler_interop --quiet
cargo test --test compiler_core_helpers --test compiler_sequences --test compiler_transients --test compiler_protocols --test compiler_records --test compiler_types --test evaluator_conformance --test compiler_transducers --quiet
cargo test --test execution --test evaluator --test analyzer --quiet
cargo test --manifest-path /tmp/light-cljs-sorted-baseline.nGRgIL/Cargo.toml --test sorted_baseline_gaps -- --nocapture
rustfmt --edition 2024 --check tests/compiler_sorted_contracts.rs tests/compiler_sorted_collections.rs
git diff --check
```

Runtime/core helper Node suites pass. The analyzer/evaluator/execution run has
99 passes and 44 ignored reference tests. The broad collection suites for core
helpers, protocols, records, sequences, transducers, transients and types pass.
The full SCI evaluator conformance run retains three failures for
`sci-into-xf-bad-xf-arity`, `sci-into-xf-bad-step`, and
`sci-into-xf-bad-completion`. A disposable `git archive` checkout of starting
commit `0462b342` at `/tmp/light-cljs-sorted-baseline.nGRgIL`, running the
seven-case malformed-into `original_into_arity_gap_observations` adapter against the original
SCI fixture sources, reproduces exactly the same current observations. These
existing gaps stay owned by `light-cljs-125`; they are not sorted regressions.

`cargo fmt --check` still reports pre-existing repository formatting differences
(first at untouched analyzer line 2386). Focused rustfmt checks for the new Rust
files and `git diff --check` pass. `cargo bench --bench evaluator -- --iterations
20` validates every result and reports total fresh-session **285057583 ns**,
reused-session **2019917 ns**, and bounded-loop-reset-recovery **1293872291 ns**.
This is a bounded smoke measurement, not a broad performance comparison. No
full intended-language strict pass or Windows/Linux run was attempted.
