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
(`core.mapSeq`, light-cljs-479). With native lazy sequences (light-cljs-479)
the 20k and 200k projects (seed 42) at `--jobs 1` get past
`workload.fonam/buvaiskum` and stop at `workload/trildorplus.cljc:6:1`: the reader-namespace
registration unit (21,246 code units by then in the 200k project) exceeds
the 16,384-unit eval limit (light-cljs-505). In `tests/fixtures/native-helper-demand`,
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
collections, so marking work grows with `n²`; light-cljs-495 owns the
collector's marking cost.

#### Proportionate collection (light-cljs-495)

Marking no longer hashes: objects use their slot mark bits, property keys a
bitset, symbols an identity-hashed set, and code shared by many closures is
walked once per collection. Byte-triggered collections are now due after half
the remaining headroom rather than half the live bytes: each persistent
collection version is charged as a complete representation (`PMap` 4 KiB an
entry), so at n = 400 the macros admit about 1.4 GB against the 64 MiB
default and the old trigger collected 444 times (46 now).

October 26, 2026, same script and flags, `before` = master `fe510a3a`
(native-only), `after` = this change (native-only), V8 from the shared cache
(`LIGHT_CLJS_V8_CACHE=require`); macOS aarch64, Rust 1.99.1; two peer
`rustc` builds running (load 4.67 before, 4.70 after). Analysis ms, median
[min–max] of five:

| `n` | V8 | before | after | k before | k after |
| ---: | --- | --- | --- | ---: | ---: |
| 0 | 24.0 [23.4–24.9] | 8.2 [8.1–8.3] | 6.9 [6.7–7.0] | – | – |
| 100 | 29.9 [29.4–30.6] | 25.7 [24.9–26.2] | 10.6 [10.2–11.6] | 3.0 | 0.6 |
| 400 | 36.7 [36.0–37.3] | 163.7 [163.2–166.3] | 26.1 [24.8–26.8] | 12.2 | 1.5 |

k at n = 400 is 1.5 (477: 12.8). The engine's macro time grows 5.2x from
n = 100 to 400 (3.7 to 19.2 ms; 4.7x in an earlier, less loaded run of
the same binaries), V8's 2.1x; the rest is the lowered `next` copy.

Not met: at n = 3200 a `/usr/bin/sample` (242 samples, same load) still has
141 (58%) under `RunGuard::collect_at_point`. Top frames: `PMap` trie walks
from `FormObject::trace`, `mark_and_sweep` itself, `Code`/`FunctionObject`
and realm intrinsic tracing (the fixed live set, ~100 µs a pause), and dropping
garbage vector nodes. The collection count is set by the per-version charges:
at n = 3200 one map version is charged 13 MB, so the 64 MiB budget forces a
collection every couple of `assoc`s (3,355 collections). Charging persistent
versions by their path copy is light-cljs-498.

#### Path-copy charges (light-cljs-498)

Each persistent map and vector trie node now carries the byte charge for its
own allocation, taken before the node is built, instead of every version
charging a complete representation (`PMap` 4 KiB an entry). An update is
charged for the nodes it copies, and a node shared by several versions stays
charged until the last of them is freed; a form's shared body is charged with
its slot. At n = 400 one `churn` call admits about 4.6 MB instead of about
350 MB, and each macro call sees at most one collection.

October 26, 2026, same script and flags (five rounds), `before` = master
`2ea9cb19` (native-only, sha256 `65fa91c2…`), `after` = this change on it
(native-only, sha256 `ceadb8b5…`), V8 from the shared cache
(`LIGHT_CLJS_V8_CACHE=require`, sha256 `1559a48e…`); macOS aarch64, 24 CPUs,
Rust 1.99.1, Node 24.22.1; a peer worker's `rustc` build running (load
averages 3.5/3.9/4.4). Analysis ms, median [min–max] of five:

| `n` | V8 | before | after | k before | k after |
| ---: | --- | --- | --- | ---: | ---: |
| 0 | 23.9 [23.8–24.4] | 10.4 [10.0–11.5] | 8.9 [8.3–9.0] | – | – |
| 100 | 29.3 [28.4–29.9] | 28.1 [26.6–28.3] | 12.0 [11.9–12.4] | 3.3 | 0.6 |
| 400 | 36.4 [35.8–36.7] | 164.4 [163.8–166.1] | 25.4 [25.1–26.0] | 12.3 | 1.3 |

k at n = 400 is 1.3 (477: 12.8; 495: 1.5). The engine's macro time grows
5.3x from n = 100 to 400 (3.1 to 16.5 ms; 5.0x and 5.4x in earlier runs of
this change on `fe510a3a` at loads 3 to 4.3), V8's 2.3x. Collection is not
the growth any more: at n = 400 the four calls make one collection between
them (1.1 ms). A variant walking `v` by index instead of `next` grows 4.5x
(3.9 to 17.6 ms, on `fe510a3a`), so the rest is the lowered operations:
`next` copies the remaining elements (`clj::tail`, quadratic over the walk),
and map path copies widen as second-level HAMT nodes fill.

At n = 3200 a native compile takes about 280 ms (master: 2,557 ms) and three
`/usr/bin/sample` runs have 18 of 122, 19 of 157 and 20 of 161 samples (13%)
under `RunGuard::collect_at_point` (master: 1,669 of 1,847, 90%), from three
or four collections per macro call. Top frames of the third run (inclusive
samples): `clj::tail` (39) with `PVector::to_vec` (20) and
`try_from_values` (30) building the `next` copy, dropping garbage vector
nodes (`Node::drop_slow` 40) and `FormObject` bodies (14), and
`mark_and_sweep` (19). Sharing the trie in `next` and `rest` instead of
copying the remainder is light-cljs-502.

#### Shared tails (light-cljs-502)

`seq`, `rest` and `next` of a native vector or list now return a list over the
same trie with a hidden prefix (`PVector::drop_first`): O(1), one list object
and no trie node, so the `(loop [s (seq v)] … (recur (next s)))` walk is linear.

October 26, 2026, same script and flags (five rounds), `before` = master
`2d5ba2d9` (native-only, sha256 `454b7d82…`), `after` = this change on it
(native-only, sha256 `0222c7b3…`), V8 from the shared cache
(`LIGHT_CLJS_V8_CACHE=require`, sha256 `de4939d0…`); macOS aarch64, 24 CPUs,
Rust 1.99.1, Node 24.22.1; peers mostly idle, one short `rustc` at the end
(load averages 3.1–3.2 falling from 4.5). Analysis ms, median [min–max] of
five:

| `n` | V8 | before | after | k before | k after |
| ---: | --- | --- | --- | ---: | ---: |
| 0 | 24.3 [23.9–25.4] | 9.0 [8.9–9.3] | 9.0 [8.5–9.6] | – | – |
| 100 | 29.4 [28.9–32.4] | 12.3 [12.0–13.2] | 12.0 [11.8–12.4] | 0.6 | 0.6 |
| 400 | 36.1 [35.7–36.7] | 25.7 [25.3–26.5] | 23.1 [22.5–23.2] | 1.4 | 1.2 |

k at n = 400 is 1.2. The engine's macro time grows 4.7x from n = 100 to 400
(3.0 to 14.1 ms; before 5.1x, V8 2.3x; an earlier run at load 4.2 gave 4.8x
and 5.1x), short of the ~4.5x aimed for. Part of that step is a threshold:
at n = 400 the four calls make one collection (1.35 ms) and at n = 100 none;
without it the growth is 4.3x. Past n = 400 the time is linear (three rounds,
same binaries): 13.4, 50.4 and 103.6 ms of macro time at n = 400, 1,600 and
3,200 (3.8x for 4x, then 2.1x for 2x; 3 and 7 collections), against V8's
12.2, 31.5 and 56.2 ms (k 1.1, 1.6, 1.8). What remains of the 100-to-400
step is not the walk; the light-cljs-498 analysis above attributes it to map
path copies widening as second-level HAMT nodes fill (not re-measured here).

At n = 3200 with sixteen `churn` calls (to get enough samples; 480 ms
analysis, before: 1,071 ms), two `/usr/bin/sample` runs of 303 and 301
samples have `clj::tail` in 3 and 4 (inclusive; before: 203 of 735, with
`PVector::to_vec` 130 and `try_from_values` 91) and no `to_vec` or
`try_from_values` under it. The top frames are now the interpreter
(`execute_budgeted` 55 and 48 self), `memmove`, `Fault` drop glue, HAMT
`Entry` copies and drops, and `mark_and_sweep` (26 and 37 inclusive).

#### Boxed sweep (light-cljs-504)

The rest of the n = 100 to 400 step was not map path copies. Variants of
`churn` (interquartile mean of 31 interleaved samples per cell, master
`032d00af`, load averages 3.5) grow about 4x without a collection and more
than 4x only with one. Ratios of macro time from n = 100 to 400:

| variant | master | this change | no collection |
| --- | ---: | ---: | ---: |
| `full` (the k workload) | 4.74x | 4.41x | 3.95x |
| `index`: walk `v` with `nth` | 4.43x | 4.27x | 4.09x |
| `maponly`: `assoc` only, no `conj` | 4.44x | 4.26x | 3.99x |
| `nomap`: no `assoc` | 4.06x | 4.19x | 3.94x |

"No collection" is master built with an allocation threshold of 2^22 (a
probe, not a proposal). With equal total work the cost per element does not
grow with n: 64 calls at n = 100, 16 at 400 and 4 at 1,600 analyse in 60.9,
58.8 and 59.7 ms on master. The step is a threshold: the four n = 400 calls
cross the 16,384-allocation trigger once, in the fourth call, and n = 100
never does (`nomap` allocates one object less per element and stays under).

That one collection cost more than its share because the sweep moved each
garbage `HeapObject` (304 bytes) out of its box into one growing `Vec`: about
5 MiB copied and reallocated per pause, a fresh huge-page allocation that
mimalloc then purged. A run at n = 400 peaked at 36 MiB RSS and 50.5 MiB
committed against 26.6 and 29.5 MiB without the collection, with 2,640 page
reclaims against 2,014. `ObjectTable::sweep` now returns the boxes; the bodies
are freed with them after the pause, as their contents already were. The
first pause falls from 1,168 to 285 µs, and the run peaks at 27.0 MiB RSS and
33.7 MiB committed (2,026 reclaims).

October 26, 2026, `bb benchmarks/native-macros-k.bb OUT --runs 15 --n
0,100,400 v8=… before=… after=…`, `before` = master `032d00af` (native-only,
sha256 `a37ee1ea…`), `after` = this change on it (native-only, sha256
`2431e3e2…`), V8 from the shared cache (`LIGHT_CLJS_V8_CACHE=require`,
sha256 `ef33476f…`); macOS aarch64, 24 CPUs, Rust 1.99.1, Node 24.22.1; peer
workers mostly between builds and tests (load averages 3.6–3.9). Analysis ms,
median [min–max] of fifteen, two runs:

| `n` | V8 | before | after | k before | k after |
| ---: | --- | --- | --- | ---: | ---: |
| 0 | 23.2 [22.3–24.9] | 8.5 [8.2–9.0] | 8.6 [8.3–9.9] | – | – |
| 100 | 28.8 [27.2–31.2] | 11.4 [10.9–15.3] | 11.6 [11.1–13.5] | 0.5 | 0.5 |
| 400 | 34.8 [33.9–37.6] | 22.2 [21.1–24.1] | 21.2 [20.6–22.9] | 1.2 | 1.1 |
| 0 | 24.0 [23.7–26.3] | 8.9 [8.6–9.5] | 9.0 [8.7–9.6] | – | – |
| 100 | 29.1 [28.6–30.6] | 11.9 [11.5–12.5] | 12.0 [11.4–12.4] | 0.6 | 0.6 |
| 400 | 36.1 [35.3–37.5] | 23.0 [22.1–23.8] | 22.0 [21.1–22.9] | 1.2 | 1.1 |

k at n = 400 is 1.1. The engine's macro time grows 4.2x and 4.3x from n = 100
to 400 (3.0 to 12.6 and 13.0 ms; before 4.7x and 4.7x; V8 2.1x and 2.4x).
The n = 100 delta is about 3 ms at 0.1 ms resolution, so five-round medians
move the ratio by several tenths: two five-round runs of the same binaries
gave 4.6x and 4.7x (before 4.6x and 5.3x). What is left above 4x is the one
collection's work after the pause, freeing its garbage.

Profiles of 400 `(churn 400)` calls (1.6 s on master, 1.39 s here) and 50
`(churn 3200)` calls (1.57 s, 1.50 s): automatic collection falls from 17%
to 10% and from 13% to 11% of the samples, and its `memmove` from 28 samples
to none. In the n = 400 profile a collection is now mostly freeing garbage
(50 of 91 samples: form bodies 35, map trie nodes 22, vector nodes 10) and
marking (20). Map `assoc` with its path copy is 35 of 938 samples and
dropping map nodes 19 (n = 3200: 42 and 31 of 1,002); the top frames are the
interpreter (`execute_budgeted` 163 and 190 self), `Fault` drop glue,
`Value` drop glue, the slot directory and `memmove`. A cheaper map node
layout would not change the root's 32 child references a path copy takes and
a dropped version releases.

## Protocols and types (light-cljs-478), October 26, 2026

`bb benchmarks/native-macros-k.bb OUTPUT --workload protocol v8=BIN NAME=BIN`
measures k the same way on a protocol-heavy macro: `churn` builds `n` values
cycling through a two-field record, a one-field record, an immutable deftype and
a number, calls the protocol methods `scale` (record `assoc`, `->Circle`,
`(Pt. …)`, number multiplication) and `area` on each, and adds `satisfies?` and
the record's `count`. Before this pellet the native backend could not run it
(`unavailable definition` for the protocol); master's native build has no
"before" figure.

Release binaries of this branch (`b8d1db4f`): native-only (`--no-default-features
--features bbp-macros`, sha256 `6478d3d3…`) and the default V8 build
(`LIGHT_CLJS_V8_CACHE=require`, sha256 `e7bc880d…`, V8 path unchanged by this
pellet). macOS aarch64, 24 CPUs, Rust 1.99.1 (Homebrew), Node 24.22.1; two peer
workers building and testing (load averages 3.9–4.2). Analysis time, median
[min–max] of five, ms:

| `n` | V8 | native | k |
| ---: | --- | --- | ---: |
| 0 | 24.8 [24.3–26.6] | 11.3 [10.8–12.2] | – |
| 100 | 28.7 [27.9–29.7] | 43.4 [42.6–44.5] | 8.2 |
| 400 | 32.3 [32.2–32.6] | 73.4 [72.8–74.0] | 8.3 |
| 1,600 | 40.9 [40.1–41.0] | 188.6 [186.2–191.1] | 10.8 |
| 6,400 | 65.9 [65.4–67.6] | 653.4 [648.8–659.4] | 15.5 |

(The 1,600 and 6,400 rows are a second run, whose `n = 0` medians were 24.4
and 11.1 ms.) The V8 deltas at `n = 100` and 400 are 3.9 and 7.5 ms, so those k
figures carry about a tenth of timing resolution. k is 8 to 15, outside the
strategy's win condition (k <= ~4). The collection-heavy `churn` workload is
unchanged by this pellet on the same binaries: k 3.4 at `n = 100` and 13.2 at
`n = 400` (12.8 recorded above; light-cljs-495's GC marking work is not in this
tree).

A `/usr/bin/sample` of twelve `(churn 6400)` calls (735 samples) puts about 9%
in automatic collection and the rest in the interpreter: the top frames are
`execute_budgeted` (171), `Fault` drop glue (40), the heap slot directory (32),
SipHash (26), `property_read_step` (22), `start_conversion` (18) and the
`call_slow`/`resolve` path of `Function.prototype.call`. Protocol dispatch
itself is one `__rt.cljDispatch` per call; around it the macro still reads
`rt.core.*` properties and calls `core.add`/`core.multiply` through
`Array.prototype.reduce` (arithmetic is not lowered in native units), and each
method call goes through `m.call(x, x, …)`.

Re-measured after the review fixes (`e0dcf363`, which change `Object` methods,
`implements?` and methodless deftypes, none of which the workload uses): the
native-only release binary (sha256 `cf85def7…`) and the default V8 release
binary built at `f294deff` (sha256 `33ea3cf4…`; the delta to `e0dcf363` touches
only native-unit code), one run of five per `n`, load averages 3.3–4.8 with two
peer workers. Analysis time, median [min–max], ms:

| `n` | V8 | native | k |
| ---: | --- | --- | ---: |
| 0 | 24.2 [23.2–24.9] | 12.1 [11.4–12.3] | – |
| 100 | 28.5 [27.9–29.9] | 46.6 [43.5–47.5] | 8.0 |
| 400 | 32.2 [31.2–33.0] | 77.2 [74.0–78.7] | 8.1 |
| 1,600 | 40.6 [39.4–43.5] | 204.1 [198.0–207.5] | 11.7 |
| 6,400 | 66.1 [63.4–69.1] | 705.4 [691.0–717.3] | 16.5 |

The same `/usr/bin/sample` profile (twelve `(churn 6400)` calls, 3 s) has the
same shape: `execute_budgeted` 271 samples at the top of the stack, `Fault`
drop glue 71, SipHash 56, `lookup_cached` 40, the heap slot directory 37,
`Value` drop glue 34, `start_conversion` 32, `property_read_step` 32,
`enter_constructor` 30 and `cached_callee` 27. The miss of k <= ~4 is
light-cljs-501: lower arithmetic and core calls in native units and call
protocol implementations without `Function.prototype.call`.

## Native protocol call lowering (light-cljs-501), October 26, 2026

The unchanged protocol workload above now lowers fixed arithmetic and comparison
calls to operators over unwrapped occurrences, binds a unit's core functions
once, and calls eligible protocol implementations through an explicit-receiver
entry. Wider arithmetic evaluates all operands before coercion, without rest
arrays or reducer callbacks. Scope-sensitive methods retain their receiver
adapter. `PROTOCOL_FIXTURES` is unchanged; all 76 `bbp_macros` tests pass with
native plus QuickJS, including new coercion/order and receiver/recur checks.
All five helpers in the existing lazy publication test now publish, versus
three before (lowered arithmetic and cached pure predicates remove their
remaining runtime-object captures).

Native-only release binaries: before = current master `26512b2f` (sha256
`5a7dcc6cd42d60ea2dae3bfb0a28bc596ad51605a846288cde8311c93ee13ed1`),
after = `c0eed2b4` (sha256
`f3e555dcab65b56f5099e1d5411918465e342885eba26e0723b8d662848df7fd`).
Both use `cargo build --locked --release --no-default-features --features
bbp-macros`. macOS aarch64, 24 CPUs, Homebrew Rust/Cargo 1.99.1, Node
26.12.1. One peer worker; own builds and suites finished before timing,
load average 5.18 at the start and 2.84 shortly after. One warm-up per binary
and n, then five rounds alternating binary order, fresh `--jobs 1 --no-cache
--optimize none --stats` processes, the original timeout/poll budgets, and
identical Node output at every n.

The sandbox cannot read the V8 machine cache. **No V8 binary was built or
timed here.** The V8 column is the most recent recorded protocol medians above
(light-cljs-478 at `e0dcf363`, reference binary `f294deff`); every k below is
therefore a **historical-reference estimate**, not a same-run V8 comparison.
The harness accepts this explicitly:

```sh
bb benchmarks/native-macros-k.bb target/protocol-interleaved \
  --workload protocol --runs 5 --n 0,100,400,1600,6400 \
  --v8-analysis-ms 0=24.2,100=28.5,400=32.2,1600=40.6,6400=66.1 \
  before=BEFORE_BINARY native=AFTER_BINARY
```

Analysis time, median [min–max] of five, ms:

| `n` | recorded V8 | before | after | estimated k before | estimated k after |
| ---: | ---: | --- | --- | ---: | ---: |
| 0 | 24.2 | 15.1 [14.7–15.3] | 15.7 [15.6–16.0] | – | – |
| 100 | 28.5 | 54.2 [53.7–54.7] | 39.6 [39.2–40.2] | 9.1 | 5.6 |
| 400 | 32.2 | 128.9 [127.8–129.6] | 70.6 [70.6–73.0] | 14.2 | 6.9 |
| 1,600 | 40.6 | 428.3 [425.4–428.9] | 192.1 [190.6–192.3] | 25.2 | 10.8 |
| 6,400 | 66.1 | 1,623.1 [1,607.9–1,628.6] | 677.6 [674.3–687.8] | 38.4 | 15.8 |

Baseline-subtracted native analysis falls by 39%, 52%, 57% and 59%. An initial
pre-edit run of the same master binary gave estimated k 9.3/14.5/25.6/39.7;
the table uses the later interleaved pair. The estimate remains above the
k <= ~4 target at every measured n; light-cljs-515 continues the residual
protocol call/resolve optimization.

Fresh `/usr/bin/sample PID 3 1 -file ...` attempts at n = 1,600 (48 calls)
and 6,400 (12 calls) both exited 255: process inspection is sandbox-denied
(`sample cannot examine process ...; try running with sudo`). No fresh CPU
frame ranking can be claimed. The last available CPU profile is the historical
478 profile above: `execute_budgeted` 271 top-of-stack samples, `Fault` drop
glue 71, SipHash 56, `lookup_cached` 40, heap slot-directory access 37,
`Value` drop glue 34, `start_conversion` and `property_read_step` 32 each,
`enter_constructor` 30 and `cached_callee` 27. Those counts describe that
earlier code and load, not this change's remaining cost.

Static inspection identifies a remaining interpreter path to profile:
calls of dynamically read helper/protocol dispatcher values still pass through
`NATIVE_PREPARE_INVOKE` (`value.call`, a per-call closure and `Reflect.apply`
with an argument array). Dispatch still runs `declare`/`resolve` and JavaScript
table-version/frontier checks before `cljDispatch`. This is a candidate,
not a measured attribution; a fresh profile at both sizes remains required.

Validation on the implementation above: native-only `bbp_macros` 74/74;
native plus QuickJS differential 76/76; engine `--all-targets` 2,112/2,112
and doctests 5/5. Compiler and engine formatting pass. The full QuickJS
nextest build stops at eight missing `research/clojurescript` include files;
`research/` was left untouched as instructed. Selecting the other binaries
with `--no-fail-fast -j 8` runs 824 tests: 822 pass, while the playground wasm
check fails nested-worktree Cargo workspace discovery and the watch CPU-idle
test's `ps` spawn is sandbox-denied. Those test/build paths are unchanged
from `26512b2f`.

Strict native Clippy stops at eight engine diagnostics in the existing
`deprecated`, `chunks_exact_to_as_chunks` and `byte_char_slices` categories
(engine source unchanged). Native plus QuickJS `--lib --bins` passes with
only those categories allowed; engine-free `--lib --bins` passes strictly.
Engine-free `--all-targets` stops at the unchanged evaluator example's missing
`print`/`evaluate` methods. Default-feature Clippy with
`LIGHT_CLJS_V8_CACHE=require` stops at the sandbox-denied V8 cache path.
These source checks do not replace the unavailable full compiler suite,
strict all-target lint or fresh CPU profile.

### Fresh V8 builds and CPU profiles (light-cljs-501 continuation)

The continuation removes the remaining adapter closure and argument arrays from
one- and two-argument native calls inside functions. Frame locals retain the
callee, its early `.call` read and evaluated operands across suspension;
`__rt.call(receiver, callee, args...)` lowers to the engine's existing
`CallMethod` instruction. Callable collections retain their occurrence and
default argument, and custom `.call` methods retain their receiver. Protocol
declaration demand and table lookup now run in one resolver. Metadata-supplied
functions retain the legacy null receiver, including strict JavaScript
functions; a focused regression returned false on the earlier native binary
and true on QuickJS before this correction.

Fresh release binaries, macOS aarch64, 24 CPUs, Homebrew Rust/Cargo 1.99.1,
Node 26.12.1:

- Before: master `058404c6`, native-only, sha256 `dd1c8ee9fe11efc9…`.
- After: `47cb9998`, native-only, sha256 `3ae827258f98ad8a…`.
- V8: the same after commit `47cb9998`, default features with
  `LIGHT_CLJS_V8_CACHE=require`, sha256 `7c182bc920c78719…`.

Both native builds use `cargo build --locked --release --no-default-features
--features bbp-macros`. The baseline was built from master's tracked sources
inside this worktree, then those sources were restored before building both
after binaries. This base includes intrinsic-property/builtin caching and the
native catalog-retention fixes. **V8 is freshly built and measured here; these
k values do not use the historical V8 estimates above.**

```sh
bb benchmarks/native-macros-k.bb target/protocol-final-idle \
  --workload protocol --runs 5 --n 0,100,400,1600,6400 \
  v8=target/v8-final master=target/native-master-final native=target/native-final
```

The original workload and harness are unchanged: four calls per compilation,
one discarded warm-up per binary and n, five rounds alternating backend order,
fresh `--jobs 1 --no-cache --optimize none --stats` processes, identical Node
output. Own builds and suites finished before timing. The last run began after
peer build/test processes stopped; another worker's benchmark was visible at
its end. Earlier runs overlapped peer macro tests and a long compiler run.
Host load across the runs was 2.8–5.3. Analysis time, median [min–max], ms:

| `n` | fresh V8 | master | native after | k master | k after |
| ---: | --- | --- | --- | ---: | ---: |
| 0 | 25.0 [24.2–27.5] | 39.3 [39.1–45.9] | 43.0 [41.1–45.2] | – | – |
| 100 | 31.5 [28.4–33.5] | 86.9 [81.5–90.7] | 67.4 [62.5–72.6] | 7.3 | 3.8 |
| 400 | 36.7 [32.0–38.8] | 179.0 [165.7–186.9] | 92.0 [88.9–102.8] | 11.9 | 4.2 |
| 1,600 | 47.3 [40.3–47.9] | 546.2 [538.3–558.2] | 203.2 [190.6–214.8] | 22.7 | 7.2 |
| 6,400 | 67.7 [66.0–77.2] | 2011.9 [1999.0–2021.4] | 635.1 [626.3–653.6] | 46.2 | 13.9 |

Baseline-subtracted native analysis falls by 49%, 65%, 68% and 70%. Two earlier
five-round runs on the same binaries gave after k = 5.2/6.0/9.3/13.6 and
5.3/6.5/9.1/13.7. The small-n rows depend on short V8 deltas and are sensitive
to shared-machine variation. Every run misses k <= ~4 through n = 1,600;
light-cljs-515 is updated with this evidence, rather than creating a duplicate.

Fresh `/usr/bin/sample PID 3 1 -file ...` profiles completed for both native
binaries at n = 1,600 (256 calls) and 6,400 (64 calls). These repeat the same
kernel long enough to sample it; their Node outputs also agree. Main-thread
sample counts are 2,212/2,206 before/after at n = 1,600 and 2,200/2,198 at
n = 6,400. Top-of-stack counts, collapsed by symbol:

| Frame | before 1,600 | after 1,600 | before 6,400 | after 6,400 |
| --- | ---: | ---: | ---: | ---: |
| `execute_budgeted` | 451 | 617 | 440 | 589 |
| `Fault` drop glue | 101 | 122 | 111 | 111 |
| `Value` drop glue | 32 | 71 | 46 | 63 |
| SipHash `write` | 54 | 70 | 65 | 73 |
| `lookup_cursor` | 56 | 47 | 53 | 51 |
| heap `Directory::slot` | 39 | 55 | 50 | 47 |
| `memmove` | 70 | 35 | 75 | 40 |
| `start_conversion` | 30 | 48 | 32 | 50 |
| `property_read_step` | 18 | 34 | 32 | 34 |
| `enter_constructor` | 31 | 50 | 35 | 43 |
| Proxy `Operations::step` | 12 | 43 | 14 | 46 |
| VM callable `resolve` | 33 | 25 | 24 | 33 |

These are distributions within separate three-second CPU samples, not costs
per operation; faster code executes more iterations in that interval. The
remaining leading frames are interpreter dispatch, drop/value handling,
hashing, property/Proxy lookup, conversion and constructor entry. JavaScript
table/frontier checks and dynamic compiler-root reads remain candidates for
light-cljs-515; this native-stack profile does not attribute bytecode time to
individual JavaScript functions.

Validation on `47cb9998`: native-only `bbp_macros` 86/86; native plus QuickJS
`--lib --test bbp_macros` 346 library tests and 88 differential tests pass
(one existing library test ignored). `PROTOCOL_FIXTURES` is byte-for-byte
unchanged. The pure `(fn [x] (map first x))` publication expectation changes
from false to true because cached functions no longer capture `rt`; a dynamic
`*print-length*` read remains a negative control. Native-only `--lib` has 271
passes and the same 27 unsupported bootstrap-macro/test-service failures
reproduced on master `529e2db6` with the same toolchain and configuration.
The after run has the same failed test names and unsupported-service/macro
signatures.

The full default V8 nextest run has 2,129 passes and one failure; the full
QuickJS-only run has 2,052 passes and the same failure:
`playground_crate_checks_for_wasm32` selects the main checkout's workspace and
rejects this nested worktree. Its test, runner and manifests are unchanged.
Those full results are reused across the subsequent native-only lowering and
catalog changes, whose non-native execution paths are unchanged. Engine
`--all-targets` 2,136/2,136 and doctests 5/5 pass; those results are reused across
the catalog rebase because no engine source changed. Compiler and engine
formatting pass. Strict default Clippy and engine-free `--lib --bins` Clippy
pass on the final source. Native/engine all-target strict Clippy reports only
the existing eight library/nine engine-test diagnostics in `deprecated`,
`clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices`; allowing
only those categories after `-D warnings` passes without source suppressions.
Engine-free all-target Clippy remains unavailable in the existing benches and
evaluation tests: absent evaluator APIs and unused/unreachable feature-dependent
test code. These full-suite and strict-lint limitations remain explicit.

### Core rebinding and generated-name review fixes (light-cljs-501)

Review found three correctness gaps in the call lowering: `binding` and
`with-redefs` targets could become invalid cached-core assignments; a separately
compiled helper could replace a function already cached by its caller; and raw
JavaScript could capture the new core-cache or protocol-receiver bindings.
Native emission now includes binding targets in its replacement set and shares
source-wide core-write information across macro, helper and prepared-unit
emission. The source index conservatively handles qualified aliases, renamed
refers, preloaded helpers, mutation operators used as macro data and unquoted
targets. Such computed writes keep all core reads dynamic. Native artifacts
are reanalyzed when the current compilation may replace a cacheable core
function, because their prepared factories can reflect an earlier
compilation's caching assumptions. Dynamic core Vars are read at each use
and their bindings do not invalidate those factories.
The new generated core, getter and numbered receiver names participate in
the existing raw-JavaScript hygiene pass.

The added differential cases cover restoration after rebinding and throws,
`set!`, forward-declared installers, arithmetic and division replacements,
shadowed core functions in separate namespaces, generated installers, earlier
helper artifacts at one and two slots, and actual cached-getter/receiver
collisions. The artifact and macro-argument installer cases both returned
`[true true]` before their fixes and `[true false]` afterward. The existing
`PROTOCOL_FIXTURES` is byte-for-byte unchanged.

Initial review-fix checkpoint `557b3f95`, rebased on `79f30c85`, macOS aarch64,
Rust/Cargo 1.99.1, Node 26.12.1; Cargo builds use three jobs, compiler nextest
runs use eight slots and engine/cargo-test runs use three test threads:

- Native-only `--test bbp_macros --test bbp_reader --test bbp_proxy_has`:
  90 + 10 + 2 passes. These use `--no-default-features --features bbp-macros`.
- Native plus QuickJS, the same binaries plus `--lib`: 346 library and
  92 + 10 + 2 integration passes, one existing library test ignored. These
  use `--no-default-features --features bbp-macros,quickjs`.
- Full default V8 `cargo nextest run --locked --no-fail-fast -j 8`, with
  `LIGHT_CLJS_V8_CACHE=require`: 2,129 passes, one failure. Full QuickJS-only
  nextest (`--no-default-features --features quickjs`): 2,052 passes, one
  failure. Both reproduce `playground_crate_checks_for_wasm32`: the nested
  worktree's playground manifest is rejected as part of the main checkout's
  workspace. The test, build script and manifests are unchanged.
- The final default `cache_matrix` and `compiler namespace_artifacts::`
  selections pass 11/11 each after the artifact-lookup change. The full
  default/QuickJS results above are reused across native-only changes; the
  added legacy artifact guard is constant false. Engine `--all-targets`
  2,137/2,137 and doctests 5/5 pass; engine sources and build inputs are
  unchanged across these compiler review fixes.

Compiler and engine formatting pass. Strict default all-target and engine-free
`--lib --bins` Clippy pass. Native plus QuickJS all-target Clippy passes when
only the existing engine categories `deprecated`,
`clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices` are allowed
after `-D warnings`; actual strict native/engine runs still fail on the same
eight library/nine engine-test diagnostics. Native-only library, binary and
the three affected integration targets pass with those same allowances.
Native-only all-target Clippy also reports `unused_enumerate_index` in the
unchanged `tests/execution.rs:1772`. Engine-free all-target Clippy still fails
in existing evaluation tests/benches without evaluator APIs and on that
unused index. These all-target limitations were not suppressed in source.

After rebasing on comparison fixes at `5dfeade5`, checkpoint `64298ac4`
passes the native-only 102 tests, native/QuickJS 450 tests (one library test
ignored), engine all-target 2,145 tests and five doctests. Native/QuickJS
all-target Clippy passes with only the three existing engine categories
allowed; refreshed strict engine Clippy still reports eight library/nine
engine-test diagnostics. Formatting passes. The incoming delta changes only
the optional native engine and native macro fixtures, so the default V8,
QuickJS-only and engine-free compiler results above are reused. The native
and engine suites were rerun because those paths changed.

Fresh release binaries from this integrated checkpoint use the same build
commands as above: native-only SHA-256 `d1d0bcb81eb5fe13…` and default V8
(`LIGHT_CLJS_V8_CACHE=require`) `4660879ae14e81c6…`. These are built from
one commit, rather than using a historical V8 timing reference.

A final guard at `05d0e954` also treats property/index writes on core functions
as replacements, including source field/index forms and dotted references.
Overriding addition's `.call` returned `42` on V8 but `3` with the native
operator lowering before this correction. Direct and demanded-helper property
write cases now pass against QuickJS. All six focused core-service, rebinding,
artifact and property-write tests pass in both native-only and native/QuickJS
builds. Formatting and the affected native Clippy targets pass with the same
three existing engine-category allowances. The preceding full-suite results
cover unchanged lowering paths; the property-assignment eligibility change
was verified by these focused cases.

The final same-commit release pair is native-only SHA-256
`1d1497870e996938…`, default V8 `3f88717fd0bb87a0…`, both at `05d0e954`.
The required post-review `n=400` timing was **not collected**: every launch
check after the binaries were ready still found active peer builds/tests or
standalone compiler runs. To preserve the requested measurement serialization,
no fresh timed run was started within the worker's time budget. No final-revision
k value or CPU profile is claimed. The four-size results and before/after
profiles at `47cb9998` above remain the original optimization evidence;
light-cljs-515 remains the existing focused numerical-target follow-up.
Rerun `bb benchmarks/native-macros-k.bb OUTPUT --workload protocol --runs 5
--n 0,400 v8=V8_BINARY native=NATIVE_BINARY` in a clear host window to finish
the review-fix performance check.

### Dynamic bindings and JavaScript holes (final review fix)

At `701a763f`, artifact invalidation uses the same `value == fn` eligibility
rule as native function caching. Dynamic Vars such as `*print-length*` have
value type `any` and keep their reads at each use. A new fresh-process cache
test binds `*print-length*` in both a macro and the application: before this
fix its warm native build had zero hits; afterward both namespaces hit,
no macros rerun, and cached/uncached JavaScript and Node output agree. The
test passes in default V8, native-only and QuickJS-only configurations.
The new JavaScript-hole differential cases replace `even?` directly and
override addition's `.call` from a demanded helper, yielding `[true false]`
and `[3 42]` on both native and legacy QuickJS. Existing protocol fixtures
and output expectations are unchanged.

Validation at `701a763f`, rebased without changes onto `5dfeade5`, uses the
same macOS aarch64, 24-CPU host, Rust/Cargo 1.99.1 and Node 26.12.1. Builds
use three jobs; compiler nextest uses eight slots; cargo tests use three
test threads. The pinned ClojureScript source link and
`LIGHT_CLJS_V8_CACHE=require` are used for default tests.

- Full default `cargo nextest run --locked --no-fail-fast -j 8`: 2,130
  passes, one known failure. Full QuickJS-only nextest with
  `--no-default-features --features quickjs`: 2,053 passes, the same failure.
  Both failures are `playground_crate_checks_for_wasm32`: Cargo rejects
  this nested worktree's playground manifest as belonging to the main
  checkout's workspace. The test, build script and manifests are unchanged.
- Native-only `cargo test --locked --no-default-features --features
  bbp-macros --test bbp_macros --test bbp_reader --test bbp_proxy_has`:
  92 + 10 + 2 passes; the new `compiler_cache` test passes separately.
  Adding `quickjs` and `--lib` gives 346 library and 94 + 10 + 2 integration
  passes, with one existing library test ignored.
- Engine `--all-targets`: 2,145 passes across 64 binaries; engine doctests:
  seven passes. Compiler and engine formatting pass.
- Strict default all-target Clippy and engine-free `--lib --bins` Clippy
  pass. Engine-free all-target Clippy stops at the unchanged unused index
  in `tests/execution.rs:1772`. Strict native/engine Clippy still reports
  the existing eight library/nine engine-test diagnostics. Native plus
  QuickJS and engine all-target Clippy pass when only `deprecated`,
  `clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices` are
  allowed after `-D warnings`. Native-only library, binary, macro, reader,
  proxy and compiler-cache targets pass with those same allowances.
  No source suppressions were added.

Fresh native `/usr/bin/sample PID 3 1 -file ...` profiles use the same
256-call n=1,600 and 64-call n=6,400 inputs as above. Node output matches
the earlier profiles. Peer suites were active (host load approximately
8.5–9.2); these are CPU sample distributions, separate from latency timing.
There are 1,879/2,115 main-thread samples. Top-of-stack counts collapsed by
symbol:

| Frame | n=1,600 | n=6,400 |
| --- | ---: | ---: |
| `execute_budgeted` | 511 | 567 |
| `Fault` drop glue | 93 | 120 |
| `step_instruction` | 59 | 82 |
| SipHash `write` | 65 | 80 |
| `Value` drop glue | 34 | 51 |
| heap `Directory::slot` | 36 | 56 |
| `lookup_cursor` | 45 | 54 |
| `start_conversion` | 50 | 42 |
| `enter_constructor` | 40 | 41 |
| Proxy `Operations::step` | 41 | 42 |
| `property_read_step` | 21 | 23 |
| VM callable `resolve` | 23 | 22 |

The residual interpreter, value/drop, hashing and property-resolution work
remains the scope of the existing focused follow-up light-cljs-515.

## Lazy sequences (light-cljs-479), October 26, 2026

`bb benchmarks/native-macros-k.bb OUTPUT --workload lazy v8=BIN NAME=BIN`
measures k on a lazy-sequence macro: `(churn n)` takes `(vec (range n))`, a
`for` with `:when` over it, `filter`, `map` and `reduce`, and counts a
`doall` of `take-while` over `iterate` (one unchunked cell per element). Before
this pellet the native backend could not run it (`core.mapSeq` and `for` were
unsupported). Release binaries of this branch after the review fixes
(`1fd4c3c0`, rebased on master `032d00af`, whose vector and list tails share
their trie): native-only (`--no-default-features --features bbp-macros`,
sha256 `836967b8…`) and the default V8 build (`LIGHT_CLJS_V8_CACHE=require`,
sha256 `c361c8b8…`, V8 path unchanged). macOS aarch64, 24 CPUs, Rust 1.99.1
(Homebrew), Node 24.22.1; two peer workers (load averages 3.8–6.1). Analysis
time, median [min–max] of five, ms:

| `n` | V8 | native | k |
| ---: | --- | --- | ---: |
| 0 | 25.6 [25.5–26.3] | 12.5 [12.2–13.4] | – |
| 100 | 31.0 [30.5–31.7] | 18.4 [18.0–18.6] | 1.1 |
| 400 | 38.2 [37.8–38.7] | 39.2 [38.2–40.3] | 2.1 |
| 1,600 | 61.8 [61.3–61.9] | 120.4 [119.8–122.8] | 3.0 |

k is inside the win condition (k <= ~4) at every measured `n`. An operation
that reads a whole sequence (`count`, and so `doall`, the runtime's array
helpers behind `vec`, `into` and the like, and the realization of a macro's
result) realizes the rest of the chain inside one realization frame. Earlier
builds of this branch measured, at `n = 400` and 1,600: 89.5 and 827.5 ms
(k 6.0 and 22.5) when such an operation executed again after every cell, and
53.5 and 246.4 ms (k 3.3 and 6.6) before the rebase and the review fixes
(sha256 `bc913db7…`), when a vector's seq copied its elements. Hashing an
unrealized sequence still executes again per cell.

The `churn` workload on the same machine, master `2ea9cb19` (sha256
`7851e3b7…`) against this branch at `877fb682` (before the rebase): k 3.5 →
3.3 at `n = 100` and 12.8 → 12.4 at `n = 400`, within the run-to-run spread;
`n = 0` rises from 9.7 to 11.9 ms (the larger runtime the engine compiles at
start).

The 20k and 200k generated projects (seed 42) now stop natively at `--jobs 1`
at `workload/trildorplus.cljc:6:1` (light-cljs-505), above.
