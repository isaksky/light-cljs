# Phase85: validated compiler-storage changes

The combined isolated V8 candidate shares metadata-map layers, stores consumed
definition inputs out of line, shares historical namespace tables, and trims
spare arena capacity before retaining completed analyses. Its reader lookup
and engine behavior otherwise follow phase76. The canonical-bridge and shared
arena-leaf experiments are excluded.

Full validation: **1,409 passed, zero failed, 106 ignored**, four test threads;
strict Clippy, formatting, and all three standalone Node workflows pass.

| Clean cold build | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: |
| render | 3.29 | 521895936 |
| main none | 20.57 | 1088978944 |
| main advanced | 26.22 | 1232338944 |

Compared with phase76's clean main run, RSS falls 12% and time falls about 2%.
Against the fresh phase72 shadow-dev measurement, main is 32% faster and uses
46% of its RSS. This still does not meet the dramatic speed goal. V8 remains
unpromoted because of its resource-limit, watchdog and packaging gaps.

The earlier diagnostic main run with terminal GC takes 20.48 s / 1040678912
peak RSS bytes. All three complete clean outputs match phase42, including the
known main ESM defect; no bundling or application execution is claimed.

`changes.patch` is the capacity-trimming delta against phase84.
`combined-changes.patch` contains all storage changes against validated phase76.
Scripts, test logs, exact benchmark commands and source/output hashes are retained.
Production remains QuickJS phase72 while its corresponding port is measured.
