# Time-gated JavaScript sampling and diagnostic upper bounds

Production remains phase33. An isolated QuickJS build samples the JavaScript
stack at interrupt checks at least 1 ms apart, using CLOCK_MONOTONIC. The
captured grammar replay returns the exact expected descriptor in both trials
(9344/9366 ms), with 16,429 eligible samples and zero dropped table entries.

This is time-gated cooperative sampling, not an asynchronous CPU profiler.
Native functions and long gaps between polls can be charged to the next
JavaScript endpoint; no percentage here establishes exact wall time.
Anonymous generated functions with identical source coordinates also merge.
The leading runtime endpoints are publicMethod, private brand predicates,
externalMethod and the declaration-demand callback, consistent with the
earlier native profile. The callback is not a compiler-global root read.

Separate non-instrumented replay experiments:

| Variant | First / second invocation ms |
| --- | ---: |
| Baseline | 9258 / 9263 |
| Normal first call, then demand entirely disabled | 9308 / 7918 |
| WeakMap memo of native protocol membership | 9157 / 9257 |

All encoded results match. Disabling demand is deliberately restricted to
the already initialized diagnostic replay; it is not valid for general
compilation. Its ~15% saving is an upper bound for removing this machinery
from this invocation. The memo variant shows no useful gain and its general
mutation semantics were not validated. Neither variant is retained.

The large replay, source/runtime snapshots and executables remain in the
phase31, phase38 and phase39 temporary directories referenced by the scripts.
