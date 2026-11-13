# Generated 200k: Goose against V8 at the same job count

November 12, 2026, at `1a60e8f7`. Every earlier Goose/V8 table in
[generated-200k-performance.md](generated-200k-performance.md) ran V8 at
`--jobs 1` while Goose N ran N compiler slots and N native workers. V8 at
`--jobs N` is parallel too: it runs deferred units on N-1 worker isolates.
This report compares the two paths at equal job counts.

**At every job count V8 is faster than Goose.** The November 6 statement
"Goose4 is 29% faster than V8" compared four Goose threads with one V8
thread; it does not hold at four V8 jobs.

## Result (check phase: one warmup and one sample per cell, machine not gated)

`compare.bb --backend all --v8-jobs 1,2,4,8 --phase check --runs 1`. Seconds
of wall time, aggregate CPU (user + system, waited children included) and
peak memory (the compiler command's maximum resident set, GB). These are
single samples without the quiet gate, so read them as direction, not as
precise ratios. A five-round run follows below.

| Jobs | V8 wall | Goose wall | Goose / V8 | V8 CPU | Goose CPU | V8 peak memory | Goose peak memory |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 7.168 | 8.296 | 1.16× | 7.80 | 8.32 | 1.040 | 0.966 |
| 2 | 5.690 | 6.569 | 1.15× | 8.55 | 8.75 | 1.163 | 0.958 |
| 4 | 4.788 | 5.342 | 1.12× | 10.32 | 9.63 | 1.363 | 0.979 |
| 8 | 4.944 | 5.484 | 1.11× | 12.91 | 11.02 | 1.576 | 1.050 |

Warmups (same order, seconds of wall): V8 8.104 / 5.793 / 4.840 / 4.899;
Goose 8.485 / 6.531 / 7.990 / 5.224.

All 16 builds exit 0 and emit the complete 3,449,022-byte output with
SHA-256 `18335556dd4672f2f63b901b68fcad3c447cf5676f457a08fda61f366cc9f3ec`,
identical to V8's reference, with all five oracle outputs and the strict
waste check passing.

Observations:

- Both paths gain from more jobs up to four and nothing from eight. Goose
  uses less peak memory at every count and less CPU at four and eight jobs.
- V8 at `--jobs 4` reports `pool.worker-isolates 3`, `pool.workers 3`,
  `pool.utilization 0.497`, `engines.v8-isolates 5` in a separate `--stats`
  run; at `--jobs 1`, one isolate and no pool.
- Goose1 here is 8.3 s; on November 6 (`8b0262ee`) it measured 6.851 s under
  the quiet gate. The five-round run below, with both compilers interleaved,
  shows no slowdown since then.

## The same comparison at earlier commits

Was V8's scaling with `--jobs` already there when the Goose claims were
made? Two earlier compilers, each built in its own detached worktree with
the same flags and run with today's `compare.bb --v8-jobs 1,2,4,8 --phase
check --runs 1` against its own fresh copy of the seed-42 project (the
manifest is identical to today's). Same caveat: single samples, not gated.
Another session's builds were running on the machine at times.

- **`0ad2514f` (October 29)**, the commit of the last shadow-cljs comparison,
  before any Goose work. Default features (V8). Its output is the older
  3,456,905-byte build (SHA-256 `1a75bc87…`), with all five oracle outputs and
  the strict waste check passing.
- **`8b0262ee` (November 6)**, the commit of the "Goose transport changes"
  measurement, which reported Goose4 4.492 s against V8's 6.316 s at
  `--jobs 1`. Built with `--features goose-macros` and its own runner. Every
  output is today's `18335556…`.

Seconds of wall time / aggregate CPU / peak memory (GB):

| Jobs | V8, Oct 29 | V8, Nov 6 | Goose, Nov 6 | V8, Nov 12 | Goose, Nov 12 |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 7.237 / 7.77 / 1.107 | 6.392 / 6.96 / 1.038 | 7.322 / 7.34 / 0.915 | 7.168 / 7.80 / 1.040 | 8.296 / 8.32 / 0.966 |
| 2 | 5.997 / 8.72 / 1.171 | 5.885 / 9.09 / 1.189 | 5.927 / 7.87 / 0.928 | 5.690 / 8.55 / 1.163 | 6.569 / 8.75 / 0.958 |
| 4 | 5.201 / 10.52 / 1.393 | 4.437 / 9.62 / 1.352 | 4.813 / 8.57 / 0.943 | 4.788 / 10.32 / 1.363 | 5.342 / 9.63 / 0.979 |
| 8 | 5.372 / 14.60 / 1.576 | 4.799 / 12.53 / 1.575 | 5.034 / 9.94 / 0.965 | 4.944 / 12.91 / 1.576 | 5.484 / 11.02 / 1.050 |

So V8's scaling predates Goose: on October 29 it already went from 7.2 s at
one job to 5.2 s at four, in line with that day's report (7.03 s and
5.07 s). On November 6 V8 at four jobs (4.44 s) matched the reported Goose4
result (4.49 s); in this run Goose was slower than V8 at every equal job
count, by 0.04 s at two jobs and 0.4–0.9 s elsewhere. The 29% Goose advantage
reported that day came from the job-count mismatch.

Compiler SHA-256: Oct 29 `1516586489ada4b96848823c90c9404cabc054632a41269aef94b847bc94433e`;
Nov 6 `279594eb1f1257ce2bd3827eb28702b174ba6d2e5bbf724a805806630f621806`,
runner `2d71378ce2161349832624597100926749c2e5665272842cdd47f35aa54725ce`
(these are fresh builds, so they differ from the hashes in the November 6
report). Raw samples:
[Oct 29](generated-200k-v8-jobs-check-oct29.edn.gz),
[Nov 6](generated-200k-v8-jobs-check-nov06.edn.gz).

## Binaries and reproduction

Release build with `--locked --features goose-macros` at `1a60e8f7`, compiler
SHA-256 `0619501607a60c4fa764cff081e18891feaa9eda8113fc7a4c97107ffcc0daf2`;
runner `compiler_session.goose`, SHA-256
`4d18abbd2edf2f01e8a04aa6af3b420dfbe5c86b1e201a1c8bc5876d95ef4d76`.
Seed 42, 199,129 lines, 450 worker namespaces, release/advanced,
`--no-cache`. Same machine as the other reports (M3 Ultra virtual machine,
24 logical CPUs, 32 GiB, macOS 15).

```sh
bb benchmarks/generated-project/compare.bb target/generated-200k --light target/release/light-cljs --runner target/generated-200k-runner --backend all --v8-jobs 1,2,4,8 --phase check --runs 1
bb benchmarks/generated-project/compare.bb target/generated-200k --light target/release/light-cljs --runner target/generated-200k-runner --backend all --v8-jobs 1,2,4,8 --phase measure --runs 5
```

Raw samples: [generated-200k-v8-jobs-check.edn.gz](generated-200k-v8-jobs-check.edn.gz),
the gzipped `samples.edn` of the check run (one EDN map per line).

## Five rounds (check phase, `--runs 5`, machine not gated)

The quiet gate (at least 90% idle, no competing build) kept rejecting the
machine: 81.4%, 85.2% and 64.7% idle, and competing `goose`/`clang` builds
from another session. The check phase with `--runs 5` runs the same
schedule as the measure phase without the gate: one warmup per cell, then
five rounds whose cell order reverses every round. During this run another
session's native test suite used about four of the 24 logical CPUs. Every
cell fits in the spare capacity, but the load adds noise, most visibly to
Goose at one worker.

Today's compiler and the November 6 compiler ran interleaved in one run;
October 29 ran right after it in a separate run, so compare it with the
others only loosely. Median wall seconds [minimum, maximum] over five
samples, then median CPU seconds and median peak memory (GB):

| Jobs | V8, Oct 29 | V8, Nov 6 | Goose, Nov 6 | V8, Nov 12 | Goose, Nov 12 |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 8.017 [7.892, 8.990] / 8.60 / 1.088 | 7.511 [7.249, 8.144] / 8.17 / 1.051 | 9.446 [7.765, 9.925] / 9.37 / 0.904 | 7.401 [7.038, 7.806] / 8.05 / 1.053 | 9.508 [8.524, 10.252] / 9.51 / 0.968 |
| 2 | 6.677 [6.621, 6.923] / 9.75 / 1.185 | 6.021 [5.858, 6.519] / 9.09 / 1.165 | 6.834 [5.830, 7.775] / 9.09 / 0.930 | 6.745 [5.543, 9.605] / 10.68 / 1.180 | 7.060 [6.638, 7.774] / 9.37 / 0.965 |
| 4 | 5.633 [5.455, 5.812] / 11.44 / 1.398 | 5.236 [4.635, 5.511] / 11.45 / 1.370 | 5.570 [4.715, 5.870] / 9.96 / 0.949 | 4.968 [4.508, 5.216] / 10.97 / 1.361 | 5.935 [4.908, 6.777] / 10.81 / 0.982 |
| 8 | 5.634 [5.581, 6.118] / 15.11 / 1.606 | 5.034 [4.621, 5.671] / 13.48 / 1.579 | 5.531 [4.784, 5.837] / 11.17 / 0.988 | 5.102 [4.653, 6.199] / 13.35 / 1.583 | 5.747 [5.001, 6.278] / 11.51 / 1.003 |

All 100 samples and 20 warmups exit 0. Every Nov 6 and Nov 12 output is the
reference `18335556…`, and every Oct 29 output is its `1a75bc87…`, with all
five oracle outputs and the strict waste check passing.

- **V8 has the lower median at every job count, for both compilers.** At
  four jobs V8 is ahead by 0.33 s (Nov 6) and 0.97 s (Nov 12); at one job,
  by about 1.9–2.1 s.
- **No Goose slowdown between November 6 and today is visible.** Interleaved
  in the same run, Goose1 is 9.45 s against 9.51 s, and the Goose ranges
  overlap at every job count. The 7.3 s and 8.3 s single samples above
  differed because of machine load, not because the compiler changed.
- **Goose is the more load-sensitive of the two.** Its one-worker CPU time rose
  with the wall time against the single samples (Nov 6: 7.3 s → 9.4 s CPU;
  Nov 12: 8.3 s → 9.5 s), more than V8's did (7.0 s → 8.2 s; 7.8 s → 8.1 s). Goose used less peak memory than V8 at every count (0.90–1.00 GB
  against 1.05–1.61 GB) and less CPU at four and eight jobs.
- Both paths stop gaining after four jobs.

Raw samples: [Nov 6 and Nov 12](generated-200k-v8-jobs-runs5.edn.gz),
[Oct 29](generated-200k-v8-jobs-runs5-oct29.edn.gz).

```sh
bb benchmarks/generated-project/compare.bb target/generated-200k --light target/release/light-cljs --runner target/generated-200k-runner --also-light nov06=NOV06/target/release/light-cljs,runner=NOV06/target/generated-200k-runner --backend all --v8-jobs 1,2,4,8 --phase check --runs 5
bb benchmarks/generated-project/compare.bb target/generated-200k-oct29 --light OCT29/target/release/light-cljs --backend v8 --v8-jobs 1,2,4,8 --phase check --runs 5
```
