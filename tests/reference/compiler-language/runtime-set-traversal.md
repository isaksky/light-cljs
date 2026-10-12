# Canonical runtime set traversal after promotion

Pellet 180 preserves both complete original sources:

```clojure
(vec (seq (set [14 3 12 9 2 15 5 10 7 0 13 8 1 6 4 11])))
```

```clojure
(let [events (atom []) f (fn* [x] (swap! events conj x) x) s #{(f 0) (f 1) (f 2) (f 3) (f 4) (f 5) (f 6) (f 7) (f 8) (f 9) (f 10) (f 11) (f 12) (f 13) (f 14) (f 15)} output (with-out-str (println (count s)))] [(count s) (contains? s 0) (contains? s 15) (vec (seq s)) @events output])
```

Fresh whole-source compilations in ClojureScript release `1.12.134` and clean
checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, with
`:optimizations :none`, `:static-fns false`, and `:def-emits-var false`,
independently return the exact sequence `[0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15]`.
The second source returns count `16`, both memberships `true`, that full sequence,
exactly-once events `[14 3 12 9 2 15 5 10 7 0 13 8 1 6 4 11]`, and captured
output `"16\n"`. The local application and demanded macro computations now match
those complete typed packets in fresh and reused hosts under QuickJS and Node.
No sequence projection, sorting of observations, or count-only credit is used.

The [36-source catalog](runtime-set-traversal-contracts.json) retains all twenty
complete sources from the historical [178 catalog](literal-set-traversal-contracts.json)
byte for byte, including the two witness IDs with their original `nonpassing`
suffixes. Their current evidence is passing; their historical sources,
expectations, and receipts are unchanged. Sixteen additional sources cover
runtime `set`, `hash-set`, `into`, the eighth/ninth-member threshold, deeper
five-bit partitions, array nodes, nil and same-hash collisions, persistent
versions and removal/reinsertion, transient updates, metadata and emptying,
`first`/`second`/`next`/`rest`, reduction, mapping, `iter`, ES6 iterators, printing,
sorted sets, and positional constants. Expectations were independently derived
at `/private/tmp/light-cljs-180-independent-runtime-set-v1` before local comparison,
then freshly rederived again by the delivery harness.

Promoted sets traverse their existing retained hash-trie nodes in ascending
branch order, keeping nil first and collision storage order. Cached traversal
belongs to the immutable entry array; updates use their own array and routes.
Small array-map-backed sets, positional constants, and sorted sets retain their
own storage order. The runtime applies the same traversal to sequences,
printing, value inspection and macro form serialization. Map traversal remains
separately owned by 181. Construction, key representatives, equality, hashing,
source operand evaluation and positional constructor storage are preserved.
The ordinary custom-key regression additionally proves that repeated set
traversal neither rehashes keys nor repeats their equality effects, and retains
the original key objects.

Dynamic duplicate scalar and vector-member literal sets remain two explicit
nonpassing controls: release deduplicates while checkout reports a duplicate.
They retain sixteen actual local frames and zero canonical acceptance credit.
The historical count/membership/effect-only witness remains separate from the
complete sixteen-member sequence witness.

The [proof producer](runtime_set_traversal.py) and
[Rust driver](../../compiler_runtime_set_traversal.rs) bind whole source bytes,
commands, checked reference identity/settings, producer source hashes,
executables, environment, emitted namespaces, raw typed results and the complete
artifact census. Current raw evidence is
`/private/tmp/light-cljs-180-runtime-set-delivery-v2/evidence.json`, SHA-256
`09a7c85ff48ee782a6c6007d08e645cfd52472cf63d6819ffcb8c64625f519fd`;
[its durable receipt](runtime-set-traversal-receipt.json) is byte-exact.
All 288 actual frames pass their exact contracts: 272 canonical accepted frames
and sixteen nonpassing version-control frames. The
[raw guard receipt](runtime-set-traversal-raw-guard-receipt.json) points to the
current executable evidence, and cannot substitute for replaying it.
The raw guard tests reject modified source, emitted namespace, QuickJS output,
Node stdin/stdout, command environment, executable hash, detached acceptance
summary and missing artifacts, including mutations with refreshed artifact
hashes. Every artifact was restored and pristine replay passed.

Verification commands:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/test_runtime_set_traversal.py
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/runtime_set_traversal.py --output /private/tmp/light-cljs-180-runtime-set-delivery-v2
PYTHONDONTWRITEBYTECODE=1 RUNTIME_SET_RECEIPT=/private/tmp/light-cljs-180-runtime-set-delivery-v2/evidence.json python3 tests/reference/compiler-language/test_runtime_set_raw_guards.py
cargo test --locked --test compiler_runtime_set_order
cargo test --locked --test compiler_map_reader_order --test compiler_transients
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/extract_inventory.py --check
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/test_inventory.py
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/test_full_ledger.py
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/report.py --output /private/tmp/light-cljs-180-static-ledger.json --json
python3 benchmarks/run_compiler.py --output /private/tmp/light-cljs-180-benchmarks-v1 --samples 9 --iterations 2
```

The focused ordinary regressions also include analyzer, equality contracts,
hash-branch effects, iterator helpers, macro source behavior, public constructors,
reader, sequences, set-project and complete sorted bodies. One verification
scope error launched the complete `compiler_macros` target, including test names
containing `limit`/`limits`; those tests passed, but were not intended to run.
No cancellation target or singular/plural limit test suite was launched, and
externally blocked 156 was neither investigated nor repaired.

The validated benchmark bundle has 423 resident timing samples, 423 allocation
samples, 180 fresh-process samples and 1,278 runtime-operation samples. Every
fixture observation was checked; concurrent verification and uncontrolled
scheduling prevent a controlled performance comparison. The static strict
ledger still reports `strict_complete: false`. This focused raw/executable proof
grants no original upstream body, public arity or full-language completion
credit, so 125 remains open under its unchanged strict criteria.
