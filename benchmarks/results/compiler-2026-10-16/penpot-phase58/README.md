# Validated isolated V8 callback hardening

Based on phase56: phase50's native CLI allocator, range traversal, namespace
member ID snapshots and thin-LTO release profile. Production remains QuickJS.

Compiler callbacks now accept only exact, nonnegative safe-integer Number IDs;
they never invoke argument coercion hooks. Rust panics and native callback
failures retain owned diagnostics and terminate the already-poisoned session
immediately. Source-string allocation failure no longer unwraps a failed V8
allocation. Missing callback state terminates safely.

The complete suite passes **1,404 tests, zero failures, 106 ignored**, with four
test threads. New regressions cover invalid IDs including a nonterminating
coercion hook, callback panics and catch-and-spin behavior. Strict Clippy,
formatting and all three Node workflows pass. The first Clippy run exposed four
existing prototype lints; after those were addressed, a second run exposed a
scratch benchmark-fixture link. Both logs are preserved. The fixture link was
corrected; the callback tests and remaining checks then passed. The full suite
preceded the behavior-preserving lint changes and fixture-link correction.

| Cold build | Seconds | Peak RSS bytes |
|---|---:|---:|
| render | 4.15 | 581,566,464 |
| main none | 25.69 | 1,374,339,072 |
| main advanced | 30.89 | 1,420,476,416 |

All complete outputs match phase42. Explicit module syntax checking passes
render and fails both main outputs with the known baseline duplicate declaration.
No bundling or browser execution was done. Fresh shadow measurements remain
phase52: dev 30.17 s / 2.232 GB; release 55.06 s / 4.046 GB.

V8 still needs stack-limit support, bounded backing stores, startup-limit and
watchdog work, exceptional registration reentry review and packaging work before
promotion. This result is only 17% faster than shadow dev and does not meet the
dramatic wall-time goal. Earlier default-concurrency deadline failures remain
recorded in phase46; this suite used four threads explicitly.

`callback-errors.patch` applies to phase56 and includes the prototype lint
changes. Full logs, exact commands, source/binary/output hashes, timings and
explicit syntax checks are preserved. The frozen binary and complete outputs
remain under `/private/tmp/light-cljs-phase58`.
