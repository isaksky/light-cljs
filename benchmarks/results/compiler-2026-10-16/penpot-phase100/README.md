# Phase 100: release completed unit IR after application assembly

The QuickJS production compiler now uses the prepared-source handoff validated
with V8 in phase99. Once application assembly consumes a deferred unit's analyzed
body, the session retains its generated JavaScript and immutable namespace
registration tables, and only a weak reference to its analyzed program. An active
reentrant owner can still use that program. Preparing source executes no user
code: registration, initialization and the emission-use counter remain lazy.
Prepared source remains available for compilation reentry.

This ports detached namespace registration from phase88 without its bytecode
cache. QuickJS evaluates registration through the existing active evaluator,
preserving reentrant callbacks. There are no runtime or port edits.

## Validation

The exact promoted source passes **1,403 tests, zero failures, 106 ignored** at
normal concurrency, strict Clippy, formatting and all three Node workflows.
New tests cover release of the strong program reference and prepared-versus-direct
source/registration equivalence, including namespace snapshot independence.
Production build inputs were compared against the validated candidate before
the release rebuild. `production/candidate-identity.txt` records this check.

## Measurements

One CPU-heavy job at a time, cold builds, the existing Penpot port and budgets:

| Build | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: |
| light-cljs render | 11.13 | 312,934,400 |
| light-cljs main none | 101.57 | 802,373,632 |
| light-cljs main advanced | 105.90 | 870,367,232 |
| shadow release main | 62.14 | 4,468,129,792 |
| shadow compile main | 35.44 | 2,187,657,216 |

The first candidate run took 102.86 seconds / 817,987,584 bytes. An immediate
rerun of the frozen phase87 baseline took 102.91 seconds / 892,370,944 bytes.
This pair shows an 8.3% RSS reduction with no meaningful time change. Comparing
against phase87's older 94.02-second measurement would incorrectly attribute
the common timing drift to this patch.

The production main build remains 2.87 times slower than fresh Shadow dev, at
36.7% of its RSS. The dramatic speed goal remains unmet. The isolated V8 backend
is not promoted by this change.

All three complete light-cljs outputs remain byte-identical to phase42. The
996-file port hash is unchanged. Explicit ESM syntax checking passes render and
fails both main outputs with the pre-existing duplicate `const`/native `var`
destructuring defect. This is not application execution validation.

## Evidence

`changes.patch` is relative to phase87. The production directory contains the
complete working-tree implementation patch, untracked `name_index.rs`, build log,
all five benchmark records and explicit syntax checks. The validation logs,
same-condition baseline comparison, commands and hashes are retained alongside
the scripts. Binaries and generated application outputs remain under
`/private/tmp/light-cljs-phase100/`.
