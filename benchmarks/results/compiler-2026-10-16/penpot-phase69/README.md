# Batched cached namespace execution — isolated V8 candidate

Based on phase68. Compiled namespace functions are placed in an engine array and
executed in one service call. All registrations still run in order; the array
roots functions across reentry/eviction. This removes a native service crossing
for each namespace without caching namespace state.

The diagnostic main-none run takes **24.59 s / 1,176,551,424 bytes peak RSS**,
including terminal GC/statistics. This is not a convincing speed difference from
phase68's 24.62 s. Cache counters are identical, and complete output matches
phase42. No standalone tests/full suite were run for phase69; combined validation
continues in phase70. Production and V8's known adoption gaps are unchanged.

Incremental patch, exact commands, timing/counters and hashes are archived.
Frozen binary and complete output remain under `/private/tmp/light-cljs-phase69`.
