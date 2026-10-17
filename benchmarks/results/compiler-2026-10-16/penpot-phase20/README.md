# Phase20 lexical-prefix reuse and bounded environment descendants

Locals descriptions now share unchanged binding rows with the preceding lexical
view, including shadowed bindings. A changed view builds its final map once.
The bridge reuses at most 128 immutable child collections in addition to its
64 complete components. References name only the previous completed import;
replacements publish after decoding and normal canonical snapshots, in encoder
order. Cached values still consume their full logical conversion budget.

An audit found that duplicate map/set entries and literal payloads can be decoded
without appearing in the final canonical snapshot. Publishing these children
would lose lexical distinctions on later reuse. The added duplicate-map-key
regression fails before the fix and passes afterward. If any final child slot
lacks a snapshot, that import clears the child cache without extra traversal.
Failure and reentry behavior retain the earlier safeguards. The bridge ABI is 4.

| Job | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: |
| render | 22.15 | 557514752 |
| main none | 163.82 | 1400553472 |
| main advanced | 167.78 | 1531445248 |
| shadow release | 54.58 | 3702325248 |
| shadow compile | 30.36 | 2260647936 |

Fresh processes, cold shadow compiler caches, one job at a time, no overlapping
builds, tests or profiles. Main none improves 9.5% from phase15's 181.02 seconds;
advanced improves 10.2% from 186.85 seconds. Observed peak RSS is essentially
unchanged from phase15. Advanced remains 3.07 times slower than fresh shadow
release at 41% of its peak RSS; none is 5.40 times slower than shadow dev at 62%
of its peak. Scope differences and JVM RSS variability still apply.

`cargo test`: 1,383 passed, zero failures, 106 ignored. Strict Clippy, formatting,
diff checks and all three direct Node workflows pass. New tests cover child
eviction with metadata order, logical node/depth budgets, and discarded-child
lexical provenance. The existing lexical-shadow regression also exercises
entering and leaving nested scopes. The first two child regressions and extended
shadow test passed against the old implementation before promotion. Fifteen
none/prepare size ceilings move to the next 1,000 bytes for the additional bridge
logic; prior ceilings were exceeded by 23–583 bytes. Advanced ceilings remain
unchanged. Preserve this size/performance rationale in the eventual commit.

All three application bodies match phase6b byte for byte outside the embedded
runtime and complete outputs pass Node syntax checking. Advanced is also
completely byte-identical to phase15. Penpot application execution remains
untested. The 996-file port hash is unchanged.

`rows-render.log`, `bridge-render.log` and `bridge-main.log` are earlier diagnostic
prototypes on the isolated phase13 profiling source, before the discarded-child
fix. They are not validated production timings. Against the phase19 diagnostic
control, main environment JS import fell from 29.94 to 18.96 seconds; binding
construction fell from 17.06 to 7.18 seconds. About 81% of described binding rows
on misses reused the preceding prefix. The clean results above include the
subsequent correctness fix. `prepare-production.bb` and `update-sizes.bb` record
one-time preparation steps, not supported standalone runners.

The production binary, exact tracked patch and untracked name-index source are
retained under `/private/tmp/light-cljs-phase20/` and fingerprinted here. All
production changes remain uncommitted.
