# Compiler concurrency measurements

Numbers for multi-threaded compilation
([design](../docs/compiler-concurrency.md), "Gates and first steps"; the
research kit behind the design is in [concurrency/](concurrency/README.md)).
Every row is Penpot `app.main --optimize none` built by
[penpot.bb](penpot.bb) (`bb benchmarks/penpot.bb OUT main-none`) in a fresh
process, on the Apple M3 Ultra virtual machine (24 vCPUs, 32 GiB) other
workers share. Baseline and candidate runs alternate, with `uptime` load
recorded, and the binaries are immutable copies selected with
`LIGHT_CLJS_BINARY`. The gate for a pellet on the serial hot paths is +2%
wall and +3% peak RSS against its parent commit, on medians.

## `Arc` instead of `Rc` (light-cljs-333, October 18, 2026)

Parent `d6439d4` against the `Arc` change with each `Arena` lookup hint
choice, three alternating rounds (parent, relaxed atomic, per-thread) at
1-minute load 11.8 to 12.5 (half the machine; another worker was building V8).
Output is 33,588,607 bytes with the same SHA-256 in all nine runs.

| Build | Wall (s), round 1-3 | Median wall | Peak RSS (GB), round 1-3 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `d6439d4` (`Rc`, `Cell` hint) | 26.30, 25.03, 25.20 | 25.20 | 1.048, 1.039, 1.040 | 1.040 |
| `Arc`, relaxed `AtomicUsize` hint (landed) | 26.25, 25.33, 25.50 | 25.33 | 1.059, 1.056, 1.040 | 1.056 |
| `Arc`, per-thread hint keyed by arena | 25.95, 25.51, 25.48 | 25.51 | 1.039, 1.043, 1.018 | 1.039 |

The landed change is +0.5% wall and +1.6% peak RSS on the medians, within
the gate and within the run-to-run spread (1.3 s wall for the parent, 19 MB
RSS for the candidate). `Arc` and `Rc` have the same size, and a relaxed
load or store is a plain load or store on arm64 and x86-64, so no difference
is expected. The per-thread hint is a `thread_local!` (owner, chunk) pair
keyed by the arena's `starts` pointer: it was +1.2% wall, probably because
one hint per thread is shared by every arena that thread reads, so
interleaved reads of `exprs`, `locals` and `globals` evict each other (not
profiled). The atomic keeps one hint per arena, as the `Cell` did.

A second set of three alternating pairs, started when the machine had gone
quiet (load 6.3) but run at load 12.5 to 19 once other workers resumed; the
first parent run (30.23 s) is an outlier, probably a cold file cache after
the idle spell; the median does not use it. The machine was not quiet for a
whole set at any point that day.

| Build | Wall (s), round 1-3 | Median wall | Peak RSS (GB), round 1-3 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `d6439d4` | 30.23, 25.43, 25.26 | 25.43 | 1.015, 1.031, 1.015 | 1.015 |
| `Arc`, relaxed atomic hint | 25.10, 25.42, 25.49 | 25.42 | 1.028, 1.019, 1.029 | 1.028 |

Here the change is -0.04% wall and +1.3% peak RSS, again within the gate,
with the same output in all six runs.
