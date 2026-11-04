# Generated 200k end-to-end performance

November 4, 2026 UTC. **V8 wins every full-project cell.** The best Goose
configuration is four execution workers: **11.532 s versus V8's 6.211 s**,
1.86× the wall time and 5.04× the aggregate CPU. Goose8 takes 18.692 s, 3.01× V8.
This is successful supported compilation, with an unfavorable performance result.

## Clean primary comparison

The unchanged [pinned target](generated-200k.md) has 199,129 nonblank lines,
450 worker namespaces, 57 macro namespaces and 653 canonical files/shims.
Same release/advanced optimization, inputs, budgets and complete observable work;
fresh compiler/native processes and `--no-cache` in every cell. Each cell had
one excluded warmup, then five measured rounds, alternating forward/reverse cell
order. All 30 builds passed. Every pre-run gate passed (90.55–97.93% CPU idle);
no failed/slow sample was discarded and no competing build/test ran.

Seconds, median [minimum, maximum]. RSS is decimal GB from macOS command rusage,
**not a simultaneous sum of compiler and native memory**. Aggregate CPU is the
median of each run's user+system CPU, including waited child processes.

| Engine / execution limit | Compiler unit slots | Wall | Aggregate CPU | Command RSS |
| --- | ---: | ---: | ---: | ---: |
| V8 / 1 | 1 | 6.211 [6.134, 6.252] | 6.77 [6.68, 6.78] | 1.045 [1.037, 1.045] |
| Goose / 1 | 1 | 20.053 [19.961, 20.212] | 19.76 [19.65, 19.93] | 0.959 [0.951, 0.962] |
| Goose / 2 | 2 | 14.234 [14.181, 14.400] | 23.22 [23.12, 23.48] | 1.037 [1.020, 1.055] |
| Goose / 4 | 4 | 11.532 [11.273, 11.760] | 34.14 [33.46, 35.02] | 1.136 [1.129, 1.142] |
| Goose / 8 | 8 | 18.692 [18.024, 18.776] | 94.45 [90.87, 97.26] | 1.262 [1.249, 1.275] |

Goose speedup over Goose1 is 1.41× / 1.74× / 1.07× at 2/4/8; corresponding
whole-build scaling efficiency is 70.4% / 43.5% / 13.4%. These vary compiler
unit slots together with native execution capacity, so they are not isolated
VM scaling measurements. The experimental whole-namespace worker path is **off**
(`LIGHT_CLJS_NAMESPACE_WORKERS` is removed). Namespace analysis follows dependency
order, with deferred units using the compiler pool. The 450 worker namespaces
are source population, not threads. The diagnostic DAG has 569 loaded namespaces,
13,477 deferred units, require depth 11 and drain ready width median 28 / p90 33 /
max 37; that compiler width is not the Goose runnable queue.

## Separate cost and concurrency diagnostics

One warmup and one repeat per cell used full native protocol capture, native-only
`time -l`, owned-tree RSS sampling, compiler counters and a temporary native phase
probe. None enters the clean table. The phase probe only adds clocks/private flat
records, writes at worker close, and is restored before compiler runs. Every Goose
session verifies **5,812 distinct chunks**: 5,226 initial bodies, 529 resumptions,
57 helper producers; 615 preparations and 672 exports. All qualified source-macro
identity/count tables match V8. Every native worker executes and resets work;
all 3/5/9/17 spawned native threads join (4/6/10/18 total including coordinator).

Repeat-run aggregate elapsed intervals, milliseconds. They overlap across workers,
include possible descheduling, and **must not be summed into wall or read as CPU**.

| Goose limit | Restore | Argument/helper import | VM slices | Continuation save | Native export | Rust bridge |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 150.1 | 2418.6 | 67.7 | 70.9 | 550.5 | 5369.3 |
| 2 | 149.4 | 2437.4 | 69.4 | 74.8 | 629.4 | 6730.5 |
| 4 | 153.0 | 2565.4 | 72.6 | 79.6 | 721.1 | 14068.8 |
| 8 | 162.6 | 2628.3 | 76.3 | 80.7 | 762.9 | 74225.9 |

Restore includes validation, copying and identity relocation; exactly 995,474,716
payload bytes are copied during restores in each session. Save includes snapshot
validation/capture and continuation-table copies. The unchanged safety probe
separately measured 843,816,693 cumulative continuation bytes, not live retention.
Import includes argument file read/decode/import and callable-helper installation;
export includes result/helper graph encoding and output writes. Queue-message
copies and other glue remain charged in native CPU and whole-build wall, without
an invented exclusive copy-time partition. Helper factory execution totals only
0.15–0.18 ms; prepared-definition/load counters total 41.6–47.3 ms.

Rust bridge timers cover argument-graph construction, argument encoding/write,
and result decode/expansion; they exclude the explicit wait for native completion.
Their concurrent intervals are not CPU samples. Whole source-macro service totals
(13.10/15.05/24.01/84.74 s) additionally include helper/wait work and overlap;
no wait was subtracted to manufacture avoidable latency. Native-only aggregate
CPU remains 5.07/5.14/5.41/5.56 s while whole-command CPU rises sharply. Thus the
measured growing boundary is compiler-side transport/conversion, rather than
native VM instruction work. Allocation/locking/cache contention causes within
that boundary remain hypotheses; this run does not identify a sampled hotspot.

Body intervals exclude queue wait, restore/import, helper factories, save and
result conversion. All **5,755** body intervals are present per session.
Warmup/repeat overlap peaks are 1/1, 2/2, 3/2, 3/3 at 1/2/4/8, with repeat body
medians 12/12/13/13 microseconds. These are overlapping elapsed lifetimes,
not simultaneous CPU instructions. Prior [safety evidence](generated-200k-probes.md#full-session-safety-and-lifetime-recheck)
observed Goose8 peaks 4/3. Neither dataset demonstrates eight-way body saturation.

Other charged costs: Rust export/lowering/write totals 506–545 ms; session spawn
until process creation returns 0.42–0.47 ms; native base load 0.055–0.062 ms;
boot 0.003–0.004 ms; base capture/**pool readiness** 0.245/0.427/1.020/2.444 ms;
explicit close/join/reader/session-directory cleanup 37.8/41.6/48.9/57.9 ms.
The readiness counter is not pure snapshot capture. Startup intervals can overlap.

V8's diagnostic source-macro service is 1.114 s within 6.240 s compiler-reported
wall (~17.9%), across 5,226 source entries out of 94,592 total macro expansions.
This is an inclusive call-boundary share, not the entire replaceable fraction.
Retained cold V8 bootstrap is 10.54 ms, evaluator creation/prelude 8.46 ms;
these inclusive counters overlap. Goose still installs V8 core/bootstrap (counts
1/3/5/9), with no target `expand_core_javascript` calls. V8 diagnostic top-level
analysis is 1.794 s, deferred units 2.165 s, application preparation 0.957 s and
emission 0.619 s; macro service overlaps analysis. Reading/resolution, native Rust
core/binding expansion, orchestration, forms, analysis and emission retain their
compiler owners. This remains **Goose source macros with V8 core/bootstrap**.

| Diagnostic repeat | Sampled owned-tree RSS peak, GB | Full native-process RSS peak, MB |
| --- | ---: | ---: |
| V8 1 | 1.023 | n/a |
| Goose 1 | 1.106 | 128.6 |
| Goose 2 | 1.204 | 154.8 |
| Goose 4 | 1.339 | 210.3 |
| Goose 8 | 1.557 | 274.0 |

Tree RSS is sampled about every 100 ms plus `ps` overhead, counts shared pages
per process, includes diagnostic wrappers, and can miss peaks. Native RSS uses
inner command rusage for the complete diagnostic process, including probe storage.
These are separate runs/scopes; do not add their maxima or treat them as clean
memory measurements. Reviewed unmodified-runtime safety peaks remain 114.6–280.8 MB
across 1–8 workers, versus ~20.8 MB peak charged retention and 17.77 MB final charge.

## Cache and edited-source scenarios

Separately instrumented V8 diagnostics, five samples after warmup: no-change
median **1.125 s [1.109, 1.149]**, 569 installed artifacts, zero macro execution
and reused output. The representative leaf is `src/workload/jourklijo.cljs`;
each round appends a distinct inert function. Cached leaf median is **2.851 s
[2.756, 2.911]**, 566 installed / three rebuilt namespaces, 244 macro expansions /
16 source-macro entries, without output reuse. Every output is checked separately.

Goose cached/no-change and cached leaf cells are **unsupported, not measured**.
A direct current CLI invocation exits 1 with `cached artifact installation is not
validated`; the raw command/error is preserved. No cache implementation or skipped
work is advertised as a Goose speedup. A separate **uncached leaf** diagnostic
does exercise edited input in Goose4: V8 6.209 s / Goose4 11.073 s, one edited
sample after each engine's warmup. Both rebuild all 569 namespaces, execute the
same 5,226 source entries (94,594 total expansions including the appended defn),
and match complete output/oracles. These instrumented single samples are not an
additional timing matrix. All six leaf edits are restored byte-for-byte and the
full original manifest is revalidated.

## Correctness, control and practical result

All **56 successful builds** across these scenarios match all 3,449,022 output
bytes, SHA-256 `18335556dd4672f2f63b901b68fcad3c447cf5676f457a08fda61f366cc9f3ec`,
all five independent oracle outputs (`687181`, `371978`, `484264`, `945410`,
`463429`, including newlines), and strict waste checks. The unchanged
[execution audit and complete-form/teardown tests](generated-200k-probes.md)
are reused by exact execution-source hashes and native identity: all eleven
families, no fallback, complete forms/scalars/metadata/provenance at native
debug and optimized 1/2/4/8. Their 82 explicitly audited V8 span ambiguities
remain a reference limitation; matching application JS alone does not prove forms.

The seed-24301 / 15-substantive-namespace / 120-call / 8192-read control remains
unchanged and passed debug/native V8/Goose1/2/4/8 in the preceding verification.
Its [reviewed final medians](expanded-results.md#final-source-clean-comparison)
are Goose8/V8 357.319/912.226 ms; the separate later one-worker comparison is
Goose1/V8/Goose8 1710.503/923.059/365.868 ms. That later raw dataset, original
harness/patch, fixture and provenance are now also preserved in this archive.
These are historical control measurements, not 200k predictions or fresh timings.
The 200k bodies are much shorter; adding source lines does not create long VM jobs.

The prototype proves full-target execution and bounded parallel source work, but
does not deliver a practical full-project speedup. Further VM parallelization
alone has little measured work to accelerate here. Profile the compiler bridge
before investing in optimizations; decide caching separately if incremental
use matters. No speculative optimization tasks were created from this loss.

## Reproduction and evidence

[Raw archive](generated-200k-performance.edn.gz): EDN `:files` maps names to
UTF-8 `:text` and SHA-256; all 262 embedded digests were reread and verified.
It includes every sample, stdout result, stderr/rusage, full native protocol,
tree sample, phase record, manifest/toolchain identity, analysis script and probe
source. Execution source is `6bf69b3959b47b347274d0a3ffeaff6d69661ba4`; clean
harness is that revision, diagnostics record their exact later diff. No execution
source changed. CLI SHA-256 is `d8d53393b3897353eb0ca0c693cba3413888ac6c92cdee9e85fc7a3cf9f798f2`;
clean native is `92140b1a31ba1fe42beadc3caae59fb6ea0916e4a3204e5caff10ef7146688e0`;
diagnostic native is `688de47c75b97e112361adc04c99ce48c002a7cda3220c79c6a599a65f6cc60d`.
Fixture manifest remains `6a4da9ff2d8682c3e524d86abb36a1628131cae16bcbb0220797b4f85c131907`.
Toolchain executable hashes accompany versions: Goose267/runtime3025e5810379,
Rust1.99.1, Node26.12.1, bb1.12.218, clang21; macOS arm64, M3 Ultra Virtual,
24 logical CPUs, 32 GiB RAM. Research checkout is `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`.

Use a fresh isolated checkout/output directory and the pinned research link:

```sh
export CARGO_TARGET_DIR="$PWD/target" GOOSE_HOME="$PWD/target/goose"
export CARGO_BUILD_JOBS=3 LIGHT_CLJS_V8_CACHE=require
cargo build --release --locked --features goose-macros --bin light-cljs
goose build crates/big-boy-pants-js/prototypes/goose/compiler_session.goose -o target/generated-200k-runner
bb benchmarks/generated-project/generate.bb target/generated-200k --seed 42 --lines 200k
bb benchmarks/generated-project/compare.bb target/generated-200k --light target/release/light-cljs --backend all --runner target/generated-200k-runner --phase measure --runs 5
bb benchmarks/generated-project/compare.bb target/generated-200k --light target/release/light-cljs --backend v8 --phase cached --runs 5
bb benchmarks/generated-project/compare.bb target/generated-200k --light target/release/light-cljs --backend v8 --phase leaf --leaf src/workload/jourklijo.cljs --runs 5
bb benchmarks/generated-project/compare.bb target/generated-200k --light target/release/light-cljs --backend all --workers 4 --runner target/generated-200k-runner --phase leaf-uncached --leaf src/workload/jourklijo.cljs --runs 1
```

Extract `phase750.bb`, `summarize750.bb` and `scenarios750.bb` from archive
`:files` into `target/`. Run `bb target/phase750.bb` (formats/checks/builds the
diagnostic runner and restores the source in `finally`), then run the comparator
with `--backend all --runner target/phase750-runner --phase instrument --runs 1`.
The two analysis scripts reproduce summaries and rejection/wrapper checks.
Phase rows are documented by archive `:phase-record-fields`; each private worker
is bounded to 8,192 records, and analyses reject missing/duplicate dispatches.

Generator/comparator tests pass **12 tests / 369 assertions**; the direct failing
native-wrapper probe preserves exit 1; `git diff --check` passes. Runtime and
compiler source is unchanged, so the preceding 36 compiler tests per native
mode, complete-project form audit and control checks are reused. No fresh broad
suite or strict-Clippy pass is claimed: the previously documented engine/root
lint, formatting and playground failures remain. Independent review and the
final conversation audit remain separate acceptance steps.
