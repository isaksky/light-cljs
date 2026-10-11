# Public core positional factories

light-cljs-142 connects the thirteen implemented public core types to their
pinned generated factories. Each factory is an ordinary saved/apply function
that constructs the canonical nominal type, including constructors represented
by JavaScript classes. Constructor, protocol and metadata identities survive
application and canonical macro execution.

| Type/factory | Exact public positional fields |
| --- | --- |
| IndexedSeq / ->IndexedSeq | arr i meta |
| UUID / ->UUID | uuid __hash |
| Range / ->Range | meta start end step chunk chunk-next __hash |
| IntegerRange / ->IntegerRange | meta start end step cnt __hash |
| PersistentTreeMap / ->PersistentTreeMap | comp tree cnt meta __hash |
| PersistentTreeSet / ->PersistentTreeSet | meta tree-map __hash |
| PersistentTreeMapSeq / ->PersistentTreeMapSeq | meta stack ascending? cnt __hash |
| Eduction / ->Eduction | xform coll |
| MultiFn / ->MultiFn | name dispatch-fn default-dispatch-val hierarchy method-table prefer-table method-cache cached-hierarchy |
| SeqIter / ->SeqIter | _seq _next |
| RangedIterator / ->RangedIterator | i base arr v start end |
| Namespace / ->Namespace | obj name |
| TaggedLiteral / ->TaggedLiteral | tag form |

The constructor audit fixes discarded public slots. IndexedSeq retains its
supplied host array and index. UUID retains spelling and its mutable hash cache.
Range retains supplied chunks, following chunks and hash; clones and metadata
copies own independent mutable chunk slots. IntegerRange retains its explicit
count/hash, including the pinned distinction between count-based reduction
without an initial value and end-based traversal/reduction with one. Sorted
map/set/sequence constructors retain trees, backing maps, stacks, counts and
hashes; construction does not traverse trees or invoke getters/comparators.
Entry-consuming operations demand private canonical payloads. Public tree
storage is an opaque first-party representation; private upstream tree-node
implementation classes are outside this construction contract.

SeqIter uses the pinned two mutable slots and session-owned INIT/START sentinels.
RangedIterator retains all six fields and crosses vector chunks from the supplied
state. Eduction and MultiFn preserve transforms, inputs and supplied atom tables.
TaggedLiteral's constructor/factory preserves raw slots; the tagged-literal
convenience function retains its separate tag validation.

`tests/compiler_core_factories.rs` runs seventeen independent contracts and
eight complete upstream test bodies in QuickJS and Node applications and
canonical macro computations. Cases cover direct constructors, saved constructors
with new, direct/saved/apply factories, protocol membership, field identity,
metadata/hash, array mutation, chunk/iterator state, supplied counts, reduction
and comparator effects, constructor getter effects, nil/undefined slots and
all thirteen factory Var signatures. Separate tests preserve phase-isolated roots,
unsupported expansion-result diagnostics with host recovery, and one thousand
factory/reduction repetitions returning 8000.

The upstream bodies cover range, sorted maps/sets, transducers/eduction,
multimethods, UUID, tagged literals and iterator/sequence equivalence. The iterator
body retains its full helper and record definitions. The multimethod body retains
both method definitions and the exact earlier hierarchy relation it consumes;
its original setup-body hash and selected derive-expression hash are recorded.
Unrelated assertions in that earlier hierarchy test are not claimed. Source
namespace keywords are explicitly qualified. `is`/`are` become assert, `testing`
becomes do, and LITE_MODE is false with the original branches retained.

All twenty-five cases pass independently on Maven CLJS **1.12.134** and clean
checkout **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e** through execution.clj,
with checked `:optimizations :none`, `:static-fns false`, `:def-emits-var false`.
Full build logs/results remain under
`/tmp/light-cljs-142-factories-final-reference-4`. The adjacent receipt retains
observations and declaration/body/signature/source/test hashes. The reconciler
retains existing source-assertion witnesses and existing phase-test identities;
new constructor/factory evidence supplements them without double counting.

```sh
python3 -B tests/reference/compiler-core-factories.py
python3 -B tests/reference/compiler-core-factories.py --reference both --artifacts /tmp/fresh-core-factory-reference
cargo test --locked --test compiler_core_factories --test compiler_public_constructors --test compiler_core_var_metadata --test compiler_sorted_contracts --test compiler_eductions --test compiler_eduction_contracts --test compiler_multimethods --test compiler_sequences --test compiler_equality --test compiler_public_protocols --test provenance
node tests/support/js_runtime.test.js
node tests/support/js_core_helpers.test.js
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B -m unittest discover -s tests/reference/compiler-language -p 'test_*.py'
cargo check --locked --all-features --all-targets
cargo bench --locked --bench evaluator -- --iterations 20
```

The distinct unresolved public seq-iter/ranged-iterator helpers have
duplicate-checked live owner **light-cljs-144**. The public ns-name result-type
difference retains owner **light-cljs-143**. The wider intended-language ledger
remains incomplete under **light-cljs-125**; this scoped construction work does
not establish language completion.

Verification also passes the eleven relevant Rust integration suites (41 active
tests, with two existing broad reference tests ignored), both Node runtime
suites, all 73 ledger/accounting tests, the metadata generator check,
all-features/all-targets checking and final formatting/diff checks. A final
constructor/sorted/provenance rerun passes seven tests. The evaluator benchmark
validates result 5050 and fresh/reused sessions with bounded-loop reset/recovery:
twenty-iteration totals were 374,454,209 ns fresh, 2,250,333 ns reused and
1,450,543,542 ns reset/recovery. These are observed regression samples, without
a whole-language performance completion claim.
