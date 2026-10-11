# Repeatable eduction views in the canonical runtime

`eduction` accepts zero or more transforms followed by a source, composing the
transforms in their supplied order. Its variadic zero-argument call and a lone
nil source both produce an empty view, matching pinned CLJS. `Eduction.` exposes
the public two-field constructor, type identity and `xform`/`coll` properties.
Construction neither applies the transform nor requests a source iterator.

Every reduction, `seq`, public `-iterator` and ECMAScript iterator creates fresh
transform state. Sequence traversal uses the canonical chunked pull adapter;
reduction uses `transduce` and a `completing` wrapper around the supplied reducer.
Consequently mapper effects repeat on new traversals, stateful take/partition
views can interleave independently, and inner completion cannot invoke the outer
reducer's completion. A source's own lazy memoization remains independent: source
mapping can stay cached while the eduction mapping runs again. Repeated traversal
of a saved sequence tail reuses that tail's memoized outputs.

The public protocols are `ISequential`, `IIterable`, `ISeqable`, `IReduce` and
`IPrintWithWriter`. Eduction is not a collection or seq, has no metadata, and
rejects `with-meta`, matching the pinned type. Its own equality uses identity;
a persistent sequential value can compare its contents to an eduction. Printing,
index methods, nth, constructor identity, callable transforms and custom iterable
sources are observed directly. The implementation stores only JavaScript values
and uses the same runtime for demanded compiler helpers and emitted applications.

## Independent reference evidence

The checked references are Maven ClojureScript **1.12.134** and clean checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, using the checked execution harness
with `:optimizations :none`, `:static-fns false` and `:def-emits-var false`.
All **22** literal observations in `tests/fixtures/compiler-eductions.tsv` agree
in both references, emitted Node and QuickJS; there are no expected differences.
They cover repeat mapper effects, empty/no-init/explicit-init reduction, exact
nested completion counts, completion-only emissions including nil, early Reduced,
take-zero demand, finite infinite-source prefixes, interleaved seqs and iterators,
lazy-source effects, nested eductions, public protocol/metadata/type behavior,
ECMAScript iteration, printing/indexing, throws and callable/iterable receivers.

The complete upstream `core_test.cljs:test-cljs-1209` body retains its preceding
`my-conj` helper and original reduction, adapting only the deftest/is harness to
do/assert. Original body SHA-256:
`cae64576dda0c8375f25b64d7ad19241f752a0f8c037ae73194f0f7be8658c7d`.
The fixture and full source ledger retain that provenance. The separate CLJS-2258
probe observes the original eduction/sequence regression without claiming to
execute the entire mixed `test-transducers` body. That body still contains
`flatten`, assigned to light-cljs-121; full reconciliation remains light-cljs-125.

Both public declaration/arity obligations are bound to the complete shared
macro/application helper. A selected ledger run verifies the complete upstream
reduction body against both references and executes all **32** bound macro
arities (27 transient, 3 sequence, 2 eduction). The selected ledger remains
**INCOMPLETE** for the intended language; independent constructor/local API
observations in this audit are not fabricated source-assertion ledger passes.
Pinned core Var metadata adds only the Eduction and eduction rows.

## Verification receipt (2026-10-11)

Commands actually executed:

```sh
cargo test --locked --test compiler_sequence_transducers --test compiler_sequence_contracts --quiet
cargo test --locked --test compiler_eductions -- --include-ignored --nocapture
cargo test --locked --test compiler_eductions --test compiler_eduction_contracts \
  --test compiler_sequence_contracts --test compiler_sequences --test compiler_deferred \
  --test compiler_transducer_contracts --test compiler_transducers \
  --test compiler_core_helpers --test compiler_language --test provenance --quiet
cargo test --locked --test compiler_sequence_contracts --test compiler_sequences \
  --test compiler_transducer_contracts --test compiler_transducers --test provenance --quiet
cargo test --locked --test compiler_eduction_contracts --quiet
cargo test --locked --test compiler_language --quiet
node tests/support/js_runtime.test.js
node tests/support/js_core_helpers.test.js
clojure -Srepro -Sdeps '{:deps {org.clojure/clojurescript {:local/root "research/clojurescript"}}}' \
  -M tests/reference/generate_core_var_metadata.clj
python3 -B tests/reference/compiler-language/extract_inventory.py
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B tests/reference/compiler-language/test_full_ledger.py
python3 -B tests/reference/compiler-language/test_report.py
python3 -B tests/reference/compiler-language/report.py --run \
  --id core_test.cljs::test-cljs-1209::assertion-001 --timeout 60 \
  --reference-timeout 240 --output /tmp/light-eduction-ledger-report.json
rustfmt --edition 2024 --check tests/compiler_eductions.rs tests/compiler_eduction_contracts.rs
git diff --check
```

The new contract suite passes **4** tests: the shared helper, application/macro
throw recovery in fresh sessions, sticky cancellation at transform/source/step/
completion boundaries followed by fresh-session recovery, and garbage collection.
Three batches of 500 abandoned views use 1,000-element captures and reducer cycles,
force GC during active JavaScript, release result handles, and keep collected
QuickJS heap growth below 128 KiB in an 8 MiB session. Node uses a 32 MiB heap.

The eduction suite passes its ordinary observation/provenance tests and its
explicitly enabled dual-reference oracle. Retained core-helper, deferred,
sequence, transducer, language-accounting and provenance suites pass **26** tests;
the separate eager reference rebuild remains ignored. Ledger regression scripts
pass **11** and **41** tests. The broad Cargo invocation initially stopped at the
preexisting accounting test's 100 ms subprocess deadline while reference checks
were running; its isolated rerun passed. Earlier fixture preparation had a
parenthesis error in the test harness, corrected before the successful oracle.
`cargo fmt --all -- --check` reported existing formatting differences throughout
unrelated sources; both new test files pass their focused rustfmt check.

Five direct Node reductions of 100,000 inputs using map-inc/filter-odd eduction
produce **2,500,000,000**, with median **1074.426 ms**. This is a local finite
traversal smoke measurement, not a paired historical benchmark or threshold.
The full mixed upstream body, full language, unrelated platform and release
benchmark suites were not run for this focused change.
