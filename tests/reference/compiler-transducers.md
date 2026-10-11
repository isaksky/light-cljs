# Compiled eager transducers

Contract: ClojureScript checkout **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**,
checked independently against Maven **1.12.134**. The shared
[checked oracle](execution.clj) verifies source identity and effective
`:optimizations :none`, `:static-fns false`, and `:def-emits-var false` settings.
Observed runtimes: checkout **0.0.135109660**, Clojure **1.12.4**, Java **25.0.2**,
Node **v24.18.0**. QuickJS uses the repository's pinned rquickjs **0.14.0**.

[compiler-transducers.tsv](../fixtures/compiler-transducers.tsv) contains
**316 independent observations**: all **285** native transducer fixture bodies,
six complete upstream test bodies, one eager excerpt, and **24** independent
CLJS probes. [compiler_transducers.rs](../compiler_transducers.rs) runs every
observation through generated JavaScript in QuickJS and Node, compares literal
expected JSON, and checks duplicate IDs, body assertion counts, and provenance.
Its ignored oracle test rebuilds all sources with both pinned references.

The fixture columns are ID, adapted source, checkout expectation, original
source identity/hash, optional release expectation, and optional persistent
implementation expectation. `-` means no difference. Effects accumulate in an
explicit local `observe` log; each observation returns an array containing the
returned/throw status, printed result, and printed effect log. Errors distinguish
arity rejection from other thrown failures without relying on engine-specific
messages. Top-level functions are canonically observed as `#function`, since the
reference printer's generated function names change between builds. Reduced
identity and nesting are tested inside the original programs before printing.
Every case creates fresh runtime/reducer instances. This harness uses ordinary
host JSON and literal expected strings rather than the shared execution encoder.

## Upstream source evidence

The complete selected bodies were read from the pinned checkout before importing
them. SHA-256 hashes in the fixture origin column identify the exact original
`deftest` source spans. Setup, assertion order, and original comparisons remain
in each complete witness. `deftest`/`testing` become `do`, `is` becomes `assert`,
and every `are` row becomes its own bound `let` and assertion. `0M` and `1M`
retain their identical JavaScript Number values as `0` and `1`.

| Source under `src/test/cljs/cljs` | Complete body | Executed assertions |
| --- | --- | ---: |
| `core_test.cljs` | `test-map-new-transducers` | 3 |
| `core_test.cljs` | `test-into+halt-when` | 2 |
| `core_test.cljs` | `test-cljs-1837` | 5 |
| `collections_test.cljs` | `test-cycle` | 43 |
| `collections_test.cljs` | `test-repeat` | 86 |
| `collections_test.cljs` | `test-iterate` | 28 |

These six bodies execute **167 assertions**. The two eager `into` assertions
from `core_test.cljs:test-transducers` also retain their original predicates,
bringing the imported selection to **169 executed assertions**. The full
`test-transducers` body has 29 assertion sites and mixes eager `into`, collection
`mapcat`, transducing `sequence`, and `eduction`; this suite explicitly selects
its two eager assertions. Transducing `sequence`, multi-source transducing
`sequence`, and `eduction` belong to a separate implementation task. They are
not exercised or claimed here. The transduce assertion in `test-cljs-1284`
depends on directly constructing an invalid `IndexedSeq` and is not claimed as
a complete imported body by this suite. Existing lazy sequence coverage remains
separately inventoried in [compiler-sequences.md](compiler-sequences.md).

## Native fixture adaptations and explicit differences

All native fixture IDs starting with `sci-transduce-`, `sci-into-xf-`, or
`sci-xf-` are ported, including constructor/reducer arity errors, callable data,
state reuse/interleaving, completion forwarding and reentry, failing callbacks,
nested `Reduced`, inner/outer lazy demand, and finite prefixes of infinite
sources. The origin column retains each SCI fixture ID and the SHA-256 of its
unadapted source. SCI values are source evidence; expected outcomes are freshly
measured with CLJS rather than copied from SCI's Number/collection model.

The source adaptations are explicit:

- `clojure.core/` references become `cljs.core/`; `catch Exception` becomes
  `catch js/Error`.
- Ratio literals become their JavaScript Number values. Large signed integers
  retain CLJS's Number rounding and arithmetic rather than JVM overflow.
- `(vswap! n inc)` becomes `(vreset! n (inc @n))`. This preserves the selected
  callbacks while avoiding the independently tracked compiler `vswap!` defect.
- The results of `native-into-xf-utf16` and `native-xf-cat-seqables` observe
  transduced string code units numerically with `charCodeAt`, preserving the
  lone-surrogate and surrogate-pair tests without depending on the independently
  tracked exact UTF-16 printer contract.

CLJS single-arity constructor applications generally accept absent/extra reducer
arguments. Their returned reducers still retain their original overloaded
arity contracts. `(map)` returns an empty lazy sequence; `(cat)` returns a
reducer capturing its absent argument. CLJS `partition-all` does not truncate or
validate its count at construction: `2.5`, negative counts, NaN, nil, strings,
and keywords can buffer the whole finite input until completion. Native
constructor/type/overflow expectations are therefore replaced by measured CLJS
results. Independent probes also check `partitionv-all`, structural and numeric
`dedupe`/`distinct` equality, indexes, false/nil replacement, drop-while latching,
partition-by callback order and Reduced completion, interpose early stopping,
take-nth edge counts, random-sample probabilities 0/1, and halt-when completion.

Exactly one reference-version difference is recorded:
`native-xf-basic-applied-unbounded` returns `[:mapped]` in both references,
but release realizes effect log `[0 1 2]`, while checkout realizes `[0 1 2 3]`.
The optional release column retains the release observation; generated
JavaScript follows the pinned checkout contract.

The former three persistent fallback differences for malformed `into` transforms
were removed when canonical transients landed. `native-into-xf-bad-xf-arity`,
`native-into-xf-bad-step`, and `native-into-xf-bad-completion` now expose the same
opaque `TransientVector` as both pinned references when completion is bypassed.
No local persistent fallback expectation remains; `into` uses the public editable
and transient protocols. See the [transient audit](compiler-transients.md).

## Verification

```sh
cargo test --locked --test compiler_transducers -- --include-ignored --nocapture
cargo test --locked --test compiler_transducers \
  eager_transducer_fixtures_match_pinned_release_and_checkout \
  -- --ignored --nocapture

cargo test --locked --test compiler_transducer_contracts \
  --test compiler_sequences --test compiler_core_helpers --quiet
cargo test --locked --test interpreter_transducers --quiet
```

The final `--include-ignored` run passes all **3 tests**. All **291** original
source hashes (285 native bodies and six upstream bodies) were independently
verified; the stricter native-ID/difference provenance test also passes.
All **316** observations pass the
checked release and checkout oracle with the explicit differences above. The
separate compiler contracts pass **6 tests**, including custom IFn/IReduce,
canonical macro execution, reducer loops, deterministic `rand` redefinition, and
cancellation at initialization, transform, source, step, and completion.
The retained sequence/core-helper suites pass **7 tests**; native transducers
pass **19 tests**. The full compiler-language ledger was not rerun for this
change. Transducing `sequence` and `eduction` suites were not executed or claimed
as coverage by this task.

Seven repeated Node measurements over 100,000 inputs gave median execution
samples of **488.396 ms** for ordinary reduce, **487.406 ms** for transducing map
inc/filter odd, and **487.824 ms** for nested cat/cat. Observed numeric results
were **4,999,950,000**, **5,000,050,000**, and **4,999,950,000**, respectively.
These are local smoke measurements, not performance thresholds or paired
historical comparisons.

Additional commands actually executed for the engine prerequisite and retained
runtime behavior:

```sh
cargo test --test evaluator --test compiler_bootstrap --quiet
node tests/support/js_runtime.test.js
node tests/support/js_core_helpers.test.js
cargo test --locked --test compiler_transducer_contracts --quiet
```

Evaluator/bootstrap: **20 passed**, with the preexisting manual evaluator test
ignored; both direct runtime assertion scripts passed; the final canonical
transducer contracts run passed **6 tests**. The six new core metadata rows were
extracted with the pinned `generate_core_var_metadata.clj` script, preserving
unrelated existing metadata rows. The former sequence namespace witness now
imports its `.cljs` macro source through ordinary `:require`, following the
current compiler contract. No full compiler-language ledger or unrelated
platform suite was rerun for this focused change.
