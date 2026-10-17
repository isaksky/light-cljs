# Share macro call inputs without losing provenance

Phase33 imports a macro call once and shares argument objects with
their elements in `&form`, matching upstream macro application. It
retains the decoded descriptor/value graph only across environment
import, then restores per-argument metadata and provenance in order.
Each argument still receives canonical snapshot validation. Whole-call
input conversion bounds already cover its argument subtrees.

Preparation or input release discards the graph. Snapshot failures
release newly assigned argument IDs and preserve the original error.
The bridge ABI is version 5; external evaluator adapters retain their
existing independent-import defaults.

Five new regressions cover shared identity and mutations with a reused
macro host; exact provenance for ratios, large integers, characters
and interned keywords after an intervening environment import; pending
graph cleanup on environment failure; partial snapshot failure; and
single construction of literal inputs. The provenance regression also
demonstrates why simply reusing existing values is insufficient.

The prototype completed render in 14.82 seconds / 536,330,240 bytes RSS
and main in 116.98 seconds / 1,235,632,128 bytes. Both application bodies
match phase32 outside the runtime. These are preliminary results,
separate from the clean production comparison below.

The full suite passes **1,399 tests, zero failures, 106 ignored**.
Strict Clippy, formatting, diff checking and all three Node workflows
pass. The readable runtime grows 1,610 bytes; all 34 none/prepare ceiling
pairs move to the next 1,000 bytes, while advanced ceilings stay unchanged.
Retain this correctness/performance rationale in the eventual commit.

## Clean rerun

Fresh processes, cold shadow caches, sequential jobs, no overlapping
CPU-heavy tests, builds or profiles.

| Job | Wall seconds | Peak RSS bytes | Output bytes |
| --- | ---: | ---: | ---: |
| render | 15.5 | 520044544 | 12622667 |
| main-none | 121.15 | 1172799488 | 33588681 |
| main-advanced | 125.18 | 1321762816 | 25665874 |
| shadow-release | 57.25 | 4102488064 | — |
| shadow-compile | 30.8 | 2209611776 | — |

Advanced remains **2.19 times slower** than shadow release at **32% of its peak RSS**; none is **3.93 times slower** than shadow dev at **53% of its RSS**. The requested speed goal is still unmet.

All three application bodies match phase32 byte for byte outside the
embedded runtime and pass Node syntax checking. The 996-file port
fingerprint is unchanged. Shadow main includes render, rasterizer and
code-highlight; light-cljs main includes its five lazy modules, with
render measured separately. Bundling and browser execution remain
untested. Single-run times and JVM peak RSS have ordinary variability.

The executable, tracked patch, untracked name-index source and emitted
JavaScript remain in `/private/tmp/light-cljs-phase33/production/`.
Source and port fingerprints accompany these results. The prototype
scripts start from phase32 source and use an isolated checkout.
