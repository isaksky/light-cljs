# Public PersistentHashMap.fromArrays

Pellet 183 delivers the pinned public static factory in the shared JavaScript
runtime, so applications and demanded macro computations use the same function.
It captures the key-array length, checks for a missing value before reading the
key/value pair, and inserts through the retained transient hash-map routing path.
It returns a persistent hash map without retaining mutable input arrays.
No analyzer lowering, ordinary runtime operand evaluation, traversal order,
collection identity, Number semantics or existing equality behavior changed.

The [original fixture](../../fixtures/compiler-hash-map-from-arrays-original.cljc)
retains the entire nine-entry source byte for byte, SHA-256
`3058d5292009a7bf86efa2e24ef9c3f60045c86e3ee548077f3a894bf8907dc0`.
Fresh independent release `1.12.134` and clean checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` builds at
`/private/tmp/light-cljs-183-independent-from-arrays-v1` confirm the complete
returned map entries, keys and values. Their order is `[:e :g :c :h :b :d :f :i :a]`
and `[5 7 3 8 2 4 6 9 1]`, with trace `[]` and stdout `""`.
Both pins use `:optimizations :none`, `:static-fns false` and
`:def-emits-var false`.

Before implementing the factory, twelve complete source expectations were
independently derived from both pins at
`/private/tmp/light-cljs-183-independent-contracts-v1`. The frozen
[contracts](hash-map-from-arrays-contracts.json) include the unchanged original,
direct/qualified/saved/apply calls, empty inputs, missing/extra values, native
calling behavior at zero/one/extra arguments, native malformed inputs, duplicate
keys, nil, numeric and string hash collisions, rounded Number keys, signed zero,
NaN and infinities, immutable versions and metadata, input mutation, operand
and accessor order, custom hash/equality effects and retained representatives.
Captured printing is asserted as `"2\n"`; complete outer trace and stdout remain
explicit in every typed packet. Nothing is sorted or projected to make a
sequence pass.

The factory follows the pinned ordinary JavaScript function behavior: extra
arguments/values are ignored; an empty key array does not demand the values
operand's length; nonempty missing values throw `Error` with
`No value supplied for key: ...`; accessing a missing key/value object can throw
`TypeError`. Saved and `apply` calls preserve these same outcomes. No artificial
fixed-arity check or new array validation is added.

The [producer](hash_map_from_arrays.py) freshly rebuilds both references and
executes the [Rust audit](../../compiler_hash_map_from_arrays.rs), then verifies
24 exact reference packets and 96 actual engine frames: all twelve sources in
application/demanded macro, fresh/reused MacroHost and QuickJS/Node contexts.
The dedicated ignored audit was actually executed. The
[durable receipt](hash-map-from-arrays-receipt.json) is byte-identical to
`/private/tmp/light-cljs-183-from-arrays-delivery-final-v2/evidence.json`, SHA-256
`cc0a75fb844f02c88e1205fcc03bd33a8d793d58ce32a04a0be25f450365d587`.
It binds full source bytes, reference identities/settings/declarations,
compiler/producer sources, Cargo audit executable, commands, environment,
reused macro warmup, emitted namespaces, raw outcomes and artifact census.

The permanent ordinary regression executes every complete packet in both
engines and phases, then exercises uncaught missing-value/type errors and seven
malformed reader sources, followed by eighteen same-host recoveries in both
engines. [Evidence guards](test_hash_map_from_arrays.py) reject altered sources,
sequence order, trace/stdout, emitted units, raw engine output, Node input,
commands, environments, executables, missing artifacts and detached summary
credit. Raw mutations remain rejected after refreshed artifact hashes; every
mutation is restored and pristine replay passes.

Verification:

```sh
cargo test --locked --test compiler_hash_map_from_arrays public_from_arrays_preserves_exact_contracts_and_same_host_recovery -- --exact
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/hash_map_from_arrays.py --output /private/tmp/light-cljs-183-from-arrays-delivery-final-v2
PYTHONDONTWRITEBYTECODE=1 FROM_ARRAYS_RECEIPT=/private/tmp/light-cljs-183-from-arrays-delivery-final-v2/evidence.json python3 tests/reference/compiler-language/test_hash_map_from_arrays.py
cargo test --locked --test compiler_runtime_map_order --test compiler_map_reader_order --test compiler_runtime_set_order --test compiler_hash_branch_effects --test compiler_equality_contracts --test compiler_transients --test compiler_public_constructors --test compiler_positional_constructors --test compiler_iterator_helpers --test compiler_sorted_contracts --test compiler_sorted_collections
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/extract_inventory.py --check
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/test_runtime_map_traversal.py
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/test_full_ledger.py
python3 benchmarks/run_compiler.py --output /private/tmp/light-cljs-183-benchmarks-final-v1 --samples 9 --iterations 2
```

The adjacent regression suites pass 24 tests; their three separate historical
reference audits remain ignored, without supplying fresh reference credit.
Formatting of the changed Rust test passes its standalone rustfmt check.
Repository-wide `cargo fmt --check` reports existing formatting differences in
unmodified source and tests; these are not rewritten by this change.

Normal compiler benchmark observations are validated in the saved bundle:
423 resident timing samples, 423 allocation samples, 180 fresh-process samples
and 1,278 runtime-operation samples.
Concurrent checks and uncontrolled scheduling do not support a controlled
performance comparison or speedup claim.

Original upstream/SCI bodies, historical map catalogs and receipts, and strict
125 criteria remain unchanged. This focused factory proof supplies zero original
upstream-body/public-arity and full-language completion credit. Source-macro
operand grouping (184) and asymmetric map equality direction (185) remain
separate open owners; their original whole-source failures retain zero credit.
No new distinct defect was found. Cancellation and singular/plural limit suites
were excluded by inspected names; externally blocked 156 was not investigated
or retried. No push or PR is involved.
