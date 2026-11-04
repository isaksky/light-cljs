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

## Compiler bridge causal investigation

November 4, 2026, investigation at `324afa5400dff552f7084d63f9c3dcd7847cb1cc`.
The new user request authorizes a finite optimization follow-up; the preceding
milestone and clean five-round table remain historical results. **The leading
scaling cause is contention on the compiler's session-wide poll fuel counter,
amplified by roughly 82 million argument-conversion polls per build.** No
production optimization is included in this investigation.

`GooseEvaluator::new` in `src/goose_evaluator.rs` gives every evaluator the same
session `ExecutionControl`. `compiler_bridge.rs::Budget::{check,bytes}` calls
`ExecutionControl::poll` for every graph-node/work and scalar-byte check. Its
`remaining.fetch_update` is a compare-and-swap retry loop on one shared atomic.
The control's clock origin and deadline also occupy the surrounding storage.
This is distinct from the Pants-specific conversion mutex in pellet 579.

### Sampling and volume

Two full-build sampling runs per V8/Goose1/4/8 cell used macOS `sample` at 5 ms,
bounded to 35 seconds or process exit, on both compiler and native processes.
Sampling, native protocol tracing and counters are excluded from clean timing.
The repeat compiler profiles contain the following stack observations after
excluding explicit read/condition/semaphore/mutex waits; these percentages are
**sampled stacks, not a CPU-time partition**:

| Goose workers | Non-waiting observations | In `ExecutionControl::poll` | Poll within bridge observations |
| ---: | ---: | ---: | ---: |
| 1 | 2,504 | 456 (18.2%) | 448 / 757 (59.2%) |
| 4 | 4,828 | 1,755 (36.4%) | 1,740 / 2,325 (74.8%) |
| 8 | 13,768 | 9,818 (71.3%) | 9,750 / 10,848 (89.9%) |

At eight workers, 5,744 self observations land immediately after the atomic CAS
(`poll+388` in the archived disassembly), with another 566 at adjacent
counter-loop instructions. Clock reads also appear frequently under `poll`.
V8 has six poll observations out of 1,108 non-waiting observations. Native
profiles show graph import/validation, file I/O and waiting rather than a
growing VM body hotspot, consistent with the earlier native CPU measurements.

A separate reversible counter build measured thread CPU with macOS
`CLOCK_THREAD_CPUTIME_ID` and elapsed intervals. Repeat aggregate **thread CPU**
seconds (argument counts/sets and log output add diagnostic overhead):

| Phase | Goose1 | Goose4 | Goose8 |
| --- | ---: | ---: | ---: |
| Argument graph construction | 5.679 | 13.984 | 74.468 |
| Encoding | 0.272 | 0.264 | 0.275 |
| Input file write | 0.547 | 0.645 | 0.665 |
| Result file read | 0.141 | 0.168 | 0.170 |
| Decode and validation | 0.055 | 0.138 | 0.843 |
| Expansion reconstruction, inclusive of final form build | 0.257 | 0.457 | 1.219 |
| Result handle clone | 0.005 | 0.005 | 0.005 |
| Stored-handle release | 0.356 | 0.919 | 1.042 |
| Input/output file deletion | 0.544 | 0.710 | 0.724 |

Argument elapsed sums are 5.696/14.028/74.541 seconds: the growth is CPU work,
not merely overlapping blocked intervals. These are separate scoped operations,
not an exhaustive additive whole-build partition. Final form construction is
nested within expansion (0.021/0.027/0.026 CPU seconds); do not add it again.
Release includes all 28,656 released handles, not just the 5,226 result handles.

At one worker, 5,226 argument graphs contain 30,519,098 nodes and 2,088,147,692
wire bytes; returned graphs add 127,734,100 bytes. The `&env` root contributes
30,324,634 nodes (99.36%) and 1,961,646,056 bytes (93.94% of input bytes).
Argument construction makes 61,295,008 node/work polls and 20,462,370 byte
polls. Four/eight workers change environment inventory volume slightly, by less
than 0.13%, while argument CPU grows thirteenfold. This is not thirteenfold
additional useful graph work.

The **whole borrowed arena hypothesis is ruled out for this path**: form
traversal visits 194,464 reachable occurrences, against 330,270 nodes summed
over the input arenas. `Form::clone` retains an `Arc`; it does not copy the
arena. Argument roots after `&form` and `&env` reuse the existing form memo.
Within each argument graph, datum traversal has 30,453,040 visits versus
30,421,943 distinct datum identities (sums across calls); only 0.10% is duplicate
within-call traversal. These counts do not claim global distinctness across
calls. The compiler constructs/caches environment components in
`MacroHost::compiler_environment`, but Goose transports the full environment
afresh. Result provenance checks compare 4,012,284,232 source-text bytes over
182,039 occurrences; their measured reconstruction cost is small beside input
construction. Source payload itself is 114,253,902 input bytes, not the bulk of
the two-gigabyte transport volume.

### Reversible interventions and follow-up decision

Three measured rounds per cell followed one excluded warmup, alternating
forward/reverse order over the same binaries. These runs have no sampling or
graph counters. They are **causal interventions, not production improvements**.
All 52 builds match complete JS, the five independent oracles and waste checks;
every Goose run has 5,226 jobs, 615 preparations and 672 exports. All admitted
pre-run quiet gates passed (minimum 90.64% idle). One gate rejected 88.35% idle
before starting a command; its event/log is retained and only the remaining
cells resumed after a passing gate. No completed timing sample was discarded.

Median wall / aggregate CPU, seconds; raw samples and spreads are archived:

| Diagnostic binary | Goose1 | Goose4 | Goose8 |
| --- | ---: | ---: | ---: |
| Unchanged control | 20.736 / 20.49 | 11.846 / 35.20 | 19.170 / 97.67 |
| Bridge fuel debit bypassed; clocks retained | 21.026 / 20.85 | 10.006 / 27.62 | 10.471 / 30.70 |
| Bridge clock read bypassed; fuel retained | 18.886 / 18.67 | 9.977 / 27.22 | 12.590 / 49.43 |
| Fuel field isolated in a 128-byte aligned wrapper; checks retained | 20.879 / 20.64 | 11.681 / 34.94 | 18.727 / 93.72 |

Contemporaneous unchanged V8 is 6.415 / 6.98 seconds. The shared fuel intervention
removes 68.6% of eight-worker aggregate CPU and 45.4% of wall, without improving
one-worker wall. Clock-read removal also helps, but padding alone does not remove
the scaling collapse. This establishes a material shared-control cost; it does
not prove a universal hardware cost model or that a safe fix will achieve the
bypass's performance. Both bypasses intentionally weaken one control and are
unsuitable production code. All probe changes were restored after each build.

**Follow-ups are finite and ordered:** light-cljs-757 implements coalesced bridge
polling or an equally small proven accounting approach, preserving one shared
allowance, exact logical charges for successful conversions, sticky failures,
bounded cancellation/deadline observation, nested conversion, cleanup and
full-form/provenance checks. Light-cljs-758 then measures the actual integrated
V8/Goose1/2/4/8 result with at least five quiet interleaved rounds. Both are in
`goose-generated-200k` with actual `model=gpt-6.1-sol`, `reasoning_effort=high`,
and independent review before landing. A remaining loss is acceptable; neither
pellet authorizes recursive optimization planning.

Searches included all statuses across the project and relevant groups. Open
579 concerns Pants' build-wide lock, not this control. Deferred 576's engine
form views and 445/458's environment omission/cache-history work are related
representation ideas, not fixes for this shared counter. Closed 733 establishes
the adapter and shared nested controls; 738 concerns the earlier serial VM;
750/751 establish the baseline and completed milestone. No duplicate was found
for the measured shared-control cause. Full `&env` retransmission is substantial,
but omission, cross-call caching, a new graph representation and file-protocol
replacement remain separate design questions with semantic/identity/budget
obligations. Within-call memoization, whole-arena copying and result rebuilding
do not explain the measured growth. No speculative children were filed for them.

### Reproduce and preservation

[Investigation archive](generated-200k-bridge.edn.gz) uses the existing EDN
`:files`/UTF-8-text/SHA-256 archive format. It preserves raw profiles, counters,
all comparator/intervention samples, commands, logs, source patches, disassembly,
analysis/build scripts and fixture/toolchain/binary identities; all **324**
embedded digests were reread and verified. The profile
parser independently reconstructs leaf weights and checks them against every
entry in `sample`'s collapsed self table (**845 entries across 14 profiles**).
An initial sampling wrapper lost its
background child's stdin: three Goose attempts failed before target execution.
The wrapper was fixed with explicit stdin inheritance; those failures and the
two passing V8 setup controls are preserved, separate from the 68 successful
post-repair builds. All 70 successful builds validated the full output hash
`18335556dd4672f2f63b901b68fcad3c447cf5676f457a08fda61f366cc9f3ec`.

Use a fresh checkout of `324afa54` and the ordinary private build environment
above. Build the original CLI/native runner and generate `target/generated-200k`;
copy the CLI to `target/light-baseline`. Extract the archive's top-level `.bb`
and `.sh` files into `target/`, then:

```sh
chmod 700 target/light-sampled.sh target/native-sampled.sh
bb benchmarks/generated-project/compare.bb target/generated-200k --light target/light-sampled.sh --backend all --workers 1,4,8 --runner target/native-sampled.sh --phase instrument --runs 1
mv target/generated-200k/macro-instrument target/generated-200k/sampled-baseline
bb target/probe756.bb instrument
bb benchmarks/generated-project/compare.bb target/generated-200k --light target/light-instrument --backend all --workers 1,4,8 --runner target/generated-200k-runner --phase instrument --runs 1
bb target/summary756.bb profiles target/generated-200k/sampled-baseline
bb target/summary756.bb counters target/generated-200k/macro-instrument
bb target/probe756.bb no-fuel
bb target/probe756.bb no-clock
bb target/probe756.bb padding
bb target/interventions756.bb
```

`probe756.bb` builds an isolated diagnostic binary and restores source in
`finally`; never use its bypasses for acceptance. If a pre-command quiet gate
stops the series, `interventions756.bb --resume` verifies binary/fixture/harness
identities and skips only already completed successful cells. Do not run builds
or profile analysis concurrently with quiet measurements. Archive assembly is
separate and expects the recorded evidence filenames; the native runner's hash
is unchanged from 750, `92140b1a31ba1fe42beadc3caae59fb6ea0916e4a3204e5caff10ef7146688e0`.
The rebuilt unchanged Rust CLI hash is
`aa73779aa6111adc6358d46f54df714779bd809dc411deeb7c523f7727483ed5`;
source changes since verified `6bf69b39` are documentation/harness only, and
execution source hashes remain equal. Machine/toolchain match the earlier run.

The latest authorizing user turn and the original architecture discussion were
retrieved directly; the prior conversation audit is reused rather than repeated.
Generator/comparator regression passes **12 tests / 369 assertions** and
`git diff --check` passes. All four diagnostic Rust variants build successfully.
No runtime changes remain, so the exact-source 749 full-form/debug/native/teardown
and expanded-control evidence remains valid. The probes are positive-workload
causal evidence, not new proofs of failure behavior. No fresh full-suite or
strict-Clippy pass is claimed; the previously disclosed baseline limits remain.

## Bounded shared-control coalescing

Pellet 757 implements the bounded fix at `b521b299`: bridge budgets accumulate
actual logical polls locally, debit the shared session control every 128 checks,
and settle the final tail before VM use or compiler-owned result publication.
The existing `poll()` remains a one-poll debit for other engine/control paths.
There are no reserved credits, refunds, locks or per-worker allowances. Successful
conversion charges exactly the original number of node/work and byte checks.
Nested callbacks reuse their existing budget; distinct budgets share the same
session allowance. Error exits settle their actual tails, including final drop.
A failed aggregate debit sets the existing sticky exhaustion state without
underflow; a caught error cannot make another result publishable.

At most 127 polls per live conversion budget can be unpaid between checkpoints.
Cancellation/deadline checks occur at the start, every 128 logical checks, and
all settlement boundaries. This bounds additional pure conversion work, rather
than promising a wall-clock latency for one scalar copy/comparison. The existing
100,000-node/default-depth-128 limits, 16 MiB graph limit and 64 MiB aggregate
byte/work allowance remain unchanged. No unpaid graph reaches the VM, and no
unpaid returned form is accepted. No quota, environment representation, protocol,
Goose runtime or heap-sharing policy changed.

### Clean before/after target measurements

Both Rust CLIs were built in the same private worktree with release optimization,
jobs=3 and the required V8 cache: before `c21a03cc`, after the implementation
above. The unmodified seed42/200k generator produced the pinned 199,129 nonblank
lines, 450 worker namespaces and 653 source files. Compiler slots and execution
workers both equal the table's worker count. The native runner remains
`92140b1a31ba1fe42beadc3caae59fb6ea0916e4a3204e5caff10ef7146688e0`.
The archive records all source, fixture, native/Rust binary and toolchain hashes.

One warmup per cell preceded three alternating-order measured rounds. The quiet
gate admitted every command (minimum 90.63% idle); no builds or other tests ran
during this series. All **28 builds** (including four V8 controls) matched the
complete emitted JS hash `18335556dd4672f2f63b901b68fcad3c447cf5676f457a08fda61f366cc9f3ec`,
all five independent oracle pairs, and strict waste checks. Every Goose run
retained 5,226 jobs, 672 exports and 615 preparations.

Median seconds, clean uncached full target:

| Goose workers | Before wall | After wall | Before aggregate CPU | After aggregate CPU |
|---|---:|---:|---:|---:|
| 1 | 20.573 | 18.752 | 20.28 | 18.46 |
| 4 | 11.590 | 9.505 | 34.50 | 25.50 |
| 8 | 18.749 | 9.901 | 95.31 | 28.43 |

After wall ranges were 18.734–18.889 / 9.502–9.514 / 9.871–9.970 seconds.
The V8 control median was 6.374 seconds, so V8 remains faster. This finite
three-round implementation validation does not replace 758's final five-round
V8/Goose1/2/4/8 matrix. CPU is macOS compiler-command rusage including waited
native children; peak RSS is not an additive process-tree peak. No overlapping
phase intervals are added or subtracted.

### Separate control and argument evidence

The same graph counters and thread-CPU instrumentation used in 756 were applied
separately, with additional counters for actual aggregate debits. Both warmup
and measured instrumented builds passed complete output/oracle/waste validation.
The prior counter measurements are reused by execution-source identity:
`6bf69b39..c21a03cc` changes only documentation/archives in `src` and the engine,
and the native runner hash is identical. These are diagnostic measurements,
separate from clean wall timings.

| Workers | Prior argument thread CPU (s) | After argument thread CPU (s) | Logical bridge polls paid | Nonzero aggregate debits |
|---|---:|---:|---:|---:|
| 1 | 5.679 | 3.828 | 83,786,909 | 660,198 |
| 4 | 13.984 | 5.261 | 83,851,130 | 660,714 |
| 8 | 74.468 | 5.728 | 83,898,527 | 661,096 |

Each of the 5,226 budgets' charged count equals its node/work plus byte checks;
the summary also verifies this independently in aggregate. At eight workers,
nonzero shared debits are 0.788% of the original per-check debit count. About
30.5 million argument nodes are still built, with the full environment still
dominating the volume. Small volume differences between worker counts follow
the compiler's concurrent environment schedule; no input family was removed.
This confirms the identified shared-control work decreased without a fuel or
clock bypass.

### Correctness, limits and reproduction

Focused deterministic tests cover exact shared boundaries at 1/2/4/8 consumers,
aggregate charging, sticky exhaustion without wraparound, distinct and reentrant
nested budgets, failed-tail cleanup, cancellation/deadline interruption during
large graph construction, and unpaid-tail rejection before execution/publication.
Fresh-session compilation succeeds after these failures. Existing malformed,
truncation, UTF-16, number-bit, borrowed-flavour, identity and caught nested-limit
coverage remains enabled.

`compiler.bb` passed native debug and optimized modes (40/41 Rust tests; one
additional Rust graph test was added between modes and is covered by the final
focused runs). `generated_200k_complete_forms` passed with optimized and debug
native runners at 1/2/4/8 after teardown: all 5,226 forms, 615 identities, and the
existing 82 narrowly audited V8 reference alias ambiguities, 43 raw/22 common
compiler-input/65 returned anonymous-parameter checks and five scalar occurrence
checks remain unchanged. Optimized/debug complete audits took 123.50/184.55
seconds; these are correctness runs, not timing cells.

The final focused default and QuickJS runs both pass all eight bridge/control
tests, including native tail-payment rejection and fresh compilation. Test-only
commit `242dc995` removes the new native test's V8 gate; production behavior is
unchanged, so the completed full-form and timing layers are reused by relevant
source identity. The earlier feature-enabled library runs have 550 default and
502 QuickJS passes, with the exact documented namespace-publication failure
(`namespace writer needs a native engine`) in both. Default also reproduces the
capture fixture's generation mismatch: only data nodes 26/69/112 differ, expected
1 versus actual 215. The isolated `source_corpus` passes. Relevant failure sources
and fixtures are unchanged; neither library run is represented as a full pass.

Expanded seed24301 controls pass in native optimized and debug modes: each runs
all 20 existing supported cells, including the 120-call/8192-read branch control
at 1/2/4/8 workers. The existing declared heavier retained-serial budget failure
remains outside that matrix. The first debug invocation stopped at the existing
output-preservation guard; the tracked debug archive was moved aside, restored
unchanged, and the fresh debug run passed. Both fresh sample sets are retained.

Changed-file rustfmt and `git diff --check` pass. Repository-wide rustfmt still
reports unchanged files beginning with `src/js_syntax.rs`. Strict all-target
Clippy with Goose enabled in default and QuickJS modes stops at the same eight
engine diagnostics described above. Additional strict `--no-deps` runs expose
the pre-existing `compiler_bootstrap.rs:822/878` byte-character-slice lints and
`namespace_publication.rs:251` single-element loop; both modes reproduce these
exact diagnostics after temporarily restoring all three changed Rust files from
`c21a03cc`, then restoring the final source. No source lint allowances were added.
Full nextest/integration suites were not rerun; the prior broader dead-code,
analyzer and playground-lint limitations remain limitations, not passing checks.

[Coalescing raw evidence](generated-200k-coalescing.edn.gz) preserves the existing
EDN `:files`/UTF-8-text/SHA-256 format: clean commands/samples and quiet gates,
complete validation results, separate graph/control/native instruments, source
patches, compiler/form/control logs, binaries' hashes and toolchains.
All 170 embedded file digests were reread and verified after assembly. The initial
verification wrapper failed before starting a test because a command vector was
passed to `shell` as one argument; its failure is retained separately and the
corrected wrapper uses `apply`. No target failure is hidden by that repair.

To reproduce, build the before/after CLIs and both native runner modes with the
ordinary environment above, generate the pinned fixture, then extract the
archive's `target/series757.bb`, `target/instrument757.bb` and verification files.
Run the series with no competing build/test. `series757.bb --resume` validates
binary/fixture/harness identities before resuming any missing successful cells.
`instrument757.bb instrument` temporarily instruments the current source and
restores it in `finally`; preserve its patch, and run the existing comparator's
instrument phase separately. The complete-form test can also be invoked through
`cargo test --release --locked --features goose-macros --lib generated_200k_complete_forms -- --include-ignored --nocapture --test-threads=1`
with each `GOOSE_COMPILER_RUNNER`. The archived verification command records the
exact prebuilt Rust test-binary hash used here.
