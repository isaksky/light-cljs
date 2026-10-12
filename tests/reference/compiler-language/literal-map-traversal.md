# Canonical map source traversal and operand effects

Pellet 179 preserves this exact original source (SHA-256
`253f3e88e82fbcb2c6d7378281fbf75bcb1143a3def580e3476824c568101433`):

```clojure
(let [events (atom []) m {:a (do (swap! events conj :a) 1) :b (do (swap! events conj :b) 2) :c (do (swap! events conj :c) 3) :d (do (swap! events conj :d) 4) :e (do (swap! events conj :e) 5) :f (do (swap! events conj :f) 6) :g (do (swap! events conj :g) 7) :h (do (swap! events conj :h) 8) :i (do (swap! events conj :i) 9)}] [(count m) @events])
```

Both independently executed ClojureScript pins, release `1.12.134` and clean
checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, return
`[9 [:e :g :c :h :b :d :f :i :a]]`, with trace `[]` and stdout `""`.
Settings remain `:optimizations :none`, `:static-fns false`, and
`:def-emits-var false`. Independent reference packets precede local comparison.

The [39-source catalog](literal-map-traversal-contracts.json) retains complete
sources and typed outcomes. It exercises eight, nine and sixteen entries;
side-effecting keys and values; numeric, character, string, keyword, symbol,
nil, boolean, signed zero and nonfinite data; same-hash collisions; nested
list/vector/map/set keys and values; quoted and executable metadata; dynamic
duplicates; count, lookup, membership, complete sequence, events and captured
output. Both pins agree on all 39 complete packets. An initial reader-equal
vector/list key probe remains an exact rejected derivation control with both
raw diagnostic packets, hashes and zero acceptance credit.

Array maps preserve insertion order through eight entries and evaluate each key
immediately before its value. Larger source maps traverse the JVM reader's
five-bit hash partitions, with nil first and stable equal-hash collisions.
Their analysis and emitted execution evaluate **all reordered keys, then all
reordered values**. Compiler-only hashes precede Number/character lowering,
including nested reader data. Metadata keeps the source traversal contract.
Macro-computed maps retain their established data order. The emitter's internal
`mapFromArrays` helper invokes the ordinary hash-map constructor; public runtime
constructor identity and Number semantics are preserved.

The unchanged sixteen-entry identity macro regression now expects effects
`14,16,12,30,2,20,18,24,10,26,0,22,8,6,28,4`, then
`15,17,13,31,3,21,19,25,11,27,1,23,9,7,29,5`, followed by
`16 true true`. Its exact original macro definitions, complete generated
application source and historical insertion-order expectation remain in the
[historical receipt](literal-map-traversal-historical-macro-receipt.json).
An independently compiled whole-source witness retains the same literal
operands and records their effects. No original SCI body or expectation changes.

The [proof harness](literal_map_traversal.py) uses actual fresh/reused application
and demanded macro computations and executes each emitted program in QuickJS
and Node. The [raw receipt](literal-map-traversal-raw-guard-receipt.json) points to
`/private/tmp/light-cljs-179-independent/final-proof39/evidence.json`, SHA-256
`6cd0879bc1eb9461896c9b0440ad6c830f7254d303b7670bc13477cf30053e6a`.
The [durable evidence](literal-map-traversal-receipt.json) is byte-identical.
All 312 actual frames are replayed: 24 accepted sources / 192 canonical
agreements, and 15 complete nonpassing sources / 120 zero-credit frames.
Collection tags, every sequence position, trace and stdout remain exact;
only the reference packet's harness ID is replaced. No sequence sorting or
collection-tag projection supplies acceptance.

Fourteen full-source map witnesses expose a separate runtime hash-map sequence
root. The source-only reader effect order, count, lookups, metadata and events
agree, while complete sequence ordering differs. The runtime-only source
`(vec (seq (hash-map :a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9)))`
proves it without map literals: both pins iterate keys `:e :g :c :h :b :d :f :i :a`,
and all eight local frames retain `:a :b :c :d :e :f :g :h :i`.
All-status duplicate searches and the complete active queue found no focused
owner; Pellet **181, Preserve canonical hash-trie traversal for runtime maps**,
is immediately before strict gate 125. The retained runtime-set control remains
owned by 180. All fifteen original complete witnesses remain zero-credit.
Two separate complete effects-only sources establish nine/sixteen-entry key and
value effects while granting no sequence credit; their full-sequence neighbors
remain unchanged and nonpassing.

Raw replay rejected ten restored mutations: source, emitted program, QuickJS
packet, Node input/output and audit command after artifact recensus, plus engine
environment, executable hash, detached summary credit and a missing artifact.
Pristine replay passed after restoration. Saved receipts never substitute for
current executable/raw evidence.

The [follow-through receipt](literal-map-traversal-followthrough-receipt.json)
records fresh unchanged numeric, character/string and set catalogs against both
pins: 736 actual phase/engine frames and 296 same-host diagnostic recovery
outcomes. Existing committed receipts remain byte-exact historical evidence;
all retained pin and diagnostic differences and runtime-set gaps remain explicit.
The new permanent map regression compares independently frozen complete accepted
packets in both phases and verifies malformed/duplicate same-host recovery.
Together with surrounding suites, 185 ordinary Rust tests pass. There are 19
literal/catalog unit guards, two executable raw guards, 18 inventory guards and
22 full-ledger guards. Cancellation and singular/plural limit tests are excluded;
externally blocked 156 remains untouched.

The final validated benchmark bundle is
`/private/tmp/light-cljs-179-benchmarks-final`: nine samples and two iterations,
with compiler resident timing/allocation, fresh-process and runtime operation
fixtures checked against their expected results. Concurrent verification and VM
scheduling prevent a controlled performance comparison. Inventory regeneration
changes only the analyzer source binding; original bodies/obligations stay
unchanged. The fresh static ledger at
`/private/tmp/light-cljs-179-final-static-ledger.json` remains
`strict_complete: false`. Focused controls grant zero original SCI/upstream-body,
public-arity or full-language completion credit.
