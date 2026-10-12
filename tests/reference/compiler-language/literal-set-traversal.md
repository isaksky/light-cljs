# Canonical set literal traversal

Pellet 178 preserves the exact reported source:

```clojure
(let [s '#{\a "a" '\b \b}] [(count s) (contains? s \b) (contains? s '(quote \b))])
```

The complete source with `(vec (seq s))` returns `[4 true true ["a" "b" (quote "b") "a"]` in both independently executed ClojureScript pins: release `1.12.134` and clean checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`. This order is required as a complete sequence. Count and membership alone grant no traversal credit.

The 20-source [frozen catalog](literal-set-traversal-contracts.json) was derived by fresh whole-source compilation and Node execution in both pins with `:optimizations :none`, `:static-fns false`, and `:def-emits-var false`. It covers heterogeneous scalars, nil, booleans, negative and floating values, namespaced names, same-hash strings, character and string values, nested quote/list/vector/map/set constants, over-eight-member sets, count, membership, complete sequence, and exact effects and captured output. The independent expectations precede local comparison. Every packet requires exact typed outcome, trace, and stdout.

The unchanged analyzer regression source is `#{(js/parseInt "1") (js/parseInt "2")}`. Its historical span order was source insertion `(js/parseInt "1")`, then `(js/parseInt "2")`; both pins' complete sequence witness `(vec (seq #{(js/parseInt "1") (js/parseInt "2")}))` returns `[2 1]`. The regression now expects spans in that pinned order while preserving the original source bytes.

The unchanged macro regression defines macros with the source string `(ns macros) (defmacro identity-form [x] x) (defmacro data [] #{:a [1 2]}) (defmacro quoted [] '(quote #{danger (unresolved 1)})) (defmacro ignore [x] nil)`. It constructs `members` by joining `(f 0)` through `(f 15)` with spaces and formats the application source `(let [f (fn* [x] (js/console.log x) x) s (identity-form #{(f 0) (f 1) (f 2) (f 3) (f 4) (f 5) (f 6) (f 7) (f 8) (f 9) (f 10) (f 11) (f 12) (f 13) (f 14) (f 15)})] (js/console.log (count s) (contains? (data) [1 2]) (contains? (quoted) '(unresolved 1))))`. The historical test expected effect lines `0` through `15` followed by `16 true true`. The two pins instead evaluate each member once in order `[14 3 12 9 2 15 5 10 7 0 13 8 1 6 4 11]`. A separate 16-member witness records that event order, count `16`, membership of `0` and `15`, and captured output `"16\n"`; the macro test keeps its original identity-form source.

The complete 16-member sequence witness remains **nonpassing**. Both pins iterate the resulting dynamic set as `[0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15]`; the local runtime iterates in its insertion order `[14 3 12 9 2 15 5 10 7 0 13 8 1 6 4 11]`. The runtime-only source `(vec (seq (set [14 3 12 9 2 15 5 10 7 0 13 8 1 6 4 11])))` independently proves the same separate runtime defect and is also nonpassing. Pellet 180 owns that runtime traversal defect. Dynamic duplicate scalar and vector-member sets remain two additional nonpassing release/checkout version differences: release deduplicates; checkout catches a duplicate. None of these four sources receives acceptance credit.

The [proof harness](literal_set_traversal.py) compiles every source as an application and an actually demanded macro computation in fresh and reused macro hosts, then executes each emitted program in QuickJS and Node. The [Rust audit driver](../../compiler_literal_set_traversal.rs) records complete phase source, compiled JavaScript, engine script, raw outcomes, Node process outputs, and the reused-host warmup. The [raw receipt](literal-set-traversal-raw-guard-receipt.json) identifies the current `/private/tmp/light-cljs-178-literal-set-delivery-v4` evidence: SHA-256 `c7899e471cff1d36da00f1a63483620373c6b1182458cd41fe4a8dff81d88759`; 160 actual engine frames, 128 accepted and 32 nonpassing. The producer binds compiler/reference inputs, executable hashes, commands, environment, exact sources, and a complete artifact census. Deliberate raw, source, compiled namespace, Node input, environment, executable hash, and detached-summary mutations were rejected by their underlying replay checks after refreshing the artifact census; pristine replay passed after restoration. The saved receipt is a pointer, never a substitute for current raw executable proof.

The compiler computes reader hashes before Number/character lowering and walks
successive five-bit hash partitions, with nil first and stable same-hash
collisions. Ordered and unordered nested reader data use their respective
collection hashes. Quoted sets emit positional constant storage, preserving
reader-distinct members at every size. Source set expressions are analyzed in
that reader order, so operand effects run exactly once in the pinned order;
macro-computed sets preserve their already established runtime traversal.
General runtime set iteration is unchanged and remains owned by 180.

Verification passed 180 ordinary analyzer/bootstrap/equality/macro/reader/literal
regressions, 13 literal proof guards, 18 inventory guards and 22 full-ledger
guards. The existing character/string and numeric catalogs were freshly rerun
against both pins and all 576 actual phase/engine frames, plus 296 same-host
recovery outcomes; their old receipts remain byte-exact historical files.
The validated benchmark bundle at `/private/tmp/light-cljs-178-benchmarks`
contains 423 compiler resident timing samples (nine samples, two iterations),
423 allocation samples, 180 fresh-process samples and 1,278 runtime operation
samples, each checked against its fixture. Measurements ran alongside verification
and establish fixture validity rather than a controlled performance comparison.
The regenerated inventory changes only the analyzer source binding. The current
static ledger at `/private/tmp/light-cljs-178-static-ledger.json` still reports
`strict_complete: false`; no original body, arity or full-language credit is
inferred from this focused cohort. Cancellation and singular/plural limit suites
were excluded; externally blocked 156 was untouched.
