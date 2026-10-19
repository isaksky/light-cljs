# Concurrency research kit

The evidence behind [docs/compiler-concurrency.md](../../docs/compiler-concurrency.md):
the timing probes, the `Arc` experiment, the parse harness, the schedule
models and the probe data they were run on. It was retained from the
gitignored `research/concurrency-opus/` of the main checkout by light-cljs-330
so that later pellets can compare against it. None of it is built by `cargo`;
the patches and the probe module are applied by hand to a scratch checkout
and reverted afterwards. Only `dag_test.bb` runs under `cargo test`
(`tests/cli.rs`, when `bb` is on PATH).

## What was measured

The compiler measured is commit `b51af6a` ("Reduce compiler retention and
accelerate native macros with V8"), checked out under `87f5d0e`, which adds
only `docs/compiler-cache.md` and two README lines on top of it. The build was
a cold `--compile --esm --optimize none` of Penpot `app.main` from the five
port roots of [benchmarks/penpot.bb](../penpot.bb), with the same macro limits.
A release binary with
`instrumentation.patch` applied produced `main-probe.tsv`; output stayed
byte-identical to the uninstrumented build.

## Files

| File | What it is |
| --- | --- |
| `instrumentation.patch` | The temporary probes, against `b51af6a` (`src/analyzer.rs`, `src/compiler.rs`, `src/lib.rs`, `src/main.rs`, `src/v8_evaluator.rs`). With `LIGHT_NS_PROBE` set, the compiler prints tab-separated `NS*` lines on stderr: load, per-source prepare, per-namespace top-level and drain time with their V8 shares, unit count, each unit's duration and the namespace's requires, then lazy load, finish, analyze, prepare and emit times. |
| `conc_probe.rs` | The probe module the patch declares as `mod conc_probe;`; copy it to `src/conc_probe.rs`. V8 time is counted at `v8_evaluator::invoke` depth 0, so it includes Rust callbacks made from JS. |
| `arc-experiment.patch` | The `Rc` → `Arc` swap in `arena`, `compiler_data`, `compiler_reader`, `layered`, `name_index` and `shared`, used for the A/B in the design document (medians 23.07 s `Rc`, 23.13 s `Arc`). |
| `parse_probe.rs` | The parse harness. It links the `light_cljs` library and mimalloc, preloads every `.cljs`/`.cljc` under the roots given as arguments, and times parsing on 1, 2, 4 and 8 threads claiming one or four files at a time; one warm-up, seven samples, rotated order. Timing covers thread creation, parsing and joining. |
| `dag.bb` | The schedule models. It reads `NSPREP` and `NSPROBE` lines, or a `--stats-file` trace (light-cljs-331), builds the require DAG and prints critical paths and list schedules (critical-path-first) for the designs in the document. From a trace it also prints the exclusive stage totals, the demand edges, the macro host's share and the ready width of each drain. |
| `phases.bb` | Attributes self samples of one thread's `sample` call tree to the outermost phase marker and the innermost light-cljs module or V8 frame. |
| `jscallers2.bb` | Attributes V8 self samples to the light-cljs operation that entered JS (the frames above `v8_evaluator::invoke`). |
| `main-probe.tsv` | The probe run itself: the build's whole stderr, `NS*` lines followed by `/usr/bin/time -l` (23.76 s wall, 962 MB peak RSS under the probes). |
| `run.bb` | The portable driver that replaces the study's `run.sh`; see below. |
| `dag_test.bb` | Regression tests for `dag.bb` (light-cljs-372): Option U with root edges on `fixtures/root-edges.trace.tsv`, and `dag.bb`'s output on `main-probe.tsv` against `fixtures/main-probe.dag.txt`. Run `bb benchmarks/concurrency/dag_test.bb`. |
| `fixtures/` | A synthetic three-namespace trace whose root edges chain a top-level wait and a unit wait (its header comment in `dag_test.bb` gives the expected times), and the retained `dag.bb`'s output on `main-probe.tsv`. |

## The models assume serial task costs

`dag.bb` gives each namespace pass and each deferred unit exactly the time it
took in the serial probe run. It records no effect or demand graph, and it
leaves out transfer, publication, isolate start-up, JIT warm-up, memory
admission and contention. Its schedules are ideal lower bounds, not
predictions. Contention was measured separately by running 1, 4, 8 and 12
concurrent `app.render` compiles (1.2x at 4, 1.4x at 8, 1.75x at 12).

## Reproducing the schedule table

```sh
bb benchmarks/concurrency/dag.bb benchmarks/concurrency/main-probe.tsv
```

This runs in about 5 s and reproduces the table in the design document's
summary as it stands, with no change to the invocation. The lines that give
each row (rounded to 0.1 s in the document):

| Row in the document | `dag.bb` section | 4 | 8 | 12 workers |
| --- | --- | ---: | ---: | ---: |
| None (today): 22.6 s | `sum cost 22.56 s` | | | |
| A. Units only | `Option U` (W=4, 8, 12) | 6.06 s | 6.06 s | 6.06 s |
| B. Merged strictly in source order | `Model D (in-order commit) with P workers` | 5.47 s | 4.24 s | 4.23 s |
| C. Each pass starts when its requires are done | `Model C` (P=4, 8, 12) | 5.47 s | 2.74 s | 2.12 s |

The same run gives the other figures the document quotes: 922 namespaces,
13,361 units (15.86 s, largest 2.03 s), top-level passes 6.00 s (3.59 s in
V8), drains 16.53 s (10.03 s in V8), DAG depth 63 and a namespace-granularity
critical path of 2.82 s.

## Reading a trace instead of probes

Since light-cljs-331 the compiler records the same costs itself: `--stats`
keeps a trace of exclusive stage times, and `--stats-file PATH` writes it as
a tab-separated file, with no patch and no rebuild:

```sh
bb benchmarks/concurrency/run.bb OUTPUT-DIR -- --stats-file OUTPUT-DIR/app.main.trace.tsv
bb benchmarks/concurrency/dag.bb OUTPUT-DIR/app.main.trace.tsv [--workers 4,8,12]
```

The file starts with `light-cljs-trace 2` (1 before the `read` rows); each
row kind has a `#` header row naming its columns. Times are microseconds.

- `stage` rows: each stage's exclusive time, its V8 time and its macro host
  entries. Every moment of the analysis is in exactly one of `other`, `load`,
  `prepare-source`, `top-level`, `drain`, `append`, `index`, `release`,
  `warnings`, `unit` and `finish`, so they add up to `trace-wall`.
  `engine-outer` is V8 time at depth 0 as the probes took it (Rust called
  back from JavaScript included), then come the phases after analysis.
- `namespace` rows, by source index: name, bytes, requires and the exclusive
  times of its load, preparation, top-level pass (with its V8 share and the
  time it waited for the units it demanded), drain bookkeeping and warnings.
- `unit` rows, in the order analyses finished: ID, namespace, Var, line and
  column, exclusive time and V8 share, macro host entries, the time it waited
  for the units it demanded, macro calls and form nodes, whether it entered
  the macro host (itself or through a unit it demanded), its demander
  (`drain`, `top-level` or `unit`, and which), its demand depth and thread.
- `execute` rows: each first evaluation of a unit's compile-time root in
  the engine (`__lightCompileUnit`), with the innermost frame that demanded
  it and whether its JavaScript was already prepared.
- `read` rows (version 2, light-cljs-359): each distinct pair of a unit
  whose compile-time root or bundle the engine forced and the innermost
  frame it was forced in, ready roots included, in the order first noted.
  A root's first evaluation is a read too.

`dag.bb` rebuilds the probes' inclusive columns from it: a namespace's
top-level pass plus the units it demanded, its drain plus the units the drain
analyzed, and each drained unit with the units it demanded. On
`main-probe.tsv` its output is unchanged.

From a trace it also runs Option U (stage A) a second time with the root
edges as dependencies: an `execute` or `read` edge makes the drained unit
or top-level pass whose frame made it wait for the drained unit whose cost
includes the root's unit. Edges inside one task, and edges from frames
outside the model's tasks (drain bookkeeping, other stages), are dropped.
`dag_test.bb` checks these waits on a synthetic trace: 1.30 s at every W with
the edges, 0.80 s from W=2 without them.

A unit *entered the macro host* when its analysis made a V8 service call
other than the compiler frontier's transitions and the bridge check after
each entry, which every unit makes; QuickJS-only builds record no engine time
and no host entries. Native macros run in Rust and are not host entries; they
are counted in the unit's macro calls, so a unit with neither is free of macro
calls altogether.

## Producing new probe data: `run.bb`

```sh
bb benchmarks/concurrency/run.bb OUTPUT-DIR [--binary PATH] [--entry NS] \
   [--source-root DIR]... [--sample] [-- EXTRA-COMPILER-ARGS...]
```

It runs one cold build with `LIGHT_NS_PROBE=1` under `/usr/bin/time -l`, with
the flags and limits of `benchmarks/penpot.bb` and `--optimize none`. The
binary is `--binary`, else `LIGHT_CLJS_BINARY`, else `target/release/light-cljs`.
Without `--source-root` it uses the five Penpot port roots under
`research/penpot-light`; the entry defaults to `app.main`. It writes
`ENTRY.js` and `ENTRY.probe.tsv` (the same format as `main-probe.tsv`) into
OUTPUT-DIR, which belongs outside the repository, and prints the `dag.bb`
command to run next.

The driver applies no patch. Without the probes the build still runs and
times, but the `.tsv` has no `NSPROBE` lines and the driver says so. To build
an instrumented binary, in a scratch checkout or worktree:

```sh
git apply benchmarks/concurrency/instrumentation.patch
cp benchmarks/concurrency/conc_probe.rs src/conc_probe.rs
cargo build --release
cp target/release/light-cljs /some/where/light-cljs-probe   # immutable copy
git checkout -- src && rm src/conc_probe.rs
```

On 2026-10-17 both patches still applied cleanly (`git apply --check`) at
`4de3e84`, and the instrumented tree type-checked
(`cargo check --no-default-features`); later commits may need the hunks
re-located.

With `--sample`, the driver also attaches `sample PID 30 1` (30 s at 1 ms) to
the compiler as soon as it starts, writes the report to `ENTRY.sample.txt`
and the main thread's call tree to `ENTRY.main-thread-sample.txt`, the input
`phases.bb` and `jscallers2.bb` expect. The sampler attaches after loading
has started, so load time comes from the probes, not the sample. Sampling
works on an uninstrumented binary too. Each run first deletes the entry's
earlier sample files in OUTPUT-DIR, so a build the sampler missed never shows
an older run's call tree.

For the `Arc` A/B, build a second binary with `arc-experiment.patch` applied
and alternate it with the baseline through `LIGHT_CLJS_BINARY` and
`benchmarks/penpot.bb ... main-none`. For the parse harness, copy
`parse_probe.rs` to `examples/parse_probe.rs` and run
`cargo run --release --example parse_probe -- ROOT...` with the five port
roots, then delete the copy.

## SHA-256 identities

The first four match the table at the end of the design document; all eight
retained files were verified against the originals in
`research/concurrency-opus/` before copying. `dag.bb` has since been extended
to read traces (light-cljs-331); the hash is that of the retained version,
`git show f68c767:benchmarks/concurrency/dag.bb`, whose output on
`main-probe.tsv` the current version reproduces byte for byte.

| File | SHA-256 |
| --- | --- |
| `main-probe.tsv` | `be8dd1084a1dda6f1b50c1920876e9680c993ee401c97b1e0ef229646f933d7f` |
| `dag.bb` | `0468372d10e42761fc37eedd7ee7ea910e9572663380ef167cfe8f827dbfcf52` |
| `instrumentation.patch` | `6b9bbf3debed7b810510acf8f64dcda95ae74bca47a6bf9d8d569f47915b90b8` |
| `arc-experiment.patch` | `1494961534a815b2db1547704166773a2703812e590448a7eb4c41c0c2c00399` |
| `conc_probe.rs` | `a1bb22cc8467d67455be6c86009b176b29474b7fcc3a7c1c5bc68a29276fd6a6` |
| `parse_probe.rs` | `4549e37a22f383c11b13e409e5b24bfa3cb955a91c20e62df242aba0e48294e6` |
| `phases.bb` | `f9e16930960deb22412248c38acd62bf71fa069e6ea57b1b88a4faf0bcba5f5f` |
| `jscallers2.bb` | `3f9650b808638b75414e7ab1d20074e7e5e5823471dca9daa064450845b1dfdf` |

Not retained, and still only in the main checkout's
`/Users/admin/src/light-cljs/research/concurrency-opus/`:

| File | SHA-256 | Why |
| --- | --- | --- |
| `main-thread-sample.txt.gz` (3.2 MB) | `fec42039bd75b5fff003ea2e53cd3dc9adda5145f835c90d3b91beac1de193f2` | The 21,501-sample main-thread call tree behind "Where the time goes"; too large to commit. |
| `run.sh` | `cd08d9dc14b079e939704ffb5296154c58fcd519dd60966f2a2340fcaa0e525b` | Embedded a temporary binary path; replaced by `run.bb`. |
| `all-experiments.patch` | `6872ef9f4b81672f9b192ff67f3b31fb1cce280e5af45af202e6588d0695b338` | Both patches above in one diff (same files). |

Check the retained copies with `shasum -a 256 benchmarks/concurrency/*`.
