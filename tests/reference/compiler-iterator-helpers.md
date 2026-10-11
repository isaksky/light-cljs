# Public iterator helpers

light-cljs-144 exposes `seq-iter [coll]` and `ranged-iterator [v start end]`
through the canonical SeqIter and RangedIterator classes. Direct, saved and
apply calls share the public helper implementation in applications and eligible
canonical macro computations. Generated Var metadata retains the exact pinned
arglists; the declarations have no upstream docstrings.

The independent contract checks INIT/START and slot identity, deferred sequence
initialization, repeated hasNext, ordered head/tail realization, empty inputs,
host array mutation and exhausted sequence errors. Ranged iterators retain their
explicit start/end, vector and chunk slots, permit mutable chunk-array changes,
and cross both 32- and 64-element boundaries. Empty/reversed/end bounds retain
the pinned initialization. RangedIterator's next remains unchecked after
exhaustion: it can read a following chunk value even when hasNext is false; an
empty iterator has a nil backing array and next throws. These observations are
verified behavior, without added bounds validation.

The complete upstream iterator-equivalence body retains its helper, record,
collection setup and iteration. It executes in QuickJS and Node applications
and canonical macros. Separate tests check phase-isolated atom effects,
unsupported helper/function/iterator expansion results, MacroHost recovery and
one thousand repetitions returning 12000. A separate reused-session witness
retains saved iterator progress across evaluations and checks fresh-session
initialization.

Both unchanged reference cases pass on Maven ClojureScript **1.12.134** and clean
checkout **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, with checked
`:optimizations :none`, `:static-fns false`, `:def-emits-var false`. Logs and build
artifacts are retained at `/tmp/light-cljs-144-iterator-helpers-reference-final`.
The adjacent receipt includes complete helper/class/sentinel declarations,
source hashes, full setup/body hashes, unchanged contract sources, observations,
settings and local test/support hashes. Four public/local execution mappings
retain declaration/signature/body/test/support hashes and focused ownership;
generated inventory remains evidence inventory, without static passing claims.
Broader language validation remains incomplete under light-cljs-125.

```sh
python3 -B tests/reference/compiler-iterator-helpers.py
python3 -B tests/reference/compiler-iterator-helpers.py --reference --artifacts /tmp/fresh-iterator-helper-reference
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B -m unittest discover -s tests/reference/compiler-language -p 'test_*.py'
cargo test --locked --test compiler_iterator_helpers --test compiler_core_factories --test compiler_sequences --test compiler_public_constructors --test compiler_core_var_metadata --test provenance
node tests/support/js_runtime.test.js
node tests/support/js_core_helpers.test.js
cargo check --locked --all-features --all-targets
cargo bench --locked --bench evaluator -- --iterations 20
```

The evaluator performance sample validates result 5050 with fresh/reused
sessions and bounded-loop reset/recovery. Twenty-iteration totals were
378,635,000 ns fresh, 2,262,042 ns reused and 1,511,432,875 ns reset/recovery.
These are observed regression samples, without a whole-language performance
completion claim. Both Node runtime suites, all 73 ledger/accounting tests,
the checked metadata generator and all-features/all-targets checking pass.
The six relevant Rust integration suites pass 18 active tests, including the
final three iterator helper tests. Metadata/factory/helper reconciliations,
inventory checking, focused Rust formatting and diff whitespace checks pass.
