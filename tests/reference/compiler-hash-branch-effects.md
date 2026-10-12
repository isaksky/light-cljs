# Public hash-map routing effects

Hash-map insertion now routes keys through immutable bitmap, array and full-hash
collision nodes alongside the existing entry storage. Splitting a leaf tests the
incoming key and rehashes the retained key; deeper shared branches repeat those
operations at the actual split depths. Bitmap expansion rehashes retained leaf
keys in branch order. Transient spare capacity governs its expansion timing.
Removal preserves child nodes and full-collision ordering, so subsequent
insertion retains the same public effects. Nil slots stay separate. Array-map
and set promotion insert the retained entries through the same routing path.

The complete original remains unchanged in
`tests/fixtures/compiler-watch-key-collision-gap.tsv` (SHA-256
`2635e669c7e1a880ad60113c2f7813a055418370d394a4095e69da33228cbc4a`):

```clojure
(let [events (atom []) a (atom 1) k (reify IHash (-hash [this] (swap! events conj :hash) 7) IEquiv (-equiv [this other] (swap! events conj :equiv) false))] (add-watch a k (fn [& _])) (doseq [i (range 8)] (add-watch a i (fn [& _]))) [@events (do (reset! events []) (remove-watch a k) (reset! a 2)) @events])
```

Both independent pins, release **1.12.134** and checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, and current local frames return
`[[:hash :hash] 2 [:hash]]`. The reference settings remain
`optimizations:none/static-fns:false/def-emits-var:false`. No private node
identity or fixed arbitrary hash count is an acceptance condition.

Sixty-six complete sources retain literal pin-checked expectations in
`tests/fixtures/compiler-hash-branch-effects.tsv`. They cover persistent,
transient and constructor insertion; one through five shared branch levels;
full collisions; sizes 8/9/16/17/20/32/40; array-map and set promotion;
matching representatives, callback/Atom identity, metadata, old versions and
nil slots; throwing incoming/retained hash and equality; removal/reinsertion;
array-node packing; and persistent versus transient collision value equality.
Both pins execute all 66 sources. Current local verification executes **528
application/macro, fresh/reused, QuickJS/Node observations**, plus **10 recovery
observations** after uncaught failures. Macro computations are demanded by
actual expansion; they return finite strings rather than replacing their
intermediate receivers or operations.

Raw current pin data is `/tmp/light-cljs-170-pins-current/evidence.json`;
local sources, emitted units, engine observations, failures and recovery units
are `/tmp/light-cljs-170-phases-current`. The current receipt is
[`compiler-hash-branch-effects-receipt.json`](compiler-hash-branch-effects-receipt.json).
Historical169 sources, expectations and receipts are unchanged. Initial170
oracle calibration runs retained `PENDING` expectations and consequently
failed; they are historical probe evidence, never passing receipts. The
performance attempt `/tmp/light-cljs-170-performance` detected source changes
and rejected its report; it receives no passing credit.

A distinct public `find` defect repeats key hashing/equality three times.
Its exact complete source and literal canonical expectation are retained in
`tests/fixtures/compiler-find-key-effects-gap.tsv`; both pins agree and the
local discrepancy is retained at `/tmp/light-cljs-170-local-probe.log` and
`/tmp/light-cljs-170-phases-probe/representative-persistent-application-fresh`.
After empty live duplicate searches, **light-cljs-171, Preserve canonical
single-search effects for public find**, was created immediately before125.
It receives zero passing credit. Representative insertion is independently
verified through keys/get in this task. Strict language completion remains
unestablished; closure170 does not satisfy125.

Safe regressions cover unchanged watch-key controls, complete upstream equality
and transient bodies, hash/metadata contracts, protocols, public constructors,
deferred operations, and the state application/macro gate. Direct Node runtime
and core-helper checks, focused Rust formatting and whitespace checks pass.
Two broader ignored reference tests are not selected; the focused driver
independently runs both pins. No cancellation or singular/plural limit test is
selected; externally blocked156 is neither retried nor investigated.

Normal validated performance artifacts are
`/tmp/light-cljs-170-performance-current`, using nine samples, two iterations,
separate allocation binaries, complete fresh-process output checks and the
unchanged starting HEAD `d27d8884fd3d304c80e1ee4cb1cc4a4c9c7b3ab9` primitive
baseline. This is a short smoke measurement with uncontrolled scheduling and
an unpaired baseline; no improvement claim is made.

```sh
cargo test --locked --test compiler_hash_branch_effects --test compiler_watch_keys
python3 -B tests/reference/compiler-hash-branch-effects.py --output /tmp/hash-branch-pins-fresh
python3 -B benchmarks/run_compiler.py --output /tmp/hash-branch-performance-fresh --samples 9 --iterations 2 --baseline-ref d27d8884fd3d304c80e1ee4cb1cc4a4c9c7b3ab9
```
