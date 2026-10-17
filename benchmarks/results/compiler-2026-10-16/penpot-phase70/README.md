# Phase70: unit hygiene and combined cache validation

Isolated V8 candidate; not a production backend change. Based on phase69.

The shared-runtime parameter now participates in the existing raw-JavaScript
hygiene pass. This removes a scan through the entire accumulated UTF-16 string
table on every deferred unit. The regression checks collisions with both the
old parameter and the new parameter and hygiene prefix.

Combined namespace-cache, batched-registration and hygiene validation:
**1,406 passed, 0 failed, 106 ignored**, four test threads; strict Clippy,
formatting and all three standalone Node workflows pass. The removed internal
scan test accounts for one fewer test; the cache adds three semantic tests.

Clean, sequential cold builds with a 256 MiB V8 heap, without statistics or
forced terminal GC:

| Build | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: |
| render | 3.69 | 567607296 |
| main none | 22.59 | 1232191488 |
| main advanced | 28.13 | 1323499520 |

The separate diagnostic run includes statistics and terminal GC: 22.89 s,
1186889728 peak RSS bytes. Its 131,239 namespace cache hits and 1,292 misses
compile 1,050,924 bytes of namespace registration. Do not compare its RSS as
though it were the same measurement configuration as the clean runs.

A 128 MiB heap now succeeds: 22.92 s, 1202274304 peak RSS bytes. This is only a
small process-memory reduction; earlier uncached 128 MiB trials failed.

Every complete output matches phase42 byte-for-byte, including the known main
ESM syntax defect. No browser execution is claimed. V8 resource-limit,
watchdog and packaging gaps remain; production remains QuickJS phase52.

`changes.patch` is relative to phase69. Scripts, full validation logs, exact
commands, timing files and hashes are preserved here. Frozen binaries and
large outputs remain under `/private/tmp/light-cljs-phase70/`.
