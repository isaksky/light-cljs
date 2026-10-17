# Phase 103: earlier V8 JIT compilation — rejected

The frozen, fully validated phase102 binary with six invocation thresholds
lowered tenfold for a cold-build trial: Maglev 400 → 40, TurboFan 3,000 → 300,
post-inline-cache-update delay 500 → 50, delayed Maglev 600 → 60, OSR 500 → 50
and Maglev OSR 100 → 10. These flags and defaults were checked against the
installed, pinned V8 15.2 source. No compiler or runtime source was changed.

| Main none | Wall seconds | User seconds | Peak RSS bytes |
| --- | ---: | ---: | ---: |
| Earlier JIT tiers | 23.71 | 32.09 | 985,366,528 |
| Default tiers, immediate rerun | 23.96 | 28.23 | 948,649,984 |

The 1.0% wall-time difference is not a meaningful gain. Earlier compilation
uses 13.7% more user CPU time and 3.9% more peak RSS. The setting is rejected.
Both complete outputs are byte-identical to phase102's corrected ESM output.
No full suite was run with these rejected flags; phase102 preserves the full
validation of the unchanged binary under its default flags. V8 remains isolated.

The two jobs ran sequentially, with the same 256 MiB V8 heap setting, source
roots and macro budgets. Exact flags, commands, timings and hashes are archived.
The complete outputs remain under `/private/tmp/light-cljs-phase103/`.
