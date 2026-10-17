# Macro argument traversal replay — QuickJS negative result

Phase52's production-equivalent QuickJS candidate with the phase61 replay-order
change. Main none takes **94.49 s / 998,801,408 bytes peak RSS**, versus the
retained production's 93.83 s / 993,214,464 bytes. This does not improve production
and is not retained. No full suite was run for the rejected optimization.

After replacing only the expected embedded compiler form bridge function,
the complete emitted program is byte-identical to phase42. Phase61 preserves
the differential semantic checks. The known main ESM failure persists.

Exact build/benchmark commands, patch, timings, output equivalence and hashes
are archived. The executable and complete output remain under
`/private/tmp/light-cljs-phase62`.
