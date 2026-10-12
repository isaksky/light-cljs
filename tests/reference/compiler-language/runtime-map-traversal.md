# Canonical runtime map traversal

Pellet 181 preserves the original runtime-only witness byte for byte:

```clojure
(vec (seq (hash-map :a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9)))
```

Its source SHA-256 remains
`973e1cab49403f2708a7220a2470df4e3de7e9fbff9a0a6e432ed0cc020b8e07`.
Both independently compiled ClojureScript pins, release `1.12.134` and clean
checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, return the complete
entry sequence with keys `[:e :g :c :h :b :d :f :i :a]`, values
`[5 7 3 8 2 4 6 9 1]`, trace `[]` and stdout `""`. The actual local
application and demanded macro computations now match in both QuickJS and Node,
with fresh and reused MacroHosts. Reference settings are `:optimizations :none`,
`:static-fns false` and `:def-emits-var false`.

The [64-source catalog](runtime-map-traversal-contracts.json) retains all 39
complete original sources from [179](literal-map-traversal-contracts.json) in
unchanged order and with unchanged source bytes and independently rederived
canonical expectations. The historical contracts, nonpassing observations and
receipts remain unchanged. The previously failed map sequence neighbors and
already-fixed runtime set neighbor now pass complete packets. A historical
count/effect-only witness never supplies sequence credit.

Twenty-five additional whole sources cover hash-map/array-map construction and
identity, eight/nine-entry promotion, immutable versions, deeper hash partitions
and array nodes, nil first, equal-hash collisions and removal/reinsertion,
transient promotion/updates/collision deletion/persistence, metadata and emptying,
public positional hash roots and array-map slots, Number boundaries/nonfinite
keys, character/string/keyword/symbol keys, `into`/`conj`, `seq`/`keys`/`vals`,
`first`/`second`/`rest`/`next`, `reduce`/`reduce-kv` and early termination,
`iter` and ES6 keys/values/entries/iteration, printing, sorted maps, retained
custom key representatives, equality traversal, and key/value hashing effects
and caching. Historical composite-key sources retain their complete sequences.
Expectations were derived before local comparison at
`/private/tmp/light-cljs-181-independent-runtime-map-v1` and
`/private/tmp/light-cljs-181-independent-runtime-map-extension-v1`, then freshly
rederived in the delivery harness. No sequence sorting or projection is used.

Maps and promoted sets share traversal of their existing retained routing
nodes, in ascending branch order with nil first and collision storage order.
The traversal cache belongs to the immutable entry array. Updates retain their
own arrays/routes and cannot alter old versions or cached traversals. Sequence,
reduction, printing, inspection and macro form serialization consume this order.
Array maps, positional array-map slots and sorted maps preserve their storage
contracts. Map equality visits entries in this order. Map hashing calls the key
and value hashes in this order on first demand and caches the result; ordinary
sequence traversal never repeats key hash/equality effects. Lookup and update
routing preserve the original key representatives and their existing effects.

Four independently confirmed neighboring gaps remain whole-source, zero-credit
controls, with focused ordinary owners placed before strict 125:

- 182: public `iterator-seq` is unresolved in all four source phases.
- 183: `PersistentHashMap.fromArrays` is missing; applications throw a type
  error and both demanded macro contexts reject.
- 184: the direct `hash-map` source macro evaluates key/value operands in
  alternating order locally, while both pins group all keys before all values.
  A separate saved runtime-function source independently confirms ordinary
  positional argument evaluation and exact final traversal.
- 185: default map equality compares value operands in the opposite direction
  from canonical public `equiv-map`; asymmetric custom values expose the gap.
  A separate symmetric custom-value source proves complete traversal effects.

These are API resolution, factory, source macro lowering and equivalence roots;
none grants runtime traversal or full-language agreement credit. Their exact
sources, expected packets, actual packets and located rejections remain visible.
Both pins agree on all 64 sources; there are no reference-version differences
in this catalog. Missing engine execution after a compilation rejection is
reported as a rejection context, never fabricated as an engine pass.

The [producer](runtime_map_traversal.py) and
[Rust driver](../../compiler_runtime_map_traversal.rs) bind source bytes, commands,
checked pin identity/settings, producer hashes, executables, environment,
emitted namespaces, raw typed packets and the complete artifact census.
Raw evidence is
`/private/tmp/light-cljs-181-runtime-map-delivery-final-v2/evidence.json`, SHA-256
`f95d8d9b391fa438620ff79fcc70909d308428f8f1e2c13809328da57ad0e4cf`.
The [durable receipt](runtime-map-traversal-receipt.json) is byte-identical.
Its 500 actual engine observations comprise 480 accepted frames from 60 sources
and twenty zero-credit neighboring frames; six additional nonpassing source
contexts reject before engine execution. Every actual frame matches its exact
retained canonical or explicitly nonpassing regression contract.

[Source/order guards](test_runtime_map_traversal.py) reject changed contracts,
sequence positions, trace/stdout and missing raw proof. The
[raw replayer guards](test_runtime_map_raw_guards.py) reject changed original
source, emitted namespace, QuickJS output, Node input/output, command,
environment, executable, detached summary and missing artifacts, including
mutations with refreshed artifact hashes. Mutations are restored and pristine
replay passes. [Permanent ordinary regression](../../compiler_runtime_map_order.rs)
executes all sixty accepted whole sources in both engines and compiler phases,
then checks seven malformed map sources in both phases and fourteen same-host
recoveries in both engines. JSON property-name canonicalization preserves every
collection tag, sequence position, effect and output.

Verification includes:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/test_runtime_map_traversal.py
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/runtime_map_traversal.py --output /private/tmp/light-cljs-181-runtime-map-delivery-final-v2
PYTHONDONTWRITEBYTECODE=1 RUNTIME_MAP_RECEIPT=/private/tmp/light-cljs-181-runtime-map-delivery-final-v2/evidence.json python3 tests/reference/compiler-language/test_runtime_map_raw_guards.py
cargo test --locked --test compiler_runtime_map_order
cargo test --locked --test compiler_map_reader_order --test compiler_runtime_set_order --test compiler_hash_branch_effects --test compiler_equality_contracts --test compiler_transients --test compiler_public_constructors --test compiler_positional_constructors --test compiler_iterator_helpers --test compiler_sorted_contracts --test compiler_sorted_collections
cargo test --locked --test compiler_reader --test compiler_sequences
cargo test --locked --test compiler_macros generated_maps_preserve_owned_children_order_keys_and_quoted_data -- --exact
cargo test --locked --test compiler_macros generated_map_keys_keep_reader_name_parts_before_runtime_spelling_equality -- --exact
cargo test --locked --test compiler_macros quoted_macro_values_preserve_symbols_effects_and_generated_provenance -- --exact
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/extract_inventory.py --check
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/test_inventory.py
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/test_full_ledger.py
python3 benchmarks/run_compiler.py --output /private/tmp/light-cljs-181-benchmarks-final-v2 --samples 9 --iterations 2
```

The compiler benchmark bundle validates all observations and contains 423
resident timing samples, 423 allocation samples, 180 fresh-process samples and
1,278 runtime-operation samples. Concurrent verification and uncontrolled host
scheduling prevent a controlled performance comparison. The static ledger at
`/private/tmp/light-cljs-181-static-ledger.json` remains `strict_complete: false`;
original upstream/SCI bodies, expectations and all strict language criteria
are unchanged. This proof grants zero original upstream-body/public-arity and
full-language completion credit. Three ordinary reference/audit tests remain
ignored in the surrounding suite; the dedicated current map audit was actually
executed. Cancellation and singular/plural limit tests were excluded by inspected
names and exact macro filters. Externally blocked 156 remains untouched.
