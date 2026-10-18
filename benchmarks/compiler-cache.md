# Compiler cache measurements

Numbers for the persistent compiler cache
([design](../docs/compiler-cache.md), "Settled decisions" for the rollout
gates). Every row is Penpot `app.main --optimize none` built by
[penpot.bb](penpot.bb) (`bb benchmarks/penpot.bb OUT main-none`) in a fresh
process, on the Apple M3 Ultra virtual machine (24 vCPUs, 32 GiB) other workers
share. Baseline and candidate runs alternate, with `uptime` load recorded next
to each, and the binaries are immutable copies selected with
`LIGHT_CLJS_BINARY`. `LIGHT_CLJS_EXTRA_ARGS=--stats` adds the report, saved as
`main-none.stats.txt`, with its counters in `results.edn` under `:stats`.

## Cold-build baseline (light-cljs-310, October 18, 2026)

Parent `f68c767` against the `--stats` change, both without `--stats`, four
alternating pairs at 1-minute load 3.5 to 10.6 (a fifth of the machine). Output
is 33,588,607 bytes in every run.

| Build | Wall (s), run 1-4 | Mean wall | Peak RSS (GB), run 1-4 | Mean RSS |
| --- | --- | ---: | --- | ---: |
| Parent `f68c767` | 22.74, 23.48, 22.22, 24.06 | 23.13 | 0.994, 0.952, 0.951, 0.969 | 0.967 |
| `--stats` change, flag off | 22.73, 24.10, 23.43, 24.01 | 23.57 | 0.964, 0.992, 0.996, 0.983 | 0.984 |

The difference, +1.9% wall and +1.8% RSS on the means, is within the +3% and
+5% gate and within the run-to-run spread of either binary (1.8 s): without
`--stats` the change reads no clock and adds one counter read per namespace.
Two runs with `--stats` took 22.37 s and 22.17 s at 0.977 and 0.963 GB. An
earlier round under load 40 (another worker's V8 build) gave 36.6 s for both
binaries and is not used.

### Phase split with `--stats`

The second `--stats` run, which is representative of both:

| Counter | Value |
| --- | ---: |
| `time.wall-ms` | 22,125 |
| `time.resolve-parse-ms` | 97 |
| `time.analysis-ms` | 5,753 |
| `time.deferred-units-ms` | 15,471 |
| `time.prepare-application-ms` | 99 |
| `time.emission-ms` | 636 |
| `time.write-ms` | 39 |
| `namespaces` | 922 |
| `macro-calls` | 93,997 |
| `macro-nodes.peak` | 5,234,662 |
| `deferred-units.registered` / `analyzed` / `prepared` | 13,367 each |
| `deferred-units.executed` | 277 |

Analysis is 26% of the wall time and draining deferred units 70%; resolving,
reading and parsing 922 sources is under half a percent, and preparing,
emitting and writing the 33.6 MB output 3.5%. The cache's saving is therefore
bounded by the analysis and drain phases it can skip, about 96% of a cold
build. Every registered deferred unit is analyzed and prepared during its
namespace's drain; 277 of them ran at compile time, for the macros that called
them. The namespaces with the most macro calls are `malli.core` (1,905),
`app.plugins.shape` (1,465), `app.render-wasm.api` (1,315),
`app.plugins.library` (1,206) and `app.common.logic.libraries` (880). The
macro-call count is the same in every run, so a cache hit will show as a
lower count.

## Dependency manifest, cache off (light-cljs-311, October 18, 2026)

Without `--cache-dir` nothing records a manifest. On the cold path what remains
is a `RefCell` check per source macro call and per name resolved outside the
`ns` form, plus a few small vectors per namespace in the loader. The runs were
three alternating pairs of the parent `d6439d4` against the change, both
release builds, at 1-minute load 7.6 to 11.2, with other workers active. Output
is 33,588,607 bytes in every run.

| Build | Wall (s), run 1-3 | Mean wall | Peak RSS (GB), run 1-3 | Mean RSS |
| --- | --- | ---: | --- | ---: |
| Parent `d6439d4` | 25.40, 24.48, 25.03 | 24.97 | 1.042, 1.031, 1.034 | 1.036 |
| Manifest change | 24.96, 22.02, 24.22 | 23.73 | 1.051, 0.960, 1.027 | 1.013 |

On the means, wall is -5.0% and RSS -2.3%, within the +3% and +5% gate.
Both differences are inside the run-to-run spread. The manifest's own size and
cost on Penpot are in [docs/compiler-cache.md](../docs/compiler-cache.md),
"Conservative keys and invalidation".

