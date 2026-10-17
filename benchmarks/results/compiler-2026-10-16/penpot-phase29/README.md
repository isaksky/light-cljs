# Search runtime parameter names without copying the string table

Every deferred unit used to convert all strings in its inherited table to
UTF-8 and concatenate them before choosing a private runtime parameter
name. Phase28 measured 5.165 seconds in this scan across 441 emissions.
The replacement searches the same concatenated UTF-16 stream with a
prefix matcher. It allocates only a small fallback table for the ASCII
candidate, preserving the old naming decision even across string boundaries
and unpaired surrogates. No analysis or macro behavior changes.

A new test compares every split of a name-containing UTF-16 table with
the old UTF-8 projection, including overlapping prefixes and lossy code
units. The existing runtime-parameter JavaScript hygiene test also passes.
The full suite passes **1,393 tests, zero failures, 106 ignored**. Strict
Clippy, formatting, diff checking and all three Node workflows pass.

The isolated prototype measured render at 14.99 seconds / 536,002,560
bytes RSS and main at 120.05 seconds / 1,359,462,400 bytes. Both complete
outputs were byte-identical to phase27. Those preliminary logs are
retained separately from the final clean results below.

## Clean rerun

Fresh processes, cold shadow caches, sequential jobs, no overlapping
CPU-heavy tests, builds or profiles.

| Job | Wall seconds | Peak RSS bytes | Output bytes |
| --- | ---: | ---: | ---: |
| render | 15.41 | 525369344 | 12620640 |
| main-none | 122.07 | 1272545280 | 33586654 |
| main-advanced | 127.33 | 1295908864 | 25665457 |
| shadow-release | 55.2 | 4574396416 | — |
| shadow-compile | 30.2 | 2430353408 | — |

Advanced remains **2.31 times slower** than shadow release at **28% of its peak RSS**; none is **4.04 times slower** than shadow dev at **52% of its RSS**. The requested speed goal is still unmet.

All three complete outputs match phase27 byte for byte and pass Node
syntax checking. The 996-file port fingerprint is unchanged. Shadow main
includes render, rasterizer and code-highlight; light-cljs main includes
its five lazy modules, with render measured separately. Bundling and
running Penpot in a browser remain untested.

The executable, tracked patch, untracked name-index source and emitted
JavaScript are retained in `/private/tmp/light-cljs-phase29/production/`.
Source and port fingerprints accompany these results.

## Canonical snapshot audit

Snapshot removal needs more than checking for Date or explicit `#js` literals:
imported symbols expose mutable name and metadata fields. The separate
`symbol-snapshot-probe.bb` compiles a macro that changes its argument's name
from `original` to `changed` and returns it. Its caller binds those names to 1
and 2 respectively; the emitted program returns 2. Unconditionally recovering
the original compiler datum would incorrectly return 1. Snapshot behavior is
unchanged by this phase.
