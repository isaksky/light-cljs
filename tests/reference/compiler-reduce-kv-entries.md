# MapEntry reduce-kv fallback

`reduce-kv` now treats a MapEntry through the public sequence/IMapEntry fallback.
MapEntry retains its vector APIs and IMapEntry identity, but does not advertise
IKVReduce. Ordinary vectors keep ordinal keys, and explicit custom/native
IKVReduce dispatch still takes precedence.

The complete unchanged original is:

```clojure
(reduce-kv (fn* [a k v] (conj a [k v])) [] (find {(find {:a 1} :a) (find {:b 2} :b)} (find {:a 1} :a)))
```

It returns `[[:a 1] [:b 2]]`, with empty original effects and stdout/stderr, in
both required canonical pins and all current application/macro contexts. Source
SHA-256 is `dc98b9aef1ab00352258d9247014772b60c45f9cb240485313a0bffc18c84f37`;
original row SHA-256 is
`1b978471ffb1985b01ba88000ddb83bfb9b70e7db25e1e4d3300a94e1d9710d0`.
Original corpus and historical nonpassing disposition bytes remain unchanged.

Both independently checked pins, release **1.12.134** and checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, pass all 15 original/control
observations at `optimizations:none/static-fns:false/def-emits-var:false`.
A plain nonnested MapEntry canonically throws a missing-protocol **Error** before
calling the reducing function. Its historical SCI execution/type category is
retained; the fixture asserts the independently observed canonical category and
empty effects. No private entry layout is an acceptance condition.

Thirteen focused sources execute through actual fresh/reused MacroHosts in
application and demanded macro computations, then run on QuickJS and Node:
**104 observations**, plus **two recovery observations** after an uncaught macro
entry failure. Controls cover ordinal vector reduction, ordered map/entry
callbacks, reduced early stopping and nested markers, public metadata/entry
APIs, custom/native IKVReduce precedence, custom ISeqable/IMapEntry fallback, and
empty collections. Raw source, emitted units, warmup, failures and engine output
remain under `/tmp/light-cljs-167-phases-final`. Checked pin artifacts are under
`/tmp/light-cljs-167-reference-final`.

The retained focused receipt is
[`compiler-reduce-kv-entries-receipt.json`](compiler-reduce-kv-entries-receipt.json),
SHA-256 `5df060b46ed9b4422ced16054a23189a0c66edd76efd7964f3fea056d1f1f66d`.
A newly executed complete 30-source canonical receipt is at
`/tmp/light-cljs-167-canonical-current/evidence.json`, SHA-256
`ead8685d31a8a3875f83c70fc1fc03297da2bad68e89393be00f3ebcaff7538f`.
Actual `full_ledger.expand_report` integration now reports **one covered original,
27 verified canonical differences, and two missing contracts**. All eight
original nested-entry frames pass. The mapv and watch-key gaps remain owned by
168 and 169. Strict comprehensive completion remains **false**; neither this
repair nor Pellet closure establishes 125 acceptance. Earlier receipts retain
their historical observations and require fresh production before supplying
current credit.

Thirty-seven affected compiler regression tests, 15 canonical accounting guards,
direct Node runtime/helper checks, exact source inventory freshness, focused
Rust formatting and whitespace checks passed. The affected sequence-contract
suite inadvertently ran one cancellation-named test, which passed and was not
repeated. No 156 investigation or repair was performed; no limit suite ran.

The normal validated compiler performance driver retained nine samples with two
iterations, separate allocations, fresh-process output checks and immutable
primitive baseline `480050e`: **423 resident timing samples across 47 phase
pairs** and **1,278 operation samples**. Collection total compilation median was
0.047729 ms, with fresh/reused macro analysis medians 24.522459/23.963354 ms.
Artifacts are at `/tmp/light-cljs-167-performance`. This is a validated smoke
measurement with an unpaired baseline and short batches; no improvement claim
is made.

```sh
cargo test --locked --test compiler_reduce_kv_entries
python3 -B tests/reference/compiler-reduce-kv-entries.py --output /tmp/reduce-kv-pins-fresh
python3 -B tests/reference/compiler-language/sci_gap_canonical_context.py --output /tmp/reduce-kv-canonical-fresh
```
