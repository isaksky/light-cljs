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

## Shared session catalog (light-cljs-335, October 18, 2026)

Parent `00d9b70` against the catalog behind a `Mutex`, Penpot `app.main
--optimize none`, three alternating pairs (parent first) at 1-minute load
9.96 to 10.05 while two other workers were active. Output is 33,624,590
bytes with the same SHA-256 in all six runs.

| Build | Wall (s), round 1-3 | Median wall | Peak RSS (GB), round 1-3 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `00d9b70` | 26.11, 25.80, 25.82 | 25.82 | 0.976, 0.960, 0.960 | 0.960 |
| Shared catalog | 25.86, 25.98, 26.14 | 25.98 | 0.976, 0.960, 0.955 | 0.960 |

The medians are +0.6% wall and 0.0% peak RSS, within the +2% and +3% gate.
Penpot registers about 13,000 units; each demand now takes an uncontended
lock two or three times where it borrowed a `RefCell`, and no thread waits.

## Host split (light-cljs-334, October 18, 2026)

Parent `6280d29` against the two-part macro host, Penpot `app.main
--optimize none`, six alternating pairs (parent first in rounds 1-3,
candidate first in 4-6) at 1-minute load 4.9 to 12.0 while two other
workers were active. Output is 33,624,590 bytes with the same SHA-256 in
all twelve runs.

| Build | Wall (s), round 1-6 | Median wall | Peak RSS (GB), round 1-6 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `6280d29` | 26.90, 26.41, 26.31, 26.58, 24.67, 24.99 | 26.36 | 0.961, 0.973, 0.976, 0.959, 0.933, 0.934 | 0.960 |
| `MacroEnvironment` and `EngineBinding` | 26.29, 26.49, 26.44, 26.24, 24.81, 24.85 | 26.27 | 0.957, 0.971, 0.964, 0.944, 0.940, 0.927 | 0.951 |

The medians are -0.4% wall and -1.0% peak RSS, within the +2% and +3%
gate; the median of the six paired differences is -0.03 s, and both builds
ran 1.5 s faster once the load fell in rounds 5 and 6. A frame is built
from the same clones as before, and the platform is initialized once per
process either way.

## Generated names per unit (light-cljs-332, October 18, 2026)

Parent `8982972` against per-unit generated names, Penpot `app.main
--optimize none`, six alternating pairs (parent first in odd rounds) at
1-minute load 9.0 falling to 2.9 while two other workers were active.

| Build | Wall (s), round 1-6 | Median wall | Peak RSS (GB), round 1-6 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `8982972` | 25.97, 25.72, 26.47, 25.31, 25.36, 26.43 | 25.85 | 0.933, 0.969, 0.945, 0.936, 0.928, 0.919 | 0.935 |
| Per-unit names | 25.53, 25.59, 27.01, 25.56, 25.80, 25.61 | 25.60 | 0.924, 0.921, 0.951, 0.927, 0.933, 0.955 | 0.930 |

The medians are -1.0% wall and -0.5% peak RSS, within the +2% and +3%
gate. Output grows from 33,610,277 to 33,622,574 bytes (+12,297, 0.04%):
the quoted symbols and names that spell a scope's 64-bit discriminator.
Both outputs are the same in every run of their build. The scope switch
costs one engine call per change of the heap's scope, not per expansion.

## Parse queue (light-cljs-337, October 18, 2026)

Parent `460b91a` against the parse queue, Penpot `app.main --optimize
none`, six alternating pairs (parent first in rounds 1-3, candidate first in
4-6) at 1-minute load 3.0 to 10.2 while two other workers were active.
Output is 33,604,484 bytes with the same SHA-256 in every run below, at
`--jobs 1` and `--jobs 8` too.

| Build | Wall (s), round 1-6 | Median wall | Peak RSS (GB), round 1-6 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `460b91a` | 25.73, 24.05, 28.29, 24.88, 27.02, 26.85 | 26.29 | 0.924, 0.949, 0.976, 0.940, 0.981, 0.962 | 0.956 |
| Parse queue, default `--jobs 1` | 24.62, 32.77, 26.98, 25.65, 27.77, 27.02 | 27.00 | 0.938, 0.948, 0.987, 0.956, 0.941, 0.983 | 0.952 |

The medians are +2.7% wall and -0.4% peak RSS; the median of the six paired
differences is +0.46 s (+1.7%), and round 2's 32.77 s is an outlier the
load does not explain. The change cannot account for that much: the whole
load phase is about 0.1 s. Three more alternating pairs with `--stats`
measured it directly, and there the build is even:

| Build | `trace.load-ms`, 3 runs | `trace.wall-ms`, 3 runs |
| --- | --- | --- |
| Parent `460b91a` | 101.1, 103.3, 100.7 | 25,038.6, 26,335.4, 25,950.6 |
| Parse queue, `--jobs 1` | 137.2, 103.5, 104.6 | 26,312.8, 25,160.6, 25,972.7 |

The median load phase is 103.3 ms against 104.6 ms (+1.3 ms for the inline
pool, the result table and the replay), and the median trace wall is
25.95 s against 25.97 s.

At `--jobs 8` the load phase (the coordinator's wall time from the first
parse job to the end of the replay, `trace.load-ms`) is 34.1, 36.3 and
34.8 ms, against 101.8, 104.8 and 104.1 ms at `--jobs 1` in the same
alternating runs: 3.0x, from 7 workers. It is 0.1% of the build either
way. It is probably held back by the graph (not profiled): the entry's file
must be parsed before anything else is known, and each level of requires
waits for its parent's job, so the frontier is narrow at first; and the
coordinator replays the walk and records the result serially.

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

## One-worker trace (light-cljs-331, October 18, 2026)

The baseline every later scaling claim is measured against: the
exclusive-stage trace that `--stats` now records, written with
`--stats-file` and read by
[concurrency/dag.bb](concurrency/README.md#reading-a-trace-instead-of-probes).

### Serial gate and the trace's overhead

Parent `ae5045c` against the trace change without `--stats`, and the change
with `--stats-file`, three alternating rounds (parent, candidate, candidate
traced) at 1-minute load 9.2 to 13.1. Output is 33,588,607 bytes with the same
SHA-256 in all nine runs.

| Build | Wall (s), round 1-3 | Median wall | Peak RSS (GB), round 1-3 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `ae5045c` | 26.34, 25.47, 26.02 | 26.02 | 0.996, 1.009, 1.006 | 1.006 |
| Trace change, `--stats` off | 27.25, 25.64, 25.94 | 25.94 | 0.997, 0.984, 1.001 | 0.997 |
| Trace change, `--stats-file` | 25.70, 26.25, 26.17 | 26.17 | 1.014, 1.036, 0.997 | 1.014 |

With `--stats` off the change is -0.3% wall and -0.9% peak RSS on the
medians, inside the +2% / +3% gate and the run-to-run spread (1.6 s): every
hook loads one relaxed atomic and returns, with no clock read and no
allocation. Tracing costs +0.9% wall and +1.7% RSS over the same binary
untraced: two clock reads per V8 service call (1.29 M calls) and 13,361 unit
rows.

The same three rounds again with the final binary (after review: the
recorder is checked per thread before any unit information is built, and
root evaluations are recorded), once the machine had quietened (load 2.7 to
6.9). Output is identical in all nine runs again.

| Build | Wall (s), round 1-3 | Median wall | Peak RSS (GB), round 1-3 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `ae5045c` | 26.87, 22.74, 24.05 | 24.05 | 0.969, 0.976, 0.967 | 0.969 |
| Trace change, `--stats` off | 23.77, 23.53, 25.27 | 23.77 | 0.959, 0.959, 0.948 | 0.959 |
| Trace change, `--stats-file` | 22.30, 24.05, 24.58 | 24.05 | 0.959, 0.951, 0.955 | 0.955 |

Here the change is -1.2% wall and -1.0% RSS untraced, and tracing adds
+1.2% wall and -0.4% RSS, all within a spread of 1.7 to 4.1 s. A lone traced
build on the quiet machine took 22.31 s and 0.954 GB.

### Totals against the design document's probe run

The design document's probe run was at `b51af6a` on October 17 (23.76 s wall
under the probes). To compare at one commit, the probes'
`NSPREP`/`NSPROBE` timers (without their V8 counters) were re-applied
temporarily on top of this change, and one build (26.31 s wall, load 11)
printed both the probe lines and the trace. `dag.bb` reads either; from the
trace it rebuilds the probes' inclusive columns.

| Quantity | Design (`b51af6a`) | Probes, same run | Trace, same run |
| --- | ---: | ---: | ---: |
| Top-level passes, with demanded units | 6.00 s | 6.56 s | 6.61 s |
| Drains, with their units | 16.53 s | 18.39 s | 18.39 s |
| Units | 13,361 | 13,361 | 13,361 |
| Drained units' own analysis | 15.86 s | 17.65 s | 17.53 s |
| V8 at depth 0 | 13.62 s | | 14.67 s |
| V8 service entries | 1,288,464 | | 1,288,469 |

At the same commit the trace and the probes agree: drains to the
millisecond, top-level passes +0.8% (the trace's frame starts before the
`#_` re-parse and the refer checks, the probe's timer after them), units
-0.7% (the trace times a unit from after its state check and macro-frame
copy; that setup, 9 µs a unit, is drain bookkeeping in the trace). Against
the design document everything is about 10% slower, uniformly: the parent
binary itself takes 26.0 s here against 23.2 s then, so the difference is
the busier machine and a day of commits, not the measurement.

### What the trace says

From the first traced gate run (25.70 s wall; the `trace.*` lines of its
report):

- **Exclusive stages** (they add up to the 24.74 s traced): deferred units
  17.13 s (10.70 s of it V8), top-level passes 6.52 s (3.78 s V8), drain
  bookkeeping 0.82 s (append 0.41, release 0.28, drain loop 0.12, index
  0.01), loading 0.11 s, other 0.12 s, warnings 0.03 s, preparing sources
  0.01 s. After analysis: preparation 0.10 s, emission 0.72 s.
- **Analysis demand edges are nearly absent.** 13,359 of the 13,361 units
  are started by their namespace's drain. Only `instaparse.gll/PROFILE`
  (0.1 ms) is demanded by a top-level pass and `cuerdas.core/repeat` (0.1 ms)
  by another unit; the top-level passes waited 0.1 ms in all. The helpers
  Penpot's macros call are drained in earlier namespaces before they are
  needed.
- **Execution edges.** 277 units' compile-time roots are evaluated (each
  once, in the one isolate), 240 first by a unit's analysis and 37 by a
  top-level pass; 258 of them belong to another namespace than their
  demander, and 2 had to be analyzed on the spot. These are the helpers a
  worker isolate must initialize again (light-cljs-359 adds the later reads
  of ready roots, which stay in JavaScript today).
- **Macro host share.** 7,335 units (54.9%) entered the macro host, and they
  are 97.7% of unit time (16.74 of 17.13 s). The 6,026 macro-free units take
  0.39 s, so light-cljs-338's narrower first cut can move about 2% of the
  unit time. Only 2,797 units (0.12 s, from the final binary's trace) make no
  macro call at all; the others expand native macros such as `fn` and `let`,
  in Rust.
- **Longest unit**: `instaparse.abnf/abnf-parser` (line 129), 2.09 s, in the
  macro host; then `app.plugins.shape/shape-proxy` 0.50 s and
  `app.plugins.api/create-context` 0.12 s. Three units exceed 100 ms, 234
  exceed 10 ms.
- **Ready width per drain.** 914 drains start units; the median drain has 6
  ready units (p90 34, max 304 in `app.main.ui.ds.foundations.assets.icon`),
  and the median count of macro-free ones is 2 (p90 16). The widest drains
  are mostly cheap: `app.main.ui.icons` has 272 host units worth 0.28 s.
- **Lower bounds.** With a barrier at each drain, the serial rest (7.38 s)
  plus per drain `max(W/P, longest unit)` gives 17.04 s at P=4 and 16.53 s at
  P=12: one long unit per drain dominates. Without the barrier, the existing
  models on the trace give 6.58 s for stage A (`Option U`, units on 4 or more
  workers behind serial top-level passes) and 2.96 s for stage C at 8 workers
  (`Model C`), against 6.06 s and 2.74 s from the probe data.
