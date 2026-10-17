# Keep reader validation out of executable literal tables

Reader identity validation now resolves keyword/symbol names without inserting
unused entries into executable literal tables. Actual lowering uses the same
name-resolution helpers and still inserts its literals. Exact UTF-16 names,
auto-resolution and generated semantic names retain their existing paths.

A nine-key map previously retained 27 names and 27 strings; the regression
now requires nine of each. It also covers nested map/set keys, symbols and
auto-resolved keywords in source and owned compiler forms. The frozen old
library reproduces the 27/27 negative control.

The allocation evidence is in phase41. QuickJS Function.call, native
membership memoization and the tested PGO configuration were rejected in
phases38–40; they are not part of this implementation.

The full suite passes **1,400 tests, zero failures, 106 ignored**. Strict
Clippy, formatting, diff checking and all three Node workflows pass.

Fresh processes, cold shadow caches and one job at a time, after all
CPU-heavy validation/build/profile work completed:

| Job | Wall seconds | Peak RSS bytes | Output bytes |
| --- | ---: | ---: | ---: |
| render | 14.86 | 491618304 | 12622667 |
| main-none | 117.05 | 1241350144 | 33588681 |
| main-advanced | 122.76 | 1304117248 | 25665874 |
| shadow-release | 55.72 | 4617011200 | — |
| shadow-compile | 30.33 | 2219737088 | — |

None remains **3.86 times slower** than shadow dev at **56% of its peak RSS**. Advanced is **2.20 times slower** than shadow release at **28% of its RSS**. The requested speed goal remains unmet.

The previous phase33 main-none run was 121.15 seconds / 1,172,799,488 bytes
RSS. This phase's single main-none observation is faster but has higher peak
RSS; it does not establish a whole-build memory reduction. The literal-table
regression demonstrates eliminated retained entries independently of RSS.

All three complete outputs match phase33 byte for byte. **Correction from
phase44:** the recorded `node --check FILE.js` commands return zero, but explicit
ESM checking (`node --check --input-type=module < FILE.js`) rejects main-none
because rumext native destructuring redeclares a `const` local with `var`.
The original syntax-pass conclusion was too strong. The 996-file port
fingerprint is unchanged. Penpot bundling
and browser execution remain untested. Scope differences and timing/RSS
variability still apply: shadow main also includes render, rasterizer and
code-highlight; light-cljs main includes its five lazy modules.

Frozen executable, tracked patch, essential untracked name-index source
and large JavaScript outputs remain in /private/tmp/light-cljs-phase42/production.

The separate live-allocation comparison of render confirms reduced retained
data: 312,381,712 to 290,492,640 bytes after analysis (7.0% less), and
104,108,000 to 94,240,688 bytes after releasing the macro session (9.5% less).
The reader-identity allocation category disappears; the classified QuickJS
allocation total is unchanged. These are live allocator sizes, not RSS.
The instrumented output also matches the validated output exactly.

The archived prepare.bb now requires LIGHT_BASELINE_LIBRARY explicitly.
The original negative-control library path was in a reused target directory;
that directory was subsequently rebuilt. Its already linked negative executable
remains frozen under /private/tmp/light-cljs-phase42 and is fingerprinted.
