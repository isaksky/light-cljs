# Fixed native macro fixture baseline

On source revision **6405a52ce18c2d5234ba0389c2b4a1416e6e9ad0**, five fresh
native-only release processes passed each of collections and environment at
jobs 1. Helper demand and every jobs 2/4/8 cell were unsupported. No scaling
factor or compiler/V8 k can be inferred. This is a small-fixture, whole-process
baseline before native compile-time core lowering, automatic GC/aggregate
budget (472), and resumable helper demand (463), not a throughput or numeric gate.

## Configuration and raw evidence

[Frozen inputs and commands](frozen.md) were recorded before measurement.
[The full report](runs/report.json) contains revision, fixture/runner/manifest/
lock/binary SHA-256 hashes, expected results, exact commands, invocation order,
per-invocation load, raw counters, identity decisions and summaries. The source
diff is empty; Git status records only this untracked measurement directory.
There were no compiler, fixture, manifest or runner changes. The report's
absolute paths identify the original worktree; copies of every retained output
are beside it under the matching sample-directory basename.

Measured start: **2026-10-25T23:32:34.735936Z**. The coordinator held all peer
builds/tests from before the runner started until the profile ended at about
23:32:46Z. OS file caches were not flushed, no warmups ran, compiler cache was
disabled, and unrelated host/VM activity was not controlled. The initial
release build ran before this window; the runner's rebuild was a cache hit.
Both builds used three build jobs. [Raw invocation log](runner.log),
[prebuild log](prebuild.log), and [raw JSONL samples](runs/raw.jsonl) are retained.
Only the authoritative report's validated `ok` samples enter medians.

Machine: Apple M3 Ultra (Virtual), 24 logical CPUs, 32 GiB RAM, macOS 15.7.3
(24G419), Darwin arm64. Homebrew Rust 1.99.1 / LLVM 23.1.3, Cargo 1.99.1,
Babashka 1.12.218, Node 26.12.1. Explicit host target `aarch64-apple-darwin`,
Cargo release thin LTO / one codegen unit, no build-environment overrides.
Native binary SHA-256:
`390d3cb28723995a0b94b1108257037b3446292e3687184e41cabc9d33120c3e`.

## Successful samples

Each cell has five successful samples; ranges are min–max and spread is max
minus min. Compiler wall time includes process start and stats, excludes Node
validation and builds. macOS `time -l` user CPU has only 10 ms precision here;
its zeros do not mean no work. RSS is the process peak, in bytes. These very
short invocations make startup and measurement overhead material.

| Metric | Collections median [range]; spread | Environment median [range]; spread |
| --- | --- | --- |
| Wall, ms | 18.136709 [15.407500–20.692042]; 5.284542 | 15.817917 [15.175875–18.984125]; 3.808250 |
| User CPU, s | 0.01 [0–0.01]; 0.01 | 0.01 [0.01–0.01]; 0 |
| Peak RSS, bytes | 20,709,376 [20,676,608–20,971,520]; 294,912 | 20,692,992 [20,692,992–20,905,984]; 212,992 |
| Form bridge, ns | 183,290 [171,545–189,377]; 17,832 | 190,956 [177,292–206,832]; 29,540 |
| Actual scaling vs jobs 1 | jobs 1: 1.0; jobs 2/4/8 unavailable | jobs 1: 1.0; jobs 2/4/8 unavailable |

The bridge is the sum of 13 / 16 per-call `macro.bridge.N.ns` counters,
respectively, covering the instrumented form import/environment/export work.
It excludes execution of JS adapters outside those timed sections
(`src/bbp_evaluator.rs`). Ratios of median bridge to median process wall are
about 1.01% / 1.21%; they are not the complete language-boundary overhead.

| Additional requested metric | Availability on this source revision |
| --- | --- |
| Runtime initialization | Unavailable: CLI trace does not isolate native initialization. |
| JS compilation | Unavailable: no separate native compilation counter; legacy `engine-us` rows are zero. |
| JS execution | Unavailable: no separate native execution counter; zero legacy timings do not mean zero execution. |
| Allocation | Unavailable: no cumulative allocated native-byte counter; live/peak memory is not allocation volume. |
| GC count / pauses | Unavailable: CLI exposes neither native collection counts nor pause durations. Baseline predates 472. |
| Parked macro tasks | Unavailable: no native macro suspension counter; worker `pool.parks` would not establish this. Helper fails before suspension. |
| Help-while-waiting depth | Unavailable: no native scheduler help-depth counter; 463 is not implemented. |
| Worker utilization | Unavailable: serial native backend emits no `pool.utilization`; rejected higher-job cells do no useful macro work. |

## Identity and unsupported cells

All five Node results per successful fixture matched the fixed expected stdout.
All five emitted JS files and all five source maps were byte-identical per
fixture. The additional profiled collections invocation also matched. There
are no successful higher-job samples for cross-job comparison; no identity
failure was converted into a timing success.

| Fixture | JavaScript SHA-256 | Source-map SHA-256 |
| --- | --- | --- |
| collections | `cb242754e411227d0fe5e518c1e87cf4071a6710feee99492d00b1eb13de2c49` | `cf9f373e1b5803e169795a3319e0c73e2bb4d8356bc54de1cd919ef4b3a9c426` |
| environment | `1d94b8acfcf818a54fcf4fa76f790353939831747eb2b8c53652a69c1b448036` | `31700664cc31df910a9084d153d061b0439060c4bf8654570d033dc419228a65` |

Ten cells stopped at their first concrete failure (one invocation each):

* Helper jobs 1: `unsupported operation: helper not preloaded:
  native-bench.helper/demanded`. The unchanged source increments its effect
  counter then actually demands the ordinary helper. It was never preloaded.
  [Full diagnostic](runs/009-helper-bbp-macros-j1/stderr.txt); existing 463 owns it.
* All three fixtures at each of jobs 2/4/8: `unsupported operation: native
  macro compilation requires --jobs 1`. Each stderr and resource sample is
  preserved in `runs/`; none enters performance summaries. There is no measured
  parallel slowdown or speedup, nor a supported worst-scaling higher-job cell.

## Optional reference and bounded profile

There was no existing verified same-commit V8 release binary: the coordinator's
available compatibility checks used older revisions or debug builds. No V8
build, repair or distribution was attempted. All ten successful native traces
also have **zero `unit` rows**, and their stage `engine-us` fields are zero.
Consequently there are no matched native/V8 macro units: median per-unit k,
longest units, longest-unit k category (`<=4`, `>4 to <=8`, `>8`), and critical-path
k are **unavailable**. No kernel ratio or whole-process ratio substitutes for them.

The slowest jobs-1 median selected collections. Exactly one unchanged
additional invocation was profiled, capped at 30 s, with a requested 1 ms
sampling interval. [Exact commands and exit status](profile/metadata.json),
[sampler stderr](profile/sample.stderr), compiler stderr, JS, map and trace are
retained. The compiler exited successfully in **17.145 ms**. The sampler exited
0 but reported `pid_for_task ... failed with error 5: (os/kern) failure` and
produced **no sample.txt / no usable stack samples**: the target ended before
attachment. Exit 0 is not a successful profile. No second profile, rerun,
artificial loop or enlarged input was used. All owned processes were stopped
before releasing the quiet window.

The first invocation of the profiling script failed during Babashka analysis
(`dir` collided with `clojure.repl/dir`), before launching a compiler or sampler.
Adding its own namespace fixed that launcher-only issue; no measurement input
or selection changed. This is the sole deviation from the original frozen
profile-script hash. The executed script's SHA-256 is
`d0758a67251d11277920e92f37d28f30281a1a2189c62d0c0d5590702f47df22`.
Afterward, the namespace name alone was changed from `fixture-profile` to
`profile` to match the file name for lint. The checked-in script hash is
`b21aae43632c239052b3bcea34192a02e1f8afc8a62062d863f6d239182f0f23`;
no compiler or sampler invocation was repeated for that naming-only change.

Available coarse counters put median top-level analysis at 6.7 ms collections /
6.8 ms environment, and application emission at 3.3 ms for each (ranges
6.6–8.0 / 6.5–7.3 ms and 3.2–3.8 / 3.2–3.7 ms, respectively). Top-level work
mixes analyzer, initialization, compilation and macro execution. Without stack
samples or separated native timings, collection operations, property access
and calls cannot be ranked or assigned percentages. The measured bridge is
small under its stated scope; there is no measured k >8 or poor scaling cause
to attribute. Existing 477 covers collection lowering, 478 covers protocol/
property dispatch, 463 covers helper suspension/scheduling, and 472 covers GC
and aggregate accounting. This evidence establishes no distinct uncovered
hotspot, so no duplicate or speculative follow-up was filed.

## Verification

`bb benchmarks/native-macros-test.bb`: 6 tests, 39 assertions, no failures.
`cljfmt check` and `clj-kondo --lint` passed for the runner, its tests, and
`profile.bb`; `cargo fmt --check` passed. The native release build and all
actual fixture commands succeeded or produced the expected unsupported
outcomes above. Full compiler/engine suites and strict Rust Clippy were not
rerun for this measurements/docs-only change; these results are not a claim
that those suites pass.
`git diff --check` on the staged artifacts reports the eleven native trace
namespace rows' final empty TSV field as trailing whitespace. Those compiler
outputs are preserved byte-for-byte; hand-written Markdown and Babashka pass.

[Independent artifact validation](verification.txt) rehashed all 20 JS files,
all 10 successful maps, reran Node on those ten outputs plus the profile,
rechecked the five-sample summaries, unsupported signatures, identity and
zero unit rows. `report.json` and `raw.jsonl` retain exact numeric samples;
the mutable runner `work/` scratch copy is omitted because every sample's
own immutable copies are retained. No binary or build directory is committed.

To repeat artifact validation from a relocated checkout, run this Babashka
expression with `bb -e` (paths are resolved against the retained artifact
basenames, not the original worktree):

```clojure
(load-file "benchmarks/native-macros.bb")
(let [base "benchmarks/native-macros-6405a52c"
      report (json/parse-string (slurp (str base "/runs/report.json")) true)
      rows (:samples report)
      ok (filter #(= "ok" (:status %)) rows)
      raw (map #(json/parse-string % true) (str/split-lines (slurp (str base "/runs/raw.jsonl"))))]
  (assert (= rows raw (validate-identities rows)))
  (assert (= {"ok" 10 "unsupported" 10} (frequencies (map :status rows))))
  (assert (= 2 (count (filter :performance-eligible (:summary report)))))
  (doseq [summary (:summary report) :when (:performance-eligible summary)]
    (assert (= 5 (:samples summary))))
  (doseq [row rows
          :let [artifact (str base "/runs/" (fs/file-name (:artifacts row)))]]
    (assert (= (:js-sha256 row) (sha256 (str artifact "/out.js"))))
    (if (= "ok" (:status row))
      (let [result (command ["node" (str artifact "/out.js")])]
        (assert (= 1 (:jobs row)))
        (assert (= (:map-sha256 row) (sha256 (str artifact "/out.map"))))
        (assert (= 0 (:exit result)))
        (assert (= (:result row) (:stdout result)))
        (assert (empty? (unit-identities (slurp (str artifact "/trace.tsv"))))))
      (assert (re-find (if (= 1 (:jobs row)) #"helper not preloaded: native-bench.helper/demanded"
                          #"requires --jobs 1") (:diagnostic row)))))
  (let [row (first ok) result (command ["node" (str base "/profile/out.js")])]
    (assert (= (:js-sha256 row) (sha256 (str base "/profile/out.js"))))
    (assert (= (:map-sha256 row) (sha256 (str base "/profile/out.map"))))
    (assert (= 0 (:exit result)))
    (assert (= (:result row) (:stdout result))))
  (println "PASS: 20 raw/report samples; 10 serial JS/maps/results; 10 unsupported; 2 five-run summaries; zero native units; profile identity."))
```
