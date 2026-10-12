# Canonical hash-map source operand groups

Pellet 184 registers `hash-map` as a compiler-owned source macro. Direct and
qualified calls emit the public `PersistentHashMap.fromArrays` factory with a
key array followed by a value array. Consequently all key operands run before
all value operands. Zero arguments return `PersistentHashMap.EMPTY` by identity.
The macro preserves the pinned pair padding/removal and stops the value group
at the first incomplete pair, including explicit `:cljs.core/missing` operands.
Saved functions, `apply`, locally shadowed names and function-position calls
retain ordinary alternating argument evaluation.

The [original fixture](../../fixtures/compiler-hash-map-source-original.cljc)
retains the entire nine-entry constructor/effects/printing source, SHA-256
`c7d164a4fef4ef73fae763dfbdd98570ef4b7eb2a6443d0ca0bd9b3bbf7d7ef0`.
Its unchanged typed result has count 9, entries in `[:e :g :c :h :b :d :f :i :a]`
order, effects `[:a :b :c :d :e :f :g :h :i 1 2 3 4 5 6 7 8 9]`, and captured
printing `"{:e 5, :g 7, :c 3, :h 8, :b 2, :d 4, :f 6, :i 9, :a 1}\n"`.
Outer trace is `[]` and stdout is `""`. Before implementation, the permanent
regression failed on the original alternating local effects; afterward the
unchanged complete source passed.

Before any local use, fresh independent release `1.12.134` and clean checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` compiled/executed all seventeen full
sources at `/private/tmp/light-cljs-184-independent-contracts-v2`. Both pins used
`:optimizations :none`, `:static-fns false`, and `:def-emits-var false` and agreed
exactly. The frozen [contracts](hash-map-source-contracts.json) cover the
original, direct/qualified/saved/apply calls, zero/one/odd arguments, throwing
key/value operands, sentinel controls, shadowing and function-position calls,
duplicate keys, nil, string hash collisions, rounded Number keys, signed zero,
NaN and infinities, metadata, retained versions and custom hash/equality effects.
Each comparison retains the complete typed value, lookup/count/sequence,
effects and printing; no collection projection or sorting supplies credit.

The [producer](hash_map_source.py) independently rebuilds both references and
runs the [Rust audit](../../compiler_hash_map_source.rs). Its exact raw replay
passes 34 pin packets and 136 actual engine frames: all seventeen sources in
application/demanded macro, fresh/reused MacroHost and QuickJS/Node contexts.
The dedicated ignored audit was actually executed. The
[durable receipt](hash-map-source-receipt.json) is byte-identical to
`/private/tmp/light-cljs-184-source-delivery-v1/evidence.json`, SHA-256
`cd6b90a83b604c819d735c88a6c7a497357d1e6530992c280956ed03c10ec083`.
It binds complete sources, reference identities/settings/declarations,
compiler and producer sources, audit executable, commands, environment,
reused-host warmup, emitted namespaces, raw outcomes and artifact census.

The ordinary permanent regression executes every complete packet in both
engines and phases, then checks uncaught odd calls and seven malformed reader
sources with eighteen same-host recoveries in both engines.
[Evidence guards](test_hash_map_source.py) reject altered sources, sequence
order, trace/stdout, emitted units, raw output, Node input, commands,
environments, executables, missing artifacts and detached summary credit.
Raw mutations remain rejected after refreshed artifact hashes; every mutation
is restored and pristine replay passes.

Verification:

```sh
cargo test --locked --test compiler_hash_map_source hash_map_source_preserves_exact_contracts_and_same_host_recovery -- --exact
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/hash_map_source.py --output /private/tmp/light-cljs-184-source-delivery-v1
PYTHONDONTWRITEBYTECODE=1 HASH_MAP_SOURCE_RECEIPT=/private/tmp/light-cljs-184-source-delivery-v1/evidence.json python3 tests/reference/compiler-language/test_hash_map_source.py
cargo test --locked --test compiler_runtime_map_order --test compiler_map_reader_order --test compiler_runtime_set_order --test compiler_hash_branch_effects --test compiler_equality_contracts --test compiler_transients --test compiler_public_constructors --test compiler_positional_constructors --test compiler_iterator_helpers --test compiler_sorted_contracts --test compiler_sorted_collections --test compiler_hash_map_from_arrays --test compiler_bootstrap --test compiler_macros -- --skip limit --skip cancel
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/extract_inventory.py --check
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/test_runtime_map_traversal.py
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/test_full_ledger.py
python3 benchmarks/run_compiler.py --output /private/tmp/light-cljs-184-benchmarks-v1 --samples 9 --iterations 2
```

The adjacent regression suites pass 146 tests. Four separate historical
reference audits remain ignored and supply no fresh reference credit; six
singular/plural limit tests are explicitly filtered by inspected names.
The changed Rust test passes its standalone rustfmt check, and
`git diff --check` passes.

Normal compiler benchmarks validated 423 resident timing samples, 423
allocation samples, 180 fresh-process samples and 1,278 runtime-operation
samples across 142 operation/size groups in the saved bundle. Concurrent
regression checks and uncontrolled scheduling do not support a controlled
performance comparison or speedup claim.

Original upstream/SCI bodies, historical traversal catalogs/receipts and strict
125 criteria remain unchanged. Their old whole-source nonpassing controls
retain zero credit; this focused proof independently verifies the resolved
constructor source without rewriting historical evidence. It supplies zero
original upstream-body/public-arity or full-language completion credit.
Map value equivalence direction remains separately owned by 185. No new
distinct defect was found. Cancellation and singular/plural limit names were
inspected and excluded; externally blocked 156 was not investigated or retried.
No push or PR is involved.
