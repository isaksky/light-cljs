# Public find performs one search

Ordinary persistent maps now return the retained key and value from one lookup
for public `find` and the `IFind/-find` fallback. Hash-map queries call custom
hash/equality only along that search; array maps compare keys once. Sorted maps
traverse their tree once and return the retained node. Direct custom `IFind`
methods still receive their actual receiver. Records and other custom
associative receivers keep the canonical contains/get fallback.

The original complete source and literal effect expectation remain unchanged in
`tests/fixtures/compiler-find-key-effects-gap.tsv`, SHA-256
`30734357a5779b0b39364e1842413ba9ced86e328c3daffe2db02e86c07f61b8`.
Historical170 reports and receipts are preserved. Their failed find observation
receives no retroactive passing credit.

Nineteen complete sources with literal expectations are retained in
`tests/fixtures/compiler-find-effects.tsv`. Independently identity-checked
release **1.12.134** and checkout **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**
execute all 19 sources with unchanged options
`optimizations:none/static-fns:false/def-emits-var:false`. The controls include
hits/misses through both APIs, hash collisions, nil/empty/absent lookups,
retained key and Atom/function value identity, metadata and old versions,
direct custom dispatch, associative and record fallback, vectors/subvectors,
MapEntry and sorted-node identity, and caught throwing hash/equality recovery.

Current local verification executes **152 QuickJS/Node observations** in
application and genuinely demanded macro computations, each in fresh and reused
MacroHosts. The macro wrapper returns a finite printed string and retains every
original computation. Four uncaught macro failures leave the actual fresh or
reused MacroHost usable. Eight uncaught application failures are observed in
QuickJS and Node, with four recoveries in the same QuickJS context and eight
QuickJS/Node executions of the host recovery expansion. Node recovery uses a
fresh process after the failed process exits.

Raw pins are `/tmp/light-cljs-171-pins-bound/evidence.json`; local complete
sources, emitted units, observations, failures and recoveries are retained at
`/tmp/light-cljs-171-phases-final`. The current source-bound record is
[`compiler-find-effects-receipt.json`](compiler-find-effects-receipt.json).
Earlier171 calibration runs found unsupported key/val-on-nil probes; those
failed runs remain in `/tmp` and receive no passing credit. The final miss
controls explicitly guard key/val extraction while preserving the lookup.

Safe regressions exercise watch-key and immutable branch effects, complete
upstream equality/transient/sorted bodies, hash/metadata contracts, protocols,
public constructors, deferred helpers, and state persistence across namespaces.
Direct Node runtime/core-helper assertions, focused Rust formatting and
whitespace checks pass. Ignored broader pin tests are not selected; this focused
driver independently checks both pins. No cancellation or singular/plural limit
test is selected. Externally blocked156 is not retried or investigated.

Normal validated performance artifacts are `/tmp/light-cljs-171-performance`,
with nine samples, two iterations, separate allocation binaries and complete
fresh-process output checks. The unchanged primitive baseline is starting HEAD
`ebc92268da2fa812825720d1d713f68bb6910def`. Scheduling and OS caches are
uncontrolled; this short smoke measurement supports no improvement claim.

Closure171 repairs this concrete strict-completion125 blocker. It does not
establish the broader zero-missing language acceptance required by125.

```sh
cargo test --locked --test compiler_find_effects
python3 -B tests/reference/compiler-find-effects.py --output /tmp/find-pins-fresh
python3 -B benchmarks/run_compiler.py --output /tmp/find-performance-fresh --samples 9 --iterations 2 --baseline-ref ebc92268da2fa812825720d1d713f68bb6910def
```
