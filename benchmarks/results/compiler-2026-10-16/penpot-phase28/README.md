# Post-environment-transport profile

An isolated copy of validated phase27 adds scoped Rust timers. No production
instrumentation or C interrupt sampling is present. Both complete outputs match
phase27 byte for byte. These are diagnostic runs, separate from its clean five-job
comparison. Timers can include nested compiler reentry; do not add them as
exclusive phase totals or interpret them as CPU percentages.

| Timer | Render seconds | Main seconds | Main calls |
| --- | ---: | ---: | ---: |
| Whole-form import | 1.247 | 7.947 | 48,216 |
| Environment import, including nil fast paths | 1.152 | 6.660 | 48,216 |
| Environment safety proof | 0.076 | 0.610 | 28,040 |
| Explicit argument import | 1.105 | 6.794 | 128,871 |
| Macro function call, including reader setup and reentry | 4.173 | 62.998 | 48,216 |
| Result export | 1.375 | 7.336 | 48,216 |
| Fragment assembly | 0.075 | 0.388 | 13,361 |
| Unit emission | 1.552 | 8.107 | 441 |
| String-table conversion within unit emission | 0.973 | 5.165 | 441 |

Render takes 15.88 seconds / 586,448,896 bytes peak RSS; main takes 123.50
seconds / 1,321,385,984 bytes. The environment guard is inexpensive. Fragment
assembly is not a useful next speed target. Unit emission repeatedly converts
and concatenates its entire string table just to choose a private runtime
parameter name; that scan warrants a targeted experiment. Macro execution and
its reader wrapper remain the largest measured interval.

Reproduce the instrumentation with `bb prepare.bb` from the repository, then
`bb run.bb` only when no other benchmark, profile or CPU-heavy test is running.
The scripts use `/private/tmp/light-cljs-phase21/project` as a reusable isolated
checkout and overwrite its compiler sources. The original diagnostic binary and
JavaScript are retained in `/private/tmp/light-cljs-phase28/`.
