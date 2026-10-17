# V8 JavaScript CPU profile

Isolated instrumentation of phase58. An in-process V8 Inspector session records
1 ms CPU samples from startup through teardown. This is a diagnostic run, not a
new production benchmark or validated backend: **26.29 s / 1,377,533,952 bytes
peak RSS**, with complete output byte-identical to phase42.

`main.cpuprofile` contains the original profile, consumable by DevTools.
`summary.edn` aggregates sampled time by source/function identity. The largest
self-time categories are idle (3.10 s), bridge `visit` (2.28 s), `forceSlot`
(1.90 s), evaluator `evaluate` (1.85 s), program (1.63 s), GC (1.43 s),
`macroArguments` (1.14 s), `publicMethod` (0.82 s) and `decodeNode` (0.78 s).
Inclusive categories overlap and must not be summed. Samples attributed to a
JavaScript function may include native work called by that function: nearly all
`forceSlot` position ticks are on its Rust `__lightCompileUnit` callback.

`line-ticks.bb` maps V8's generated runtime line numbers to the exact source.
Bridge cycle checking, weak provenance updates and protocol dispatch are visible
costs. This motivates independent replay and active-ancestor-stack trials; it
does not establish their effectiveness. Instrumentation is not promoted, and
the existing V8 resource-limit gaps and baseline main ESM failure remain.

The patch is relative to phase58. Commands, timings and hashes are retained;
the frozen executable and complete JS output remain in `/private/tmp/light-cljs-phase60`.
