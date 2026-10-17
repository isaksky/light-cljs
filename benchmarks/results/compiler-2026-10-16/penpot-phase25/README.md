# Fixed protocol metadata and compiler declaration frontiers

This phase combines three changes:

- Public protocol dispatch caches the private protocol identity, mask, marker
  name, native predicate and arity-slot names. Receiver membership and methods
  remain live reads. Declaration demand, external dispatch, omitted-method
  errors and the native check after a method getter are preserved.
- Declaration frontiers index successfully initialized extension bundles in
  source order. Ordinary root bundles no longer participate in frontier scans.
  Replay advances by declaration order even when a Proxy patch initializes
  another bundle. Every changed frontier still invalidates demand results.
- Frontier entry/exit uses an evaluator service instead of evaluating a new
  JavaScript source string and allocating/releasing a result handle. It keeps
  the normal control boundary, original exception bookkeeping, dynamic lookup
  and active-context reentry. External MacroEvaluator implementations retain
  the previous behavior through a default trait method.

Five new tests cover nested callbacks, error preservation and cancellation,
out-of-order extension initialization, and initialization from a Proxy patch
during replay, plus inherited numeric properties in the arity cache. The Proxy
probe produces `[true,3,1,2,3]` with both the old scan
and the final index; a preliminary array-index loop produces
`[false,3,1,2,3]`. That preliminary implementation was not retained.

The arity cache uses a null-prototype object. An array would inherit numeric
properties from Array.prototype: the new probe returns 30 with the old dispatch
and the final cache, but throws a missing-method error with the preliminary
array cache. The full suite and clean benchmarks were rerun after this fix.

The full suite passes **1,389 tests, zero failures, 106 ignored**. Strict
Clippy, formatting, diff checking and all three direct Node workflows pass.
Runtime manifest headers now include the direct external-dispatch hook use.
The full runtime grows 1,051 bytes; all 34 none/prepare fixture ceilings move
to the next 1,000 bytes. Measured advanced fixture sizes and ceilings are
unchanged. Retain this size/performance rationale in the eventual commit.

## Diagnostic evidence

The isolated interrupt-sampled main run falls from phase22's 155.48 seconds to
148.13 seconds with protocol metadata caching (phase23), then 141.83 seconds
with the preliminary frontier index (phase24). Render is 19.86 and 18.90
seconds respectively. Both diagnostic application bodies match phase20 outside
the runtime, and complete outputs pass `node --check`.

These are diagnostic runs, not the final clean benchmark. Interrupt samples
count poll occurrences, not wall-clock shares. Phase24 precedes the Proxy
reentry correction and direct evaluator service. Phase23's preliminary
manifest annotations also precede the final slicing corrections; neither
diagnostic candidate was accepted without the full validation above.

## Clean rerun

Fresh processes, cold shadow caches, sequential jobs, and no overlapping
CPU-heavy builds, tests or profiles.

| Job | Wall seconds | Peak RSS bytes | Output bytes |
| --- | ---: | ---: | ---: |
| render | 19.91 | 545423360 | 12620640 |
| main-none | 147.56 | 1307590656 | 33586654 |
| main-advanced | 151.64 | 1431175168 | 25665457 |
| shadow-release | 56.6 | 4075552768 | — |
| shadow-compile | 33.53 | 2217279488 | — |

Main none improves 7.2% from phase22's 159.03 seconds; advanced improves
6.9% from 162.93 seconds. RSS varies between runs. Advanced remains **2.68 times slower** than shadow release at **35% of its peak RSS**; none is **4.40 times slower** than shadow dev at **59% of its RSS**. The requested speed goal is still unmet.

All three application bodies match phase6b byte for byte outside the
embedded runtime, all complete outputs pass Node syntax checking, and the
996-file port fingerprint is unchanged. `preliminary-results.edn` retains the
earlier run before arity-cache hardening; the table above is the final build.

The scope remains slightly different: shadow main includes render, rasterizer
and code-highlight; light-cljs main includes its five lazy modules, with render
measured separately. Bundling and running Penpot in a browser remain untested.

The executable, complete tracked patch, untracked name-index source and emitted
JavaScript are retained in `/private/tmp/light-cljs-phase25/production/`.
The final source and port fingerprints accompany the archived results.
