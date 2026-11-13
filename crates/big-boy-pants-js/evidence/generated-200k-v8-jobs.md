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
precise ratios. The five-round measurement follows below when it has run.

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
  the quiet gate. Whether that is machine load or a slowdown since then is
  not settled by this run.

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

A first `--phase measure` attempt stopped at its first measured sample: the
quiet gate read 81.4% idle twelve times while another project's test suite
ran on the machine. It produced no measured samples.
