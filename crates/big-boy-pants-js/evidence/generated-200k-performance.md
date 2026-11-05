# Generated 200k end-to-end performance

November 4, 2026 UTC. **The integrated bridge optimization improves Goose wall
and CPU time; V8 still wins every full-project cell.** The final five-round
comparison gives Goose4 **9.516 s versus V8's 6.319 s**, 1.51× the wall time
and 3.71× the aggregate CPU. Goose8 takes 9.916 s, with 47.0% less wall time
and 69.9% less CPU than its historical pre-coalescing result. This completes
the finite bridge investigation with supported compilation and a useful
optimization, while retaining the unfavorable comparison with V8. See the
[final integrated measurement](#final-integrated-five-round-measurement).
A later [factorial follow-up](#independent-compiler-slots-and-native-capacity)
separates compiler slots from native capacity: extra compiler slots account for
most of the replicated four-to-eight slowdown. The
[measurement without V8](#goose-compiler-path-without-v8) (November 5) supersedes
the Goose timings: Goose4 7.028 s against V8's 5.762 s, and a four-to-eight
compiler-slot penalty of 0.066 s.

## Historical clean primary comparison, before coalescing

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
goose build crates/big-boy-pants-js/src/compiler_session.goose -o target/generated-200k-runner
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

## Final integrated five-round measurement

Pellet 758 measures production source `2209df6f7f3d06395dbe04c97ed9eed65064c135`,
tree `2c1152f345202971c9ef75f8806300731e712e5c`, after independent review of
757. No runtime, generator, budget, cache, fixture or comparison-harness source
changed. A fresh seed42/200k generation matches all **653 canonical files/shims**,
199,129 nonblank lines and 450 worker namespaces; manifest SHA-256 remains
`6a4da9ff2d8682c3e524d86abb36a1628131cae16bcbb0220797b4f85c131907`.
All source/configuration/tooling hashes are checked before and after every build.

The existing `compare.bb --backend all --phase measure --runs 5` ran one
excluded warmup per cell followed by five forward/reverse interleaved rounds.
All **30 builds** passed, with no failed/discarded samples or rejected gates;
every pre-command gate admitted **91.81–97.80% idle**. No competing build/test
ran. Every command is a fresh process, release/advanced, `--no-cache`, with
compiler slots equal to the execution limit. Whole-namespace mode stays **off**;
the 450 generated worker namespaces are source population. These cells vary
compiler and Goose capacity together and do not isolate VM scaling.

Seconds, median [minimum, maximum]; RSS is decimal GB of command rusage.
Aggregate CPU is user+system including waited children; command RSS is
**not a simultaneous sum of compiler/native memory**.

| Engine / execution limit | Compiler slots | Wall | Aggregate CPU | Command RSS |
| --- | ---: | ---: | ---: | ---: |
| V8 / 1 | 1 | 6.319 [6.240, 6.381] | 6.85 [6.79, 6.94] | 1.041 [1.033, 1.054] |
| Goose / 1 | 1 | 18.786 [18.684, 18.987] | 18.52 [18.47, 18.69] | 0.965 [0.961, 0.966] |
| Goose / 2 | 2 | 13.431 [13.288, 13.607] | 21.64 [21.46, 21.93] | 1.041 [1.030, 1.060] |
| Goose / 4 | 4 | 9.516 [9.445, 9.695] | 25.41 [25.39, 25.71] | 1.144 [1.128, 1.155] |
| Goose / 8 | 8 | 9.916 [9.802, 9.948] | 28.45 [28.35, 28.66] | 1.277 [1.257, 1.287] |

Relative to the historical 750 five-round table, Goose1/2/4/8 wall decreases
6.3% / 5.6% / 17.5% / 47.0%; CPU decreases 6.3% / 6.8% / 25.6% / 69.9%.
V8 is 1.7% slower in this later run. Hardware, toolchain, generator, optimization,
budgets and native binary match, but these are separately built compiler
binaries at different times and revisions. The contemporaneous 757 before/after
control above gives the stronger causal comparison: Goose1/4/8 wall
20.573→18.752 / 11.590→9.505 / 18.749→9.901 s and CPU
20.28→18.46 / 34.50→25.50 / 95.31→28.43 s. The present five-round results
agree with that improvement. They do not equate a historical timing difference
with an isolated measurement of the changed instructions.

Goose2/4/8 whole-build speedups over Goose1 are 1.40× / 1.97× / 1.89×;
four workers are fastest. Even this best cell takes 1.51× V8 wall and 3.71×
V8 CPU. The bounded optimization removes the eight-worker collapse and yields
useful full-build savings, but does not make Goose preferable to V8 for this
target. This finite investigation is complete; no speculative optimization or
planning pellets were created.

### Separate phase, concurrency and memory diagnostics

One warmup and one repeat in each V8/Goose1/2/4/8 cell used the existing
`phase750.bb` native instrument, full protocol, inner native rusage and owned
process-tree sampler. All ten outputs passed. Each native session confirms
**5,226 source jobs, 615 preparations, 672 exports**, 529 resumptions and 57
helper producers: 5,812 distinct dispatches, including 5,755 body intervals.
Every qualified macro identity/count table equals V8. All workers execute/reset
work; total native threads are 4/6/10/18 and all 3/5/9/17 spawned threads join.
The unchanged supported source-job boundary has no fallback or pre-expansion.

Repeat-run aggregate elapsed intervals, milliseconds, **not CPU or exclusive
wall components**. They overlap across workers; do not sum them into wall,
subtract waits, or infer idle CPU from their difference.

| Goose limit | Restore | Argument/helper import | VM slices | Continuation save | Native export | Rust bridge |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 151.1 | 2425.5 | 71.6 | 72.1 | 590.2 | 3836.2 |
| 2 | 150.5 | 2496.8 | 73.0 | 75.6 | 715.8 | 4314.4 |
| 4 | 157.3 | 2575.0 | 75.5 | 79.8 | 821.4 | 5120.8 |
| 8 | 162.0 | 2629.2 | 78.3 | 81.4 | 862.8 | 5437.9 |

Restore includes validation/relocation and **995,474,716 copied payload bytes**
per session. Import includes input read/decode and helper installation; native
export includes graph conversion/writes. Whole command CPU charges the remaining
queue/file/protocol copying. Rust bridge excludes the explicit native wait but
its concurrent intervals remain elapsed timers. Source-macro service totals
11.903/13.047/15.422/16.358 s are inclusive and overlap too. Native-only CPU is
5.26/5.27/5.57/5.70 s; VM slices remain a small measured component.

Body-overlap warmup/repeat peaks are **1/1, 2/2, 2/3, 2/3** at 1/2/4/8.
Repeat body interval medians are 13/13/13/14 microseconds. These are elapsed
lifetimes, not simultaneous instructions; this does not demonstrate eight-way
VM saturation. The compiler still owns reading/resolution, core expansion,
orchestration, analysis and emission, with dependency-ordered namespaces and
deferred unit concurrency. Additional worker capacity does not lengthen the
short source bodies.

Charged setup/cleanup remains explicit: Rust export/lowering/write 521–635 ms;
session spawn 0.45–0.53 ms, base load 0.061–0.084 ms, boot 0.003–0.004 ms,
base capture/pool readiness 0.296/0.627/0.800/2.900 ms, prepared load totals
43.7–48.9 ms, helper factories 0.161–0.196 ms, and close/join/readers/directory
cleanup 37.7/40.2/45.0/53.1 ms. Pool readiness includes worker setup; these
are overlapping charged boundaries rather than an additive startup partition.

V8's diagnostic source-macro service is **1.144 s**, approximately 17.4% of
its 6.560 s measured instrumented command. This inclusive boundary share is
not the whole replaceable fraction. V8 cold bootstrap costs 11.108 ms and
evaluator/prelude creation 8.448 ms, with overlap. Goose retains V8 core/bootstrap
installation counts 1/3/5/9 and no target `expand_core_javascript` calls.
This remains **Goose source-macro execution with V8 core/bootstrap**; all its
costs remain in command timing.

| Diagnostic repeat | Sampled owned-tree RSS peak, GB | Full native-process RSS peak, MB |
| --- | ---: | ---: |
| V8 1 | 1.095 | n/a |
| Goose 1 | 1.131 | 140.1 |
| Goose 2 | 1.226 | 173.1 |
| Goose 4 | 1.359 | 205.2 |
| Goose 8 | 1.559 | 274.6 |

Tree RSS is sampled roughly every 100 ms plus `ps` overhead, includes wrappers,
counts shared pages in each process and can miss peaks. Native RSS is inner
command rusage for the complete instrumented native process, including probe
storage. These are separate scopes/runs; do not add maxima or call them clean
memory measurements. The earlier exact-native safety/retention evidence remains
valid; no retention quota or heap lifetime policy changed.

### Bounded shared-poll confirmation

A separate fresh Rust counter binary, using 757's unchanged reversible
`instrument757.bb instrument` patch, ran a Goose8 warmup and repeat with the
production native runner. Both passed. In the repeat, all **5,226 budgets** pay
exactly their node/work plus byte checks: **83,898,311 logical polls through
661,086 nonzero shared debits**, 0.788% of the original per-check debit count.
Warmup independently pays 83,898,026 polls through 661,091 debits. No fuel,
deadline or cancellation bypass is used.

Repeat argument-conversion thread CPU is **5.808 s**, versus 757's 5.728 s and
756's unchanged shared-control baseline 74.468 s at eight workers. This is a
bounded confirmation of the identified hotspot reduction, separate from clean
timing; it is not a fresh statistical profile. Roughly 30.56 million argument
nodes and the full environment are still transported. Small scheduling-dependent
volume differences remain visible rather than removing input work. Native VM
source/binary, accounting semantics, 128-check debit cadence, settlement
boundaries and the maximum 127 pending polls are unchanged from reviewed 757.

### Reproduction, evidence reuse and remaining limits

The [integrated raw archive](generated-200k-integrated.edn.gz) preserves the
existing EDN `:files`/UTF-8-text/SHA-256 layout: all samples, gates, commands,
stdout/oracle/waste records, stderr/rusage, complete protocol, native phase
records, process-tree samples, scripts/patches, canonical sources/shims,
generator, independent model/oracle, configuration and manifests. Every embedded
digest is reread after assembly (**847 embedded files**). No failed build or
favorable-sample selection is hidden. All **42 successful builds** (30 clean,
ten phase diagnostics, two
counter diagnostics) match every one of the **3,449,022 JS bytes**, SHA-256
`18335556dd4672f2f63b901b68fcad3c447cf5676f457a08fda61f366cc9f3ec`,
all five independent oracle pairs including newlines, and strict waste checks.

The clean CLI hash is
`8a08b23cb3599f5d9f12dce45580b65cac3eea47fb1046f06dba4f7e05dce3e2`;
native production remains
`92140b1a31ba1fe42beadc3caae59fb6ea0916e4a3204e5caff10ef7146688e0`;
phase-native remains
`688de47c75b97e112361adc04c99ce48c002a7cda3220c79c6a599a65f6cc60d`.
The archive records the separate Rust counter binary, executable toolchain
digests and versions, 580 execution/build/source hashes, source tree, environment
and absence of inherited build overrides. Hardware remains M3 Ultra Virtual,
24 logical CPUs, 32 GiB, macOS arm64; Goose267/runtime3025e5810379, Rust1.99.1,
Node26.12.1, bb1.12.218 and clang21 match the preceding runs. The research
checkout is still `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`.

All **580 source hashes match 757**, with the identical production native binary,
toolchain and relevant build configuration. Its reviewed full-form/provenance
verification is reused: native optimized/debug 1/2/4/8, 5,226 expansions across
615 identities after teardown, complete forms/scalars/UTF-16/metadata/provenance,
and exactly the existing 82 narrowly audited V8 span ambiguities. The 43 raw /
22 common compiler-input / 65 returned anonymous-parameter checks and five
equal-scalar occurrence checks remain unchanged. The archive identifies the
actual prebuilt Rust full-form test binary, not this fresh CLI. Native debug
and optimized compiler, settlement/limit/failure/cleanup tests, default/QuickJS
eight-test bridge runs and all 20 expanded-control cells per native mode are
also reused. The seed24301/15-namespace/120-call/8192-read control has no new
timing claim. No source changed that would invalidate those checks.

No new cached or leaf matrix is run: cache implementation/source is unchanged,
and Goose cached installation remains explicitly unsupported. Historical V8
cached/no-change, cached-leaf and separately labeled uncached-leaf evidence
above remains historical; skipped macro execution is never a Goose speedup.
Generator/comparator regression passes **12 tests / 369 assertions** and
`git diff --check` passes. No fresh broad-suite, root-format or strict-Clippy
pass is claimed. The matching 757 default/QuickJS library namespace-publication
failure, default capture generation mismatch (isolated capture passes), eight
engine/three root strict-Clippy diagnostics, unrelated root formatting and
earlier analyzer/playground limits remain explicit; full nextest was not rerun.

Use the ordinary fresh-checkout build/generate recipe above, then:

```sh
cp target/release/light-cljs target/light-production
bb benchmarks/generated-project/compare.bb target/generated-200k --light target/light-production --backend all --runner target/generated-200k-runner --phase measure --runs 5
```

Extract `target/phase750.bb`, `target/instrument757.bb`, `target/summary756.bb`,
`target/summarize758.bb` and `target/archive758.bb` from the integrated archive.
Run the native phase builder and comparator instrument command separately, then
the Rust probe with **`instrument` only** and its Goose8 comparison. Exact
commands/moves are in archive `:commands`; the builders restore source in
`finally`. The archived bypass implementations from 756 are diagnostic history
and never enter these production measurements. Run summaries/archive assembly
after timing. All useful raw evidence is committed; ignored build products and
extracted copies contain no unique evidence needed before worktree retirement.

## Independent compiler slots and native capacity

November 4, 2026 local / November 5 UTC, execution source
`6c9bf60f2bddd742206881a59a53f04068a7e60e`. **Increasing compiler slots accounts
for most of the replicated Goose4-to-Goose8 slowdown.** The preceding tables
changed two controls together. This follow-up varies them independently;
it includes no runtime optimization and does not replace the historical data.

[`--jobs`](../../../src/main.rs) sets compiler CPU work slots, including
the calling thread. The [experimental CLI](../../../src/goose/compiler_cli.rs) independently reads
`LIGHT_CLJS_GOOSE_WORKERS` when starting the shared native session. The comparator
now accepts `--compiler-slots 4,8 --workers 4,8` for their Cartesian product;
the original coupled defaults, filenames and existing results are preserved.
The experimental whole-namespace worker path remains off. The workload remains
seed42/199,129 lines/450 worker namespaces/653 files, release/advanced/uncached,
with source macros in Goose and retained V8 core/bootstrap.

Each cell has one excluded warmup and five measured samples, interleaved in
forward/reverse round order. No competing build or test ran during timing.
All admitted pre-run gates were **93.99–98.80% idle**. One **88.50%** gate stopped
before launching round3's `(4,4)` command: the 19 completed builds were retained,
and an archived recovery script ran only the five missing samples in the
original order. No completed sample was discarded or rerun for its speed.

Seconds, median [minimum, maximum]. Aggregate CPU is each command's user+system
CPU, including waited children; it is not a sum of overlapping phase intervals.

| Compiler slots | Native workers | Wall | Aggregate CPU |
| ---: | ---: | ---: | ---: |
| 4 | 4 | 9.923 [9.882, 9.944] | 26.59 [26.51, 26.81] |
| 4 | 8 | 9.992 [9.841, 10.013] | 26.82 [26.53, 26.86] |
| 8 | 4 | 10.240 [10.170, 10.420] | 29.35 [29.23, 29.76] |
| 8 | 8 | 10.318 [10.275, 10.350] | 29.46 [29.31, 29.63] |

Differences of cell medians isolate the two settings. Compiler slots4→8 adds
**0.318/0.326 s wall** and **2.76/2.64 s CPU**, at native capacity4/8 respectively.
Native capacity4→8 adds **0.070/0.078 s wall** and **0.23/0.11 s CPU**, at compiler
slots4/8. The compiler-slot increase is slower in every paired round at both
native capacities. Native-capacity wall differences have mixed signs across
rounds and overlapping ranges; five samples do not establish a precise small
native-worker penalty. They show no native-capacity speedup on this target.
The full coupled difference is 0.396 s (~4.0%); the wall difference of differences
is only 0.008 s. The archive preserves both raw samples and paired contrasts.

This identifies the dominant **configuration effect**, not a specific CPU
hotspot. Extra scheduling/coordination, retained compiler isolates, and remaining
transport contention are mechanism hypotheses; this experiment does not assign
their CPU shares. The original setup coupled two independent controls, so its
four/eight comparison could not by itself attribute the loss to native VM scaling.
Absolute times differ from the earlier session; comparisons here use this
interleaved factorial rather than subtracting historical medians.

Separate diagnostics confirm that the controls actually produce different pools:
four/eight compiler slots create **three/seven deferred-unit pool threads**, all
with engines and participating in work, plus the calling thread. These counts
exclude auxiliary compiler threads. Native capacity4/8 creates **10/18 native
threads** including its coordinator; all **9/17 spawned threads join**, and each
private worker receives work. All source-macro identity/count tables match fresh
V8 diagnostics at both compiler-slot counts.

| Compiler slots / native workers | Body overlap peak, warmup / repeat | Repeat VM slices, aggregate elapsed ms |
| --- | ---: | ---: |
| 4 / 4 | 3 / 3 | 79.848 |
| 4 / 8 | 2 / 3 | 81.871 |
| 8 / 4 | 3 / 3 | 81.757 |
| 8 / 8 | 3 / 3 | 82.777 |

Each native diagnostic has all **5,812 distinct dispatches** (5,226 initial,
529 resumptions, 57 helper producers) and **5,755 body intervals**. Repeat body
medians are 14/15/14/14 µs in table order. Body lifetimes exclude restore/import,
helper factories, save and export; their overlap is elapsed concurrency, not
simultaneous CPU execution. VM slice sums may overlap and include descheduling;
they are neither CPU nor an additive wall-time partition. These diagnostics
again show short bodies and no eight-way body saturation. Their startup,
native-only rusage, per-worker counts, compiler pool counters and owned-tree RSS
remain in raw evidence, separate from the clean timing table.

The [new raw archive](generated-200k-concurrency.edn.gz) uses the existing
`:files`/UTF-8 text/SHA-256 layout and preserves every sample, rejection/recovery
log, command, selected environment/build overrides, source/binary/toolchain
identity, canonical fixture and independent model/oracle, phase/protocol records,
process-tree samples, and reproduction/analysis scripts. Every embedded digest is
reread and verified. All **38 builds** (24 clean, 12 phase diagnostics, two V8
reference builds) pass the full **3,449,022 output bytes**, SHA-256
`18335556dd4672f2f63b901b68fcad3c447cf5676f457a08fda61f366cc9f3ec`, all five
independent oracle outputs including newlines, and strict waste checks.

All **580 execution/build source hashes match 757/758 before and after**.
Production native remains
`92140b1a31ba1fe42beadc3caae59fb6ea0916e4a3204e5caff10ef7146688e0`, phase native
remains `688de47c75b97e112361adc04c99ce48c002a7cda3220c79c6a599a65f6cc60d`, and the
fresh CLI is `02e4d7fae8e254e08d00fe99fdcf4325fa5286dfa6115b2ff4a6a5176d977f6a`.
Goose267/runtime3025e5810379, Rust1.99.1, Node26.12.1, bb1.12.218, clang21,
hardware and research revision match the preceding run. The reviewed complete-form,
scalar/UTF-16/metadata/provenance/teardown tests, 82 audited V8 span ambiguities,
debug/optimized 1/2/4/8 coverage, settlement/limit/failure/cleanup checks and expanded
control are reused by those exact identities. No new cached/leaf scenario or
broader correctness claim is made. Generator/comparator regression passes
**13 tests / 383 assertions**; `git diff --check` passes. The documented broad-suite,
strict-Clippy, root-format, capture and playground limitations above remain;
those suites were not rerun for this harness/report change.

After the existing fresh-checkout build/generate recipe, reproduce the primary run:

```sh
bb benchmarks/generated-project/compare.bb target/generated-200k --light target/release/light-cljs --backend v8 --phase check --runs 1
bb benchmarks/generated-project/compare.bb target/generated-200k --light target/release/light-cljs --backend goose --workers 4,8 --compiler-slots 4,8 --runner target/generated-200k-runner --phase measure --runs 5
```

Separately extract `target/phase750.bb`, `target/summarize-concurrency.bb` and
`target/archive-concurrency.bb` from the new archive, then run the phase builder,
the same comparator with `--backend all --runner target/phase750-runner
--workers 4,8 --compiler-slots 4,8 --phase instrument --runs 1`, and the two
analysis/archive scripts. The phase builder restores source in `finally`.
`target/resume-concurrency.bb` records the exact checkpoint recovery used here;
it skips only successful cell/round identities and retains all checks.

## What the extra compiler slots actually do

November 4, 2026, execution source `bbf05c0f6631cb342795b5af2d99ef0e75776fe1`.
**Repeated compiler catalog installation is a demonstrated contributor, but this
investigation does not establish the cause of most of the remaining CPU cost.**
The fixed-native4 comparison uses the same seed42/200k/653-file target,
release/advanced/no-cache settings and current batched fuel accounting. No
runtime change or optimization is shipped.

Five fresh quiet, interleaved rounds (one excluded warmup per cell) confirm the
effect: compiler slots4/8 have median wall **9.756/10.264 s** and aggregate CPU
**26.31/29.25 s**, differences **0.508 s wall / 2.94 s CPU**. These are a new
paired session, not subtractions from historical measurements.

The retained V8 work is inside the measured Goose build. It is not the separate
reference backend, and it is not required by Goose itself. The prototype's
[CLI](../../../src/goose/compiler_cli.rs) calls `use_goose_source_macros`; that
[method](../../../src/macro_host.rs) sets `EngineBinding.goose` alongside the
existing `bootstrap`, rather than replacing the compiler evaluator. Consequently
`new_compiler_evaluator` and
[`worker_view`](../../../src/compilation_session.rs) still construct V8.
`MacroHost::bind` installs the cached core/sequence bootstrap definitions in each
instance; `prepare_engine` installs `compiler_roots.js`, catalog/build services
and runtime state. `WorkerView::analyze` calls `install`, which replays each
worker's pending root and declaration-bundle registrations into that instance.
`analyze_unit` also sets and restores its declaration frontier and gensym scope.
These are real V8 operations, even when no target macro body needs that instance.

Core macros first try the [Rust expansion path](../../../src/analyzer.rs).
The remaining [`expand_core_javascript`](../../../src/compiler_bootstrap.rs)
fallback explicitly excludes a `source_only` engine, so it retains the bootstrap
V8 capability. In contrast, `expand_source_javascript` explicitly selects
`EngineBinding.goose`; the [Goose evaluator](../../../src/goose_evaluator.rs)
executes those source macro/helper bodies in the native session. On this target,
the detailed general macro-expansion counter is **5,226**, exactly the **5,226
Goose calls**, and the prior exact-source macro probes have no `core/` entries.
Thus the observed V8 work here is bootstrap, catalog and compiler bookkeeping;
core fallback is a retained capability, not an observed target-body execution.

A separate diagnostic uses thread-local counters and macOS thread CPU clocks
(not elapsed waits). Four/eight slots create three/seven compiler worker views.
The number of catalog registrations installed rises **87,552→175,056**, including
**21,020→42,032 bundle registrations**. Each engine catches up its own catalog;
the extra workers therefore repeat registration work. Bundle registration formats
JavaScript and passes it through `evaluate_unit`/release. Retained V8 evaluated
source rises **26.64→53.20 MB** across **21,056→42,100 calls**; these are largely
registration programs, not duplicated execution of Goose macro bodies.

In the detailed measured pair, catalog-install CPU is **1.110→2.037 s (+0.927 s)**.
Foreground V8 entry CPU rises **0.736 s**, including **0.499 s** in unit
evaluation; these scopes overlap registration and must not be added to it.
Worker-view creation rises only **0.060 s**, including **0.020 s** more bootstrap
installation. Aggregate command CPU rises **3.27 s** in this instrumented pair;
native-child CPU accounts for **0.18 s**, and measured compiler-owned thread CPU
accounts for **2.909 s**. The residual is about **0.18 s** of other/background or
end-of-process work. It is not evidence of a large hidden background-V8 cost,
and it cannot specifically be attributed to V8 compilation or GC. Sampled stacks
also show more V8 activity, but their sleeping and runnable observations are not
CPU accounting and are not used to assign the remainder.

A bounded diagnostic ablation skips `WorkerView::install` on Goose-bound workers,
while keeping their V8 instances/bootstrap, the coordinator catalog, native
capacity and Goose source-macro execution. This intentionally incomplete
implementation is only a probe for this target, not a generally correct fix.
All its completed builds preserve the full output and oracles. Four warmups and
two complete interleaved measured rounds give these **within-round slots8 minus
slots4 CPU differences**:

| Round | Unmodified CPU difference | Ablated CPU difference | Reduction |
| ---: | ---: | ---: | ---: |
| 0 | 2.52 s | 1.71 s | 0.81 s |
| 1 | 2.20 s | 1.90 s | 0.30 s |

Their mean reduction is **0.555 s**, about **24%** of those rounds' mean baseline
penalty. This supports a partial causal contribution; it is not interchangeable
with the separate instrumented **0.927 s** phase delta or a precise population
fraction. Wall reductions have mixed signs (**+0.054 / −0.217 s**), so this probe
does not establish an end-to-end wall improvement. The third ablation pair is
retained, but its baseline pair never launched: the quiet gate rejected at
**84.62%** idle, then recovery attempts at **75.71% / 13.77%** amid unrelated load.
No completed sample was discarded or rerun; there is no balanced three-round
estimate and no further measurement campaign.

Work counts rule out a large increase in target macro execution or argument
volume: coordinator/worker calls remain **1,140/4,086**, deferred-form analyses
remain **14,218**, and argument nodes rise only **30.541→30.556 million** (~0.05%).
The same-count argument conversion, result release and Goose worker-root replay
cost **0.309 / 0.187 / 0.203 s** more CPU in the detailed pair. These identify
where some cost is paid, not why each operation costs more. Allocation, reference
counting/atomics, locks, cache effects and scheduling remain unproven explanations
for that remainder. Whole-environment volume dominates conversion, but its size
alone does not explain the slots4/8 difference. The earlier per-poll shared-fuel
problem is already fixed and is not reintroduced or bypassed here.

The [raw archive](generated-200k-slot-costs.edn.gz) retains all **40 completed
builds** (12 clean confirmation, eight CPU diagnostics, four sampled, 14 ablation,
two reference), interrupted records, every sampled stack, exact patches and
restoring builders, commands, identities and analysis scripts. All builds pass
the complete **3,449,022 output bytes**, the preceding SHA-256, all five independent
oracles and strict waste checks. All **580 execution/build source hashes** and
toolchain binary hashes match the preceding archive; native binary remains
`92140b1a31ba1fe42beadc3caae59fb6ea0916e4a3204e5caff10ef7146688e0`, and this
worktree's clean production CLI is
`da9275674b82a54f96a095d841c9df0ad265097c323b6f82055218e35e74bcf3`.
Diagnostic source edits are restored. Exact-source complete-form/control evidence
and the preceding broad-suite limitations are reused; no broad tests or new
performance assertions are added for this report-only change. The archive is
reread with every embedded digest verified, and `git diff --check` passes.

## Goose compiler path without V8

November 5, 2026 (light-cljs-781), the final measurement for light-cljs-759.
**Removing V8 from the Goose compiler path made every Goose cell faster, used
less CPU and less peak memory, and shrank the four-to-eight compiler-slot
penalty.** Goose4 now takes **7.028 s against V8's 5.762 s** (1.22× the wall
time, 2.82× the aggregate CPU); before the change it took 8.848 s. V8 is still
faster on this target.

Binaries, all release builds with `--locked`:

| Label | Commit | Features | SHA-256 |
| --- | --- | --- | --- |
| predecessor | `5e77771d` (last master commit before light-cljs-776) | `goose-macros` | `b74332c218af56a7f02e05cd673ae4f565b62e1619ae52941e4436b1b93e9ea6` |
| integrated | `63386769` (master after 776–780) | `goose-macros` | `f0d5b042a8aedd1766253c978f75568bdbee8220a46e16c6a03f36b838bd279d` |
| Goose build | `63386769` | `--no-default-features --features goose-macros` (no V8 linked) | `e85537eab4c407ca2d76b05501a120a2d13ee6da1937dd243ab439b00bfbaf03` |
| runner | `compiler_session.goose`, identical at both commits | `goose build` | `cf38f8f648a3ad1bcf3f32a2d43d0c2618a80527496ffb64427f54b0fe3f57d6` |

The Goose sources under `crates/big-boy-pants-js/src` do not differ between
the two commits, so one runner serves all three binaries. The samples record
the harness commit (`6483f706`, master plus the compare.bb change below);
compiler sources are those of `63386769`. Toolchains: Rust 1.99.1, Goose
v2026.3-267 (runtime 3025e5810379), on the same machine as the sections above.
Workload: seed 42, 199,129 lines, 450 worker namespaces, 653 files,
release/advanced/`--no-cache`, checked against the frozen manifest.

Harness: compare.bb gained `--also-light LABEL=BINARY[,goose]`, which puts
another CLI's cells into the same rounds right after the primary cell of the
same setting and records its hash; `,goose` limits the Goose build to Goose
cells. The coupled matrix and the slot factorial are two invocations, each
with one excluded warmup per cell and five measured rounds in forward/reverse
order. Every quiet gate admitted on its first reading (96.9–99.9% idle for the
coupled matrix, 91.6–99.9% for the factorial). An earlier coupled attempt
stopped when one gate read **88.93%** idle after 17 measured samples; it is kept
in the archive, unused, and the gate now waits 5 s and retries (at most 12
times), keeping every rejected reading. No completed sample was discarded.

Seconds, median [minimum, maximum], five samples each. Aggregate CPU is the
compiler command's user+system CPU, waited children (the native runner)
included. Peak memory is macOS `time -l` maximum resident set of the compiler
command, in GB; it is not an additive process-tree peak and excludes the
separately reported native-runner statistics.

Coupled cells (compiler slots = native Goose workers; V8 has one execution
worker):

| Binary | Backend | Compiler slots | Native workers | Wall | Aggregate CPU | Peak memory |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| integrated | V8 | 1 | 1 | 5.762 [5.755, 5.952] | 6.27 [6.24, 6.45] | 1.048 [1.028, 1.053] |
| predecessor | V8 | 1 | 1 | 5.739 [5.719, 5.828] | 6.22 [6.21, 6.35] | 1.047 [1.037, 1.057] |
| predecessor | Goose | 1 | 1 | 17.242 [17.229, 17.391] | 16.95 [16.93, 17.18] | 0.971 [0.951, 1.003] |
| integrated | Goose | 1 | 1 | 14.503 [14.441, 14.573] | 14.12 [14.11, 14.14] | 0.910 [0.884, 0.910] |
| Goose build | Goose | 1 | 1 | 14.557 [14.497, 14.638] | 14.14 [14.13, 14.22] | 0.912 [0.899, 0.916] |
| predecessor | Goose | 2 | 2 | 12.278 [12.224, 12.467] | 19.68 [19.64, 19.81] | 1.027 [1.019, 1.044] |
| integrated | Goose | 2 | 2 | 10.084 [9.982, 10.322] | 15.47 [15.36, 15.65] | 0.926 [0.916, 0.958] |
| Goose build | Goose | 2 | 2 | 10.112 [10.026, 10.232] | 15.45 [15.32, 15.68] | 0.937 [0.936, 0.961] |
| predecessor | Goose | 4 | 4 | 8.848 [8.802, 8.960] | 23.61 [23.46, 23.78] | 1.121 [1.092, 1.139] |
| integrated | Goose | 4 | 4 | 7.028 [6.988, 7.114] | 17.69 [17.54, 17.73] | 0.947 [0.937, 0.973] |
| Goose build | Goose | 4 | 4 | 7.043 [6.979, 7.135] | 17.73 [17.63, 17.87] | 0.947 [0.920, 0.969] |
| predecessor | Goose | 8 | 8 | 9.238 [9.196, 9.514] | 26.89 [26.77, 27.30] | 1.250 [1.247, 1.284] |
| integrated | Goose | 8 | 8 | 7.155 [7.124, 7.330] | 19.71 [19.62, 19.76] | 1.001 [0.982, 1.030] |
| Goose build | Goose | 8 | 8 | 7.184 [7.113, 7.285] | 19.72 [19.63, 19.78] | 1.006 [0.978, 1.012] |

Integrated against predecessor, Goose 1/2/4/8: wall **−2.739 / −2.194 /
−1.820 / −2.083 s** (−15.9 / −17.9 / −20.6 / −22.5%), aggregate CPU **−2.83 /
−4.21 / −5.92 / −7.18 s**, peak memory **−0.061 / −0.101 / −0.174 / −0.249 GB**.
The ranges do not overlap in any cell. The comparison measures the five
children of light-cljs-759 together (compiler-owned helper-root catalog, core
macros in Rust, no bootstrap engine, every engine refused, the Goose build); it
does not split the saving between them. At one slot the predecessor had no
pool, so its saving there is coordinator-side work: its V8 coordinator and
bootstrap, catalog and core-macro fallback. The CPU saving grows with the slot
count, consistent with each extra predecessor slot having had its own V8
isolate catching up the catalog. The two V8 cells agree within 0.023 s, so the
compiler's V8 path did not change. The Goose build and the integrated binary
agree within 0.054 s wall and 0.04 s CPU in every cell, with overlapping
ranges: once the Goose path makes no engine, linking V8 or not makes no
measurable difference.

Slot factorial (native workers fixed at 4):

| Binary | Compiler slots | Native workers | Wall | Aggregate CPU | Peak memory |
| --- | ---: | ---: | ---: | ---: | ---: |
| predecessor | 4 | 4 | 8.925 [8.875, 9.330] | 23.71 [23.66, 24.06] | 1.119 [1.103, 1.128] |
| integrated | 4 | 4 | 7.065 [7.009, 7.186] | 17.70 [17.63, 17.81] | 0.956 [0.928, 0.968] |
| Goose build | 4 | 4 | 7.067 [7.050, 7.131] | 17.85 [17.58, 17.94] | 0.940 [0.922, 0.962] |
| predecessor | 8 | 4 | 9.226 [9.210, 9.255] | 26.70 [26.63, 26.88] | 1.279 [1.275, 1.291] |
| integrated | 8 | 4 | 7.131 [7.122, 7.237] | 19.58 [19.47, 19.80] | 1.007 [0.991, 1.013] |
| Goose build | 8 | 4 | 7.171 [7.111, 7.259] | 19.61 [19.52, 19.66] | 0.999 [0.983, 1.008] |

**The slot penalty**, compiler slots 4→8 at native workers 4: before, **+0.301 s
wall and +2.99 s CPU** (the earlier sessions above found +0.318/+0.508 s and
+2.76/+2.94 s); after, **+0.066 s wall and +1.88 s CPU** (Goose build +0.104 s,
+1.76 s). The wall penalty is now within the cells' spread. About 1.1 s of the
CPU penalty disappeared with the per-slot V8 isolates and their catalog
catch-up, more than the 0.555 s the earlier, partial catalog ablation removed. The remaining ~1.9 s of CPU is the extra
compiler workers themselves; this run does not assign it further.

**V8 isolate count.** Integrated and Goose build: zero. The instrument run's
Goose report for every Goose cell of both binaries reads `ENGINES v8-platform=0
v8-isolates=0 quickjs-runtimes=0 pants-runtimes=0` (compare.bb asserts it), and
separate `--stats` runs at slots/workers 1/1, 2/2, 4/4, 8/8 and 8/4 print
`engines.v8-isolates 0` and `pool.worker-isolates 0`. Predecessor, derived:
its `--stats` reports `pool.worker-isolates` 0 / 1 / 3 / 7 / 7 at those five
settings (one per pool worker, every worker with an engine); add the
coordinator's isolate, and the bootstrap-preparation isolate when the pool has
engines, as inventoried in light-cljs-759. That gives **1 / 3 / 5 / 9 / 9** V8
isolates. The bootstrap-preparation isolate is from that inventory; the
predecessor's statistics do not count it separately.

**Achieved concurrency.** Native Goose overlap comes from the instrument run's
complete native protocols (`.protocol`, all 5,812 job openings: 5,226 starts,
529 resumptions, 57 helper producers), counted with the event accounting of
`src/goose_evaluator.rs`: START/RESUME/PRODUCE open a job lifetime and
NEED/DONE/PRODUCED close it. These are overlapping job lifetimes, not
simultaneous VM instructions. Integrated, Goose 1/2/4/8 (Goose build in
parentheses):

| Native workers | Peak overlapping jobs | Mean active jobs at an opening | Openings at full width |
| ---: | ---: | ---: | ---: |
| 1 | 1 (1) | 1.00 (1.00) | 5,812 (5,812) |
| 2 | 2 (2) | 1.27 (1.27) | 1,552 (1,595) |
| 4 | 4 (4) | 1.77 (1.76) | 189 (177) |
| 8 | 8 (8) | 2.22 (2.23) | 6 (3) |

Every worker count reaches its full width at least once, but rarely: at eight
workers, 41% of openings find no other job active and only 6 of 5,812 reach
eight. The native session creates 4/6/10/18 threads, its coordinator included,
and all 3/5/9/17 spawned threads join. Separately, the compiler pool's own
statistics (`pool.utilization`: elapsed unit-analysis time, waits included,
over pool workers × pool time) are **0.729 / 0.757 / 0.380** at compiler slots
2/4/8 with 1/3/7 pool workers (Goose build 0.735 / 0.764 / 0.394), every pool
worker participating. Aggregate CPU over wall in the clean runs is **0.97 /
1.53 / 2.52 / 2.75** at Goose 1/2/4/8 (predecessor 0.98 / 1.60 / 2.67 / 2.91).
Together: going from four to eight adds width the build seldom fills, which
agrees with the run's DAG lower bounds (9.83 s at P=4 against 9.71 s at P=8 for
the drain-barrier model). That is a statement about this target's available
overlap as these measurements see it, not proof that no schedule could use
eight workers.

**Correctness.** All **197 builds** (28 check, 31 in the stopped attempt, 84
coupled, 36 factorial, 18 instrument) exit 0 and produce the complete
**3,449,022-byte** output, SHA-256
`18335556dd4672f2f63b901b68fcad3c447cf5676f457a08fda61f366cc9f3ec`, identical to
V8 with no normalization, pass all five independent oracle outputs and the
strict waste check. The instrument run's executed source-macro identities and
counts match V8 for every Goose cell. Diagnostics (instrument run, `--stats`
runs) are separate from the clean timings. No timing assertions are added.

The [raw archive](generated-200k-no-v8.edn.gz) is the five phases'
`samples.edn` (check, the stopped attempt, coupled, factorial, instrument),
each after a `{:kind :file}` marker line, then one `{:kind :stats}` line per
`--stats` run with its `pool.*`, `engines.*` and `memory.live.*` lines, then one
`{:kind :native-overlap}` line with the overlap counts for every instrument
protocol and the Babashka script that computed them.
To reproduce, build the three binaries and the runner as in
benchmarks/generated-project/README.md (the predecessor from a `git archive
5e77771d` tree), generate the target, then:

```sh
C="bb benchmarks/generated-project/compare.bb target/generated-200k --light BIN/integrated --runner BIN/runner --also-light predecessor=BIN/predecessor --also-light goose-build=BIN/goose-build,goose"
$C --backend all --phase check --runs 1
$C --backend all --phase measure --runs 5
$C --backend goose --workers 4 --compiler-slots 4,8 --phase measure --runs 5
bb benchmarks/generated-project/compare.bb target/generated-200k --light BIN/integrated --runner BIN/runner --also-light goose-build=BIN/goose-build,goose --backend all --phase instrument --runs 1
```

## One job worker per job

November 5, 2026 (light-cljs-784; design in
[javascript-coverage.md](../docs/javascript-coverage.md), decision 3). The
runner now spawns one contained job worker per dispatch from the coordinator,
which owns the session image, and a dependency wait blocks in its worker
within a retention budget of 32. **The 200k project stays byte-identical with
its five oracles passing, and gets slower: Goose 1/2/4/8 take 16.683 /
11.540 / 7.831 / 8.640 s against 14.738 / 10.121 / 7.167 / 7.274 s for the
runner before the change (+1.945 / +1.419 / +0.664 / +1.366 s).** The cause is
the per-thread cost of Goose data-stack regions: a job worker has 46, and
setting them up and tearing them down costs about 7.5 microseconds each
(probe P12), paid 5,283 times a session, mostly as kernel time. No job comes
near its limits, so no evacuation is needed, and `trim_memory()` gains nothing
worth its cost.

Binaries: light-cljs `--release --locked --features goose-macros` at this
branch (SHA-256 `3077acf9…68e7`, used for every cell); runner before
(`compiler_session.goose` at `7e46777f`, `c8a991a9…f8d425a`); job-worker
runner (`b0357f66…5362a`). Goose v2026.3-267, Rust 1.99.1, the machine of the
sections above, seed 42, 199,129 lines, release/advanced/`--no-cache`.
Samples, census rows and totals: `generated-200k-job-workers.edn.gz`.

**Check gate**, `compare.bb --backend all --phase check --runs 1` once per
runner, one sample after one warmup per cell; these are single samples, not
a timing run. Seconds of wall, and the compiler command's user and system
CPU (the runner included):

| Runner | Goose 1 | Goose 2 | Goose 4 | Goose 8 | V8 |
| --- | ---: | ---: | ---: | ---: | ---: |
| before | 14.738 (11.30 + 3.03) | 10.121 (12.13 + 3.30) | 7.167 (13.56 + 4.12) | 7.274 (15.03 + 4.88) | 5.826 |
| job workers, waits block (default) | 16.683 (11.87 + 5.76) | 11.540 (13.11 + 6.67) | 7.831 (14.89 + 10.00) | 8.640 (15.89 + 22.14) | 5.752 |
| job workers, every wait saved (`BBP_RETAINED_WORKERS=0`) | 17.301 (12.17 + 6.04) | 11.908 (13.27 + 7.11) | 8.435 (15.06 + 10.26) | 9.132 (16.25 + 17.27) | 5.781 |

Every cell of all three runs passed the byte-identity, oracle and waste
checks. Peak memory of the compiler command moved by at most 0.033 GB
(0.897–0.999 GB before, 0.930–0.974 GB blocking). User CPU rose by 0.6–0.9 s;
system CPU rose by 2.7 s at one worker and 17.3 s at eight, which is where
the wall time went. The kernel cost grows with concurrency because exiting
workers unmap their regions while others map theirs.

**Spawns, latency and copies**, from the final `JOBS` line of traced runs
(the runner in `readiness-test` mode, which adds trace lines only) of the
same build, one session each:

| Mode, workers | Job workers | Waits (blocked / saved) | Spawn call in coordinator | Spawn to first instruction | Last publication to join | Copied into workers (arguments + restores) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| blocking, 1 | 5,283 | 529 / 0 | 15 µs | 132 µs | 229 µs | 155.9 + 153.9 MB |
| blocking, 2 | 5,283 | 529 / 0 | 17 µs | 152 µs | 309 µs | 155.9 + 153.9 MB |
| blocking, 4 | 5,283 | 529 / 0 | 27 µs | 266 µs | 720 µs | 155.9 + 153.9 MB |
| blocking, 8 | 5,283 | 529 / 0 | 79 µs | 656 µs | 2,076 µs | 155.9 + 153.9 MB |
| saving, 1 | 5,812 | 0 / 529 | 25 µs | 148 µs | 244 µs | 1,005.5 + 995.9 MB |
| saving, 8 | 5,812 | 0 / 529 | 59 µs | 552 µs | 1,770 µs | 1,005.5 + 995.9 MB |

Means per worker. 5,283 = 5,226 source macro starts + 57 helper productions;
saving adds the 529 resumes as fresh workers. The spawn-to-first-instruction
interval is on each job's critical path (0.70 s a session at one worker, 3.47
s of job time at eight); the join interval is not, but its kernel work
competes with the next spawns. Blocking keeps the job's state in its worker:
it copies 840 MB less per session and is faster than saving at every worker
count, so it is the default. The 200k project never had more than one worker
blocked at once (peak live workers 2 / 3 / 8 / 16), far below the budget of
32; the budget's cap of 128 keeps the peak thread count, 2 + 2 × (8 + 128) =
274, below the 354-worker cap of `goose check --stacks`.

**Job heaps.** The instrument phase (`compare.bb --backend all --phase
instrument --runs 1`, passing, with compare.bb's new JOBS/THREADS checks)
records each job worker's table rows at its exit and how many no root of that
exit reaches (`src/census.goose`: a diagnostic walk from the result, the
bindings, frames, locals, operands, effect arguments, writes, constants and
the global object; nothing is freed). Goose 1, 5,283 exits (5,226 finished
macros, 57 produced helpers):

| Table | Median | 90th percentile | 99th percentile | Largest | Job limit | Unreachable at exit |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| nodes | 5,841 | 10,515 | 11,601 | 12,234 | 100,000 | 98.7% |
| values | 5,820 | 10,493 | 11,581 | 12,299 | 1,048,576 | 99.1% |
| properties | 28 | 31 | 36 | 36 | 1,048,576 | 16.9% (75.9% of those the job added) |
| string units | 37,000 | 66,822 | 73,733 | 77,524 | 1,048,576 | 98.5% |

The restored image is about 28 nodes; nearly everything a job allocates is
garbage by its end, and its worker's exit frees it. Goose 8 gives the same
figures within rounding. The largest job uses 12.2% of its node limit, so no
200k job exceeds a limit because of unreachable data. On the conformance
harness (`conformance.bb --check --heap-stats`, 181 cases; 78 Goose runs
reached the VM, the rest were refused by the exporter or not scored) heaps are
tiny: at most 20 nodes, 5 values, 1 property and 5,560 string units, 4.5% of
nodes and 10.2% of units unreachable, because most cases stop at an
unimplemented opcode. **No evacuation pellet is filed:** no target workload
needs collection today. Area pellets should rerun `--heap-stats` as more cases
run; long-running JavaScript is where it may be needed.

**`trim_memory()`.** Instrumented runs call it in the coordinator whenever
the session goes idle after a batch of jobs and record resident memory around
it. Goose 1: 5,226 idle points (each job is a batch at one worker), resident
15.85 MB before and 15.64 MB after on average (−0.22 MB, 1.4%), 164
microseconds a call, 0.86 s a session. Goose 8: 945 calls, 25.16 → 24.83 MB,
202 microseconds each. The runner's own resident memory is small (the 0.9 GB
peak is the compiler's), so the gain is not worth the time and nothing calls
it outside instrumented runs. The input thread and reapers hold two data
stacks of a few bytes each, so they were not measured separately; job workers
never call it.

**Stack settings** (rule 8): see the design document's "Stack settings".
A `-DGS_STACK_STATS=1` runner at 1/2/4/8 workers reported main 51 data stacks
(52 regions), every job worker 46, every reaper and input thread 2.

Reproduce (the 200k gate commands of the generated-project README, run once
per runner and once more with `BBP_RETAINED_WORKERS=0`; the instrument phase
once with the job-worker runner):

```sh
C="bb benchmarks/generated-project/compare.bb target/generated-200k --light target/release/light-cljs"
$C --backend all --runner RUNNER --phase check --runs 1
BBP_RETAINED_WORKERS=0 $C --backend all --runner RUNNER --phase check --runs 1
$C --backend all --runner RUNNER --phase instrument --runs 1
bb crates/big-boy-pants-js/scripts/conformance.bb --check --heap-stats
```

## Reused job workers

November 5, 2026 (light-cljs-795; design in
[javascript-coverage.md](../docs/javascript-coverage.md), "Revisited without
byte identity", its "Implemented (light-cljs-795)" notes and "Stack
settings"). A job worker now serves many jobs: it restores the base image
once, installs each job's rows above it and truncates its tables back to the
image after the job, or retires once its tables end a job above
`BBP_WORKER_TABLE_BYTES`. **The 200k project stays byte-identical with its
five oracles passing at the default cap (1.25 MiB), with a cap no job
reaches (16 MiB) and with a cap of 0 (a fresh worker per job). Goose 1/2/4/8
take 15.422 / 10.561 / 7.254 / 7.700 s at the default, against 16.683 /
11.540 / 7.831 / 8.640 s for light-cljs-784's worker per job (−1.261 /
−0.979 / −0.577 / −0.940 s) and 14.738 / 10.121 / 7.167 / 7.274 s for the
runner before it (+0.684 / +0.440 / +0.087 / +0.426 s).** With the 16 MiB cap
they take 15.027 / 10.259 / 7.071 / 7.161 s (+0.289 / +0.138 / −0.096 /
−0.113 s against that runner), but idle workers then keep up to 5.5 MB more of
the runner's peak memory; the default retires a quarter of the workers' jobs
to stay within 5% of the per-job control.

Binaries: light-cljs `--release --locked --features goose-macros` at this
branch (SHA-256 `44f3ccd5…`, every cell), runner `compiler_session.goose` at
this branch (`1915e0b6…`); the caps are set by environment. Goose v2026.3-267,
seed 42, 199,129 lines, release/advanced/`--no-cache`, master at `72ce6f84`
(light-cljs-801's dispatch loop included, so the 784 and pre-784 figures,
from the sections above, are not same-day; the cap-0 row is the same-day
control). Samples, traced JOBS lines and runner rusage:
`generated-200k-reused-workers.edn.gz`.

**Check gate**, `compare.bb --backend all --phase check --runs 1`, one
sample after one warmup a cell, not a timing run. Seconds of wall, then the
compiler command's user + system CPU (the runner included) and its peak
memory:

| Cap | Goose 1 | Goose 2 | Goose 4 | Goose 8 | V8 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1.25 MiB (default) | 15.422 (11.43 + 4.02) 0.910 GB | 10.561 (12.39 + 4.48) 0.944 GB | 7.254 (13.94 + 5.82) 0.957 GB | 7.700 (15.49 + 8.90) 0.994 GB | 5.800 |
| 16 MiB (no retirement) | 15.027 (11.29 + 3.13) 0.917 GB | 10.259 (12.12 + 3.51) 0.956 GB | 7.071 (13.79 + 4.32) 0.926 GB | 7.161 (14.91 + 5.27) 1.003 GB | 5.901 |
| 0 (a worker per job) | 17.101 (12.02 + 6.37) 0.906 GB | 11.559 (12.89 + 7.52) 0.922 GB | 8.363 (14.94 + 12.07) 0.935 GB | 9.514 (16.00 + 26.40) 0.993 GB | 5.877 |

Every cell of the three runs passed the byte-identity, oracle and waste
checks. System CPU is where the per-job spawns went: 26.4 s at eight workers
with a worker per job, 5.3 s without retirements (4.9 s before
light-cljs-784).

**Workers, latency, resets and the runner's memory**, from the final `JOBS`
line of traced sessions (the runner in `readiness-test` mode, which adds
trace lines only, with `BBP_RESET_CHECK=0`), one session each, and the
runner process's own peak memory (`/usr/bin/time -l` around it):

| Cap, workers | Job workers (retired) | Queue to first instruction | Reset | Runner peak memory |
| --- | ---: | ---: | ---: | ---: |
| 1.25 MiB, 1 | 1,282 (1,282) | 61 µs | 0.28 µs | 28.3 MB (−0.7%) |
| 1.25 MiB, 2 | 1,282 (1,282) | 66 µs | 0.32 µs | 34.1 MB (−2.6%) |
| 1.25 MiB, 4 | 1,284 (1,284) | 88 µs | 0.36 µs | 46.8 MB (−5.8%) |
| 1.25 MiB, 8 | 1,285 (1,285) | 177 µs | 0.44 µs | 68.7 MB (+2.3%) |
| 16 MiB, 1 | 2 (0) | 14 µs | 0.31 µs | 34.0 MB (+19.1%) |
| 16 MiB, 2 | 3 (0) | 16 µs | 0.33 µs | 40.6 MB (+15.8%) |
| 16 MiB, 4 | 4 (0) | 18 µs | 0.42 µs | 47.4 MB (−4.6%) |
| 16 MiB, 8 | 8 (0) | 21 µs | 0.46 µs | 73.1 MB (+8.8%) |
| 0, 1 | 5,283 (5,283) | 187 µs | — | 28.5 MB |
| 0, 2 | 5,283 (5,283) | 226 µs | — | 35.0 MB |
| 0, 4 | 5,283 (5,283) | 437 µs | — | 49.7 MB |
| 0, 8 | 5,283 (5,283) | 1,137 µs | — | 67.2 MB |

Every session ran 5,283 jobs (5,226 macro starts, 57 helper productions;
529 waits, all blocked and answered). The queue-to-first-instruction mean is
per job and includes the spawns of retired workers' successors; the reset is
per reset performed (the image is 28 nodes, so truncation is cheap); the
percentages compare with the cap-0 row. Traced runs at 2 MiB, 1.5 MiB and
1 MiB (in the samples file) retired 0, 577 and 2,090 workers and gave the
runner 33.9, 30.8 and 28.5 MB at one worker: a reused worker keeps its data
stacks' high water, its largest job's tables and scratch, so only a cap that
retires the workers of the largest jobs bounds that, and 1.25 MiB is the
largest tried that stays within 5% at every worker count.

**Isolation cost.** light-cljs-792 projected about 5,283 × 15 µs, 0.1 s a
session, against 784's 1.9 s. Measured as the time from queuing a job to its
first instruction: 0.073 s a session at one worker with the 16 MiB cap (0.11 s
at eight), as projected; 0.32 s at the default (0.94 s at eight), where 1,282
retirements each pay the region cost of a spawn (probe P12), plus their
joins off the critical path; 0.99 s with a worker per job (6.0 s of job time
at eight). The gap to the projection at the default is the memory bound, not
the reset.

**Stack settings** (rule 8): `goose check --stacks` gives main 60 regions,
`run_worker` 53, `supervise` and `input` 2, a thread cap of 307 against at
most 274 live threads; see the design document.

Reproduce (the generated-project README's gate commands; `traced.bb` was a
scratch wrapper: the runner run as `RUNNER DIR WORKERS readiness-test` under
`/usr/bin/time -l` through `LIGHT_CLJS_GOOSE_RUNNER`, with
`LIGHT_CLJS_GOOSE_REPORT` set):

```sh
C="bb benchmarks/generated-project/compare.bb target/generated-200k --light target/release/light-cljs"
$C --backend all --runner RUNNER --phase check --runs 1
BBP_WORKER_TABLE_BYTES=16777216 $C --backend all --runner RUNNER --phase check --runs 1
BBP_WORKER_TABLE_BYTES=0 $C --backend all --runner RUNNER --phase check --runs 1
```

## Definition and helper installs in reused workers

November 5, 2026 (light-cljs-796; decision in
[javascript-coverage.md](../docs/javascript-coverage.md), "Measured
(light-cljs-796)"). How much of a 200k session goes to installing prepared
definitions and helpers in reused job workers, the work a per-worker cache
would save. **Under 1% of Goose wall at one worker, against the pellet's 3%
bar, so no cache was built.** The `JOBS` line now ends with the workers'
snapshot-install nanoseconds (`snapshot::extend`), their helper-install
nanoseconds and count (`callable::decode_install` on a helper-cache miss, or
a bridge import), and the coordinator's hand-off nanoseconds (copying a
prepared definition into a job and queueing it, spawns excluded).

Binaries: light-cljs `--release --locked --features goose-macros` at this
branch (SHA-256 `da7a41e8…`), runner `compiler_session.goose` at this branch
(`e1fb35ce…`), master at `31d329fd`, default cap (1.25 MiB). Samples and the
traced `JOBS` lines: `generated-200k-worker-installs.edn.gz`.

**Check gate**, `compare.bb --backend all --phase check --runs 1`, one
sample after one warmup a cell: passes, byte-identical with its five
oracles. Goose 1/2/4/8 15.497 / 10.419 / 7.101 / 7.483 s, V8 5.688 s
(light-cljs-795: 15.422 / 10.561 / 7.254 / 7.700 s; the runner only adds
counters).

**Traced sessions** (the runner in `readiness-test` mode with
`BBP_RESET_CHECK=0` through a scratch wrapper set as `--runner`, `--backend
goose`; Goose 1/2/4/8 15.788 / 10.746 / 7.319 / 7.778 s), the sample
session's `JOBS` line:

| Workers | Snapshot installs | Helper installs | Hand-off | Queue to first instruction | Joins | Spawn arguments | Restore copies |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 78.0 ms | 3.8 ms (529) | 43.9 ms | 311 ms | 335 ms | 131.8 MB | 102.6 MB |
| 2 | 79.9 ms | 4.0 ms (529) | 46.3 ms | 336 ms | 432 ms | 131.8 MB | 102.6 MB |
| 4 | 85.1 ms | 4.4 ms (529) | 54.1 ms | 451 ms | 790 ms | 131.8 MB | 102.6 MB |
| 8 | 110.5 ms | 4.5 ms (529) | 73.2 ms | 926 ms | 2,457 ms | 131.8 MB | 102.6 MB |

Every session ran 5,283 jobs, 5,283 snapshot installs (14.8 µs each at one
worker) and 529 helper installs (7.3 µs each). The worker-side installs,
what a cache could remove, are 81.8 ms, 0.52% of 15.788 s; with the
hand-off, 0.80%. The warmup sessions agree within 8 ms.
