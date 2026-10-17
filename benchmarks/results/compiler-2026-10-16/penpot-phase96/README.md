# Phase96: shared V8 interrupt scheduler

The isolated V8 candidate replaces one polling thread per evaluator with one
process scheduler. Idle sessions sleep without consuming poll fuel. The registry
holds weak tasks, releases its lock before calling V8, and checks again before
sleeping to avoid a lost wake. An RAII activity guard handles panic unwinding.

Normal-concurrency validation passes: **1,412 tests, zero failures, 106 ignored**,
strict Clippy, formatting and all three Node workflows. New tests cover concurrent
deadlines, idle fuel preservation and activity release after a caught host panic.

| Cold build | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: |
| render | 3.40 | 524910592 |
| main none | 21.05 | 1139851264 |
| main advanced | 26.41 | 1222787072 |

All complete outputs match phase42. Explicit ESM checking passes render and
reproduces the known main syntax defect. The 996-file port is unchanged.
This is runtime hardening, with no measured speed gain over phase85.

`changes.patch` is relative to phase85. The candidate remains isolated: production
is still phase87 QuickJS. Stack limits are handled in phase97; startup monitoring,
backing-store limits and packaging remain adoption work. Scheduler thread creation
still uses `expect`, so creation-failure handling also remains to be hardened.
