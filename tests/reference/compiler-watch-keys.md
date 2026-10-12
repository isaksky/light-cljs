# Atom watch-key demand and equality effects

Removing an absent lazy watch key now preserves the canonical nil-key watch
without hashing or realizing that candidate. Atom's public IWatchable dispatch
continues to delegate to ordinary map operations. Small-map lookup compares on
the query side without hashing; nil/string/number keys use canonical specialized
search. Array-map insertion defers key hashes until promotion, and matching
removal retains canonical search/copy equality effects. Hash-map nil slots and
empty non-nil roots avoid unnecessary candidate hashing. Required large-table
hashing remains observable.

The complete unchanged original is:

```clojure
(let [a (atom 1) k (lazy-seq (observe :key) (list :key))] (add-watch a nil (fn [& _] (observe :nil-watch))) (remove-watch a k) (reset! a 2))
```

Release **1.12.134**, checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, and all current local
application/macro frames return `2` with trace `[:nil-watch]` and empty stdout.
Both references were independently executed with
`optimizations:none/static-fns:false/def-emits-var:false`. Original source
SHA-256 is `12c9352edf5e727173dbae03d2b88ac500f94b8e6b673dc121c96bfb46bbf3e6`;
original row SHA-256 is
`b16a81daad3dd51d213dbe579117cfe2fc58d382efa2b0d8576d5f8abf5dcadd`.
The SCI corpus and historical nonpassing disposition bytes remain unchanged.

Twenty focused sources execute through fresh/reused MacroHosts, as applications
and demanded macro computations, on Node and QuickJS: **160 observations** plus
**two recovery observations** after an uncaught required key-comparison failure.
Controls cover absent keys beside nil/scalar/sequential keys, matching lazy and
equivalent keys, retained callback keys and Atom receiver identity, metadata,
watch counts 8/9/32, reentrant mutation, ordinary map dissoc, custom IHash/IEquiv
and IWatchable dispatch, query equality direction and repeated removal effects.
Both pins pass the unchanged original and all twenty controls. Raw artifacts
are `/tmp/light-cljs-169-reference-verified` and
`/tmp/light-cljs-169-phases-verified`; the focused receipt SHA-256 is
`dceee475737899db0cafa250fcf9087c9f8f9b1b117b8f1d0ef2bf3722e5d2db`, retained in
[`compiler-watch-keys-receipt.json`](compiler-watch-keys-receipt.json).

Fresh complete canonical evidence is
`/tmp/light-cljs-169-canonical-current/evidence.json`, SHA-256
`214c3f69d78df7402c2949832eec9c43f9b435c58b85a992070c4508cd2973d8`; actual
`full_ledger.expand_report` replay is `/tmp/light-cljs-169-ledger.json`. It now
records **three covered originals, 27 explicit canonical SCI differences and
zero missing contracts in this 30-source cohort**. Strict comprehensive
completion remains **false**. This repair and closure do not establish 125
acceptance, and historical receipts provide no current positive fallback.

An additional full source exposed a separate public hash effect during
hash-map branch collision insertion. Both pins return
`[[:hash :hash] 2 [:hash]]`; current local returns `[[:hash] 2 [:hash]]` in all
eight phase/engine/lifecycle frames. The exact source and canonical expectation
remain in `tests/fixtures/compiler-watch-key-collision-gap.tsv`; retained raw
pin and local data are in the focused receipt and
`/tmp/light-cljs-169-collision-gap`. After searching the live queue for an
existing owner, **light-cljs-170, Preserve public hash effects when hash-map
branches collide** was created immediately before 125. This source receives
zero passing credit. It is a distinct insertion/rehashing obligation for
ordinary maps, transients and watch tables.

The initial promotion control expected one hash with hash value 7; both pins
proved two hashes for its colliding branch. That attempt remains nonpassing
at `/tmp/light-cljs-169-reference-final` and
`/tmp/light-cljs-169-phases-final`. The corrected passing promotion control uses
hash value 31 with distinct branches, while the complete colliding source stays
owned by 170. No historical source or expectation was replaced.

Focused/compiler regression suites passed: equality and its complete upstream
bodies, hash/metadata contracts, transient complete upstream bodies, protocols,
deferred operations, sequence Object.equiv, callable core helpers and the
13-workflow state application/macro gate. Direct Node runtime assertions,
22 canonical accounting guards, inventory freshness, focused Rust formatting
and whitespace checks passed. The two ignored broader differential tests were
not selected; the focused original/control reference driver ran both pins.
No cancellation or singular/plural limit test ran; externally blocked 156 was
not investigated, retried or repaired.

Normal validated compiler performance used nine samples with two iterations,
separate allocation binaries, fresh-process complete-output checks, and the
unchanged primitive baseline `480050e`. Artifacts are
`/tmp/light-cljs-169-performance`: **423 resident timing samples across 47 phase
pairs** and **1,278 operation samples**; collection total compilation median
was **0.047396 ms**. This is a short smoke measurement with
uncontrolled scheduling and an unpaired baseline; no improvement claim is made.

```sh
cargo test --locked --test compiler_watch_keys
python3 -B tests/reference/compiler-watch-keys.py --output /tmp/watch-keys-pins-fresh
python3 -B tests/reference/compiler-language/sci_gap_canonical_context.py --output /tmp/watch-keys-canonical-fresh
```
