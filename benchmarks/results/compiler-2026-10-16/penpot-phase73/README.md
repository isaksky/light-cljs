# Phase73: CPU profile after namespace and root registration changes

Diagnostic V8 run based on phase71, with the same inspector instrumentation
used in phase60. It takes **23.00 s / 1293156352 peak RSS bytes**. Complete
output matches phase42, including the known main ESM syntax defect.

In a 22.37-second sampled interval, notable self time is bridge `visit` 2.446 s,
GC 1.263 s, macro argument preparation 0.982 s, protocol `publicMethod` 0.842 s,
and `decodeNode` 0.755 s. Native execution is partly represented as `(idle)`
3.233 s and `(program)` 1.623 s. These are not proof of actual idle CPU time.

Inclusive macro reader-context execution takes 7.548 s; encode 3.058 s and
decode 2.461 s. Inclusive categories overlap and must not be summed. Deferred
`forceSlot` self time is down to 0.582 s from 1.904 s in phase60; it includes
the Rust callback and is not just JavaScript lookup.

`main.cpuprofile`, `summary.edn`, scripts, timing, hashes and the instrumentation
patch are preserved. `line-ticks.bb` maps profiler positions to the generated
runtime. This profile does not replace a clean benchmark or full validation.
The prototype is still unpromoted; production remains phase52.
