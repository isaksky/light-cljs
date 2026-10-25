# Bounded native backend project attempts, October 25, 2026

## Configuration recorded before compilation

Compiler source: `b3fac88911d69b3925a0b76d9a49fafe25a625ff`.
Release build command, from the repository root:

```sh
CARGO_BUILD_JOBS=3 cargo build --release --locked --no-default-features --features bbp-macros --bin light-cljs
bb benchmarks/generated-project/generate.bb research/project-attempt-200k --seed 42 --lines 200k --namespace-lines 400
```

This selects only big-boy-pants-js, using the real native-form macro backend;
neither V8 nor QuickJS is compiled. Defaults are unchanged. This baseline includes
462, 474 and 487. It predates 472's aggregate memory/automatic-GC work; the raised
macro memory flag is **not** an enforced aggregate native heap budget here.

Machine: Apple M3 Ultra (Virtual), 24 logical CPUs, 32 GiB RAM, macOS 15.7.3
(24G419), aarch64. Homebrew rustc 1.99.1 (58cf8f9c3 2026-10-13), Cargo 1.99.1
(5f94df478 2026-08-27), Babashka 1.12.218, Node 26.12.1. Initial preparation
load averages: 11.51, 6.54, 4.71. Peer 472 may build/test concurrently. Initial
support probes are not performance comparisons. A successful workload would
get a separately coordinated quiet window for five fresh processes per supported
jobs cell in alternating order, then at most one 30-second profile.

Each compiler process has a 120-second external deadline (`attempt.bb` kills
the process tree and reaps its child), with one initial jobs-1 attempt per full
project and at most one reduction pass per failed project. Each exact argv/cwd
and deadline is written to `*.command.edn` before the process starts. Compiler
cache is off, processes fresh, OS filesystem caches left warm (no purge).
Macro timeouts also use 120,000 ms. Native jobs 2/4/8 are explicitly rejected by
this revision (`src/compiler.rs`, `native macro compilation requires --jobs 1`).

Penpot input: upstream `10955f17e27943429a10e90360d0404e3f56e27c`, clean checkout
at `/Users/admin/src/light-cljs/research/penpot`. The actual input is the existing
local **ported** tree `/Users/admin/src/light-cljs/research/penpot-light`, not the
unmodified upstream app. Its five roots are `frontend/src`, `common/src`,
`vendor`, `generated`, and `shims`, as in `benchmarks/penpot.bb`; entry `app.main`,
ESM, optimization `none`. The input manifest `penpot-inputs.tsv` pins every file
in those roots: 996 files, 12,004,510 bytes. Manifest SHA-256:
`032a12356280fadfce9537e0a25d781b29e5d177924293388ae2dbc357f27264`.
The local port script SHA-256 is
`4be905392e8b52549c7046844ac2973bea042584b76e0c279247a7b1d9f205d7`.
This local port is not versioned by upstream; reproducing it requires the exact
manifest-matching port inputs, including its existing shims/vendor content.
No port regeneration, source rewrite, npm install or shared-tree mutation is used.

Generated input uses the existing generator at the compiler commit above,
`generate.bb` SHA-256
`13e707a203894b666553998dc812c9ffb7bffe2f698be50e0106d5be9d552a1d`.
Seed 42, requested 200,000 code lines, namespace-lines 400: 450 workers,
596 source namespaces, 199,129 code lines, 213,984 physical lines, 9,966,430
bytes, 15,450 top-level forms. `generated-input.edn` retains the generator's
source hashes and size/configuration fields; `generation.txt` is its raw summary.
Entry `workload.main`, roots `src` and `macros`, optimization `advanced` as in
the generated release config, emitted as one ESM with an external source map.
This uses direct CLI equivalents of the generated release budgets and disables
cache, allowing source-map comparison (the project's chunked release command
does not support map flags). No smaller generated input may replace this attempt.

Both commands request `--stats-file` plus an external source map. An error can
prevent all three of JavaScript, source map and stats trace from being emitted;
absence will be recorded rather than treated as identity or a zero metric.

## Outcomes

Both full projects are **unsupported** on this pinned backend. No full-project
success, speedup, or output-equivalence claim follows from these attempts.

| Workload / attempt | Exit | Wall / user / system seconds | Peak RSS bytes |
| --- | ---: | --- | ---: |
| Penpot app.main, initial jobs 1 | 1 | 0.19 / 0.07 / 0.04 | 81,362,944 |
| Generated 200k, initial jobs 1 | 1 | 0.18 / 0.07 / 0.04 | 89,243,648 |
| Penpot reduction, namespace invocation | 1 | 0.00 / 0.00 / 0.00 | 14,204,928 |
| Generated reduction, namespace invocation | 1 | 0.00 / 0.00 / 0.00 | 14,204,928 |

These are the single raw `/usr/bin/time -l` samples, rounded by that utility,
not medians. The runner also retains elapsed monotonic time in `*.result.edn`;
it includes runner/process overhead and is not substituted for child wall time.
No process reached its deadline. Every process was waited/reaped before the next.

Penpot first fails at `app/common/math.cljc:217:3`, expanding `max`, defined
at line 20. Generated first fails at `src/workload/jokoum.cljs:7:1`, expanding
`workload.fonam/stipopi`, defined at `macros/workload/fonam.cljc:34:1`.
Both errors report:

```text
UnsupportedOperand: `this` of sloppy-mode code called with undefined as its receiver is not supported (it needs the global object or a primitive wrapper)
anonymous instruction 9 [get_this @ byte 25], task#1, world#0
called from native macro runtime at instruction 169 (source 93:148)
```

`*-initial.stderr` preserves the full first diagnostic and resource counters.
This is an emitted variadic macro calling-boundary failure; no evidence here
attributes collection/property/call/bridge **time shares**. Both first macros
have rest arguments. One candidate reduction per workload retained its exact
macro body, removed unrelated definitions/requires and reduced the call:
`reduced-penpot/app/common/math.cljs` and `reduced-generated/workload/fonam.cljs`.
Both reproduce the same `get_this` error in namespace mode. They are not proven
globally minimal: the macro's syntax quote, environment lookup (Penpot), and
protocol construction (generated) remain unreduced. No second source-reduction
pass or missing-semantics implementation was attempted.

The first invocation of each reduction mistakenly used single-file CLI mode,
which rejects `ns` before executing macros. Those raw `*-reduced.*` records are
retained as invocation errors, not reproductions of the project failure. The
same candidate bytes were moved into namespace-shaped directories and invoked
correctly once; `*-reduction-namespace.*` are the actual reproductions. For example:

```sh
target/release/light-cljs --compile --entry app.common.math --source-root benchmarks/project-results/native-2026-10-25/reduced-penpot --jobs 1 --no-cache --macro-timeout-ms 120000
target/release/light-cljs --compile --entry workload.fonam --source-root benchmarks/project-results/native-2026-10-25/reduced-generated --jobs 1 --no-cache --macro-timeout-ms 120000
```

One focused follow-up, **light-cljs-489**, covers this common receiver boundary
and both reductions. Searches for `sloppy`, `variadic`, and `receiver` in
`bbp-integration` returned no existing item. The existing collection (477) and
helper-demand (463) work do not cover this first failure. Fixing it may expose
later unsupported operations; this report does not predict full-project support.

## Matrix, outputs and unavailable measurements

| Workload | jobs 1 | jobs 2 | jobs 4 | jobs 8 |
| --- | --- | --- | --- | --- |
| Penpot | Unsupported: first attempt failed | Unsupported backend, not attempted | Unsupported backend, not attempted | Unsupported backend, not attempted |
| Generated 200k | Unsupported: first attempt failed | Unsupported backend, not attempted | Unsupported backend, not attempted | Unsupported backend, not attempted |

There are no supported cells, so no five-run matrix, median/spread, scaling ratio,
quiet timing window or additional profile was collected. Profiling allowance:
0 of 1 per workload used. No V8 build or reference attempt was made: optional
references require a supported workload, which neither is.

Both initial stdout files are zero bytes; neither requested map nor stats JSON
exists. Neither macro returns a result. Thus macro-result verification and
JavaScript/source-map identity across runs/jobs are unavailable, not passing.
No application execution or generated oracle comparison can be made.

Wall/user/system CPU and RSS are above. Runtime initialization, JS
compilation/execution time, bridge cost and allocation metrics are unavailable:
failure aborts before the compiler emits its requested stats trace, and the
external timer cannot separate those phases. GC count/pauses are unavailable:
this baseline predates automatic-GC/aggregate-budget work and exports no such
project trace on failure. Parked tasks, help-while-waiting depth and worker
utilization are unavailable: the native backend is serial at this revision,
resumable helper demand/pool execution are not integrated, and no scheduler
trace survives these failures. Page-reclaim counters are not allocation counts.

**k** is unavailable for median units, longest units and critical path separately:
there are no completed native unit traces or matched same-commit V8 unit samples.
Longest-unit k therefore cannot be classified as <=4, >4 to <=8, or >8.
The 0.19/0.18-second failure latencies are not native macro-unit timings and are
not divided by historical V8 full-project measurements.

## Validation

The native release binary's SHA-256 is in `binary.sha256`. After all attempts,
all 996 Penpot input hashes and all 653 entries in the generated manifest
(including tooling's macro-loading shims) still matched. Both full-project
stdout files were checked empty and their map/stats paths absent. The pre-attempt
configuration was committed before either initial compile.

```sh
CARGO_BUILD_JOBS=3 cargo test --release --locked --no-default-features --features bbp-macros --test bbp_macros -- --test-threads=3
cargo fmt --check
clj-kondo --lint benchmarks/project-results/native-2026-10-25/attempt.bb
git diff --check
```

Native macro integration: 17 passed, zero failures (0.55 seconds of test
execution); formatter, diff check and script lint passed (zero lint warnings).
The one attempt script was exercised on both projects and both reductions.
Full compiler/engine suites and Rust Clippy were not rerun for this measurement,
documentation and Babashka-only change. The existing deprecated
`Atomic::fetch_update` warning appeared in the release build; it was not changed
or suppressed. All build, test and attempt processes completed and were reaped;
no setup symlinks or external build directories were created.
