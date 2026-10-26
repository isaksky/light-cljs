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
