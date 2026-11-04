# Expanded actual compiler comparison

2026-11-03, Apple M3 Ultra (Virtual), macOS arm64. On the heavier collection-read workload, Goose branching at eight execution workers took **482.54 ms**, versus **2149.76 ms** at one worker and **910.51 ms** for single-worker V8: **4.45×** and **1.89×** speedups respectively. Light and medium Goose branching remained **3.02×** and **2.41× slower than V8** at eight workers. This is a bounded compiler fixture, not a whole-application or general backend speedup.

## Workload and admission

The actual namespace compiler loads **15 substantive macro-using namespaces**: seven independent cold-wave namespaces, one downstream join, and seven independent reuse-wave namespaces. Each executes eight real emitted `work` invocations: seven deferred definition initializers and one top-level invocation that emits a consumed definition. Every one of the **120 results** is included in the final application output. The join consumes all first-wave results; the second wave reuses the initialized macro definition and once-produced shared helper in the same compilation.

There are also **12 ordering-only gates**, one macro/helper namespace and one entry namespace: **29 loaded namespaces total**. Gates do not count as substantive work. Two six-gate require chains establish phase boundaries under the unchanged `analyzer::DRAIN_LAG = 6`. The empty `input.cljs` file only satisfies the older harness's input-file interface and is not loaded as a namespace.

The kernel builds a 32-element vector and map using `conj`/`assoc`, traverses the vector with `seq`/`next`/`first`, and exercises map `get`, `rest` and `count`. It then performs **16 / 256 / 8192 repeated collection-read iterations** (plus one on alternating calls), using vector/map `get` and vector `count`. All iterations contribute to the result; payloads carry a distinct call marker and 32 values from seed `24301`. An independent Babashka oracle checks `592 + 2016*quot(n,32) + 32*r + r*(r-1)`, where `r=mod(n,32)`, plus every payload and shared `[11 22]` value. There are no sleeps, arithmetic-only timing kernels, precomputed macro results or fallback execution.

This bounded construction/read kernel differs from the historical growing-collection churn kernel. The historical two-work-namespace fixture, six gates, datasets and benchmark modes remain intact as overhead diagnostics.

Namespace count alone did not expose eight-way work: an initial all-deferred layout reached seven worker bodies at an eight-slot setting, matching the compiler's seven pool workers. The final mix supplies useful coordinator work while those pool workers run. A uniform 33-substantive-namespace/eight-call layout exceeded the unchanged session's **128-entry waiter table**, whose inactive entries remain retained; its 129th helper wait asserted. The final 120-call layout stays inside that ceiling while satisfying the requested minimum of twelve substantive namespaces. Sixteen independent namespaces per wave remains a preference this bounded uniform mix cannot meet under that limit. No scheduler, waiter, worker, exporter or runtime policy was changed.

The oversized diagnostic also exposed an **unresolved shutdown hang after the assertion**. A native process sample showed `gs_abort → gs_end → stdio flush → flockfile` waiting for a mutex while its input thread remained inside `read_line`; the Rust caller waited for the native response. I stopped that owned native process to recover the failed run. This was not a clean rejection and was not fixed here. The final harness bounds subprocess trees with a watchdog. [Captured failure and process samples](expanded-oversized-diagnostic.edn.gz) preserve the evidence. To reproduce in a disposable checkout, change the driver constants to `namespaces-per-wave=16`, `phase-counts=[128 8 128]` (33 substantive / 47 loaded namespaces), then run `--expanded-instrument` with a fresh `GOOSE_EXPANDED_SAMPLES` path; the one-worker light cell reaches the retained waiter assertion. Keep this diagnostic separate from valid performance cells.

## Clean timings

Setup-inclusive process wall milliseconds, **median [minimum, maximum]**, five fresh processes per cell:

| Engine / execution workers | Light | Medium | Heavier |
| --- | ---: | ---: | ---: |
| V8 | 93.99 [91.97, 96.82] | 121.96 [119.09, 127.47] | 910.51 [902.41, 921.79] |
| Pants | 154.98 [150.11, 157.58] | 211.22 [209.47, 223.30] | 2167.47 [2147.99, 2176.53] |
| Goose retained serial | 148.53 [145.11, 159.38] | 194.11 [191.65, 197.21] | unsupported: work quota |
| Goose branching 1 | 756.49 [750.18, 825.36] | 751.48 [742.43, 755.42] | 2149.76 [2128.95, 2231.47] |
| Goose branching 2 | 713.21 [700.92, 719.60] | 714.48 [698.21, 718.62] | 1964.57 [1950.57, 1972.56] |
| Goose branching 4 | 313.62 [309.67, 336.85] | 320.29 [306.52, 330.39] | 776.26 [772.32, 792.22] |
| Goose branching 8 | 283.45 [268.29, 299.06] | 293.65 [279.24, 305.22] | 482.54 [462.53, 491.22] |

One full-process warmup per supported cell preceded measurement. Round order alternated forward/reverse. The three quiet gates observed **95.66%, 96.48%, 97.75% idle** with no competing compiler/build process. Builds, fixture generation and Node verification were outside the timed process; macro/core setup, source loading, actual analysis/expansion, emission, result conversion, helper service, session close and teardown were inside. The same source fixture was compiled by every supported engine, with exact emitted-JS SHA-256 equality and exact Node output plus the independent oracle checked after every measured process. There were **100 measured processes across 20 supported cells**.

The heavier retained serial Goose control is **unsupported under the existing session work quota**, not a timing result. A separate diagnostic reports `Fault.WorkLimit`, `work=10000000`, `nodes=4223`, `values=26959`, `steps=315145` during the fifteenth call. Thus this is a cumulative retained-work limit, not exhaustion of the 100,000-node or 1,048,576-value budgets. Branching isolates reset their private work accounting and all heavier branching/reference cells completed. No ratio to a failed serial control is reported.

## CPU and memory

Clean `/usr/bin/time -l` medians. User/system CPU uses Darwin's waited-process accounting and is coarse to 10 ms. RSS is the high-water statistic reported for the test invocation; it is **not total simultaneous process-tree memory**.

| Case | Engine / workers | User / system ms | RSS high-water MiB |
| --- | --- | ---: | ---: |
| light | V8 | 120 / 10 | 59.8 |
| light | Pants | 140 / 0 | 44.3 |
| light | Goose retained serial | 80 / 50 | 44.8 |
| light | Goose branching 1 | 110 / 190 | 43.0 |
| light | Goose branching 2 | 150 / 220 | 48.1 |
| light | Goose branching 4 | 170 / 280 | 58.2 |
| light | Goose branching 8 | 250 / 660 | 85.1 |
| medium | V8 | 180 / 10 | 64.2 |
| medium | Pants | 200 / 0 | 44.0 |
| medium | Goose retained serial | 120 / 50 | 44.0 |
| medium | Goose branching 1 | 160 / 200 | 42.0 |
| medium | Goose branching 2 | 190 / 210 | 47.7 |
| medium | Goose branching 4 | 210 / 290 | 60.3 |
| medium | Goose branching 8 | 300 / 680 | 85.6 |
| heavier | V8 | 980 / 30 | 68.8 |
| heavier | Pants | 2150 / 0 | 47.7 |
| heavier | Goose branching 1 | 1520 / 220 | 40.6 |
| heavier | Goose branching 2 | 1590 / 240 | 48.4 |
| heavier | Goose branching 4 | 1630 / 300 | 58.2 |
| heavier | Goose branching 8 | 1720 / 590 | 86.3 |

At eight workers, light/medium pay substantially more system CPU while still losing to V8. Heavier Goose user+system CPU rises from **1740 ms** at one worker to **2310 ms** at eight while wall decreases. V8's heavier CPU is **1010 ms**. A separate instrumented native-child wrapper reports heavier native RSS high-waters of **25.3 MiB** at one worker and **36.8 MiB** at eight; these separate-run peaks must not be added to the test-invocation peaks.

## Cold and reuse phases

Median milliseconds. Phase boundaries are expansion-completion checkpoints for 56 cold, eight join and 56 reuse calls. Cold includes setup before its first completion; the join and reuse checkpoints include intervening source/gate work. Cleanup includes close/join and compiler-owned value disposal. These phases do not replace end-to-end process timing.

| Case | Engine / workers | Cold wave | Join | Reuse wave | Cleanup |
| --- | --- | ---: | ---: | ---: | ---: |
| light | V8 | 55.60 | 4.23 | 20.37 | 2.46 |
| light | Goose branching 1 | 359.37 | 48.22 | 331.93 | 2.33 |
| light | Goose branching 8 | 139.95 | 18.85 | 101.94 | 3.97 |
| medium | V8 | 69.89 | 5.93 | 31.99 | 2.62 |
| medium | Goose branching 1 | 355.98 | 48.32 | 330.39 | 3.74 |
| medium | Goose branching 8 | 148.73 | 18.21 | 107.77 | 3.55 |
| heavier | V8 | 440.34 | 58.75 | 402.35 | 2.09 |
| heavier | Goose branching 1 | 1012.28 | 143.05 | 984.88 | 2.18 |
| heavier | Goose branching 8 | 230.67 | 42.89 | 193.32 | 3.96 |

## Separate execution and readiness evidence

The instrumentation build is generated under `target/expanded-benchmark/instrumented`; clean timings use the unmodified native source. It records native ready queues, actual worker-body intervals, VM-run intervals excluding continuation save, restore/save intervals, compiler runnable/backlog events and idle-turn counters. Append failures abort; the summary validates exact record counts and unique chunk identities. Instrumented timings are not mixed with the clean dataset.

| Case | Workers | Body / VM overlap peaks | Native ready queue peak | Compiler runnable peak | Initial VM median / p90 / max ms | Restore median ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| light | 8 | 8 / 7 | 8 | 31 | 0.136 / 0.375 / 1.388 | 0.268 |
| medium | 8 | 8 / 6 | 8 | 25 | 0.524 / 0.731 / 1.471 | 0.290 |
| heavier | 1 | 1 / 1 | 1 | 16 | 11.779 / 12.129 / 12.330 | 0.055 |
| heavier | 2 | 2 / 2 | 2 | 48 | 12.366 / 12.585 / 14.032 | 0.056 |
| heavier | 4 | 4 / 4 | 4 | 35 | 12.284 / 12.668 / 13.032 | 0.066 |
| heavier | 8 | 8 / 8 | 8 | 9 | 12.537 / 13.033 / 14.879 | 0.136 |

The final heavier eight-worker run observed **eight queued independent macro requests**, **eight overlapping worker bodies**, and **eight overlapping VM-run intervals**. This proves opportunity and live admission beyond a configured worker count. Intervals are elapsed lifetimes and can include descheduling; they do not prove eight physical CPUs executing at every instant. Light/medium are coordination dominated: one-worker initial VM medians were **0.111 / 0.470 ms**, versus **11.779 ms** for heavier work. The final eight-worker light/medium VM-overlap peaks were seven/six, even though all eight worker bodies were exercised.

Each branching compilation recorded **241 execution chunks**: 120 initial calls, 120 helper resumptions and one helper producer. The native process created 243 threads including its coordinator/input roles; it joined 242 before coordinator exit. At eight workers its peak was ten native threads. Compiler pool, transport and V8 platform threads are additional parent-process roles. These are fresh per-chunk execution threads, not a persistent pool. Stats are captured again **after session close**, retaining final `THREADS`/`SCHED` records.

At eight workers the observed idle-turn classifications (input / macro worker / compiler helper key / helper worker) were **50/57/7/1** for light, **46/48/7/1** for medium and **43/208/4/1** for heavier. Compiler unadmitted backlog peaks were zero; ready/runnable queues supplied the work. A coordinator waiting while a worker computes is different from callback-key or input waiting. For example, heavier one-worker coordinator backoff spans totalled about 2.04 seconds, overlapping VM execution; they cannot be subtracted from wall as avoidable latency. Likewise worker, restore, save and coordinator intervals overlap and must not be added.

## Reproduction and evidence

Measured implementation: `ea01e21cfc0409126fffaecee2c325817323a7f8`, based on integration `b59de9321cf89d854fda9e38fc27ad213f75e33e`, with an empty tracked diff at measurement start. Goose `v2026.3-267-g369c7a2c`, runtime `3025e5810379`; Rust `1.99.1`; Node `v26.12.1`. The runner uses its own `target` and `GOOSE_HOME=target/goose`, release Rust with default features plus `goose-macros`, and the default optimized native Goose build. Macro timeout is 120 seconds; per-command watchdogs bound composite processes.

Rust executable SHA-256: `9527bc920509b9f6296fc86eaaf71ffd1b70ece122f1140904e764c5f749406e`. Clean native executable SHA-256: `72e00ca53b44f4cb5c435a7f4362d22fed205dbb093af6d7f4856a8170e68a0c`. Raw manifests retain full per-source/build hashes, toolchain/environment, quiet snapshots, all wall/CPU/resource samples, phase rows and trace records.

From the repository root (use fresh output paths to preserve the committed datasets):

```sh
bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --expanded-check
GOOSE_BENCH_DEBUG=1 GOOSE_EXPANDED_SAMPLES="$PWD/target/expanded-debug-repeat.edn" bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --expanded-check
GOOSE_EXPANDED_SAMPLES="$PWD/target/expanded-repeat.edn" bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --expanded-measure
bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --expanded-summarize target/expanded-repeat.edn
GOOSE_EXPANDED_SAMPLES="$PWD/target/expanded-instrument-repeat.edn" bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --expanded-instrument
bb crates/big-boy-pants-js/prototypes/goose/expanded-instrument-summary.bb target/expanded-instrument-repeat.edn
GOOSE_EXPANDED_SAMPLES="$PWD/target/expanded-limits-repeat.edn" bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --expanded-limits
```

Raw evidence: [clean timings](expanded-measure-samples.edn.gz), [separate instrumentation](expanded-instrument-samples.edn.gz), [debug checks](expanded-check-debug-samples.edn.gz), [retained quota failure](expanded-limits-samples.edn.gz). Native debug actual-compiler checks passed all supported 1/2/4/8-worker cells. Focused release tests `complete_form_comparison_rejects_scalar_changes` and `goose_compiler_concurrent_helper_once` passed; the latter checks full forms/provenance and helper-once behavior at 1/2/4/8. `rustfmt --check`, native format/check, instrument record validation and `git diff --check` passed.

V8 handles compiler bootstrap/core syntax on the Goose compiler path; Goose executes the emitted user macro and shared helper. Pants uses its supported native-form core. These engines implement matching observable collection work with different internal runtime surfaces. This is one virtual macOS machine, no affinity or machine isolation, one instrumentation pass per cell, and a bounded supported corpus. Two-worker scaling is weak; no scheduling cause is inferred from wall timings alone.

This work establishes the expanded fixture/baseline portion of light-cljs-743. It does not implement event-driven notifications or persistent workers, and does not complete either optimization pellet. Light-cljs-743 should compare its exact predecessor/current on this same fixture; light-cljs-744 should reuse the fixture after the wakeup change. Both remain open.

A post-measurement review correction makes the expected-failure `--expanded-limits`
path use the same process-tree watchdog as ordinary commands. It does not change
successful benchmark execution or the measured dataset. From the repository root,
`bb crates/big-boy-pants-js/prototypes/goose/expanded-watchdog-test.bb` verifies that
a timed-out owned parent and child both terminate, and that an expected nonzero
exit preserves diagnostic output.

## Event-driven readiness: light-cljs-743

2026-11-03, same virtual M3 Ultra/macOS toolchain. The existing **heavier**
fixture remains unchanged: 15 substantive namespaces, 120 consumed calls,
8192/8193 collection reads, seed 24301, and 29 loaded namespaces. Only this
workload was timed. Its complete emitted-JS hash was
`1f4ea5cbde9fc56f74ae08a7c0eeb8ee18216d48b80196b75445fbc8e91a5c75`
in every warmup and measured process; Node output and the independent oracle
also matched. The unsupported retained-serial heavier control was not rerun.

Five fresh processes per cell, setup/cleanup included; milliseconds,
median [minimum, maximum]:

| Revision / execution engine | Wall | User / system CPU | Test invocation RSS MiB |
| --- | ---: | ---: | ---: |
| Historical baseline, V8 1 | 910.511625 | 980 / 30 | 68.8 |
| Historical baseline, Goose 8 | 482.540292 | 1720 / 590 | 86.3 |
| Exact predecessor `eef06bf3`, Goose 8 | 489.974958 [477.477416, 511.894875] | 1720 / 620 | 85.69 |
| Readiness `dbbc94a7`, Goose 8 | 415.312375 [401.275334, 426.868333] | 1710 / 480 | 85.16 |
| Contemporaneous V8 1 | 918.948000 [890.945500, 954.877875] | 990 / 30 | 69.55 |

The new Goose median is **15.24% lower than its fresh paired predecessor** and
**2.21× faster than contemporaneous V8** on this fixture. All five adjacent pairs
favored the readiness implementation. Goose still spends more CPU than V8.
Historical timings are context, not substitutes for the new predecessor pairs.
One full-process warmup per cell preceded measurement. Orders were
V8/predecessor/current, current/predecessor/V8, alternating for five rounds.
The quiet gate measured **96.75% idle**, with no competing compiler/build.
User/system CPU retains Darwin's coarse waited-process accounting. RSS is the
test invocation's high-water statistic, not total simultaneous process-tree RSS.

Cold/join/reuse/cleanup checkpoint medians were **233.61/41.28/199.58/3.80 ms**
for the predecessor and **201.82/34.62/163.42/2.08 ms** for readiness. The V8
checkpoints were **443.76/59.74/401.90/2.06 ms**. These retain the original phase
definitions and do not replace the setup-inclusive process wall measurement.

### Protocol and separate instrumentation

Commands, terminal/suspend worker events and helper-file publications all leave
a token in a **one-slot coalescing wake queue**, after publishing their payload.
A refused wake enqueue means a token already exists. The coordinator clears
tokens **before** draining/rechecking every payload source; at a stable idle
point it performs an atomic blocking receive. A publication after any source
scan therefore either leaves a token or finds one already pending. Repeated or
stale tokens only trigger another source scan. Normal progress has no timed
receive, fixed sleep, helper-file polling thread or global request/reply lock.

The input reader blocks on credits reserving the 128 command slots, including
close/EOF; consumption returns a credit. Worker events have 16 slots for at most
eight active workers, each publishing exactly one event before joining. Helper
payloads retain atomic file publication; Rust sends `-2` through the framed input
pipe only after renaming the key. Cancellation uses the same close notification;
host deadlines retain bounded process kill/reap as failure handling. Per-chunk
threads remain unchanged; this change does not implement the persistent pool.

Separate instrumented predecessor/current runs validate all **241 chunks**, 120
initial bodies, 120 continuations and one helper producer. Both show eight
overlapping worker/VM intervals, eight eligible jobs and `THREADS 0 243 10 242`:
coordinator/input plus 241 execution chunks, peak ten native threads, every
spawned thread joined. The predecessor recorded **259 timed idle backoffs**;
readiness recorded **126 blocking parks/wake returns**, each with a `PARK` hook.
Their elapsed wait totals were **348.733/278.861 ms**, overlapping worker work;
they are never subtracted from wall or added to other elapsed intervals.
Native instrumented RSS was **36.81/39.13 MiB**, a separate-run high-water that
must not be added to the table above. Initial VM medians were **12.582/12.588 ms**;
restore medians were **0.284/0.129 ms**. Single instrumented samples cannot isolate
the cause of that restore difference. Instrumented times are outside clean data.

The earlier `b59de932` diagnostic used two dependent eight-call batches,
work 100/101 and width 100. Removing only idle sleep changed one-worker wall
138.50→89.77 ms and eight-worker wall 120.25→114.40 ms. Native user/system CPU
rose from 10/20→70/20 ms and 10/60→90/70 ms respectively. This was a busy-poll
diagnostic with wrapper overhead and overlapping eight-worker ranges, not a
solution or a general application performance claim. The current coordinator
blocks instead; its fresh clean system CPU decreased rather than substituting
that diagnostic spin.

### Verification, reproduction and limits

`compiler.bb` passed **21 focused tests** with native debug and optimized builds,
including correlated requests, full forms/metadata/provenance, helper/initializer
once, private mutation and one-worker nested reentry without replay, at 1/2/4/8.
The native readiness driver deterministically covers notices before scans,
during transition and racing atomic receive, repeated/spurious notices, a full
wake/payload queue and exhausted admission credits. Actual compiler tests gate
delayed helper publication on the `PARK` hook, exercise 256 repeated notices,
idle close/cancel and unchanged failure/diagnostic ownership assertions.

The original **128 retained waiter limit remains**. The 129th suspended call now
returns `waiter-budget`, rather than asserting while the input reader holds
stdio state. A focused actual native test completes 128 waits, rejects the next,
observes graceful `CLOSED 1` and verifies no native assertion. This fixes the
specific oversized-fixture rejection documented above without raising quotas.
Malformed native packets can still abort the runtime; existing injection tests
at 1/2/4/8 prove host cleanup kills/reaps the session. The watchdog test passes.

Heavy expanded correctness passes in native **debug and optimized** modes at
1/2/4/8, with exact output checks. `smoke.bb` passes format/check/debug/native
VM, source, bridge, snapshot, artifact, dependency, scheduler and effect cases.
QuickJS-only `goose_compiler_bridge` passes three tests, the measured release
compiler's full-form scalar-corruption comparator passes, and the engine's
`cargo check --locked -p big-boy-pants-js --all-targets` passes with existing
warnings. Changed-file rustfmt,
Goose formatting/checking and `git diff --check` pass. Strict all-target default
and QuickJS Clippy stop at the same eight existing engine deprecated-atomic,
constant-chunk and byte-string diagnostics. Audits allowing only documented
existing lint categories pass both configurations; no source suppressions were
added. Root formatting still differs in unchanged files. Broad full suites were
not rerun: the documented namespace-publication, order-dependent capture and
playground wasm lint failures remain limitations. No shared VM, bridge value or
export semantics changed; unchanged broad differential layers are not claimed
as newly executed checks.

Measured source is `dbbc94a73aa381d45509c2ebe8aa0e4488250cba`, with empty tracked
diff at measurement. Final follow-on changes only collapse a test conditional
for Clippy and add this evidence; production/native/fixture content is identical.
Predecessor source, optimized native and release compiler binary were preserved
before implementation, using the same Goose v2026.3-267/runtime 3025e5810379,
Rust 1.99.1, Node 26.12.1 and own `target/goose`. The raw manifests retain both
native and Rust binary hashes and source revision/hashes. The predecessor Rust
binary hash is `132014734613e8c6bd751f73c989218ba8da0790169678fd42ceafbe368dca13`;
current is `b92a153e6ca065f6304a17376ca366abd6243c7f3aedac5d004d44213cf90a8a`.

For a new comparison, preserve the immediate predecessor under an owned target
directory before editing: `git archive` its prototype and `src/goose_evaluator.rs`,
build its native `compiler_session.goose` and release `final_benchmark` test
binary, and copy them as `compiler-session`/`compiler-binary`. Put the full SHA in
`revision.txt` and copy its `compiler_session.goose` beside them. Keep the archived
source hierarchy for the predecessor's own instrumentation transform. Then:

```sh
GOOSE_EXPANDED_FOCUS=heavier GOOSE_EXPANDED_PREDECESSOR="$PWD/target/predecessor" GOOSE_EXPANDED_SAMPLES="$PWD/target/readiness-pairs.edn" bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --expanded-measure
GOOSE_EXPANDED_FOCUS=heavier GOOSE_EXPANDED_PREDECESSOR="$PWD/target/predecessor" GOOSE_EXPANDED_SAMPLES="$PWD/target/readiness-instrument.edn" bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --expanded-instrument
GOOSE_EXPANDED_FOCUS=heavier GOOSE_BENCH_DEBUG=1 GOOSE_EXPANDED_SAMPLES="$PWD/target/readiness-debug.edn" bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --expanded-check
```

Omit `GOOSE_BENCH_DEBUG` for optimized correctness. Focused checks retain 1/2/4/8;
focused instrumentation/measurement use Goose 8 and V8 1. Without focus/pair
options the historical fixture/matrix defaults remain intact. Fresh output paths
preserve historical datasets. Raw evidence: [paired clean samples](readiness-paired-samples.edn.gz),
[separate paired instrumentation](readiness-instrument-samples.edn.gz),
[debug correctness](readiness-check-debug-samples.edn.gz), and
[optimized correctness](readiness-check-native-samples.edn.gz).
