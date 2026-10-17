# Combined heap/backing-store budget — validated, still isolated

The phase109 backing-store quota left heap storage and backing storage with
separate limits. The bounded negative control demonstrates both allocation
orders retaining about 22.15 MB of JavaScript heap plus 20.97 MB of buffers under
a 32 MiB session limit.

The isolated backend now combines those storage categories. GC epilogues refresh
the heap observation used by the allocator's retry path. Operations with backing
storage check the combined usage before returning; active JavaScript also checks
at most once per 5 ms through the existing interrupt scheduler. An over-budget
snapshot triggers collection before terminal failure, so unreachable storage
does not cause a false refusal. Sessions without backing storage retain the
existing near-heap-limit path, avoiding heap-statistics queries on the normal
macro-transport hot path. This is cooperative storage accounting, not an exact
process-RSS ceiling or an accounting of all engine/native overhead.

Fourteen focused buffer tests pass, including five new tests for both allocation
orders, valid mixed storage without double-counting, startup, a retained heap
followed by an infinite loop, and GC refresh before retry. Full validation passes
**1,440 tests, zero failures, 106 ignored** at normal concurrency, strict Clippy,
formatting and all three Node workflows. The native engine/archive is unchanged
from phase109; only the Rust host and regression tests change.

Sequential cold runs:

| Build | Wall | Peak RSS bytes |
| --- | ---: | ---: |
| render | 3.86 s | 481,804,288 |
| main none | 23.52 s | 989,708,288 |
| main advanced | 29.68 s | 1,161,953,280 |
| immediate frozen phase109 main | 23.46 s | 983,367,680 |

All complete outputs match phase109 byte-for-byte and pass explicit ESM syntax
checks. The 996-file port fingerprint is unchanged. Penpot has not been bundled
and run in a browser. The immediate comparison shows no material time or memory
change from aggregate accounting. Phase109 retains the latest fresh Shadow runs:
33.08 s / 2,197,323,776 bytes for dev and 60.08 s / 4,129,030,144 bytes for release.

A separate main run without the experimental 256 MiB heap threshold takes
**23.82 s / 1,294,237,696 bytes** and produces the identical output. Native
integration therefore needs a sensible ordinary collection threshold, while
still allowing live heaps to use the configured session budget. Backend
packaging remains incomplete; production remains phase101.

The patches, negative control, full validation, precise environments, frozen
engine hashes, output hashes and port fingerprint accompany this file.
