# Generated 200k baseline and comparison harness

November 4, 2026 UTC. The unchanged [pinned target](generated-200k.md) builds
successfully with V8 and with native Goose source-macro execution at 1/2/4/8.
Every completed full build below matched all five independent oracle outputs,
the strict waste check, and **all 3,449,022 emitted JavaScript bytes**. No output
normalization was used. This establishes the harness and V8 reference baseline;
it is not the final full-form/safety audit or a Goose performance matrix.
The later [full-project performance report](generated-200k-performance.md)
contains the complete five-round Goose/V8 comparison; the historical observations
below retain their original scope.

## Reproduce

Use the commands in the [generated-project guide](../../../../benchmarks/generated-project/README.md#pinned-source-macro-comparison).
The same `compare.bb` retains its original Shadow comparison mode. New explicit
`--backend v8|goose|all` cells share the actual release/advanced compiler pipeline.
`--workers 1,2,4,8` names Goose execution limits; samples separately name compiler
slots, equal to execution workers here. The generated 450 worker *namespaces*
are not threads. Goose selection requires the `goose-macros` feature and a
release command; unsupported entry points, worker counts and cached Goose
installation reject explicitly. Selected Goose source jobs cannot fall back to
a reference engine.

The primary invocation was:

```sh
bb benchmarks/generated-project/compare.bb target/generated-200k \
  --light target/release/light-cljs --backend v8 --phase measure --runs 5
```

Build beforehand with `cargo build --release --locked --features goose-macros
--bin light-cljs`, `CARGO_BUILD_JOBS=3`, `LIGHT_CLJS_V8_CACHE=require`, and private
`CARGO_TARGET_DIR=target`, `GOOSE_HOME=target/goose`. Build the native runner from
`compiler_session.goose` without `--debug`. Release uses thin LTO and one codegen
unit. The actual compiler command is `release bench --optimize advanced --jobs
1 --no-cache`; its macro budgets and source-map settings are the frozen generated
configuration. Each run is a fresh process, including its process-wide bootstrap
UNITS cache miss. Compilation startup through joined cleanup is timed; building,
generation, oracle/Node checks and strict waste checks are outside that interval.

## Input, source and executable identity

Two fresh generations matched the full frozen manifest and every canonical
source/shim hash: 653 files, 199,129 nonblank lines, 450 worker namespaces,
57 macro namespaces. The immutable manifest SHA-256 remains
`6a4da9ff2d8682c3e524d86abb36a1628131cae16bcbb0220797b4f85c131907`.
Sources, population, generator scripts, model, build configuration and runners
are checked before/after comparisons; leaf edits have explicit temporary hashes.

- Rust compiler binary: `3d7d1882264b75980f765d82ea6ee710373a5cf6620b68b2b0aab7719c94c83d`.
- Native Goose runner: `92140b1a31ba1fe42beadc3caae59fb6ea0916e4a3204e5caff10ef7146688e0`.
- Emitted application: `18335556dd4672f2f63b901b68fcad3c447cf5676f457a08fda61f366cc9f3ec`.

The raw phase manifests identify harness/source commits and hashes independently:
`91360fb9` for native instrumentation, `88537729` for clean/cached/leaf runs.
The latter only broadens source-hash coverage; compiler/native execution source
and binaries match throughout. Full toolchain, compiler-input hashes, exact
commands, stdout, rusage, stderr, traces and qualified macro tables are in
[generated-200k-baseline.edn.gz](generated-200k-baseline.edn.gz), a gzip-compressed
EDN map with `:files` containing each raw text file and `:sha256` its digest.

Validated environment: macOS arm64 / Darwin 24.6.0, Rust 1.99.1, Node 26.12.1,
Babashka 1.12.218, clang 21.0.0, Goose `v2026.3-267-g369c7a2c` / runtime
`3025e5810379`. Exact version output is in each phase manifest.

## Clean V8 reference

One excluded warmup, then five quiet uncached samples, all validated separately:

| Metric | Result |
| --- | ---: |
| Wall median | 6.373567 s |
| Wall min / max | 6.274530 / 6.553900 s |
| Aggregate CPU median (user + system) | 6.93 s |
| User / system medians | 6.51 / 0.42 s |
| Command peak RSS median | 1,035,845,632 bytes |
| CPU idle before measured samples | 92.56–97.76% |

The gate requires >=90% idle and no competing compiler/build. Every sample
passed; no slow sample was discarded. Wall time is the enclosing process
watchdog interval. CPU/RSS are raw macOS `/usr/bin/time -l` command rusage;
RSS is not an additive process-tree memory peak. V8 has no Goose subprocess.
All oracle stdout, including final newlines, was exactly:
`687181`, `371978`, `484264`, `945410`, `463429`, for the fixed input/iteration
pairs in the target. Strict waste left 455 markers: 406 required defmethods and
49 best-effort arities, with zero removable/compile-time/orphan markers.

## Separate instrumentation

One warmup and one instrumented full build per V8/Goose cell passed. These are
correctness/diagnostic runs, **not five-round clean Goose timing cells**.
The instrument command compares each qualified generated source-macro entry
count against V8, in addition to complete emitted bytes and all oracle outputs.

- 94,592 total compiler macro expansions; 5,226 JavaScript source-macro entries
  across 615 qualified generated macros, identical for V8 and Goose 1/2/4/8.
- 57 distinct demanded helper identities. Compiler helper-demand callbacks:
  V8 57; each Goose cell 529. These are callbacks, not repeated producer
  compilations. Goose reports 672 exported definitions (615 macros + 57 helpers),
  615 macro preparations, and 5,226 source-macro jobs in every cell.
- 569 loaded namespaces (567 generated plus two bundled libraries), 13,477
  deferred units. Require DAG depth 11. Drain ready width: median 28, p90 33,
  max 37; macro-free width median 23, p90 28, max 35. Of 13,477 drained units,
  2,493 entered the host and 10,984 did not. The existing DAG tool models
  2/4/8 workers; its worker-only P=1 model is inapplicable. Serial data is the
  actual one-slot trace, not a simulated estimate.

The V8 instrumented compiler phase counters were 65.2 ms resolve/parse,
2,120.2 ms analysis, 2,503.6 ms deferred units, 955.4 ms application preparation,
652.0 ms emission and 0.8 ms writing, within 6,318.8 ms reported build wall.
Source-macro service intervals totalled 1,109.6 ms (~17.6% of that wall).
This is the measured call-boundary share, including conversion/callback work;
source-code preparation and helper/bootstrap setup outside those intervals
also remain relevant. It is not the total replaceable engine fraction.

Retained V8 core/bootstrap costs are charged in every Goose build. V8's cold
`build_in` took 11.824 ms: core compilation 3.589 ms, sequence compilation
3.960 ms, core installation 3.251 ms and sequence installation 1.004 ms.
Compiler-evaluator creation/prelude took 8.971 ms. These inclusive diagnostics
can overlap and must not be summed. No later `expand_core_javascript` entry
occurred on this target: reached core shapes used Rust native expansion.
Bootstrap installation counts were 1/3/5/9 at Goose 1/2/4/8 (the parallel
configurations include a transient preparation bootstrap), with the first
cold compilation and later in-process UNITS reuse recorded separately.
This remains **Goose source-macro execution with V8 core/bootstrap**; reading,
resolution, Rust core/binding expansion, analysis, optimization/emission and
Node application execution retain their established owners.

The native diagnostic mode preserves its own capacity/reset/thread evidence:

| Goose workers / compiler slots | Dispatches and resets | Private capacity bytes, summed slots | Native thread total / spawned threads joined | Active private owners |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 5,812 | 5,303,554 | 4 / 3 | 1 |
| 2 | 5,812 | 10,551,800 | 6 / 5 | 2 |
| 4 | 5,812 | 20,982,021 | 10 / 9 | 4 |
| 8 | 5,812 | 41,530,591 | 18 / 17 | 8 |

The native total includes its coordinator; every spawned native thread joins.
Rust/V8 service threads are additional. All slots executed work. The protocol
trace is bounded to 4,096 early rows, with final POOL/THREADS/CLOSED records
displacing early prefix entries;
its sampled native RSS maxima (14.0/15.4/22.4/32.7 MB) are prefix observations,
not full-process peaks. Capacity sums are private table capacities, not RSS.
Full-run session counters cover export, load/boot, snapshot, job load, bridge and
elapsed time; cleanup is recorded after explicit close. Macro service intervals
and aggregate bridge costs overlap across workers and must not be added to wall.
The bounded protocol prefix does not prove a full-run peak of simultaneous VM
instructions. Clean Goose scaling/process-tree peak memory remain later cells.

## Cached and leaf scenarios

These are separately labelled **instrumented V8 diagnostics**, not clean primary
cells. The no-change rebuild installed all 569 namespaces, hit all 569 artifacts,
reused its output, and executed zero macros (one sample, 1.121 s). A separately
labelled leaf scenario appended two distinct inert definitions, one per rebuild,
to `src/workload/jourklijo.cljs`. Each had 566 hits / three misses, 244 total
macro expansions / 16 generated source-macro entries, and no output reuse;
the two diagnostic wall times were 2.935 / 2.884 s. Each complete output still
matched the unmodified V8 reference, all oracle cases and strict waste checks.
Exact source bytes were restored and the full frozen manifest revalidated.
Goose cached installation rejects explicitly; no skipped macro work is presented
as a Goose speedup.

## Verification and limits

Generator/comparator regressions: 11 tests / 367 assertions, including exact
stdout/full-JS rejection, changed qualified counts, native join accounting,
leaf restoration, nonzero exit propagation and watchdog child reaping.
The full native instrumented target passes at 1/2/4/8, with identical source
macro counts and all application checks. Wrong worker count, cached Goose and
unsupported CLI entry points fail explicitly; the final CLI selection regression
passes in feature-enabled V8 and QuickJS-only builds.

The broader CLI suites each had 39 passes and the known playground wasm failure:
unchanged `program_fragment::{published,expr,append_published}` dead code under
`-D warnings`. Strict Clippy retains eight engine diagnostics and four default/QuickJS root
library/test diagnostics; root formatting retains unrelated existing differences,
including `compilation_session.rs`. Changed Rust files pass focused formatting
(the new two-line session hook is outside its baseline differences), and
`git diff --check` passes. No successful full-suite/strict-lint claim is made.
Unchanged reviewed VM/compiler regression evidence is reused; full forms, scalar
pools, metadata/provenance, failure/scale budgets and the five-round Goose matrix
remain their existing acceptance stages. The known V8 equal-scalar provenance
observation limitation is not erased by matching application JavaScript.
