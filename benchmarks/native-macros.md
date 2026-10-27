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

The final integrated same-commit release pair is native-only SHA-256
`2411fe939ceeb648…`, default V8 `fbf1cefdc45a2546…`, both at `79973136`
on master `ae4373fd`. Both variants were freshly built with the commands
above, after `cargo clean --release -p light-cljs`, with
`cargo clean --release -p big-boy-pants-js` before the native build. V8 uses
`LIGHT_CLJS_V8_CACHE=require`; these are measured V8 timings, not historical
references. The unchanged harness verifies identical Node output:

```sh
bb benchmarks/native-macros-k.bb target/fix4-protocol-integrated-9 \
  --workload protocol --runs 9 --n 0,100,400,1600,6400 \
  v8=target/fix4-integrated-v8 native=target/fix4-integrated-native
```

All own builds/tests had finished. After waiting for load to fall, an initial
five-round run started at one-minute load 2.55 and ended at 2.71; a peer
compiler process still overlapped. Its k values were 56.7/6.1/9.3/14.9.
The n=100 ratio is unstable: its V8 medians are 28.8 ms at n=0 and 29.1 ms
at n=100, a denominator of only 0.3 ms. At n=0/400, V8 measures 28.8/37.5 ms
and native 47.2/100.1 ms (k=6.1).

The nine-round repeat on exactly the same binaries started at load 2.98 and
ended at 4.27; peers remained active. Neither run is claimed as an idle-host
or fully serialized measurement. Analysis time, median [min–max] of nine, ms:

| `n` | fresh V8 | native | k |
| ---: | --- | --- | ---: |
| 0 | 27.6 [24.3–33.6] | 44.5 [40.2–56.4] | – |
| 100 | 31.2 [28.4–33.4] | 70.1 [61.5–76.5] | 7.1 |
| 400 | 35.7 [31.8–36.9] | 99.0 [88.2–113.1] | 6.7 |
| 1,600 | 45.8 [40.5–53.2] | 206.1 [192.4–220.7] | 8.9 |
| 6,400 | 72.3 [65.9–94.9] | 646.5 [636.5–727.9] | 13.5 |

Both integrated runs miss the target at n=400 and n=1,600; n=100 remains
sensitive to startup variation. Before integration, the five-round review-fix
pair at `701a763f` measured k=3.5/5.9/8.9/13.9 at load 3.05–3.27 with a
single peer stress test active. The before/after optimization
comparison at `47cb9998` above remains the baseline evidence; the residual
costs remain assigned to light-cljs-515, without another follow-up.

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

After rebasing onto native reduction fixes at `ae4373fd`, checkpoint
`79973136` passes the same 104 native-only integration tests and the new
compiler-cache regression, plus 346 library and 106 native/QuickJS integration
tests (one existing library test ignored). Default V8 and QuickJS-only cache
suites each pass all 24 tests, with one existing test skipped; these reruns
verify the compiler identity that also hashes native sources. Strict default
all-target Clippy, native/QuickJS Clippy with the same baseline allowances,
and formatting pass. The base delta touches only `bbp_runtime.js` and its
feature-gated fixtures: broader default/QuickJS-only and engine suite results
above are reused for their unchanged execution paths.

Fresh native `/usr/bin/sample PID 3 1 -file ...` profiles use the same
integrated `79973136` binary and the 256-call n=1,600 and 64-call n=6,400
inputs above. Node output matches the earlier profiles. Peers were active
(one-minute load 8.04 before and 8.98 afterward); these are CPU sample
distributions, separate from latency timing. There are 2,101/2,133 main-thread
samples. Top-of-stack counts collapsed by symbol:

| Frame | n=1,600 | n=6,400 |
| --- | ---: | ---: |
| `execute_budgeted` | 573 | 605 |
| `Fault` drop glue | 100 | 120 |
| `step_instruction` | 81 | 71 |
| SipHash `write` | 72 | 65 |
| `Value` drop glue | 58 | 43 |
| heap `Directory::slot` | 42 | 45 |
| `lookup_cursor` | 46 | 47 |
| `start_conversion` | 44 | 48 |
| `enter_constructor` | 46 | 52 |
| Proxy `Operations::step` | 48 | 49 |
| `property_read_step` | 20 | 34 |
| VM callable `resolve` | 39 | 33 |

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

## Generated 200k and Penpot backend comparison, October 27, 2026

The V8 timings in this section include the compiler regression introduced by
`b4976476`. The [follow-up investigation and repair](generated-shadow-vs-light.md#default-v8-regression-check-october-27-2026)
records the corrected default V8 baseline. The ratios below describe the
historical binaries measured here.

Both release compilers use the unchanged tree at
`d9b1b5f9e5bcee19c7366caa71091c53d221f8f7`, including builtin/callback/name
lowering, protocol lowering, the shared catalog and shared multimethods.
macOS 15.7.3 (24G419), aarch64, Apple M3 Ultra (Virtual), 24 logical CPUs,
32 GiB RAM; Homebrew Rust/Cargo 1.99.1, Babashka 1.12.218 and Node 26.12.1.
No other implementation workers or heavy builds/tests ran during measurement.
Builds and measurements ran sequentially; each sample starts a fresh process
with `--no-cache`. OS file caches were retained, with no discarded warmups.
Release compilation, generation, hashing and Node validation are outside timing.

```sh
# From the isolated worktree; no source changes between builds.
mkdir -p target/native-project-522
LIGHT_CLJS_V8_CACHE=require CARGO_BUILD_JOBS=3 cargo build --locked --release \
  --no-default-features --features bbp-macros
cp target/release/light-cljs target/native-project-522/light-cljs-native
LIGHT_CLJS_V8_CACHE=require CARGO_BUILD_JOBS=3 cargo build --locked --release
cp target/release/light-cljs target/native-project-522/light-cljs-v8
bb benchmarks/generated-project/generate.bb \
  target/native-project-522/generated-200k --seed 42 --lines 200k
```

Binary SHA-256: native
`2988f73ae93d0423d8678d9a6e59a78162e341f2dd9ed5b403aefebb2f133711`;
default V8
`afadc511abc87cf084e5089cba27d3eb162aaf052b791a2d4b8a5250a54b90a8`.
The default build used the existing V8 machine cache with
`LIGHT_CLJS_V8_CACHE=require`. Neither compiler has allocation instrumentation.

The seed-42 fixture has 199,129 nonblank lines, 213,984 physical lines,
9,966,430 source bytes, 15,450 top-level forms and 450 worker namespaces.
Generated builds use `--compile --esm --entry workload.main --optimize advanced`
with both generated `src` and `macros` roots. This matches the generated
`release bench` optimization and budgets, using the direct CLI to obtain a
source map (chunked `release` output does not support maps). Penpot builds use
`--compile --esm --entry app.render|app.main --optimize none` and the five
absolute read-only roots under `/Users/admin/src/light-cljs/research/penpot-light`:
`frontend/src`, `common/src`, `vendor`, `generated`, `shims`. No port regeneration
or research writes were performed.

All commands add `--jobs N --no-cache --stats --source-map MAP
--source-map-include-sources`, using one fixed absolute `MAP` path for all runs.
Macro budgets (generated / Penpot) are expansions 10,000,000 / 100,000,000;
nodes 1,000,000,000 / 4,000,000,000; memory 2,147,483,648 / 4,000,000,000 bytes;
polls 100,000,000,000 / 1,000,000,000,000; timeout 3,600,000 ms for both.
`/usr/bin/time -l` measures process wall, user CPU and peak resident bytes,
including statistics and map writing. An external one-hour timeout bounds
each process. Three rounds visit generated, render, then main; jobs run
1/2/4/8 in rounds 1 and 3 and 8/4/2/1 in round 2. Generated backend order
alternates at each job count and between rounds. The tables report independent
medians, wall min–max, and the three pre-run load averages in round order
(1/5/15 minutes). Full logs, per-run statistics, emitted JS/maps and the
separate profile stay under the worktree's ignored `target/native-project-522/`;
the results below are the committed measurement record.

### Native Penpot coverage

Both native entry builds at jobs 1 exit 1 with exactly this first error:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:906:3: macro expansion: while expanding fnc: /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:906:3: macro rumext.v2/fnc: big-boy-pants-js: unsupported operation: compiler literal; definition /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:320:1 (bytes 28314..28909)
```

| Entry | Wall to failure, s | User CPU, s | Peak RSS, bytes | Load before → after (1/5/15 min) |
| --- | ---: | ---: | ---: | --- |
| `app.render` | 13.44 | 13.10 | 267,288,576 | 2.38/2.35/2.01 → 2.37/2.34/2.02 |
| `app.main` | 13.18 | 12.83 | 303,415,296 | 2.37/2.34/2.02 → 2.30/2.33/2.02 |

These are failed-build coverage probes, excluded from successful build timing.
The gap is already tracked by light-cljs-535. Only V8 is measured for Penpot;
there is no native Penpot speed or output-identity comparison.

### Correctness checks

All 24 generated builds (three repeats at each job count for both backends)
emit byte-identical JavaScript, SHA-256
`02e9caa87ea83816ed71f2427162fabcf97bd7e2a534831691b3045fab7f4ff2`,
and all 24 applications print the independent Babashka oracle's `484264` for
input 42, three iterations. Each backend's maps are byte-identical across
repeats and job counts, but the backends differ: V8
`42fb37caabe52643291c4b864d52242a2321923bf32bb2853817ae8ec991140c`,
native `7fab5fe22ba4771ce98aa15269b54d8bd8f95dfabf3b4d6d671bc1dd31e3b683`.
The parsed maps have identical `version`, `file`, `sources` (570), `names`
(41,923) and `sourcesContent`; only `mappings` differs (3,418,912 V8 characters,
3,417,969 native characters). This is an output-identity finding, separate
from successful application execution and from elapsed build time.
The first unequal mapping string character is at offset 147,619 (zero-based):
V8 segment `CAS+D` maps generated line 677, column 553 to
`generated-200k/src/workload/skeidren.cljs`, line 204, column 65; native segment
`IACC` instead maps generated line 677, column 556 to the same file, line 196,
column 3. Lines here are one-based and columns zero-based. The surrounding
JavaScript is the same `klekkloul` expansion of `buvaiskum`.

Each Penpot V8 entry also emits byte-identical JS and maps across its 12 builds
(three repeats at jobs 1/2/4/8):

| Entry | JS SHA-256 | Source-map SHA-256 |
| --- | --- | --- |
| `app.render` | `72868c0c8c996750e2009792858084c234e5e947389a7c9d89cfeb24da4275b5` | `bd1794adfbb3a647309e1658a4302a9168469f7443ab23932ca85b2c9d077b49` |
| `app.main` | `ab66dc52e2046310f3c3b694afb9f25531ab2bd4fe89780bbea9b81ff024027a` | `3ea65b5a278d4e7119c211860af6850bb0e45263add890d7736fadd50321ba5c` |

Penpot validation covers compilation and emitted-byte identity; the application
was not run in a browser. The generated oracle/Node check is a separate execution
check and does not establish source-map correctness.

### Build timings

Three cold processes per cell, 48 successful builds in total. Each entry is an
independent median; brackets show minimum–maximum wall time. Wall/user CPU are
seconds; RSS is GiB (2^30 bytes). Analysis and deferred-unit analysis are separate
`--stats` phase counters, in seconds. The V8 engine counter includes traced engine
work, rather than only macro execution; native does not populate it, so its
reported zero is shown as unavailable. Each load cell contains the three pre-run
1/5/15-minute averages, in round order. Successful emission timings do not
establish source-map equivalence; the correctness finding above still applies.

#### Generated 200k, advanced

| Backend | Jobs | Wall [min–max], s | User CPU, s | Peak RSS, GiB | Analysis, s | Deferred, s | Traced engine, s | Loads (1/5/15 min) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| native | 1 | 221.66 [221.32–222.09] | 219.41 | 1.368 | 30.794 | 188.306 | n/a | 2.54/2.45/2.13; 2.30/2.44/2.51; 2.81/2.68/2.60 |
| native | 2 | 207.43 [206.89–208.17] | 209.94 | 1.439 | 28.795 | 175.989 | n/a | 2.60/2.45/2.19; 2.31/2.61/2.59; 2.56/2.64/2.59 |
| native | 4 | 196.78 [196.50–201.11] | 204.48 | 1.571 | 27.587 | 166.701 | n/a | 2.46/2.40/2.26; 2.64/2.84/2.65; 2.67/2.60/2.58 |
| native | 8 | 198.45 [196.57–201.72] | 214.14 | 1.776 | 29.422 | 166.443 | n/a | 2.62/2.45/2.30; 3.46/2.85/2.52; 2.38/2.55/2.56 |
| v8 | 1 | 106.22 [105.30–106.96] | 106.28 | 1.093 | 8.077 | 95.654 | 1.467 | 2.37/2.42/2.08; 2.25/2.50/2.54; 2.49/2.61/2.57 |
| v8 | 2 | 103.81 [102.78–104.85] | 106.27 | 1.139 | 8.177 | 93.142 | 1.194 | 2.61/2.45/2.24; 2.79/2.65/2.60; 2.51/2.58/2.57 |
| v8 | 4 | 99.92 [99.80–101.58] | 105.06 | 1.333 | 7.964 | 89.513 | 1.007 | 2.50/2.43/2.25; 2.79/2.92/2.66; 2.38/2.52/2.55 |
| v8 | 8 | 100.57 [99.57–101.46] | 109.11 | 1.533 | 8.050 | 90.051 | 1.025 | 2.75/2.54/2.36; 2.80/2.91/2.62; 2.47/2.54/2.56 |

#### Penpot app.render, none

| Backend | Jobs | Wall [min–max], s | User CPU, s | Peak RSS, GiB | Analysis, s | Deferred, s | Traced engine, s | Loads (1/5/15 min) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| v8 | 1 | 12.35 [12.27–12.45] | 12.69 | 0.470 | 0.955 | 10.926 | 0.699 | 2.55/2.51/2.36; 2.69/2.57/2.54; 2.40/2.49/2.53 |
| v8 | 2 | 11.79 [11.75–11.86] | 13.50 | 0.549 | 1.133 | 10.183 | 0.408 | 2.44/2.49/2.35; 2.54/2.54/2.53; 2.26/2.46/2.52 |
| v8 | 4 | 11.64 [11.54–11.80] | 14.15 | 0.649 | 1.131 | 10.056 | 0.390 | 2.37/2.47/2.35; 2.48/2.53/2.53; 2.15/2.43/2.51 |
| v8 | 8 | 11.64 [11.60–11.66] | 15.37 | 0.802 | 1.167 | 10.019 | 0.393 | 2.39/2.47/2.35; 2.57/2.55/2.54; 2.27/2.44/2.51 |

#### Penpot app.main, none

| Backend | Jobs | Wall [min–max], s | User CPU, s | Peak RSS, GiB | Analysis, s | Deferred, s | Traced engine, s | Loads (1/5/15 min) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| v8 | 1 | 72.10 [72.09–72.81] | 74.21 | 1.009 | 4.953 | 65.756 | 6.751 | 2.31/2.45/2.34; 2.41/2.57/2.55; 2.31/2.44/2.51 |
| v8 | 2 | 66.13 [66.11–67.11] | 76.37 | 1.104 | 5.144 | 59.619 | 3.318 | 2.47/2.47/2.35; 2.40/2.61/2.57; 2.28/2.40/2.48 |
| v8 | 4 | 65.47 [65.33–65.93] | 78.39 | 1.255 | 5.159 | 58.997 | 2.848 | 2.38/2.42/2.34; 2.91/2.73/2.60; 2.44/2.42/2.49 |
| v8 | 8 | 65.65 [64.68–65.79] | 82.04 | 1.683 | 5.196 | 59.106 | 2.834 | 3.05/2.60/2.41; 2.75/2.59/2.55; 2.67/2.52/2.52 |

### Ratios and scaling

Native/V8 is median native wall time divided by median V8 wall time; above 1
means native is slower. Speedup is each backend's jobs-1 median divided by its
median at the named job count. These whole-build ratios are not the per-thread
macro-engine factor k.

| Jobs | Generated native/V8 wall | Native speedup from jobs 1 | V8 speedup from jobs 1 |
| ---: | ---: | ---: | ---: |
| 1 | 2.09x | 1.00x | 1.00x |
| 2 | 2.00x | 1.07x | 1.02x |
| 4 | 1.97x | 1.13x | 1.06x |
| 8 | 1.97x | 1.12x | 1.06x |

Penpot `app.render` V8 jobs 1 → 8: 12.35 → 11.64 seconds, 1.06x speedup. Native/V8 is unavailable because native fails before emission.

Penpot `app.main` V8 jobs 1 → 8: 72.10 → 65.65 seconds, 1.10x speedup. Native/V8 is unavailable because native fails before emission.

Native is slower at every measured job count on the generated project: 2.09x
at jobs 1 and about 1.97x at jobs 4 and 8. Native jobs 8 (198.45 s) is still
1.87x V8 jobs 1 (106.22 s). Shared runtime/helper publication has not made this
workload faster than V8. Penpot supplies V8 scaling evidence and a native
coverage blocker, not evidence of native performance on that application.

### Native jobs-8 profile

A separate fresh generated build used the same native binary, source roots,
budgets and advanced/map flags, at jobs 8. Immediately after launch:

```sh
/usr/bin/sample PID 600 1 -mayDie \
  -file target/native-project-522/profile/sample.txt
```

Sampling ran from launch through process exit, starting at 08:37:02 -0600.
The compiler and sampler both exited 0. This instrumented run reports 220.383 s
in `time.wall-ms`, versus the unprofiled 198.45 s median, and is excluded from
the timing tables. Recorded load before/after was 2.34/2.47/2.50 →
2.18/2.45/2.49. Its JS/map hashes match the native matrix and Node again prints
`484264`. LLVM `llvm-cxxfilt` demangled the selected Rust symbols.

The main thread has 163,545 samples; 136,211 (83.3%) are under this inclusive
call path:

```text
analyze_entry → merge_namespace → NamespaceAnalysis::merge_front
  → SharedCatalog::release_application_ir
    → emit_native_macro_unit → emit_js_mode
```

Leading collapsed top-of-stack counts across **all** threads (waiting threads
included; these counts are not CPU percentages):

| Category | Frame | Leaf samples |
| --- | --- | ---: |
| Worker waits | `__psynch_cvwait` | 1,269,744 |
| Compiler arena traversal | `Iter<GlobalBinding>::next` | 76,458 |
| Compiler arena lookup | `Arena<GlobalBinding>::index` | 34,437 |
| Compiler preparation | `Emitter::new` | 20,318 |
| Native core-replacement scan | `emit_js_mode` flatten/filter iterator | 9,638 |
| Mutex waits | `__psynch_mutexwait` | 6,515 |
| Compiler preparation | `emit_js_mode` body closure | 4,069 |
| Waits | `__ulock_wait` | 2,277 |
| Native macro VM | `execute_budgeted` | 2,248 |
| Buffer copying | `_platform_memmove` | 2,194 |
| Native GC | `Heap::mark_and_sweep` | 1,195 |
| Native GC tracing | `OrdinaryObject::trace` | 706 |
| Native GC | `mark_and_sweep` closure | 703 |
| Native GC tracing | `Code::trace` | 554 |

Worker 5 has 156,884 of 161,345 samples (97.2%) in the scheduler's condition
wait. The `light-cljs-units` coordinator also spends almost all its samples
waiting. The main thread is busy preparing native executable source while the
pool has little work to execute. Macro VM and GC frames are visible, but much
smaller than the compiler preparation frames in this profile.

Source inspection at the measured commit shows why this preparation can grow:
`SharedCatalog::release_application_ir` calls `DeferredUnit::prepare_native`
as application IR is released, retaining source for later demand. That calls
the general `emit_js_mode`; `Emitter::new` scans the snapshot's complete
`program.exprs` for native multimethod arguments, and the native core-replacement
scan visits that entire expression prefix again. The arena symbol names in the
table are as sampled. Shared storage avoids copying the prefix but does not
avoid repeatedly traversing it. This preparation appears in analysis/deferred
phase time, not just in the final application `time.emission-ms` counter;
the large deferred times must not be interpreted as pure VM execution.

All 12 native generated runs compile/publish 57 helpers, have zero per-task
helper fallbacks, compile 742 native units, and report zero effect-lane wait time.
At jobs 8, worker utilization is 1.9/3.2/3.2% (median 3.2%) for native and
1.1/1.1/1.1% for V8. Median reported worker-startup time is 0.150 s native,
0.101 s V8, small beside whole-build time. The measured limitation is repeated
compiler-side preparation on the main thread despite shared helper publication.
Focused follow-up **light-cljs-542** covers removing those repeated catalog-prefix
scans. The existing protocol VM and task-memory work remains separate.

### Verification

Both locked release builds pass. The matrix completes all 48 builds with three
successes per cell; all 24 generated Node results equal the Babashka oracle.
The separate profiled build passes the same execution and hash checks.
`node --check` passes for each Penpot entry's emitted ES module. Native Penpot
has the two recorded compiler-literal failures; source-map backend identity
has the recorded generated-project difference. No compiler code was changed.

`cargo fmt --check`, `git diff --check`, and
`LIGHT_CLJS_V8_CACHE=require CARGO_BUILD_JOBS=3 cargo clippy --locked --all-targets -- -D warnings`
pass with default features; the vendored V8 dependency emits its existing
`Atomic::fetch_update` deprecation warning. The full compiler/engine test suites
were not run for this documentation/measurement-only change. These results use
the measured compiler tree above; subsequent commits change only this document.

## Deferred native unit preparation (2026-10-27, light-cljs-542)

The baseline production sources are `d92276a7`, including light-cljs-534's live
catalog-prefix reuse and light-cljs-543's V8 repair. The latter moved the native
multimethod scan out of ordinary emission, so the preceding section's roughly
100-second V8 reference does not describe this baseline. Native emission still
scanned the complete expression arena twice per prepared unit.

`1e9a69d1` replaces those scans with one traversal from the unit's root. Explicit
Var metadata and other expression children are included; a global read keeps
its separate lazy demand. The traversal records multimethod function operands,
local core writes and type method owners. Cross-unit and later core writes keep
using the session's conservative source index. Preparation retains source and
reader tables, runs no initializers, and retains no old program.

The regression in `src/emitter.rs` counts actual indexed and iterator arena
reads. Adding frozen prefixes of 512 and 4,096 unrelated expressions and global
bindings leaves the read count unchanged. Its unit follows the frozen prefix
and can reference metadata within it. The cases cover explicit Var metadata,
multimethod operands, core property writes/bindings, type fields and local
scope; mutating the live program afterward preserves the snapshot's metadata,
reader tables and emitted source. The initial test failed against the old
scans, before applying the traversal change.

A separate reduced fixture constructs `set!` and `cljs.core/even?` with
`(symbol "...")` inside a macro, then calls that generated installer from a
demanded value initializer. Both baseline and optimized native binaries print
`[true true]`; the same-base V8 reference prints the expected `[true false]`.
The lexical core-write index misses this generated mutation. **light-cljs-544**
contains the reproduction and tracks that pre-existing correctness gap.

### Generated 200k measurement method

The same macOS arm64 VM has 24 logical CPUs, an Apple M3 Ultra (Virtual),
32 GiB RAM and macOS 15.7.3. Both revisions were rebuilt with locked release
settings, thin LTO and one codegen unit, without allocation instrumentation.
The default V8 binary was built from each native binary's production sources.
`LIGHT_CLJS_V8_CACHE=require` reused the read-only machine cache. Build jobs
were capped at three. Binary SHA-256 values:

| Production sources | Backend | SHA-256 |
| --- | --- | --- |
| `d92276a7` | native-only | `8fbbf4c83c91bc2829dcd737875d3ffbd4625868a09d7b97490db6c7114bc3b4` |
| `d92276a7` | default V8 | `4abb5da3738a91a1d2f1eb2c075899e5999f05e45f22e1da032d4f4ca2362cab` |
| `e0f4a2c1` | native-only | `3d1387f2debc54a45dbef5ac675fc183f7bad2180cb1ddb1107f1f7006c5201f` |
| `e0f4a2c1` | default V8 | `a20c3662590e5c16eb22bc660f1639d84b0faa36595be5f22fa8dfb7e3b34696` |

```sh
CARGO_BUILD_JOBS=3 cargo build --locked --release \
  --no-default-features --features bbp-macros --bin light-cljs
LIGHT_CLJS_V8_CACHE=require CARGO_BUILD_JOBS=3 cargo build --locked --release \
  --bin light-cljs
bb benchmarks/generated-project/generate.bb \
  target/preparation-542/generated-200k --seed 42 --lines 200k
# In the generated project, using each saved absolute binary path:
/usr/bin/time -l BINARY release bench --no-cache --jobs N --stats
node run-light.mjs 42 3
bb oracle.bb 42 3
```

The fixture has 199,129 nonblank lines, 213,984 physical lines, 9,966,430 source
bytes, 15,450 top-level forms and 450 worker namespaces. The generated release
configuration uses advanced ESM output, disables persistent caching and sets
macro budgets of 10,000,000 expansions, 1,000,000,000 nodes, 2,147,483,648 heap
bytes, 100,000,000,000 polls and 3,600,000 ms. Each compiler process also has a
one-hour external timeout enforced by a Babashka process supervisor. The measured
process includes statistics/output writing; generation, hashing, Node and the
oracle are outside timing. OS file caches were retained, with no discarded
warmups or per-process engine reuse.

Three rounds visit jobs 1/2/4/8, 8/4/2/1 and 1/2/4/8. At alternating job cells,
binary order is before-V8/after-V8/before-native/after-native or its reverse.
Every application is executed against the oracle (`484264`), and all generated
JavaScript files are compared byte-for-byte within each backend across revisions,
rounds and job counts.

The host was shared with other implementation/review workers. Initial launches
waited for peer Cargo/test/compiler processes; repeated restarts of their checks
made that wait lengthy. Remaining launches waited for aggregate background CPU
below 350% (3.5 of 24 logical CPUs). Load averages and process snapshots were
recorded before/after each run. Peer work could start during a process, so exact
wall-time ratios retain that interference limitation; independent user CPU,
flat arena-read counts and sampled call paths provide additional evidence.
No compiler builds or feature suites from this worker overlapped its timings.

### Generated 200k results

Each entry is the independent median of three fresh processes; brackets show
minimum–maximum wall time. Wall/user CPU are seconds; peak RSS is GiB (2^30
bytes). Loads are the three pre-run 1/5/15-minute averages, in round order.
Two initial jobs-1 native processes overlapped peer work and were repeated:
their excluded wall times were 234.58 s before and 38.77 s after. The retained
replacement processes took 222.16 s and 36.22 s. No other samples were discarded.

| Backend | Change | Jobs | Wall [min–max], s | User CPU, s | Peak RSS, GiB | Loads (1/5/15 min) |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| native | before | 1 | 226.49 [222.16–231.51] | 224.10 | 1.273 | 2.33/2.52/2.88; 2.38/3.22/4.41; 2.23/2.75/3.89 |
| native | after | 1 | 35.14 [34.69–36.22] | 33.27 | 1.257 | 2.82/2.58/2.81; 2.77/3.40/4.53; 2.35/2.55/3.53 |
| native | before | 2 | 215.37 [211.28–232.51] | 217.79 | 1.393 | 11.09/10.83/9.71; 3.29/4.56/5.24; 2.26/2.49/3.44 |
| native | after | 2 | 23.67 [23.18–23.80] | 26.46 | 1.383 | 4.70/4.69/7.46; 2.41/3.37/4.55; 2.30/2.51/3.47 |
| native | before | 4 | 199.68 [198.72–218.95] | 207.59 | 1.497 | 3.00/6.27/7.94; 2.88/3.17/5.02; 2.58/2.52/3.21 |
| native | after | 4 | 18.48 [18.44–18.51] | 24.60 | 1.528 | 2.74/4.46/6.80; 3.26/3.25/5.09; 2.40/2.45/3.03 |
| native | before | 8 | 198.60 [196.93–198.80] | 216.28 | 1.728 | 2.65/4.22/6.60; 2.99/3.52/5.73; 2.61/2.48/3.01 |
| native | after | 8 | 21.43 [20.82–22.74] | 33.89 | 1.702 | 2.58/4.32/6.70; 2.89/3.17/5.12; 2.30/2.42/3.01 |
| v8 | before | 1 | 7.26 [7.14–7.34] | 7.40 | 1.017 | 2.10/6.23/9.65; 2.32/2.81/3.94; 2.20/2.77/3.92 |
| v8 | after | 1 | 7.17 [7.11–7.23] | 7.32 | 1.021 | 2.39/6.16/9.58; 2.35/2.83/3.96; 2.25/2.76/3.90 |
| v8 | before | 2 | 6.14 [5.95–6.15] | 8.45 | 1.125 | 2.87/6.42/8.02; 3.76/4.71/5.31; 2.53/2.50/3.22 |
| v8 | after | 2 | 5.96 [5.93–6.64] | 8.31 | 1.103 | 2.74/6.58/8.10; 3.49/4.62/5.27; 2.49/2.49/3.22 |
| v8 | before | 4 | 5.07 [5.07–5.18] | 9.87 | 1.320 | 2.80/6.35/7.98; 3.65/4.70/5.31; 2.41/2.48/3.21 |
| v8 | after | 4 | 4.99 [4.98–5.33] | 9.72 | 1.314 | 2.74/6.28/7.95; 3.28/4.64/5.29; 2.78/2.55/3.23 |
| v8 | before | 8 | 5.11 [5.05–5.19] | 13.26 | 1.511 | 2.68/3.49/5.77; 3.27/3.60/5.80; 3.01/2.64/2.96 |
| v8 | after | 8 | 5.12 [5.11–5.18] | 13.31 | 1.503 | 2.48/3.46/5.78; 3.07/3.54/5.75; 2.85/2.60/2.94 |

| Jobs | Native speedup, before/after | Native wall reduction | Before native/V8 | After native/V8 |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 6.45x | 84.5% | 31.20x | 4.90x |
| 2 | 9.10x | 89.0% | 35.08x | 3.97x |
| 4 | 10.81x | 90.7% | 39.38x | 3.70x |
| 8 | 9.27x | 89.2% | 38.86x | 4.19x |

The removed preparation work cuts whole-build wall time by 84.5–90.7%.
Native user CPU falls from 224.10/217.79/207.59/216.28 s to
33.27/26.46/24.60/33.89 s. Peak RSS stays close to the baseline. The repaired
default V8 reference remains essentially unchanged: its before/after medians
are 7.26/7.17, 6.14/5.96, 5.07/4.99 and 5.11/5.12 s; all wall ranges overlap.
There is no material default-engine regression.

Native remains slower than the same-commit V8 reference at every job count.
The optimized build is fastest at four jobs (18.48 s), 3.70x V8 at four jobs;
eight jobs take 21.43 s, 4.19x V8. These are whole-build ratios, not the
per-thread macro-engine factor k. Removing prefix scans does not close the
remaining native engine/bridge gap.

| Jobs | Native analysis before/after, s | Native deferred phase before/after, s |
| ---: | ---: | ---: |
| 1 | 26.611 / 16.145 | 197.389 / 16.485 |
| 2 | 25.093 / 14.639 | 187.611 / 6.386 |
| 4 | 23.299 / 15.325 | 173.959 / 0.711 |
| 8 | 23.798 / 18.229 | 172.428 / 0.721 |

These are coordinator phase counters, including preparation and overlap with
worker analysis; deferred time is not pure VM execution. Every retained native
run publishes exactly 57 helpers, has zero per-task helper copies and compiles
742 native units. All 48 retained builds pass the Node/oracle check and the
within-backend generated-JavaScript identity checks.

### Fresh jobs-8 profiles and source maps

Separate fresh native processes used the same source roots, advanced ESM mode,
budgets and `--no-cache --jobs 8`, with direct `--compile --entry workload.main`
output and `--source-map generated.map --source-map-include-sources`. This
produces maps that chunked `release bench` cannot emit. Each process was sampled
from launch through exit with:

```sh
/usr/bin/sample PID 600 1 -mayDie -file SAMPLE.txt
```

Both compiler/sampler pairs exit zero. Instrumented wall times are 224.141 s
before and 26.583 s after; neither is included in the timing matrix. Recorded
load before/after is 3.36/2.72/2.85 → 2.47/2.65/2.79 for the baseline and
2.47/2.65/2.79 → 2.71/2.69/2.80 for the optimized build. LLVM `llvm-cxxfilt`
demangled the sampled symbols.

The baseline main thread has 161,278 samples. Across its disjoint call paths,
139,580 (86.5%) are inside `SharedCatalog::release_application_ir`; the largest
single path, through `merge_namespace → NamespaceAnalysis::merge_front`, has
138,059. The optimized main thread has 18,350 samples, of which only 506 (2.8%)
are inside `release_application_ir`. Absolute samples in that preparation
hotspot fall by 99.6%. The remaining main-thread work primarily follows macro
expansion and native catalog application, with VM, GC and contention visible.

Reported collapsed leaf samples across **all threads**, including waiting
threads, are counts rather than CPU percentages:

| Frame | Before | After |
| --- | ---: | ---: |
| `Iter<GlobalBinding>::next` | 76,069 | 131 |
| `Arena<GlobalBinding>::index` | 33,015 | 154 |
| `emit_js_mode` body closure | 23,783 | 9 |
| Native replacement flatten/filter iterator | 9,714 | below reporting cutoff (5) |
| VM `execute_budgeted` | 2,358 | 2,319 |
| `_platform_memmove` | 2,198 | 2,198 |
| `Heap::mark_and_sweep` | 1,347 | 1,508 |
| Condition-variable waits | 1,250,101 | 106,512 |
| Mutex waits | 7,955 | 12,552 |

Arena names are the symbols emitted by the optimized binary; the source scans
removed by this change traversed expressions. The previous prefix traversal
dominates the baseline and disappears from the optimized profile. Similar
absolute VM/copying counts and unchanged helper/unit counts are consistent with
removing compiler bookkeeping while retaining the macro work.

Both profiled applications print `484264`. Direct native JavaScript and maps
match byte-for-byte across revisions, using one fixed map path:

- JS SHA-256: `d260017e883e3ec20d63eb933bb14deac41664920b5e59e81e7b5c6276ebc0d1`.
- Map SHA-256: `03350e1b7536eec8c4acb71cba49722f975b2097faf7b2a6418fc332ca511e7e`.

No expected source-map data was changed. The earlier native-versus-V8 map
identity finding is separate from this native before/after identity check.

### Compiler verification

Production sources at `1e9a69d1`, also used by documentation-only `e0f4a2c1`,
were checked with Homebrew Rust/Cargo 1.99.1, Node 26.12.1 and Babashka 1.12.218
on macOS 15.7.3 arm64. All feature variants ran sequentially, with
`CARGO_BUILD_JOBS=3` and nextest `-j 8`. The pinned ClojureScript research checkout
was linked read-only; default builds used `LIGHT_CLJS_V8_CACHE=require`.

- Default `cargo nextest run --locked -j 8 --no-fail-fast`: 2,131 passed,
  one failed. QuickJS-only, adding `--no-default-features --features quickjs`:
  2,054 passed, one failed. Both failures are
  `cli::playground_crate_checks_for_wasm32`: Cargo discovers the enclosing
  main-checkout workspace instead of treating this nested worktree's excluded
  playground crate as standalone. No tests or assertions were disabled.
- Native-only `--lib --test bbp_macros`, with `LIGHT_CLJS_STRESS_SEEDS=12`:
  370 passed, 27 failed. All 96 macro tests pass, including pool and effect-lane
  stress at one/two/four/eight slots. The library contributes 274 passes and
  27 failures. Rebuilding the unchanged `d92276a7` library gives 273 passes and
  the same 27 failed tests, with identical error signatures. These tests assume
  legacy bootstrap macros, readers or host services unavailable in native-only
  mode. The additional passing test is the preparation regression.
- Native plus QuickJS `--lib --test bbp_macros`: all 447 tests pass, with one
  normally ignored test. This includes native/QuickJS comparisons, session and
  retention checks, shared multimethods, native call/protocol lowering, dynamic
  core replacement, cache/artifact behavior, binding and once-only effects.
- `cargo fmt --check`, `git diff --check` and default all-target strict Clippy
  pass. Native strict Clippy retains eight existing engine library diagnostics
  in `deprecated`, `clippy::chunks_exact_to_as_chunks` and
  `clippy::byte_char_slices`; allowing only those categories passes all targets
  with native plus QuickJS. Engine-free library/binary strict Clippy passes.
  Its all-target check finds missing legacy evaluator methods and an unused
  enumeration index in untouched tests/benches; focused checks on the unchanged
  base reproduce every diagnostic from the final-content run.

### Final rebase verification

The final tested production tree is `7cd02298`, rebased onto `1888f327`.
The base delta from `d92276a7` is confined to native comparison implementation,
its form tests and additional `native_reduce_kv_matches_quickjs` table cases;
it changes no compiler emission, scheduler, effect-lane or legacy-engine code.
The timing/profile matrix above retains its original source pair to isolate
the preparation change from these independent comparison fixes.

On the rebased tree, all eight engine comparison tests and all 38 form tests
pass. Native-only library/macro coverage was rerun with twelve stress seeds:
370 pass and the same 27 native-only library failures remain, matching the
unchanged baseline's failed names and error signatures. Both pool/effect-lane
stress tests pass again. Native plus QuickJS again passes all 447 tests, with
one normally ignored test. The added standalone core-read counter case passes,
so supplied cross-unit replacement metadata is exercised outside a local write
or multimethod function. Default/QuickJS full-suite results above are reused:
their production source paths and inputs are unchanged and they do not build
the native comparison code.

Final formatting and default all-target strict Clippy pass. Native strict
Clippy still reports the same eight engine diagnostics; the all-target combined
check passes with only the three documented baseline categories allowed.
Engine-free library/binary strict Clippy passes. Its unsupported engine-dependent
tests/benches still prevent an all-target pass: every one of the 112 surfaced
diagnostic/location pairs reproduces on the unchanged rebased base with
`--keep-going` (which additionally reaches other pre-existing failures).
No source suppressions or skipped assertions were added.

Fresh release binaries from this rebased tree were checked once more at jobs 8:
native 22.42 s wall / 34.77 s user / 1.681 GiB peak RSS, default V8
5.26 s / 13.44 s / 1.486 GiB. These single-process checks are separate from the
three-run matrix. Loads before/after were 3.35/3.58/3.21 → 3.21/3.54/3.20 for
native and 3.21/3.54/3.20 → 3.60/3.61/3.23 for V8. Both print `484264` and
their release JavaScript matches the earlier output. A fresh mapped native
build also matches both original native JS and map hashes byte-for-byte.
Rebased binary SHA-256 values are native
`2c6ea7b40427a0f66196e20f1e7a8732d40b8dfaf92be5e22fc59ea5f7fd0119`
and default V8
`252204e2a389fdc26388d09813632da382a664c7943f4af212cad1dc6ca20b5e`.
