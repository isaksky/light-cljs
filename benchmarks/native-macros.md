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

## Native core lowering (light-cljs-477), October 26, 2026

`bb benchmarks/native-macros.bb` (five runs per cell, jobs 1/2/4/8) ran on
master `21f68ddb` (before) and on the lowering branch `dd619ba8` (after), one
after the other on the machine of `native-macros-6405a52c/README.md`, with one
peer worker active (load averages about 2.6–3.4). Medians of compiler wall time,
ms [min–max]:

| Fixture, jobs | Before | After |
| --- | --- | --- |
| collections, 1 | 18.15 [17.99–26.22] | 18.63 [18.00–23.19] |
| collections, 8 | 19.73 [18.32–20.32] | 18.96 [18.55–19.02] |
| environment, 1 | 18.20 [18.02–24.09] | 18.34 [17.26–18.61] |
| environment, 8 | 18.96 [18.71–19.29] | 19.70 [18.97–19.87] |

Jobs 2 and 4 are within the same 18.4–18.9 ms band on both. Every cell is
dominated by process start; the differences are within the run-to-run spread,
so these fixtures show neither a gain nor a loss from lowering. They cannot
give the per-thread factor k: no V8 figures were recorded for them (the
6405a52c baseline had no qualified V8 binary), they emit no `unit` rows, and
their macro work is far below process start. k is measured on the
collection-heavy macro below instead ("Per-thread factor k").

Helper publication from `--stats` on the helper fixture, at every job count:
before `native.helpers.published 0`, `per-task 1`; after `published 1`,
`per-task 0` (the runner still labels the cell `unverified-helper`). The
largest workload this backend supports is still these fixtures and the
`tests/bbp_macros.rs` namespaces: the seeded generated project (20k lines,
seed 42) now passes native sets and `vec` but stops at lazy `map`
(`core.mapSeq`, light-cljs-479). In `tests/fixtures/native-helper-demand`,
`pool` goes from 0 published / at least 1 per-task to 1 / 0, and the new
`collections` namespace (a set constant and two collection helpers) publishes
3 of 3 at one and two slots.

A supplementary, synthetic collection-heavy macro (not one of the fixed
fixtures, and not a k figure) shows the lowering where macro work is not
hidden by process start. Four calls of

```clojure
(defmacro churn [n]
  (loop [i 0 v [] m {}]
    (if (= i n)
      (loop [s (seq v) total 0]
        (if s
          (recur (next s) (+ total (first s) (get m 1 0) (count (rest [1 2 3]))))
          total))
      (recur (+ i 1) (conj v i) (assoc m (count v) (count m))))))
```

with `n` 400 (`--jobs 1 --no-cache`, five alternating fresh processes of the
same two release binaries) took a median 1,947.5 ms [1,938.2–1,949.7] before
and 167.3 ms [165.5–167.6] after, 11.6x less, with identical output. `next`
still copies the remaining elements, so the loop stays quadratic.

### Per-thread factor k

k is the engine's per-thread macro time as a multiple of V8's on the same
macro work. `bb benchmarks/native-macros-k.bb OUTPUT v8=BIN NAME=BIN ...`
compiles the `churn` source above (namespace `k.churn`, four calls of
`(churn n)`) as fresh `--jobs 1 --no-cache --optimize none` processes, one
discarded warm-up per backend and `n`, then five rounds alternating the backend
order, checks that Node prints the same line for every backend, and takes the
macro time from `time.analysis-ms` (analysis with macro expansion). k at `n` is
a backend's median analysis time at `n` minus at `n = 0`, over V8's: process
and engine start and compiling the macro definition cancel out, and only the
`churn` calls remain.

Run on October 26, 2026 at `d2f9b7cb` (this pellet's review fixes; "after")
with release binaries from the same tree: native-only (`--no-default-features
--features bbp-macros`, sha256 `120940b1…`) and the default V8 build
(`LIGHT_CLJS_V8_CACHE=require`, the shared cache's V8, sha256 `6cada940…`);
"before" is the native-only build of master `86d0616b` (sha256 `24a96423…`).
macOS aarch64, 24 CPUs, Rust 1.99.1 (Homebrew), Node 24.22.1; one peer worker's
test binary used two cores (load averages 5.2/3.9/3.3). Analysis time, median
[min–max] of five, ms:

| `n` | V8 | before | after | k before | k after |
| ---: | --- | --- | --- | ---: | ---: |
| 0 | 24.3 [23.9–25.0] | 7.9 [7.1–8.6] | 8.0 [7.9–8.3] | – | – |
| 100 | 29.4 [29.1–29.5] | 112.4 [111.4–113.2] | 25.1 [24.6–25.2] | 20.5 | 3.4 |
| 400 | 36.2 [34.6–37.0] | 1,929.7 [1,924.6–1,943.0] | 160.9 [160.1–161.1] | 161.5 | 12.8 |

At `n = 0` the engine finishes analysis in a third of V8's time (no isolate or
snapshot to start). After lowering, k is 3.4 at `n = 100`, inside the strategy's
win condition (k <= ~4), and 12.8 at `n = 400`, outside it: the engine's macro
time grows 8.9x from `n = 100` to `n = 400`, V8's 2.4x. The V8 deltas are 5.1 and
11.9 ms, so the k figures carry a few percent of timing resolution.

The growth is the automatic collector, not the lowered operations. A
`/usr/bin/sample` of the native compiler on `n = 3200` (1,464 samples) has
1,216 (83%) under `RunGuard::collect_at_point` → `Heap::mark_and_sweep`, whose
top frames are SipHash hashing of `TraceEdge` keys in the mark set's
`HashMap` (`core::hash::sip::Hasher::write` 454, `BuildHasher::hash_one` 267,
`HashMap::contains_key` 191, `mark_and_sweep` itself 84) and tracing the live
`PMap`/`PVector` tries. Each collection marks everything live (the growing
vector and map), and the copies `next` and `rest` make keep triggering
collections, so marking work grows with `n²`. light-cljs-477 leaves this to a
follow-up on the collector's marking cost.
