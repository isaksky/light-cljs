# Variadic mapv collection demand through apply

`map` and `mapv` now advertise their canonical four-argument fixed prefix and
variadic apply entry point. Their extra collection arguments stay in a sequence;
`mapv` delegates through `apply map`, whose existing tuple traversal stops when
any input is empty. The previous plain JavaScript fallback materialized the
argument tail and demanded invalid keys before mapping could terminate.
Shared fixed-input iteration and `apply` demand rules remain intact.

The complete unchanged original is:

```clojure
[(apply mapv vector [] [] [] (keys [9])) (apply mapv vector [1] [2] [] (keys [9])) (apply mapv vector [1] [2] [3] (cons [] (keys [9])))]
```

It returns `[[] [] []]` with empty original effects and stdout/stderr in release
**1.12.134**, checkout **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, and all
current application/macro contexts. Both pins were independently executed with
`optimizations:none/static-fns:false/def-emits-var:false`. Original source
SHA-256 is `e0a907824be6de52f6110387000b5a50bdc2a73524df8dfe1edb88d55f121303`;
original row SHA-256 is
`73a201bf60f8ee89a8037528864be6e3f22cd6083c97d1b3f8387cc90e79a4d6`.
Corpus and historical nonpassing disposition bytes remain unchanged.

Fifteen focused sources run through actual fresh/reused MacroHosts, in
application and demanded macro computations, then execute on QuickJS and Node:
**120 observations**, plus **two recovery observations** after an uncaught macro
key failure. Controls retain empty first/middle/late inputs, deferred extra
collections, ordered public ISeqable/IMapEntry effects, nonempty map/mapv
results, fixed arities, metadata and callable wrappers, required invalid-key
failure and host recovery. The pinned arity-count lookahead demands one extra
argument-tail step; the focused deferred-tail control preserves that effect
while avoiding a later unneeded throw. Both pins pass all 16 original/control
checks. Artifacts remain under `/tmp/light-cljs-168-reference-final` and
`/tmp/light-cljs-168-phases-final`.

The retained focused receipt is
[`compiler-mapv-apply-receipt.json`](compiler-mapv-apply-receipt.json), SHA-256
`48660e6bf3fb50fc1e8c8428112ef82294069f16aa3fe36285d21055bca9d9e3`. Fresh complete 30-source canonical
execution is at `/tmp/light-cljs-168-canonical-current/evidence.json`, SHA-256
`a27d10d93f81223de79ff257faf7e85a0124ee2fd6b494aed9e04b29f82073cf`.
Actual `full_ledger.expand_report` replay now records **two covered originals,
27 verified canonical differences, and one missing contract**, with every
original mapv application/macro frame passing. The remaining watch-key gap is
owned by 169. Strict comprehensive completion remains **false**; this focused
repair and closure do not establish 125 acceptance. Historical receipts supply
no current positive fallback credit.

Twenty-two relevant focused/compiler regression tests and 18 canonical
accounting guards passed, alongside direct Node runtime/helper checks, source
inventory freshness, focused Rust formatting and whitespace checks. Only two
named safe sequence-contract tests were selected. No cancellation or singular/
plural limit test ran, and no 156 investigation or repair occurred.

The normal validated compiler performance driver retained nine samples with two
iterations, separate allocations, fresh-process output checks, and immutable
primitive baseline `480050e`: **423 resident timing samples across 47 phase
pairs** and **1,278 operation samples**. Collection total compilation median was
0.046792 ms. Artifacts are at `/tmp/light-cljs-168-performance-final`.
This is a short validated smoke measurement with an unpaired baseline;
no improvement claim is made.

Provisional controls under `/tmp/light-cljs-168-reference` and
`/tmp/light-cljs-168-phases` failed because their newly authored scalar-key and
argument-tail expectations were incorrect; the unchanged original passed.
Those attempts retain no control pass credit. The first performance capture at
`/tmp/light-cljs-168-performance` rejected a formatting change during execution
and is also nonpassing. Corrected controls were independently checked on both
pins, and the frozen-source performance rerun passed.

```sh
cargo test --locked --test compiler_mapv_apply
python3 -B tests/reference/compiler-mapv-apply.py --output /tmp/mapv-apply-pins-fresh
python3 -B tests/reference/compiler-language/sci_gap_canonical_context.py --output /tmp/mapv-apply-canonical-fresh
```
