# Public positional construction

`Cons`, `Reduced`, and `Symbol`, including `->Cons`, `->Reduced`, and `->Symbol`,
initialize the same branded canonical stores as ordinary core values. Direct
construction, saved constructor values with `new`, and saved/applied factories
execute in Node and QuickJS applications and in eligible macro computations.

| Constructor | Exact public fields | Observed contract |
| --- | --- | --- |
| Cons (4) | meta, first, rest, __hash | Retain the raw supplied tail, without realizing or normalizing it; nil rest becomes an empty list. Preserve sequence identity, reduction, clone, metadata and cached hash. |
| Reduced (1) | val | Dereference retains nil, false and nested markers; reduction stops at the marker. The public val slot remains mutable. |
| Symbol (5) | ns, name, str, _hash, _meta | Keep namespace/name independent of supplied printed spelling; preserve callable lookup, equality, metadata and cached hash. |

A nil cached hash computes and caches the public hash. An explicit `-1` is a
cached hash, including for Symbol; it is not a missing-cache sentinel. Hash
returns the JavaScript int32 coercion of a supplied cache, while the public slot
retains its supplied value. Metadata and clone copies retain their own hash
slots. Accessors update only existing branded values; inheriting a prototype and
writing fields cannot initialize a canonical payload.

The local constructors diagnose wrong arities with `Invalid arity: N`. This is
a deliberate diagnostic difference from permissive upstream JavaScript fixed
functions/constructors accepting absent or extra arguments. Invalid sequence
tails fail when consumed (`42 is not ISeqable`), without a recursion overflow.
Eligible macro computations return finite persistent values/forms; constructed
Cons forms and quoted Symbol forms expand correctly. Reduced objects and
constructor functions remain unsupported expansion results, with a located
`Unsupported macro result` diagnostic and reusable host recovery. Application
and macro mutation roots remain separate. `satisfies?` witnesses use literal
protocol names, as required by the pinned source macro's protocol-symbol lookup.

## Reproducible evidence

`tests/compiler_public_constructors.rs::public_constructor_api_arities_and_macro_phase_execute`
executes five complete construction contracts and four complete upstream bodies
in both application engines and as canonical macros. Those bodies are
`core_test.cljs::test-symbol-meta`, `core_test.cljs::test-reductions-obeys-reduced`,
`primitives_test.cljs::test-symbols-and-keywords`, and
`seqs_test.cljs::test-seq-operations`: all 32 original assertion sites, setup,
iteration and effect paths remain present. `is` becomes `assert`; `testing`
becomes `do`. Their existing assertion witness links are retained; this adds no
new claim to unrelated source sites. Constructor contracts independently exercise
all explicit arguments, raw/lazy tails and ordered effects, supplied/computed
hashes, nominal and protocol identity, first/rest/next/reduce, callable symbols,
dereference, clone, saved/new and saved/apply calls.

The adapter verifies pinned body/source spans and exact signature hashes, then
binds six public declarations/factories and six local metadata registrations to
the complete named execution test and hashed fixture files. The generated ledger
now includes authoritative factory Var metadata and current source registrations.
The separate diagnostics/recovery test and direct JS brand regressions also pass.

Executed commands:

```sh
python3 -B tests/reference/compiler-public-constructors.py --write
python3 -B tests/reference/compiler-public-constructors.py --reference both --artifacts /tmp/light-cljs-139-constructors-final-reference-2
cargo test --locked --test compiler_public_constructors -- --exact public_constructor_api_arities_and_macro_phase_execute --nocapture
cargo test --locked --test compiler_public_constructors --test compiler_core_var_metadata --test compiler_equality --test compiler_sequences --test compiler_transducers --test compiler_extensions --test compiler_public_protocols
cargo test --locked --test provenance
node tests/support/js_runtime.test.js
node tests/support/js_core_helpers.test.js
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B -m unittest discover -s tests/reference/compiler-language -p 'test_*.py'
clojure -Srepro -Sdeps '{:deps {org.clojure/clojurescript {:local/root "research/clojurescript"}}}' -M tests/reference/generate_core_var_metadata.clj --check
cargo check --locked --all-features --all-targets
cargo bench --locked --bench evaluator -- --iterations 20
rustfmt --check tests/compiler_public_constructors.rs
git diff --check
```

All nine reference cases pass independently on Maven CLJS **1.12.134** and the
clean checkout **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, through the checked
`execution.clj` harness with `:optimizations :none`, `:static-fns false`, and
`:def-emits-var false`. The retained receipt identifies source/test hashes and
reference outcomes; disposable full builds/logs remain in the artifact directory.
The focused regression run passed 22 tests with two existing ignored broad
reference tests; the nine new cases did execute both pins. All 73 ledger runner
and accounting tests pass. A twenty-iteration evaluator run validates result
5050, fresh/reused sessions and sticky-limit reset/recovery; timing is an observed
sample, not a whole-language performance acceptance claim. Final totals for 20
iterations were 300,235,042 ns fresh, 2,041,958 ns reused and 1,318,021,459 ns
limit/reset recovery.

## Remaining audited public construction

The builtinType empty fallback still affects thirteen intended public types:
List, EmptyList, Keyword, Cycle, Repeat, Iterate, PersistentVector, Subvec,
ChunkedSeq, PersistentQueue, PersistentArrayMap, PersistentHashMap, and
PersistentHashSet. A direct construction audit against both checked runtimes
returns `[1,0,"foo",1,7,0,0,2,1,0,1,0,0]` from first/count/name/get observations;
local construction overflows, rejects those operations, or returns nil. Exact
probes and receipts are recorded in live **light-cljs-141**.

Missing factories on already implemented types (including ->UUID and
->IndexedSeq) have focused live owner **light-cljs-142**. The rest of the
independent intended constructor inventory remains visible under its live ledger
owners. These are public construction obligations; private trie nodes and
specific generated JS spellings are excluded for the existing scope reason.
**light-cljs-125 remains incomplete.** This constructor work establishes only the
six named public contracts and their directly executing evidence.
