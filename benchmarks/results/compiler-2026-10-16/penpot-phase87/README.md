# Phase87: validated production reader lookup and shared compiler storage

Promoted production-phase72 variant. It carries phase76's demand-based private
reader binding lookup and the storage changes validated in the V8 phase85
candidate, preserving QuickJS's own namespace-global bookkeeping.

Full validation: **1,401 passed, zero failed, 106 ignored**, normal test
concurrency, strict Clippy, formatting and all three Node workflows. All source,
test, vendor, bench and build inputs were checked identical after promotion.
The two changed documentation files also match the candidate.

| Sequential cold build | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: |
| render | 10.56 | 351944704 |
| main none | 94.02 | 889634816 |
| main advanced | 98.33 | 1051295744 |
| shadow release | 56.86 | 3932160000 |
| shadow dev | 31.17 | 2433712128 |

Main RSS falls **11.8%** versus phase72's production rerun. Time does not improve
beyond run variation. Production remains **3.02 times slower** than shadow dev,
using 36.6% of its RSS. Advanced is 1.73 times slower than shadow release, using
26.7% of its RSS. The dramatic speed goal remains unmet.

All three complete outputs match phase42; explicit ESM checking passes render
and reproduces the known duplicate declaration failure for both main outputs.
The 996-file port hash is unchanged. No application execution is claimed.

The initial isolated trial took 91.34 s / 893534208 peak RSS bytes before final
formatting and validation. Its `source-hashes.txt` and `changes.patch` remain as
that trial's evidence. `shared-storage.patch` is the actual promoted delta;
`validated-source-hashes.txt` and `production/` identify the validated production
build. The full working-tree implementation and untracked name index are saved.

The patch is against phase72. The reader portion was applied by its existing
structural preparation script because the V8 analyzer's bookkeeping differs;
the other changes applied directly. Scripts, logs, exact commands and hashes
are retained. QuickJS namespace-registration trials in phases88–89 start from
this port but are rejected and excluded from production.
