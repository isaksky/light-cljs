# Three real native macro fixtures

`bb benchmarks/native-macros.bb OUTPUT` builds the explicit native-only release
compiler with `cargo build --locked --release --no-default-features --features
bbp-macros --target HOST --target-dir REPO/target` and three build jobs, then starts
a fresh compiler process for every sample. `HOST` comes from `rustc -vV`; the
explicit target and directory override Cargo environment/configuration defaults.
The runner executes `REPO/target/HOST/release/light-cljs` and records that path,
target, command and binary hash together. Requires Babashka, Cargo/Rust, Node
(`NODE` overrides it), and macOS or Linux `/usr/bin/time`. It never enables another
macro backend as a fallback.

```sh
# One smoke attempt per fixture/job count, including unsupported cells:
bb benchmarks/native-macros.bb target/native-macro-smoke --smoke
bb benchmarks/native-macros-test.bb target/native-macro-smoke/report.json

# Five measured runs per supported cell; use an otherwise idle machine:
bb benchmarks/native-macros.bb target/native-macro-matrix
# Restrict jobs or change the external per-process deadline:
bb benchmarks/native-macros.bb target/native-macro-serial --jobs 1 --timeout-ms 120000

# Runner validation, including a temporary Cargo fixture and reaped timeout:
bb benchmarks/native-macros-test.bb
cljfmt check benchmarks/native-macros.bb benchmarks/native-macros-test.bb
clj-kondo --lint benchmarks/native-macros.bb benchmarks/native-macros-test.bb
```

Output must be a new directory. Default jobs are 1/2/4/8, in ascending order on
odd rounds and descending order on even rounds. Each fixture gets five attempts
per supported cell; the first failure stops that cell and preserves its full
stderr. Smoke mode runs once and never marks its summaries performance eligible.
The compiler cache is disabled (`--no-cache`); there are no warmups, and OS file
caches are not flushed. Macro limits are fixed in the recorded command (60 s,
100 million polls, other CLI defaults); the external timeout defaults to 120 s
for both compiler and Node. Timed-out process trees are forcibly killed and
reaped. Node validation and the release build are outside compiler wall timing.

Sources are checked in under `fixtures/native-macros/native_bench/`. Their seeds
come from `tests/bbp_macros.rs` (finite sequences and metadata/environment tests),
not synthetic JavaScript engine kernels. Sizes are UTF-8 bytes; native calls
exclude the compiler's Rust bootstrap macro calls.

| Fixture | Bytes | Native calls | Expected stdout |
| --- | ---: | ---: | --- |
| collections | 575 | 13 | `(1 2) ([:a 1]) ("a" "b")`; four `true`; five `true`; `2`, on four lines |
| environment | 510 | 16 | Four `native-bench.environment` values on each of two lines; `1 1 1 1`; `42 42`; `42 42` |
| helper | 308 | 1 attempted | `[42 1]` plus actual park/resume/no-replay evidence |

The exact expected strings, source hashes and sizes are included in each report.
The helper macro increments a compile-time effect counter before demanding the
ordinary `demanded` function, then returns its result and the counter. It is never
preloaded. Its current first failure is `unsupported operation: helper not
preloaded: native-bench.helper/demanded`, from the actual CLI call. This is the
minimal reproducer for existing **light-cljs-463**, not a new duplicate follow-up.
Even if a future backend returns `[42 1]`, the runner labels it
`unverified-helper` and excludes it from successful samples until compiler
instrumentation establishes actual task suspension, resumption and no replay.
A successful application result alone does not establish that contract.

The current native backend rejects jobs 2/4/8 with `unsupported operation: native
macro compilation requires --jobs 1`. These are attempted and reported as
unsupported, never counted as serial scaling. The runner does not change engine
semantics, defaults or scheduling. A smoke on the native-only release compiler
at base `6acf5899` passed collections/environment at jobs 1 and reproduced the
helper and parallel-job failures. Those smoke durations are not performance
results; the repeated measurement is separate work (light-cljs-484).

`raw.jsonl` contains samples in invocation order with provisional statuses.
`report.json` is authoritative: it validates expected Node stdout and exact
JavaScript/source-map SHA-256 identity across repeats, job counts and optional
backends. The fixed map location within an invocation avoids output-directory
artifacts in those comparisons. Any identity disagreement invalidates **every**
successful sample for that fixture. Each sample keeps generated JS, map, stderr,
and the complete `--stats-file` trace. Summaries report median, min, max and
max-minus-min spread only for validated samples, and require five successes for
`performance-eligible`. Unexpected compilation/result/identity failures or
external timeouts exit nonzero; diagnosed unsupported/unverified cells remain
visible in a completed report. Never use raw provisional status as a timing gate.

Reports record revision, dirty diff/hash, runner and fixture hashes, binary hash,
Cargo manifest/lock hashes, release command, relevant build environment,
toolchains, machine/CPU count, per-sample load, exact commands, run order and
cache/timeout policy. Compiler wall time includes process launch and stats;
`time(1)` supplies user CPU and peak RSS (macOS bytes, Linux KiB converted to
bytes). Bridge nanoseconds are summed from `macro.bridge.N.ns`; all original
stats remain available. Runtime initialization, separate JS compilation and
execution, allocated bytes, GC counts/pauses, parked macro tasks, help depth and
worker utilization are explicitly unavailable when the CLI lacks a suitable
counter. In particular, worker `pool.parks` is not proof of macro suspension,
zero legacy engine trace counters are not zero native execution time, and live
heap bytes are not cumulative allocation. No new instrumentation is added here.

An **already available**, separately stored V8 compiler may optionally be used:

```sh
bb benchmarks/native-macros.bb target/native-macro-reference \
  --v8-bin /absolute/path/to/existing-v8-light-cljs \
  --v8-revision FULL_CURRENT_GIT_HEAD
```

The revision flag is the caller's attestation of that binary's source and V8
backend; the runner records that limitation and its binary hash, and neither
builds nor distributes V8. Use a clean checkout and a reference with matching
release build settings. Backend order alternates within rounds as well as across
rounds. Raw trace unit IDs and namespace/name/line/column identities are preserved
for matching the same fixture and commit; inspect demander edges in the full TSV
if identities are ambiguous. Current top-level native macro fixtures emit no
`unit` rows, so a per-unit V8 ratio is unavailable, even if whole-process timings
exist. No V8 reference is needed for the smoke or claimed by the implementation
verification. No performance ratio is computed automatically.
