# Penpot compiler performance investigation — October 16–17, 2026

Latest validated production rerun (phase112): **21.97 s / 0.948 GB RSS** for
main none, **28.24 s / 1.178 GB** for advanced, and **3.73 s / 0.498 GB** for
render. Fresh Shadow dev is **32.08 s / 2.551 GB** and release is **59.12 s /
3.901 GB**. Native clean compilations now use the pinned, quota-aware V8 backend
by default; interactive sessions and WebAssembly retain QuickJS. Main takes
31.5% less wall time and 62.8% less RSS than Shadow dev. All experimental engine
settings were unset. These final runs use the main working tree's rebuilt binary;
its complete outputs match the validated candidate byte-for-byte.

Default-backend validation passes **1,442 tests**, QuickJS-only passes **1,406**,
with zero failures and 106 ignored in each. Both pass strict Clippy; formatting,
three Node workflows, the source-engine build, ABI rejection check and WebAssembly
playground checks also pass. All Penpot outputs are byte-identical to phase111
and pass ESM syntax checks; the port is unchanged. Penpot has not been bundled
and run in a browser. [Phase112 evidence](results/compiler-2026-10-16/penpot-phase112/README.md).

The native backend and release defaults are promoted. A final collection-store
specialization experiment yielded only a 0.7% average timing reduction and was
not promoted; [phase113](results/compiler-2026-10-16/penpot-phase113/README.md)
records the controlled comparison. Further large gains are not established by
the current profiling evidence.

## Earlier measurements

The entries below retain the state and decisions at each earlier phase.

Previous isolated V8 validation (phase111): **22.95 s / 0.967 GB RSS** for main
none, **28.81 s / 1.162 GB** for advanced, and **3.80 s / 0.479 GB** for render.
These use ordinary adaptive heap defaults, with no threshold override. All
1,442 tests, Clippy, formatting and three Node workflows pass; complete outputs
are unchanged. Native packaging and a final focused speed experiment are in
progress. [Phase111 evidence](results/compiler-2026-10-16/penpot-phase111/README.md).

Previous validated production rerun (phase101): **103.58 s / 0.803 GB RSS** for
main none, **108.10 s / 0.901 GB** for advanced and **11.47 s / 0.309 GB** for
render. Fresh shadow dev is **36.19 s / 2.331 GB**; release is **64.96 s /
4.345 GB**. The compiler now handles Rumext's native destructuring without
redeclaring its lexical locals. **All three complete outputs pass explicit
ES-module syntax checking.** This fixes the defect documented in phase44.

The exact promoted candidate passes **1,406 tests, zero failures, 106 ignored**
at normal concurrency, strict Clippy, formatting and all three Node workflows.
Regression coverage includes macro execution, every optimization level, mixed
raw/local bindings, rest/default destructuring, nested function shadowing,
strict directives, loops and captured iteration values. The port is unchanged.

The output audit finds only intended declaration changes: main none has 8,012
`const` declarations changed to lexical `let` for locals exposed to raw JS,
and 74 native `var` destructuring statements changed to assignments. Advanced
has 7,704 and 74 respectively; render has 409 and zero. All other lines match
phase100. This is syntax and focused semantic validation, not a bundled/browser
execution of Penpot. Production is **2.86 times slower** than fresh shadow dev,
at 34.5% of its RSS. The dramatic speed goal remains unmet.
[Phase101 evidence](results/compiler-2026-10-16/penpot-phase101/README.md) retains
the patch, full validation, output audits and all five benchmark runs.

Previous validated production rerun (phase100): **101.57 s / 0.802 GB RSS** for
main none, **105.90 s / 0.870 GB** for advanced and **11.13 s / 0.313 GB** for
render. Fresh shadow dev is **35.44 s / 2.188 GB**; release is **62.14 s /
4.468 GB**. After application assembly consumes a deferred unit's IR, the
session keeps generated source and immutable reader-registration tables instead
of retaining the full analyzed program. Macro execution remains lazy.

The exact promoted candidate passes **1,403 tests, zero failures, 106 ignored**
at normal concurrency, strict Clippy, formatting and all three Node workflows.
All three outputs remain byte-identical to phase42 and the port is unchanged.
Explicit ESM checking passes render and reproduces the known main defect below.

An immediate frozen-baseline comparison takes **102.86 s / 0.818 GB** for the
candidate versus **102.91 s / 0.892 GB** for phase87: 8.3% less RSS, with time
unchanged. The common drift from earlier measurements is not a patch regression.
Production remains **2.87 times slower** than fresh shadow dev, at 36.7% of its
RSS. The dramatic speed goal remains unmet. The faster V8 candidate below still
has unresolved runtime limits and remains unpromoted.
[Phase100 evidence](results/compiler-2026-10-16/penpot-phase100/README.md) retains
the implementation, validation, comparison and all five production benchmarks.

Previous validated production rerun (phase87): **94.02 s / 0.890 GB RSS** for
main none, **98.33 s / 1.051 GB** for advanced and **10.56 s / 0.352 GB** for
render. Fresh shadow dev is **31.17 s / 2.434 GB**; release is **56.86 s /
3.932 GB**. Private reader qualification now resolves bindings on demand.
Metadata-map layers and historical namespace tables share storage; completed
units release their boxed inputs and trim spare arena capacity.

This exact production candidate passes **1,401 tests, zero failures, 106 ignored**
at normal concurrency, strict Clippy, formatting and all three Node workflows.
All compiler, runtime, test, vendor and build inputs match the validated candidate.
The three complete outputs match phase42 and the 996-file port hash is unchanged.
Explicit ESM checking passes render and reproduces the known main defect below.

Main memory falls **11.8%** versus phase72. This batch does not improve production
time beyond run variation: main remains **3.02 times slower** than fresh shadow
dev, at 36.6% of its RSS. Advanced is 1.73 times slower than shadow release, at
26.7% of its RSS. The dramatic speed goal remains unmet. The faster V8 candidate
below still has unresolved runtime limits and remains unpromoted.

A native profile of this exact production binary (phase90) attributes **53.1%**
of main-thread observations exclusively to QuickJS's interpreter loop. JavaScript
macro expansion accounts for **87.5% inclusively**, including the bridge and
nested compiler work. These overlapping figures must not be added. The next
substantial speed gain needs to address macro execution; the retained storage
changes mainly improve memory.

Nested timing instrumentation (phase91) separates 48,216 JavaScript expansions:
54.29 seconds in macro calls, 7.35 in form import, 6.83 in result export, 6.46 in
environment import, 4.01 in argument preparation and 0.62 in root release.
Complete `rumext.v2/defc` expansions account for 21.33 seconds; one Instaparse
grammar expansion takes 12.37 seconds. Macro totals include transport and must
not be added to stage totals. The diagnostic run takes 91.16 seconds and produces
the identical output.

QuickJS safepoint sampling (phase92) finds repeated protocol selection and
private collection-store access alongside bridge traversal. `publicMethod`
appears in 32.6% of sampled stacks inclusively; its own top-frame share is 6.9%.
The declaration-demand callback contributes 7.8% of top frames; private-store
`has`/`get` wrappers contribute about 5.6%/3.9%. Safepoint sampling can miss
native work and merges anonymous functions with the same declaration line, so
these are targeting evidence rather than precise wall-time percentages.

Whole-build follow-ups do not turn those samples into useful further gains:
a ready-root shortcut takes **91.43 s / 0.916 GB**; a shorter protocol-demand
path takes **90.54 s / 0.908 GB**; native private-store methods take
**89.13 s / 0.917 GB**, against phase87's isolated **91.34 s / 0.894 GB**.
All are rejected, without full-suite runs. The demand-path regression passes;
complete application output is unchanged, with differences confined to the two
deliberately edited runtime units where applicable. The store trial also lacks
a justification for changing the runtime's established V8 allocation strategy.
Phases93–95 preserve the negative results. A stale instrumented-engine run in
phase93 is explicitly excluded; subsequent builds force the vendored engine
inputs and check the frozen binary for the diagnostic marker.

Previous validated production rerun (phase72): **92.72 s / 1.008 GB RSS** for
main none, **98.37 s / 1.153 GB** for advanced and **10.60 s / 0.397 GB** for
render. Fresh shadow dev is **30.19 s / 2.377 GB**; release is **56.55 s /
3.798 GB**. Unit runtime parameters now use the existing raw-JavaScript hygiene
pass, removing the scan through all previous string literals. This is a small
production gain (1.2% for main), not a dramatic further speedup.

The exact promoted candidate passes **1,400 tests, zero failures, 106 ignored**,
strict Clippy, formatting and all three Node workflows. One obsolete internal
scan test was removed; the existing raw-JavaScript regression now checks the new
parameter and its hygiene prefix. All source, test, vendor, bench and build
inputs match the validated candidate. All three complete outputs match phase42,
and the 996-file port hash is unchanged. Explicit ESM checking again passes
render and fails both main outputs with the known defect below.

Production remains **3.07 times slower** than shadow dev, at 42.4% of its RSS;
advanced is 1.74 times slower than shadow release, at 30.4% of its RSS. This is
about 22 times faster than the original 2,057-second build, but the goal of
dramatically outperforming shadow on both speed and memory remains unmet.

Previous validated production rerun (phase52): **93.83 s / 0.993 GB RSS** for
main none, **99.31 s / 1.162 GB** for advanced, and **10.70 s / 0.401 GB** for
render. Fresh shadow dev is **30.17 s / 2.232 GB**; release is **55.06 s / 4.046 GB**.
Production now selects mimalloc in the native CLI and emits compact, equivalent
registration strings for compile-time units. Library allocator selection and
WebAssembly dependencies are unchanged. The candidate passes **1,401 tests,
zero failures, 106 ignored**, strict Clippy, formatting and all three Node
workflows; all compiler, runtime, test and vendor inputs were checked identical
after promotion. All complete outputs match phase42 and the port is unchanged.
The known main ESM defect below persists.

Compared with phase47, main is 15.6% faster and uses 20.6% less peak RSS.
It remains **3.11 times slower** than shadow dev, at 44.5% of its peak RSS;
advanced is **1.80 times slower** than shadow release, at 28.7% of its RSS.
The overall speed goal remains unmet. The bundled allocator also passed a
Linux ARM64 C probe using the host's existing GCC; the host has no Rust
toolchain, so this is not a Linux compiler build or suite result. Phase52
preserves the dependency lock, incremental patch, full implementation patch,
commands, test logs, source/output hashes, timing and Linux probe evidence.

The latest isolated V8 candidate (phase110) combines JavaScript heap and backing
storage in its session budget. A negative control retained roughly 43.1 MB under
a 32 MiB limit when each category was checked separately. GC now refreshes the
allocator's heap observation, and operation/interrupt checks collect unreachable
storage before rejecting a combined over-budget state. This is cooperative
storage accounting, not an exact process-RSS cap.

The full suite passes **1,440 tests, zero failures, 106 ignored**, strict Clippy,
formatting and all three Node workflows. Main none takes **23.52 s / 0.990 GB**,
advanced **29.68 s / 1.162 GB**, and render **3.86 s / 0.482 GB**. An immediate
phase109 main takes **23.46 s / 0.983 GB**, showing no material overhead. All
complete outputs are identical and pass ESM syntax checking; the port is
unchanged. The latest fresh Shadow comparison remains phase109's **33.08 s /
2.197 GB** dev and **60.08 s / 4.129 GB** release.

Without the experimental 256 MiB heap threshold, main takes **23.82 s / 1.294 GB**
with the same output. An ordinary adaptive collection threshold and reproducible
backend packaging are next; production remains phase101.
[Phase110 evidence](results/compiler-2026-10-16/penpot-phase110/README.md) retains
the combined-limit negative control, patch, full validation and measurements.

The preceding isolated V8 candidate (phase109) adds backing-store quotas to a
source-built, pinned engine. Fixed and resizable buffers, shared buffers, Wasm
memory, growth, shrinkage and release now share an atomic backing-store budget.
Thirteen previously successful over-budget cases now fail with the owned
memory-limit diagnostic. Garbage-collection retries, zero-filled growth,
coercion order, caught errors and backing stores outliving an isolate are covered.
Allocator callbacks do not reenter V8.

The candidate passes **1,435 tests, zero failures, 106 ignored**, strict Clippy,
formatting and all three Node workflows. Cold main none takes **24.19 s /
0.967 GB**, advanced **29.05 s / 1.185 GB**, and render **3.89 s / 0.488 GB**.
Fresh Shadow dev takes **33.08 s / 2.197 GB**; release **60.08 s / 4.129 GB**.
All complete outputs match phase108 byte-for-byte and pass ESM syntax checks;
the port remains unchanged.

An immediate old-engine main takes 22.80 s. Two further alternating comparisons
give candidate times of **22.99 / 22.93 s** and baseline times of **23.15 /
23.09 s**. The three-run medians are 22.99 and 23.09 seconds, so the quota patch
does not show a consistent timing penalty or gain. Main remains about 30.5%
faster than fresh Shadow dev at roughly 44% of its RSS. Heap and backing storage
still have separate budgets; their combined session limit and backend packaging
remain to be addressed. Production stays at phase101 and the dramatic speed
goal remains open. [Phase109 evidence](results/compiler-2026-10-16/penpot-phase109/README.md)
retains the engine/compiler patches, negative control, full validation and runs.

The preceding isolated V8 candidate (phase107) installs its memory callback before
Context creation and monitors JavaScript runtime initialization with the shared
interrupt scheduler. Failed startup cleans up the partial session and preserves
the owned resource diagnostic; scheduler thread creation returns an error and
can be retried. Native Isolate/Context creation remains synchronous, and the
conservative 16 MiB startup minimum remains. Backing-store memory accounting is
still unresolved, so the backend remains isolated.

The exact candidate passes **1,426 tests, zero failures, 106 ignored** at normal
concurrency, strict Clippy, formatting and all three Node workflows. Cold builds
take **22.74 s / 0.956 GB** for main none, **28.93 s / 1.158 GB** for advanced,
and **3.77 s / 0.507 GB** for render. Fresh Shadow dev takes **33.15 s / 2.593 GB**;
release takes **61.55 s / 4.810 GB**. All three outputs match corrected production
byte-for-byte and pass explicit ESM syntax checks; the port is unchanged. Main is
31.4% faster than Shadow dev at 36.9% of its RSS. The dramatic speed goal remains
unmet. [Phase107 evidence](results/compiler-2026-10-16/penpot-phase107/README.md)
retains the implementation, full checks and all five runs.

A pinned V8 source-build baseline (phase108) now passes the same **1,426 tests,
zero failures, 106 ignored**, strict Clippy, formatting and three Node workflows.
The published crate omitted ICU data and vendored Rust libraries; these were
restored from its exact upstream submodule revisions. Chromium's pinned native
toolchain builds the unmodified engine in 5 minutes 40 seconds. Frozen library
and generated-binding hashes tie validation to that exact build.

Main none takes **23.12 s / 0.986 GB**, versus an immediate frozen prebuilt-engine
rerun at **23.06 s / 0.967 GB**. Advanced takes **29.07 s / 1.166 GB** and render
**3.84 s / 0.486 GB**. All outputs match phase107 and pass explicit ESM checks.
This establishes the native baseline for the backing-store fix; it does not
implement that fix or promote V8.
[Phase108 evidence](results/compiler-2026-10-16/penpot-phase108/README.md) preserves
the pinned restoration steps, setup failures, build, full checks and measurements.

The preceding isolated V8 candidate (phase102) includes phase101's native
destructuring fix and passes **1,421 tests, zero failures, 106 ignored** at
normal concurrency, strict Clippy, formatting and all three Node workflows.
Clean cold builds take **24.21 s / 0.971 GB** for main none, **30.51 s /
1.180 GB** for advanced and **3.90 s / 0.479 GB** for render. All three outputs
are byte-identical to corrected production and pass explicit ESM syntax checks.
Main is 33.1% faster than fresh Shadow dev, using 41.6% of its RSS. V8 remains
isolated because of the runtime-limit and packaging gaps documented below.
[Phase102 evidence](results/compiler-2026-10-16/penpot-phase102/README.md) retains
the exact patch, tests, commands, hashes and measurements.

Lowering V8's JIT invocation thresholds tenfold (phase103) takes **23.71 s /
0.985 GB** for main versus an immediate default-flags rerun at **23.96 s /
0.949 GB**. The small wall-time difference comes with 13.7% more user CPU time
and 3.9% more RSS, so the setting is rejected. Complete outputs are unchanged;
the rejected flags did not receive a full-suite run.
[Phase103 evidence](results/compiler-2026-10-16/penpot-phase103/README.md) records
the exact settings and comparison.

A fresh native/JavaScript profile of phase102 (phase104) takes **24.87 s** and
produces the identical output. JavaScript bridge traversal accounts for 10.1%
of sampled self time, while encoding and decoding take 3.16 and 2.67 seconds
inclusively. Macro calls take 9.02 seconds inclusively; these overlapping totals
must not be added. Native samples also show repeated namespace-prefix counting
and validation of unread environments. The profile is targeting evidence, not
an uninstrumented benchmark; the native sampler itself appears in 7.7% of
main-thread observations.
[Phase104 evidence](results/compiler-2026-10-16/penpot-phase104/README.md) retains
both original profiles, summaries, source mapping and instrumentation.

A lazy error-path experiment (phases105–106) passes 1,400 differential cases but
does not produce a useful whole-build gain. V8 takes **22.51 s / 0.955 GB** versus
an immediate frozen baseline at **22.95 s / 0.963 GB**. QuickJS takes **96.12 s /
0.816 GB** versus **95.27 s / 0.798 GB**. The change is rejected; output differences
are confined to the deliberately edited runtime function and complete ESM syntax
checks pass. No full suite was run for these rejected candidates.
[Phase105](results/compiler-2026-10-16/penpot-phase105/README.md) and
[phase106](results/compiler-2026-10-16/penpot-phase106/README.md) preserve the probes
and sequential comparisons.

The preceding isolated V8 candidate (phase99) passes **1,418 tests, zero failures,
106 ignored at normal concurrency**, strict Clippy, formatting and all three
Node workflows. After application assembly consumes a deferred unit's analyzed
body, the session retains generated source and immutable reader-registration
tables instead of the full analyzed program. Macro execution and registration
remain lazy. Prepared source and registration match direct emission.

Clean cold builds take **22.87 s / 0.953 GB** for main none, **28.92 s / 1.120 GB**
for advanced and **3.70 s / 0.481 GB** for render. Main peak RSS falls 15.2% versus
phase97; time is essentially unchanged. All complete outputs match phase42,
including the known main ESM defect. Phase98's teardown measurements explain
the target: only 277 of 13,367 retained programs had been emitted, and freeing
those saved just 4.0 MB; freeing the still-lazy programs released another
205.5 MB of requested Rust allocations. These are ordered releases, not RSS.
[Phase98 evidence](results/compiler-2026-10-16/penpot-phase98/README.md) and
[phase99 evidence](results/compiler-2026-10-16/penpot-phase99/README.md) retain
the diagnostic, implementation and validation. The corresponding QuickJS change
was subsequently validated and promoted in phase100, described above.

The preceding isolated V8 candidate (phase97) passes **1,416 tests, zero failures,
106 ignored at normal concurrency**, strict Clippy, formatting and all three
Node workflows. It adds a shared interrupt scheduler (phase96) and enforces
native stack limits at outer engine entries while preserving the boundary
through nested compiler callbacks. Linux ARM64 and Windows x64 native probes
pass for the OS boundary helper; full V8/compiler testing remains on macOS.

Clean cold builds take **23.13 s / 1.124 GB** for main none, **29.30 s / 1.236 GB**
for advanced and **3.71 s / 0.514 GB** for render. An immediate comparison of the
frozen scheduler-only and bounded-stack binaries takes **22.64 versus 22.67 s**
for main, showing no meaningful stack-limit cost in that pair. Complete outputs
remain byte-identical to phase42; render passes ESM syntax checking and the main
defect persists. The port is unchanged. V8 remains isolated: backing-store
memory limits, startup monitoring, scheduler creation failure and packaging
still need work. The dramatic speed goal is
unmet. [Phase96 evidence](results/compiler-2026-10-16/penpot-phase96/README.md)
and [phase97 evidence](results/compiler-2026-10-16/penpot-phase97/README.md) retain
the patches, commands, tests, hashes, native probes and benchmark comparisons.

The preceding combined isolated V8 candidate (phase85) passes **1,409 tests, zero
failures, 106 ignored**, four test threads, strict Clippy, formatting and all
three Node workflows. Clean cold builds take **20.57 s / 1.089 GB** for main
none, **26.22 s / 1.232 GB** for advanced and **3.29 s / 0.522 GB** for render.
Shared map layers, smaller completed-unit entries, shared namespace tables and
trimmed arena capacity reduce clean main RSS by 12% versus phase76; time changes
by about 2%. All complete outputs match phase42, including its known ESM defect.
Main is 34% faster than phase87's fresh shadow dev and uses 45% of its RSS. The dramatic
speed goal remains unmet, and V8 remains unpromoted with the same runtime gaps.

The preceding combined isolated V8 candidate in phase76 passes **1,408 tests, zero
failures, 106 ignored**, four test threads, strict Clippy, formatting and all
three Node workflows. Clean cold builds take **20.98 s / 1.237 GB** for main
none, **26.36 s / 1.311 GB** for advanced and **3.40 s / 0.556 GB** for render.
This includes typed root registration and resolves reader bindings only when
syntax quotation reads them, removing private runtime binding-table construction
and declaration-kind scans. The public reader's binding introspection stays
unchanged. All complete outputs match phase42. Main is about 31% faster than
fresh shadow dev, at 52% of its RSS, but the dramatic speed goal remains unmet.
V8's resource-limit, watchdog and packaging gaps still prevent promotion.

Allocation diagnostics (phases77–79) separate churn from retention. At the end
of analysis, the phase76 prototype retains 648 MB of requested Rust allocations;
dropping its macro session releases about 451 MB. These counters exclude V8/C++
and allocator overhead. Sharing metadata-map layers removes **2.317 GB (15.2%)**
of cumulative allocation during analysis, but does not reduce that retained
state. The isolated layer-sharing build takes **20.31 s / 1.176 GB RSS** and
passes two focused snapshot tests; it has not passed a full suite. Complete
output remains identical to phase42. This is a copying improvement, not a fix
for the remaining session retention.

Teardown probes (phases81–82) find 13,367 completed units and no unfinished
definitions. Clearing their analyzed programs releases 314 MB, including about
183 MB from expression tables and 61 MB from namespace tables; clearing catalog
entries releases another 52 MB. These are releases in that order, not independent
object-size estimates. The phase85 changes address the namespace and empty-entry
costs. Shared entries within modified arena leaves (phase80) and reuse of exact
canonical bridge subtrees (phase86) show no useful gain and are not retained.

QuickJS namespace registration trials also fail to justify their complexity.
Splitting registration from unit code and caching compiled namespace functions
takes **89.04 s / 0.944 GB**; the same binary with caching disabled takes
**90.25 s / 0.947 GB**, versus the storage-only candidate's isolated
**91.34 s / 0.894 GB**. Four cache tests pass, including reentry, fresh namespace
state, restored source views, replaced eval and exceptions, but the timing change
is small and memory is worse. Neither trial is retained or given a full suite.
The exact outputs remain identical; phases88–89 preserve both results.

The preceding combined isolated V8 candidate in phase70 passes **1,406 tests, zero
failures, 106 ignored**, using four test threads, plus strict Clippy, formatting
and all three Node workflows. Clean cold builds take **22.59 s / 1.232 GB**
for main none, **28.13 s / 1.323 GB** for advanced and **3.69 s / 0.568 GB**
for render. All complete outputs match phase42, including the known main ESM
syntax defect. A 128 MiB V8 heap now completes in **22.92 s / 1.202 GB**;
the smaller managed heap only slightly reduces process memory. Main is 25%
faster than the latest shadow dev observation, but the dramatic speed goal is
still unmet. V8 remains unpromoted because its resource-limit, watchdog and
packaging gaps remain.

The preceding combined isolated V8 candidate passes **1,404 tests, zero failures,
106 ignored**, using four test threads, plus strict Clippy, formatting and the
Node workflows (phase58). It takes **25.69 s / 1.374 GB** for main none,
**30.89 s / 1.420 GB** for advanced and **4.15 s / 0.582 GB** for render, with
all complete outputs matching phase42. Its callback failures now retain owned
diagnostics without unchecked conversion/allocation unwraps or catch-and-spin
delays. Stack, backing-store, startup-limit, watchdog and packaging gaps remain;
V8 is still unpromoted. It is only 17% faster than shadow dev, so the dramatic
wall-time goal remains unmet.

The preceding thin-LTO observation was 25.14 s / 1.31 GB; small differences
between these runs are not evidence of a regression or a further gain. Range
iteration alone shows no timing gain. Compiler-runtime storage trials range
from 24.61 to 25.17 s and are not retained. A smaller 128 MiB V8 heap
fails during Instaparse expansion; that failed run is not a memory improvement.
Phases53–58 preserve the samples, patches, commands and negative results. One
shared-target Cargo artifact was stale; its measurement is explicitly excluded,
and subsequent isolated builds force compilation of both crate roots. None of
these trials changes the validated production result above.

The next isolated buffer guard (phase59) is **incomplete**. Fixed-size backing
storage tests pass, but growable SharedArrayBuffer bypasses both the custom
allocator and V8's external-memory total: a 33 MiB buffer succeeds under a
32 MiB limit. Its failing regression and counters are preserved. This confirms
that the faster backend's remaining resource-limit gap is real; the guard and
V8 backend are not promoted.

Further profiling and rejected trials (phases60–64): an in-process V8 CPU
profile identifies bridge traversal/snapshots, native deferred compilation,
generated-code evaluation and protocol dispatch as remaining costs. The
`forceSlot` samples are mostly its Rust callback, not JavaScript lookup itself.
Reusing macro argument traversal order gives 25.29 s on V8 and **94.49 s on
QuickJS**. Replacing active-ancestor weak sets with a bounded stack gives
25.09 s on V8 and **94.22 s on QuickJS**. Neither improves retained production,
so neither is promoted or put through a full suite. All output differences are
confined to the exact edited runtime function; the rest matches phase42.
The replay trial also passes 500 differential semantic cases. The original
profile, patches, exact commands, negative results and hashes are preserved.

Phases65–66 also test skipping `&form` transport for unwrapped core macros
whose formal is proven unread, with conservative metadata/literal/limit guards.
Ten focused bootstrap tests pass and complete Penpot output matches phase42.
Results are **24.82 s / 1.344 GB on V8** and **92.23 s / 1.012 GB on QuickJS**.
The production timing difference is only 1.7%, with slightly higher memory;
this does not justify the extra complexity, so the shortcut is not retained
and no full suite was run for it. Production remains the validated phase52.
An initial phase65 build reused a stale generated runtime despite rebuilding
Rust; that run is excluded. Scratch builds now force `build.rs` as well as
both crate roots, and the clean rerun verifies complete output identity.

The next measurements expose a larger duplication (phases67–69): **437 deferred
units emit 102.9 MB of namespace registration for only 0.92 MB of code**. A
bounded V8 cache compiles each namespace's latest registration function once,
while executing every registration in order with fresh namespace maps/sets.
It reduces compiled registration source to **1.05 MB** (131,239 hits / 1,292
misses). Diagnostic main-none runs, including terminal GC/statistics, take
**24.62 s / 1.173 GB** and **24.59 s / 1.177 GB** with batched execution.
All output matches phase42. Six focused V8 tests pass for the first cache,
including mutation, source-view restoration, eviction, replaced eval, reentry
and exceptions. Phase70 adds the full validation reported above and replaces
the whole-program string-table scan for unit parameter names with the existing
raw-JavaScript hygiene pass. Production and
the faster backend's remaining resource-limit gaps are unchanged.

Phase71 uses typed root registration in the isolated V8 evaluator, retaining
dynamic callback lookup and the replaced-eval fallback. It cuts normal generated
source evaluations from 14,977 to 993 and takes **22.40 s / 1.178 GB** in a
diagnostic run with terminal GC. Eight focused tests pass; phase76 subsequently
validates this delta in the full suite. Output matches phase42. Phase73's CPU profile and
phase74's native sample now put bridge traversal/snapshots, macro execution,
reader-view construction and global iteration ahead of namespace parsing.
Reader-view updates account for 882 of 9,130 inclusive native observations;
overlapping sample categories must not be summed. These are prototype findings,
not improvements to the production benchmark above.

The phase75 reader configuration revision trial takes **22.54 s / 1.240 GB**,
so it does not improve phase71's diagnostic run and is not retained. No full
suite was run for that rejected trial. Further phase74 sample attribution puts
386 of the 479 exclusive global-iterator observations inside reader-view updates;
phase76 resolves only the bindings syntax quotation actually reads and passes
the full suite reported above.

Further isolated trials, **not production changes**: selecting mimalloc for
the V8 CLI lowers main-none from 35.71 s to **26.51/26.58 s**, at **1.34/1.35 GB
RSS**, with identical complete output. This is faster than the latest shadow
dev observation, but the allocator variant has not passed a full suite and
the V8 resource-limit/portability gaps remain. The separate QuickJS quoting-only
trial took **104.20 s / 1.17 GB**, with identical output; phase52 validates and
retains it together with the native CLI allocator. The string-scan prefilter trial
takes 34.91 s in V8, a small difference requiring a repeat. Phases49–51 preserve
the isolated patches, exact dependency versions, commands and measurements.
The earlier validated production result is phase47 below.

Previous validated production rerun (phase47): **111.18 s / 1.25 GB RSS** for
main none, **116.56 s / 1.34 GB** for advanced, and **14.07 s / 0.50 GB** for
render. Fresh shadow dev is **30.27 s / 2.14 GB**; release is **56.36 s / 5.34 GB**.
Production passes **1,401 tests, zero failures, 106 ignored**, strict Clippy,
formatting, diff checks and all three standalone Node workflows. All complete
outputs match phase42 and the port is unchanged; the known main ESM defect
below persists.

ASCII compiler names now allocate their shared UTF-16 slice directly, avoiding
a temporary vector. The compiler's private reader view keeps only namespaces
that can affect name qualification: current, referred and aliased namespaces.
The regression preserves alias collisions, renamed refers, exclusions and
captured macro definition views, and passes against the old reader too.

Production main is 5.7% faster than phase45, with 1.8% higher peak RSS. It is
still **3.67 times slower** than shadow dev at 58% of its RSS; advanced is
**2.07 times slower** than shadow release at 25% of its RSS. The combined
isolated V8 candidate takes **35.71 s / 1.37 GB**, still 18% slower than shadow
dev. Its full suite passes **1,402 tests** with four test threads. An earlier
default-concurrency run hit two deadline-related failures, both passing alone;
that evidence remains recorded. V8 remains unpromoted. See phases46–48 for
patches, complete logs, allocation evidence and the fresh native sample.
The overall speed goal remains unmet.

Previous validated production rerun (phase45): **117.87 s / 1.23 GB RSS** for
main none, **123.01 s / 1.33 GB** for advanced, and **15.05 s / 0.51 GB** for
render. Fresh shadow dev is **30.90 s / 2.46 GB**; release is **57.24 s / 3.84 GB**.
Production passes **1,400 tests, zero failures, 106 ignored**, strict Clippy,
formatting and diff checks. Complete outputs match phase42; the unchanged
main ESM defect below prevents treating this as executable correctness.

Deferred analyzers now inherit the namespace-global count from their definition
snapshot, avoiding repeated scans of a frozen global prefix. Native sampling
identified that scan in 9.7% of main-thread observations in the isolated V8
compiler. Its clean main time falls from 47.32 s to **42.74 s / 1.44 GB**, and its
full suite passes 1,401 tests. Production's main time is essentially unchanged
from phase42; the V8 speedup is not evidence of a production speedup. V8 remains
unpromoted. Production is still 3.81 times slower than shadow dev, at 50% of its
RSS. Evidence is in `results/compiler-2026-10-16/penpot-phase45/`. The overall
speed goal remains unmet.

**Syntax-validation correction (phase44):** Earlier `node --check FILE.js`
commands returned zero, but that was insufficient for these ESM outputs.
Explicit `node --check --input-type=module < FILE.js` rejects the frozen
phase42 main-none output: rumext native destructuring emits `var` for locals
already declared `const`. The V8 prototype reproduces the same output exactly.
Earlier syntax-pass claims below are historical observations of the inadequate
check, not evidence that Penpot can execute. Byte equivalence and timing results
remain valid; bundling/browser correctness remains unestablished. Phase101
subsequently fixes this defect in the compiler, and all three new production
outputs pass the explicit ESM check. Historical outputs remain unchanged.

An **isolated, unpromoted V8 compiler prototype** (phases43–44) passes the full
suite with V8 selected for noninteractive compiler macros: **1,401 passed,
zero failed, 106 ignored**. The final clean runs are render **7.91 s / 0.65 GB**,
main none **47.32 s / 1.41 GB**, and advanced **53.84 s / 1.53 GB**, using a
256 MiB V8 heap limit. All complete outputs match phase42. Render passes an
explicit ESM syntax check; both main outputs fail as described above.

The prototype exposed another retention issue: V8 kept the whole script source
of macro functions, including large repeated namespace catalogs. Render's
heap snapshot contained 80 MB of strings out of about 90 MB live heap.
Separating registration from the function source reduces final live engine
heap to 11 MB for render and 27 MB for main. Compact compile-time string
quoting then reduces generated registration text. Disabling V8's compilation
cache did not meaningfully resolve retention. These live heap measures are
distinct from process peak RSS.

The prototype remains **1.56 times slower than shadow dev**, with 64% of its
peak RSS; it is 2.47 times faster than production, but uses 14% more peak RSS.
Its stack-limit support, bounded ArrayBuffer allocation, startup budgets,
watchdog behavior and exceptional registration reentry need work before
promotion. The production backend and dependency files remain unchanged. Evidence,
full test logs, failed heap trials, the reproducible patch and limitations are
in `results/compiler-2026-10-16/penpot-phase44/`; phase43 contains the standalone
engine and boundary probes. The requested overall goal remains unmet.

The prototype's scoped profile accounts for 14.61 s in top-level namespace
analysis, 10.19 s in deferred analysis, 7.69 s in macro calls, 9.57 s in
transport and 3.57 s in unit emission, with nested measured work subtracted.
Initial loading, final preparation and output together are below one second.
The namespace category includes source preparation and lazy loading, so native
profiling is needed to identify its expensive operations. Engine replacement
alone does not meet the speed goal.

Previous validated cold rerun (phase42): main none **117.05 s / 1.24 GB RSS**, main advanced **122.76 s / 1.30 GB**. Fresh shadow dev is **30.33 s / 2.22 GB**, release **55.72 s / 4.62 GB**.

Reader identity validation now resolves keyword/symbol names without inserting
unused entries into executable literal tables. Actual lowering uses the same
name-resolution helpers and still inserts its literals. Exact UTF-16 names,
auto-resolution and generated semantic names retain their existing paths.

A nine-key map previously retained 27 names and 27 strings; the regression
now requires nine of each. It also covers nested map/set keys, symbols and
auto-resolved keywords in source and owned compiler forms. The frozen old
library reproduces the 27/27 negative control.

Full suite: **1,400 passed, zero failed, 106 ignored**. Strict Clippy,
formatting and all three Node workflows pass. Render is **14.86 s / 0.49 GB**.
All three complete outputs match phase33 byte for byte; the port is unchanged.
Explicit ESM syntax checking fails for main-none as described above. Penpot
browser execution remains untested.

Main none is faster than phase33's 121.15 s observation, but its peak RSS
is higher than phase33's 1.17 GB. This run does not establish a whole-build
memory reduction; the regression checks demonstrate eliminated dead entries.

The separate live-allocation comparison of render confirms reduced retained
data: 312,381,712 to 290,492,640 bytes after analysis (7.0% less), and
104,108,000 to 94,240,688 bytes after releasing the macro session (9.5% less).
The reader-identity allocation category disappears; the classified QuickJS
allocation total is unchanged. These are live allocator sizes, not RSS.
The instrumented output also matches the validated output exactly.

None remains **3.86 times slower** than shadow dev at **56% of its peak RSS**. Advanced is **2.20 times slower** than shadow release at **28% of its RSS**. The requested speed goal remains unmet.

Evidence: `results/compiler-2026-10-16/penpot-phase42/`. Phase41's heap
snapshots found the discarded reader names; its live allocation totals are
not RSS measurements. The CLI already drops its host after analysis.

Previous validated cold rerun (phase33): main none **121.15 s / 1.17 GB RSS**, main
advanced **125.18 s / 1.32 GB**. Fresh shadow dev is **30.80 s / 2.21 GB**, release
**57.25 s / 4.10 GB**. The full suite passes **1,399 tests, zero failures,
106 ignored**. Strict Clippy, formatting and all three Node workflows pass.

Phase33 imports a macro call once and shares argument objects with
their elements in `&form`, matching upstream macro application. It
retains the decoded descriptor/value graph only across environment
import, then restores per-argument metadata and provenance in order.
Each argument still receives canonical snapshot validation. Whole-call
input conversion bounds already cover its argument subtrees.

Preparation or input release discards the graph. Snapshot failures
release newly assigned argument IDs and preserve the original error.
The bridge ABI is version 5; external evaluator adapters retain their
existing independent-import defaults.

Five new regressions cover shared identity and mutations with a reused
macro host; exact provenance for ratios, large integers, characters
and interned keywords after an intervening environment import; pending
graph cleanup on environment failure; partial snapshot failure; and
single construction of literal inputs. The provenance regression also
demonstrates why simply reusing existing values is insufficient.

Render is **15.50 s / 0.52 GB**. All three application bodies match phase32
outside the embedded runtime; syntax checks pass and the port is unchanged.
Evidence is in `results/compiler-2026-10-16/penpot-phase33/`.

Advanced remains **2.19 times slower** than shadow release at **32% of its peak RSS**; none is **3.93 times slower** than shadow dev at **53% of its RSS**. The requested speed goal is still unmet. Penpot browser execution
remains untested; scope differences and JVM RSS variability still apply.

Before phase42, the following diagnostics left production at phase33. The phase35 full-main
profile measures **54.67 s** in macro calls, **26.60 s** in macro transport,
and **19.60 s** in deferred analysis/emission after subtracting nested
instrumented work. Rumext `defc`/`html`, Instaparse and the two CSS macros
account for about 77% of the macro-call total. Uninstrumented callback work
remains included in its enclosing stage. Its complete output matches phase33.

Phase36's native sampling also reproduces the complete output. QuickJS's
interpreter accounts for about 40% of observed leaf samples; native runtime
operations and allocation are prominent too. Canonical comparison has only
six leaf samples and is not a large target in this profile. Recursive stack
counts are not additive time measurements; the full profile is archived.

Phase34's direct-method/mask prototype improves one macro replay but breaks
private-brand checks on forged objects, so it is rejected. Other store and
method variants have only small replay gains and are not retained. Phase37's
primitive-brand shortcut shows no measurable improvement and is rejected.
Evidence is in `results/compiler-2026-10-16/penpot-phase34/` through
`penpot-phase37/`; these diagnostics do not replace the clean table above.

Phase38 also rejects a QuickJS `Function.call` ownership optimization: repeated
macro replays show no useful gain. A deliberately invalid frame-removal variant
fails the stack-frame semantic probe and saves only about 1–2%. Both diagnostics
reproduce the captured result; neither changes production. Evidence is in
`results/compiler-2026-10-16/penpot-phase38/`.

Phase39 time-gated interrupt sampling again points to protocol dispatch and
declaration demand. Disabling demand after the replay's first call saves about
15%, but is not valid for general compilation. Native membership memoization
shows no useful gain. These diagnostic variants are not retained; the sampling
limitations and exact-result checks are recorded in `penpot-phase39/`.

Phase40 rejects a native Clang PGO build trained on the captured grammar:
its matching replays are about 13% slower than the ordinary build. No compiler
flags or profile data are retained. See `penpot-phase40/` for reproduction.

The previous phase32 comparison was main none **122.11 s / 1.28 GB**,
advanced **126.80 s / 1.39 GB**, shadow dev **30.73 s / 2.42 GB**, and
shadow release **55.79 s / 4.16 GB**. Its full suite passed 1,394 tests.
That phase fixed incorrect general-equality dispatch in record-field
lookup, contains and assoc, matching upstream keyword comparisons.
The captured Instaparse macro replay improved from 11.0 seconds to
9.56/9.42 seconds, but full main timing was essentially unchanged.

Phase30 counted 1,307,500 compiler-global reads in main; a ready-root
microbenchmark suggests only about 0.31 seconds, so another root cache
was rejected. Phase31 replayed the real Instaparse grammar macro with
identical encoded results: QuickJS took 11.0 seconds and Node 1.4 seconds,
against 11.1 seconds in the original compiler invocation. This identifies
JavaScript execution as the dominant cost for that macro, not a predicted
whole-build speedup or a decision to change engines. Both diagnostics
have separate evidence directories.

The previous phase29 comparison was main none **122.07 s / 1.27 GB**,
advanced **127.33 s / 1.30 GB**, shadow dev **30.20 s / 2.43 GB**, and
shadow release **55.20 s / 4.57 GB**. Its full suite passed 1,393 tests.
That phase removed string-table copying during runtime-parameter name
selection; all three complete outputs matched phase27 byte for byte.

Phase28's separate profile measured 5.165 seconds in string-table conversion
within 8.107 seconds of unit emission. Fragment assembly was only 0.388
seconds. Macro calls, including reader setup and reentry, were 62.998 seconds;
this is an inclusive measurement, not an exclusive CPU percentage. Its
outputs match phase27. Evidence is in `results/compiler-2026-10-16/penpot-phase28/`.

The previous phase27 comparison was main none **124.17 s / 1.29 GB**, advanced
**130.43 s / 1.40 GB**, shadow dev **30.18 s / 2.23 GB**, and shadow release
**54.72 s / 4.00 GB**. Its full suite passed 1,392 tests.

Phase27 avoids transporting `&env` to compiler-owned bootstrap macros whose
resolved bodies never read it. Source macros and reader wrappers keep the
complete view. Tight conversion bounds and compiler literals use the full
transport, preserving its failures. Three regressions cover the proof,
resource-limit recovery and literal metadata. Main none is 15.9% faster
than phase25; render is **15.70 s / 0.56 GB**. All three complete outputs
are byte-identical to phase25, Node syntax checks pass and the port remains
unchanged. Evidence is in `results/compiler-2026-10-16/penpot-phase27/`.

In the phase27 comparison, advanced was **2.38 times slower** than shadow release at **35% of its peak RSS**; none is **4.11 times slower** than shadow dev at **58% of its RSS**. The requested speed goal is still unmet. Penpot browser execution
remains untested; scope differences and JVM RSS variability still apply.

The previous phase25 comparison was main none **147.56 s / 1.31 GB**, advanced
**151.64 s / 1.43 GB**, shadow dev **33.53 s / 2.22 GB**, and shadow release
**56.60 s / 4.08 GB**. Its full suite passed 1,389 tests.

Phase25 caches fixed protocol metadata, indexes initialized extensions by
declaration order, and calls frontier entry/exit through an evaluator service
without reparsing JavaScript. Receiver properties and protocol methods stay
live. Regressions cover Proxy reentry during replay, inherited array properties,
original exceptions, cancellation and nested compiler callbacks. Render is
**19.91 s / 0.55 GB**. All three application bodies match phase6b outside
the runtime, Node syntax checks pass, and the port is unchanged. Penpot browser
execution remains untested. Evidence and the preliminary experiments are in
`results/compiler-2026-10-16/penpot-phase25/`.

In the phase25 comparison, advanced was **2.68 times slower** than shadow release at **35% of its peak RSS**; none is **4.40 times slower** than shadow dev at **59% of its RSS**. The requested speed goal is still unmet. Scope differences and JVM
RSS variability still apply.

The previous phase22 comparison was main none **159.03 s / 1.36 GB**, advanced
**162.93 s / 1.42 GB**, shadow dev **30.65 s / 2.30 GB**, and shadow release
**55.70 s / 3.98 GB**. Its full suite passed 1,384 tests.

Phase22 builds lists with their known stored counts, avoiding public protocol
dispatch on every newly created tail. It also fixes an ICounted override
corrupting a three-item list's stored count to 1000; explicit public count calls
still honor the override. The regression matches upstream ClojureScript 1.12.134
and passes at every optimization level and in macros. Main improves about 3%
from phase20; advanced peak RSS is about 7% lower in this observation. The
manifest's list-building effect targets are updated, all three Node workflows
and strict Clippy pass, and the port remains unchanged. Render is
**21.09 s / 0.54 GB**. Diagnostic motivation and poll-frequency limitations are
recorded in `results/compiler-2026-10-16/penpot-phase21/`.

Phase20 shares unchanged binding rows between adjacent lexical views and reuses
up to 128 immutable descendants in the environment bridge. Main none improves
9.5% from phase15, advanced 10.2%; peak RSS is essentially unchanged. References
resolve against the previous completed import, and cache publication requires
canonical snapshots. A new regression caught discarded map values losing their
numeric spelling on reuse; this is fixed without additional snapshot traversal.
Eviction order, shadow chains, conversion limits, reentry and failed imports
remain covered. All three Node workflows and strict Clippy pass.

All three generated application bodies match phase6b outside the embedded
runtime and pass Node syntax checking. Penpot application execution is still
untested. The clean five-job run and validation are retained in
`results/compiler-2026-10-16/penpot-phase22/`.
Advanced remains 2.93 times slower than shadow release at 36% of its measured
peak RSS; none is 5.19 times slower than shadow dev at 59% of its RSS. These
single-run RSS values include JVM variability and the documented scope differences.

Phase14 carries an arity index across namespace analysis instead of rescanning
the growing expression arena for each namespace. The index includes finalized
new expressions, unions redefinitions and overloads, and retains global identity
across namespaces. A regression passes with both the old and new implementations.
Standalone and deferred analysis retain their existing diagnostic scope.
The earlier five-job comparison is retained in
`results/compiler-2026-10-16/penpot-phase14/`.

Phase15 releases each parsed namespace tree after analysis has produced its
owned forms, retaining source text for diagnostics. All three complete outputs
are byte-identical to phase14 and pass Node syntax checking. Render is
**24.24 s / 0.53 GB**; main none's peak RSS is about 50 MB below phase14, while
advanced's peak and all elapsed times are essentially unchanged. The full suite,
strict Clippy and formatting pass. Evidence is in
`results/compiler-2026-10-16/penpot-phase15/`.

The diagnostic import breakdown (phase19, using the isolated phase13
profile source) narrows the remaining transport cost. For main, constructing
locals takes **17.06 s** and snapshotting them **6.61 s**: **23.67 s of 29.94 s**
inside the environment import service. Macro inventory construction and its
snapshot total only **0.56 s**; namespace data totals **0.41 s**, and reader-view
data **1.42 s**. The larger whole-view locals cache experiment did little because
most misses are distinct lexical views. This identified the phase20 target:
changing one binding reconstructed all of the view's binding records. The
phase20 prototype reduced locals construction to **7.18 s** and the entire
environment JS service to **18.96 s**. About 81% of binding rows on misses reuse
the previous prefix. These prototype measurements precede the discarded-child
correctness fix; the clean phase20 results above include that fix.
Raw render/main logs and output-equivalence checks are in
`results/compiler-2026-10-16/penpot-phase19/`. These are diagnostic measurements,
not additional clean benchmarks.

The original main problem was the amount of work and history retained per macro expansion
and per definition. The bridge repeatedly transports complete environments,
including all loaded macro metadata, and retains their provenance and canonical
copies for the entire compilation. Deferred analysis adds another multiplier by
copying whole namespace tables. Arity diagnostics repeatedly scan the expression
arena. These costs grow independently of the emitted JavaScript's size.

Investigated baseline: `4d2a0d69004dd1c993aab3a65e3080079858e4bc`.
The diagnostic investigation below used an isolated source archive in
`/private/tmp/light-cljs-investigation-20261016`. The subsequent production
changes and fresh measurements are recorded under **Implementation and rerun**
at the end; line references in the investigation describe the baseline.

## Complete `app.render` profile

This is the existing Penpot port, with all five source roots, raised macro
limits, `--esm --optimize none`, and a fresh release process. It analyzes
268 namespaces, 6,695 globals, 6,541 deferred units, and 476,709 expressions.
The diagnostic control completed in **254.42 seconds / 4,706 MiB peak RSS**.
The earlier uninstrumented benchmark reported 214.98 seconds / 4,686 MiB;
do not treat the instrumented control as a new production performance result.

| Analysis work | Exclusive seconds |
| --- | ---: |
| Convert environment descriptors into JavaScript values | 117.21 |
| Export the just-imported environment back into descriptors | 47.21 |
| Decode and retain the environment snapshot in Rust | 14.83 |
| Encode the environment in Rust | 13.91 |
| Execute macro functions, including their reader-context wrapper | 15.75 |
| Collect expression warnings | 12.32 |
| Everything else in analysis | 28.54 |
| Total profiled analysis | 249.77 |

Environment import, including its small surrounding overhead, consumes
**193.28 seconds, 77% of profiled analysis**. Only 10,372 JavaScript macro
expansions are involved. Merely constructing the environment in Rust takes
about one second; transporting it is the expensive part.

The supplied `app.main` sample agrees with this diagnosis: of 15,987 samples,
12,680 (79.31%) include `import_data`, and 2,423 (15.16%) include
`collect_expression_warnings`. These are inclusive counts with repeated
ancestors deduplicated. This is a roughly 16-second slice of an earlier build,
not a whole-build percentage or a profile of the final 2,057-second run.

## 1. The macro ABI repeatedly round-trips large immutable data

Relevant code:

- `src/macro_host.rs`, `compiler_environment` (line 1218): rebuilds locals,
  namespace imports/requires, and `:light-cljs/macros`. The latter sorts and
  includes **every loaded source macro's metadata**, not just the called macro.
- `src/macro_host.rs`, `expand_javascript` (line 161): imports `&form`, imports
  `&env`, then imports each explicit argument separately. Argument subtrees
  already transported in `&form` are transported again.
- `src/evaluator.rs`, `ActiveEvaluator::import_data` (line 1109): encodes Rust
  data, imports it into JavaScript, immediately exports it again, and decodes a
  canonical Rust snapshot. This happens on every import.
- `src/form_bridge.rs`, `encode_node` / `decode_node`: recursively allocate
  descriptors and compiler values. Existing identities do not skip encoding.
- `src/js_runtime.js`, `compilerFormBridge` (line 3667): creates actual runtime
  collections, names, metadata and traversal maps from those descriptors.

Snapshotting supports real semantics: unchanged syntax recovers original source
locations and reader distinctions, while mutated `#js` and Date values must not
reuse stale compiler data. Applying that mechanism to every environment and
every immutable subtree is extremely expensive.

The cost is proportional to expansion count times transported environment size.
Environment size increases with locals, imports and the loaded macro inventory.
A trivial `when` therefore becomes more expensive as unrelated compiler state
grows. Rebuilding the full metadata map is especially wasteful when a macro
never inspects it.

A controlled reproduction makes this visible. Each file contains the same
1,000 functions using `when`, preceded by different numbers of unused identity
macros. All three runs execute 1,024 JavaScript macro expansions, including
bootstrap work:

| Unused macro declarations | Wall seconds | Environment work, seconds | Peak RSS, MiB |
| --- | ---: | ---: | ---: |
| 0 | 1.01 | 0.50 | 99.1 |
| 50 | 6.80 | 6.10 | 225.7 |
| 100 | 12.94 | 12.08 | 354.0 |

Environment work includes building and importing it. Macro execution itself
takes only 0.036–0.048 seconds. Adding 100 unused macros makes this workload
nearly 13 times slower, overwhelmingly by increasing transport work.

`withMacroReaderContext` also copies JavaScript Maps/Sets on each call. It should
be improved, but the complete macro-call bucket, which includes that wrapper,
is only 15.75 seconds here. The bridge deserves priority over that wrapper.

## 2. Releasing JavaScript roots does not release bridge history

`BridgeState` owns strong references in `originals`, `identities`, `canonical`
and `forms` (`src/form_bridge.rs`, line 26). Encoding appends original data;
snapshot decoding stores canonical copies. There is no pruning during the
session. `Evaluator::release` only deletes the JavaScript value handle
(`src/evaluator.rs`, line 1097).

For `app.render`, the bridge ended with:

- 3,048,882 original entries, including 2,917,040 newly retained by environment
  imports;
- 2,388,812 canonical entries and 131,000 retained form entries;
- **2,527,298,561 bytes freed by dropping the bridge's Rust tables**;
- 119,662,687 bytes reported by QuickJS at evaluator teardown.

Live requested Rust allocations at the end of analysis were 4,084,066,555 bytes.
The bridge's measured release alone is about 62% of that amount. Requested
allocation bytes and RSS are different metrics; the QuickJS number is a teardown
observation, not a measurement of its peak heap.

This is compilation-lifetime retention, not evidence that memory survives
process teardown. Increasing JavaScript GC frequency cannot free those strong
Rust references. The `--macro-memory-limit` allocator bounds QuickJS allocation;
it is not a limit on the whole Rust compiler process.

## 3. Deferred snapshots still copy whole namespace tables

`Shared<T>` is an `Rc<T>` with `Rc::make_mut` on mutation
(`src/shared.rs`, line 35). For `Shared<HashMap<String, RuntimeReaderNamespace>>`,
that means cloning the whole map and its owned alias/refer/name vectors.

Each deferred unit starts from a cloned `resolution_program`, then inserts its
reader context (`src/analyzer.rs`, lines 3251–3277). It later mutates the same
table's macro list. `CompilationSession` retains both the definition snapshot
and the analyzed `Program` (`src/compilation_session.rs`, lines 12–54).

Consequently, sharing the original table does not make subsequent per-unit
mutation cheap. With D deferred definitions and N namespaces, this path can
retain O(D × N) namespace records, including their strings and vectors.
Other shared tables and copy-on-write arena chunks also deserve auditing.

The catalog audit in the combined `app.render` ablation below found
**6,798 distinct reader-context maps,
869,893 namespace records and 3,691,021 alias entries** retained across the
6,541 units' snapshots and analyzed programs. Reader-table cloning allocated
about 510 MB cumulatively; that is a copy-allocation counter, not peak RSS.

A separate fixture contains 40 plain identity functions per namespace, with ten
aliases of the preceding namespace. It invokes no JavaScript macros:

| Namespaces | Definitions | Retained namespace records | Reader-table copy allocations, MB | Peak RSS, MiB |
| --- | ---: | ---: | ---: | ---: |
| 25 | 1,000 | 13,300 | 9.14 | 63.8 |
| 50 | 2,000 | 52,225 | 38.51 | 168.5 |
| 100 | 4,000 | 206,950 | 158.24 | 432.5 |

Doubling the workload approximately quadruples this copying. The original
single-namespace scaling fixture cannot expose this namespace multiplier.

## 4. Arity diagnostics perform repeated whole-program searches

`collect_expression_warnings` iterates each new call expression. For a global
callee, it searches the expression arena for definitions of that global
(`src/analyzer.rs`, line 4024). At the outer namespace pass, the search starts
at expression zero. Calls inside function bodies participate too.

The `app.render` run examined **3,358,001,433 expression candidates** in these
arity searches. This is O(global calls × expression count), rather than a
lookup by `GlobalId`. It runs even when this CLI invocation does not print
warnings. The deferred-unit warning pass also means some call analysis occurs
again when the assembled namespace is checked.

An indexed, declaration-aware arity table would preserve warning semantics
without these scans. It must retain the current treatment of redefinitions,
constructors, variadic methods, metadata wrappers, and deferred visibility.

A fixture with N functions, each calling the same global function, reproduces
the quadratic scan independently of JavaScript macro expansion:

| Functions | Expressions examined for arities | Warning-pass seconds |
| --- | ---: | ---: |
| 1,000 | 16,987,995 | 0.043 |
| 2,000 | 67,975,995 | 0.173 |
| 4,000 | 271,951,995 | 0.707 |
| 8,000 | 1,087,903,995 | 2.844 |

## Diagnostic ablation

The isolated build has switches to import environment values without registering
provenance identities or taking their canonical snapshots, and to skip the
expression-warning pass. The actual environment values still reach the macro.
This does not cache environments or avoid their JavaScript construction.

| `app.render` experiment | Wall seconds | Peak RSS, MiB | Bridge bytes freed at teardown |
| --- | ---: | ---: | ---: |
| Instrumented control | 254.42 | 4,706.3 | 2,527,298,561 |
| Environment provenance/snapshot disabled; warning pass disabled | 159.90 | 2,102.7 | 131,361,521 |

The emitted 12,616,500-byte JavaScript files have the same SHA-256 hash:
`f7cbd4bdce25d5f7a5bfb1f5a370975bac78d6aa6a2f5605d5a4ddd5660b6b64`.
This run is 37% faster and uses 55% less peak RSS. Environment construction in
JavaScript still takes 106.95 seconds, so this leaves the largest remaining
CPU cost intact. It is evidence for the diagnosis, not a production-ready fix
or a claim that these changes close the shadow-cljs gap.

The original 4,000-function single-namespace fixture was also run with only
environment provenance/snapshot disabled: 16.91 → 13.61 seconds and
737.7 → 414.0 MiB RSS, with identical output. The untouched release binary
took 16.93 seconds / 727.8 MiB on that fixture.

## Other avoidable work

Deferred analysis rebuilds the global-name lookup map from the snapshot's full
global arena (`src/analyzer.rs`, line 3252). Macro expansion also constructs a
fresh `LineIndex` by scanning the entire source file, even though `Source`
already has a cached line index (`src/analyzer.rs`, lines 9069 and 9166;
`src/compiler_data.rs`, line 137). These are concrete secondary targets, but
the measurements above do not assign them individual percentages.

## Fix order

1. Give environment transport a bounded lifetime and avoid canonical snapshots
   for data that does not need syntax provenance. Preserve escaped source forms,
   nested/reentrant expansion and mutable-literal behavior; clearing all bridge
   tables after every macro is not a safe general fix.
2. Reuse immutable JavaScript environment components by namespace/macro revision.
   Transport local changes, rather than all aliases and all macro metadata on
   every expansion. Avoid importing explicit arguments twice. Keep reflection
   and reader visibility correct.
3. Replace whole namespace-table copies with structural sharing and per-unit
   changes. Release completed units' obsolete analysis context when its consumers
   no longer need it. Audit the remaining `Shared` mutations and arena copies.
4. Index global arities, then reuse global lookup maps and source line indexes.
5. Measure again before changing engines or parallelizing analysis. A JVM JIT
   and concurrency help shadow-cljs, but upstream's analyzer also directly
   passes its existing `form` and `env` to the macro function
   (`research/clojurescript/src/main/clojure/cljs/analyzer.cljc`, line 4309).
   This implementation adds repeated conversion and retained copies at that
   boundary. More workers would also multiply its current memory demand.

The original `none`/`advanced` timings establish that optimization is small
relative to this frontend cost, not that optimization literally costs zero.
The scope and port differences prevent a precise apples-to-apples speed ratio,
but they cannot explain the measured billions of scans and repeated bridge
round trips. The surfaced compatibility gaps are separate correctness work.

## Evidence and limitations

Raw logs, the isolated diagnostic patch, fixture generator and output hashes
are in [the evidence directory](results/compiler-2026-10-16/penpot-investigation/).
These are sequential, single-observation diagnostic runs, not a statistical
benchmark suite. Instrumentation counts requested allocation bytes and exclusive
phase time, and adds overhead. The initial investigation did not rerun the full
34-minute `app.main` build.
All ten generated synthetic modules were executed in Node 24.18.0 and returned
the expected results. The diagnostic patch passes `git apply --check` against
the investigated source. The initial investigation did not rerun the production suite.

The diagnostic switches deliberately remove work to establish causality. They
are not production fixes. Penpot output has not been bundled or executed, and
matching emitted bytes cannot establish that the original port is correct.

## Implementation and rerun

The working-tree implementation replaces the session-wide Rust provenance
registry with opaque engine-owned handles. Object provenance lives in a weak
map and is collected with the imported object. Macros that save objects retain
their source forms, canonical payloads and lexical distinctions. Scalars have
no observable object lifetime, so their latest provenance is retained by value,
as before, rather than by expansion. Leaf provenance holds a single form node,
not the entire input syntax arena. Source text remains available for diagnostics.
Reusing a collection restores provenance for all its original children,
including number spellings that collapse to the same JavaScript Number.

Environment transport caches up to 64 immutable top-level components per
evaluator. Macro metadata is built once per macro inventory revision and shared
by equivalent definition frames; namespace metadata is reused while its inputs
match. Cache hits send a reference and charge the original logical node/depth
budget. Fresh components still take canonical snapshots. Components containing
host literals are copied and checked on every import. This preserves mutation
isolation, macro reflection, reader metadata and escaped environments. Failed
imports invalidate the native cache so interactive recovery cannot refer to an
engine slot that was never installed.
Reentrant imports during cache population use the ordinary bridge: constructing
maps can invoke user hashing/equality code, so nested analysis must not see
pending cache slots before their JavaScript values exist.

Reader-context tables now share a layered map and individual namespace records.
A namespace is registered before definition snapshots are taken, and unchanged
contexts avoid writes. JavaScript reader contexts use overlays instead of
copying their parent Maps and Sets during macro expansion; nested calls restore
the prior view in `finally`. Arity warnings index the same declaration range
once per pass, preserving constructor signatures, metadata wrappers, overloads,
variadic methods, redefinition unions and deferred-unit visibility.

The shared runtime grows by 1,344 bytes for the bridge and reader machinery.
The `none`/`prepare` size ceilings increase by 2,000 bytes to cover it; advanced
ceilings remain unchanged because ordinary application slices discard these
compiler-only units.

The benchmark driver is [penpot.bb](penpot.bb). It runs one build process at a
time, uses all five existing port roots and the original macro limits, and
clears only shadow's generated compiler cache before each shadow invocation.
It does not regenerate or modify the port. Large outputs stay outside Git.

Validation on the final implementation:

- `cargo test --release --no-fail-fast`: **1,363 passed, 0 failed, 106 ignored**.
  This is the same regular suite used for the original 1,355-pass baseline;
  optional ignored reference/oracle suites were not enabled.
- `cargo fmt --check` and `cargo clippy --locked --all-targets -- -D warnings`: passed.
- The three Node checks in the conformance workflow (`js_runtime`,
  `js_core_helpers`, `execution`): passed.
- New tests cover native provenance reclamation, scalar retention by value,
  saved subforms and scalars across GC, immutable environment reuse, mutable
  literal isolation, logical conversion limits, failed-import recovery,
  reentry during environment construction, and indexed arity diagnostics.

Measured host: macOS `VirtualMac2,1`, 24 logical CPUs, 32 GiB RAM;
Rust 1.99.0 and Node 24.18.0. The 996 files under the five port roots had a
combined path/content SHA-256 of
`b36daf80aa370322598f53f5bf776391036ab70cf993835793daa1285015ba48`
before the rerun. Each compiler invocation is a fresh process; shadow's cache
is cleared, while OS filesystem caches and installed dependencies are retained.
The same input hash was verified after all builds.

### Penpot results

RSS below is in **GiB** (bytes divided by 2³⁰), calculated from the raw
`/usr/bin/time -l` logs rather than the original rounded GB labels.

| Build | Original wall, s | Fixed wall, s | Speedup | Original RSS, GiB | Fixed RSS, GiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| `app.main --optimize none` | 2,057.49 | **525.39** | **3.92×** | 18.59 | **5.27** |
| `app.main --optimize advanced --pseudo-names --elide-asserts` | 2,043.00 | **568.97** | **3.59×** | 18.57 | **5.32** |
| `app.render --optimize none` | 214.98 | **59.63** | **3.61×** | 4.58 | **1.38** |

Peak RSS falls by 71.6%, 71.4% and 69.9%, respectively. The initial new
`main-none` run, which was sampled, took 570.40 seconds and 5.23 GiB. Its clean
repeat is the 525.39-second row above. This variation is larger than the sample
interval itself; do not attribute it all to profiler overhead or treat these
single observations as precise optimizer phase measurements. Both main builds
still spend almost all their wall time on one core.

Fresh shadow measurements, with its compiler cache removed for each run:

| Build | Wall, s | User CPU, s | System CPU, s | RSS, GiB | Build summary |
| --- | ---: | ---: | ---: | ---: | --- |
| `release main` | **60.88** | 147.03 | 456.88 | **3.75** | 1,176 files; 916 compiled; 0 warnings |
| `compile main` | **35.77** | 92.13 | 494.86 | **2.04** | 1,209 files; 1,189 compiled; 0 warnings |

The advanced-to-release wall-time gap is now **9.35×**, versus 36.5× in the
original measurements. The dev comparison is still 14.7×. Scope remains
different: shadow includes render, rasterizer and code-highlight; light's
main includes the app and its five lazy modules, with render measured separately.
These fixes substantially reduce the gap but do not make the compiler fast
relative to shadow.

### Scaling checks and output verification

The same untouched baseline binary and the final release binary compiled each
synthetic fixture sequentially, outside the Penpot timed runs:

| Fixture | Baseline wall, s | Fixed wall, s | Baseline RSS, MiB | Fixed RSS, MiB |
| --- | ---: | ---: | ---: | ---: |
| 8,000 functions calling a global | 7.22 | 4.22 | 521.1 | 525.0 |
| 100 namespaces × 40 identity functions | 1.95 | 1.72 | 437.3 | 232.6 |
| 1,000 macro-using functions; 0 unused macros | 1.02 | 0.93 | 100.6 | 84.0 |
| Same; 50 unused macros | 6.80 | 1.21 | 225.9 | 107.0 |
| Same; 100 unused macros | 12.95 | 1.48 | 343.2 | 97.3 |
| Original 4,000-function `when`/`cond` fixture | 16.66 | 14.86 | 722.5 | 343.8 |

The unused-macro multiplier has largely disappeared, and namespace snapshots
use much less memory. The call fixture still grows roughly quadratically:
1,000/2,000/4,000/8,000 functions take 0.11/0.34/1.12/4.22 seconds after the fix.
The original `when`/`cond` fixture saves memory much more than time. These
results support the remaining-work list below; removing arity scans and reader
table copies does not remove every source of repeated analysis work.
All **22 baseline/fixed synthetic modules executed in Node and returned the
expected values**.

All three Penpot outputs pass `node --check`. Render's application code matches
the original byte-for-byte after excluding the embedded runtime. The current
port's generated font catalog already contains **1,910 different font UUIDs**
from the old main-build artifacts. The new emitted IDs/UUIDs match that source
exactly. Mapping each current UUID to the old UUID for the same font ID makes
both main application bodies byte-identical to their old outputs; there are no
other application-code differences. The advanced embedded runtime is also
byte-identical. The none/render runtime grows by the documented 1,344 bytes.
The sampled and clean new `main-none` outputs are byte-identical to each other.

Penpot was not bundled or run. The checks establish syntax and preservation of
the emitted application code, accounting for the existing font-input difference;
they do not establish correctness of the original port. Raw timing logs,
commands, input/output hashes, tests and verification scripts are retained in
[the fixes evidence directory](results/compiler-2026-10-16/penpot-fixes/).

### Remaining work after these fixes

A five-second, 10 ms sampling interval late in the first new `app.main` run
collected 449 main-thread samples. Attributing each sample to its innermost
compiler phase gives 39.2% in macro calls, 19.8% in environment import, 11.1%
in JavaScript unit evaluation, 10.2% in ordinary form import, 4.5% in expression
warnings, and 4.2% in form export. Nested analysis is attributed separately.
The earlier sample put 79.3% in ordinary `import_data`, which then included
environment transport. The samples cover different short intervals; they show
the changed bottleneck, not whole-build phase percentages or an exact phase
speedup. The sampled run is recorded separately from the clean timing.

Analysis remains single-threaded. Ordinary form transport still imports
`&form` and explicit arguments separately and takes canonical snapshots.
Reader visibility and locals still travel with each expansion. Deferred units
still rebuild global-name lookup maps, and several other shared tables and
arena chunks can still copy when written. Source line indexes are still
reconstructed at expansion sites. The arity fix removes the scan for every
global call, but it still builds an index for each diagnostic pass. These are
remaining optimization opportunities; this change does not claim to remove
every source of nonlinear scaling or reach shadow-cljs performance.

## Second optimization pass

Starting from `415620c` (the first fixes), this pass addresses additional copying
and repeated work that the original hotspots had obscured:

- Deferred frames now share a persistent global-name trie. They no longer
  reconstruct a `HashMap` from every global in the definition snapshot.
- Deferred analysis reuses its source object and cached line index. An empty
  fragment extension no longer invokes `Rc::make_mut` on shared metadata tables.
- Completed units consume and release their original form, definition snapshot,
  and analysis host; their analyzed program remains available for linking and
  later macro demand.
- Frozen AST chunks are trees of leaves containing at most 64 entries. Merging
  chunks shares roots instead of cloning historical entries; a write copies a
  path and one bounded leaf. A regression test holds 300 snapshots of 21,000
  entries and verifies that freezing/merging clones no entries.
- Arena iterators jump directly for `nth`/`nth_back`, so `skip(prefix)` does not
  walk every old entry. This matters in deferred linking and fragment append.
- Compile-time units omit unused local declarations for globals held in the
  demand registry. Successful protocol-demand lookups are cached within an
  unchanged registration/frontier view; failures and reentrant changes are not
  cached, and entering the bootstrap frontier skips user-extension demand.
- Bridge strings and symbol/keyword components use QuickJS's native UTF-16
  string API instead of per-code-unit arrays. Descriptor ABI 3 preserves empty
  strings, NUL, all 65,536 code units, lone surrogates, and a null namespace for
  unqualified names. Mutable payload snapshots and provenance remain intact.

The intermediate tree implementation reduced memory but regressed CPU time:
8,000 plain definitions took 4.48 seconds, versus 0.87 seconds immediately before
that change. A separate sampling run attributed 1,797 of 2,356 main-thread
samples to iteration in `drain_deferred`, with another 288 in fragment append.
Those paths were walking old globals just to skip them. Direct iterator jumps
remove that work; the final probe takes 0.61 seconds. The intermediate regression
is not the production result.

| Probe | First pass, seconds | Second pass, seconds | First pass RSS, bytes | Second pass RSS, bytes |
| --- | ---: | ---: | ---: | ---: |
| 8,000 functions calling a global | 4.22 | 0.61 | 550,469,632 | 378,552,320 |
| 100 namespaces, 4,000 definitions | 1.72 | 0.75 | 243,941,376 | 77,463,552 |
| 4,000 functions with `when`/`cond` | 14.86 | 11.68 | 360,497,152 | 271,302,656 |

These are individual cold runs, not statistical estimates. In particular,
removing table copying helps plain definitions much more than macro-heavy code.

The full suite passes **1,372 tests, 0 failed, 106 ignored**. Strict Clippy,
formatting, and the three Node runtime/helper/harness workflows pass. All 996
port inputs retain the earlier path/content hash. Evidence and reproduction
scripts are in [the second-pass directory](results/compiler-2026-10-16/penpot-phase2/).

### Remaining costs measured over the whole render build

A separate instrumented render run takes 44.59 seconds and 823,869,440 bytes
peak RSS. Its exclusive analysis buckets are:

| Work | Seconds |
| --- | ---: |
| Environment import | 11.40 |
| Macro invocation, including unscoped demand work | 10.09 |
| Import `&form` | 3.20 |
| Import explicit arguments again | 2.68 |
| Export expanded forms | 2.70 |
| Emit namespace registrations | 3.99 |
| Other unit emission | 1.14 |
| Evaluate units, including frontier operations | 2.23 |
| Warnings | 1.01 |
| Build environments and reader contexts | 0.37 |
| Other analysis | 5.22 |

Nested named scopes are subtracted from their caller. Uninstrumented nested
work, including some demanded helper analysis, remains in the caller's bucket;
these are not pure JavaScript CPU totals.

There are still 10,372 JavaScript macro expansions and 269 emitted compile-time
units. Building the environment in Rust is cheap compared with importing it.
The bridge's identity cache covers top-level components, but locals and reader
contexts are freshly constructed each time. Most bootstrap macros never read
`&env`, yet receive its full transport. Avoiding that work must preserve reader
registration acknowledgments, resource limits, source macro wrappers, and the
few bootstrap macros that actually inspect the hidden arguments. Namespace
registration also still emits all visible namespace catalogs per demanded unit.
These measured costs remain targets; this pass does not meet the goal of
outperforming shadow-cljs in both time and memory.

Other retention candidates still need measurement: macro-host frames retain
`Layered` maps whose own layers are copied on clone, and demanded units cache
emitted strings containing repeated namespace catalogs. The persistent global
name index does not replace those other maps. Neither candidate has been
assigned a share of the remaining main-build RSS by this pass.

### Clean second-pass Penpot results

| Build | Wall seconds | Peak RSS, GiB | Output bytes |
| --- | ---: | ---: | ---: |
| light-cljs render, none | 45.04 | 0.759 | 12,617,418 |
| light-cljs main, none | 481.76 | 2.977 | 33,583,432 |
| light-cljs main, advanced | 481.44 | 3.027 | 25,663,355 |
| shadow-cljs release main | 60.38 | 4.323 | — |
| shadow-cljs compile main | 31.80 | 2.122 | — |

The original-to-current main improvement is about **4.3× in time and 84% in
peak RSS**. Against the first fixes, main-none time improves only 8.3%, while
peak RSS falls 43.5%. The memory changes therefore do not close the CPU gap.
Current advanced is 7.97× slower than fresh shadow release; none is 15.15×
slower than shadow development and uses more peak memory. The earlier scope
caveat still applies: shadow emits more application modules in this one job.

Both shadow builds report zero warnings. Release compiled 916 of 1,176 files;
development compiled 1,189 of 1,209 files. The driver removed generated shadow
compiler caches before each process. None of these five timing runs was sampled
or overlapped with another compiler/test build.

All three application bodies are byte-identical to the first-pass outputs when
the embedded runtime is excluded. The render and none runtimes shrink by 426
bytes; advanced is byte-identical in full. All three pass `node --check`, and
the three new synthetic modules execute their expected results in Node. The
input hash remains `b36daf80aa370322598f53f5bf776391036ab70cf993835793daa1285015ba48`.
This verifies preservation of emitted application code, not Penpot runtime
correctness: the port still has not been bundled or run.

### Whole-main second-pass profile

A separate instrumented main run completed in 472.40 seconds at 3,193,061,376
bytes peak RSS, producing byte-identical output to the clean main-none run.
There were 48,216 macro expansions and 441 emitted compile-time units. The
exclusive buckets were 128.13 seconds in environment import, 156.56 seconds in
macro calls (including uninstrumented nested demand work), 67.47 seconds in
namespace registration emission, 22.06 seconds importing the whole call,
18.63 seconds importing explicit arguments, 16.15 seconds exporting forms,
12.25 seconds evaluating units/frontiers, 9.99 seconds in warnings, 6.40
seconds in other unit emission, 4.10 seconds building environments, and 28.33
seconds in other analysis. Raw evidence is `penpot-phase2/profile-main.log`.

Environment transport, repeated namespace registration, and macro execution
remain the leading costs at full application scale. Warning collection is
now a comparatively small part of this measured run.

## Third optimization pass

Compiler environments reuse one unchanged local-binding map and one unchanged
reader-context map across analysis frames. Their keys include the complete
ordered local descriptions (including shadows), or the reader declaration
generation/cursor/visibility, refers, excludes and own bindings. Reader
acknowledgment remains separate from reuse. Namespace emission groups globals
once instead of scanning the entire global table for every namespace. Failed
macro calls now release successfully imported arguments on ordinary exceptions.

The full suite ran 1,375 tests: all existing tests and the new failure cleanup
and reader-view tests passed. The new lexical-cache test initially called the
sequence inspection API on an empty map; after correcting that assertion, both
cache regression tests passed. Strict Clippy and formatting passed. Evidence
is in `results/compiler-2026-10-16/penpot-phase3/`.

Clean render takes **36.65 seconds and 821,428,224 bytes peak RSS**. Its output
is byte-identical to phase2f. The 4,000-function macro probe falls from 11.68
to 9.18 seconds; the two plain-definition probes are essentially unchanged.
A separate render profile attributes 7.38 seconds to environment import
(previously 11.40), and 0.62 seconds to namespace registration (previously
3.99).

The separate main profile takes **325.23 seconds and 2,911,387,648 bytes peak
RSS**, also producing byte-identical output. This run includes a two-second
sampling window and a terminal heap inspection/GC; it is a diagnostic run,
not a clean benchmark. Environment import falls from 128.13 to 48.28 seconds;
namespace registration falls from 67.47 to 3.18 seconds. Macro execution and
its uninstrumented nested work remain expensive: `rumext.v2/defc` takes
42.90 seconds, one Instaparse grammar expansion 31.22 seconds, and the two
style macros 30.73 seconds combined.

QuickJS reports 1,014,816,592 allocated bytes before explicit GC and
792,560,272 afterward. Function records account for 682,836,605 bytes, versus
31,980,997 bytes of bytecode. The next diagnostic separates retained function
source from variable/constant metadata. A late two-second CPU sample also
finds substantial native Map/WeakMap lookup and strict-equality work; the
vendored engine's bucket hash preserves pointer alignment and small-integer
encoding patterns, making it a further candidate for investigation.

## Engine hashing and completed initializer retention

QuickJS's native Map hash multiplied aligned pointer bits and encoded numbers
without mixing high bits into the low bucket bits. A final avalanche mixer fixes
this without changing key equivalence or insertion order. A 20,000-key probe with
ten read rounds falls from 152 to 15 ms for numeric keys and 78 to 14 ms for
object keys. This microbenchmark does **not** establish a Penpot speedup: clean
render remains 36.70 seconds versus 36.65 before the change.

The vendored engine's build script also failed to track its original C sources
and headers after copying them into Cargo's output directory. The first hash
trial therefore used the old engine. Explicit `rerun-if-changed` entries now
ensure rebuilds. The phase4b measurements and phase5 validation use the rebuilt
engine; the initial phase4 trial is not valid evidence for the hash change.

A detailed render heap profile finds 85.1 MB of retained function source out
of 85.8 MB of function records. Completed compiler roots and declaration bundles
kept their initializer closures after storing the resulting values or errors.
Each initializer retained its generated source, including namespace catalogs.
The Rust compilation session also cached emitted source which successful roots
never requested again. Completed/failed initializers now release their closures,
and the session retains an emission flag rather than the source string. Deferred
analyzed programs remain available for later compilation demands.

The full suite passes **1,376 tests, zero failures, 106 skipped**. Strict Clippy
passes. Clean render takes **34.25 seconds and 630,505,472 bytes peak RSS**;
its complete output is byte-identical to phase2f and passes `node --check`.
Evidence is in `results/compiler-2026-10-16/penpot-phase5/`.

The separate main profile takes **285.24 seconds and 1,622,540,288 bytes peak
RSS**. Its application body is byte-identical to phase2f; the embedded runtime
differs only by reader timing instrumentation. It includes diagnostic counters
and a terminal GC, so is not a clean benchmark. Retained function source falls
to **1,579,653 bytes** and function records to 2,352,621 bytes. QuickJS reports
102,269,672 allocated bytes before GC and 77,168,368 afterward. Overall malloc
usage remains much larger, at about 1.08 GB before GC: remaining memory is
predominantly outside the engine. The final program has 1,220,906 expressions
of 160 bytes each, before their separately allocated payloads and analysis views.

Reader setup alone takes **44.40 seconds over 20,126 wrapped macro calls**.
Environment import takes 39.70 seconds, whole-form import 16.55, argument import
14.28 and result export 12.63. This still leaves a substantial time gap against
shadow-cljs; the memory improvement does not establish the requested speed goal.

## Protocol lookup and fresh cold benchmarks

Generic collection operations repeatedly searched the fixed protocol-mask table
and constructed its property names. A precomputed map now supplies each protocol's
property and bit, retaining the last occurrence of duplicate ABI entries. Method
selection, declaration demand, native fallbacks and custom protocol calls are
unchanged. The first suite run caught a five-byte output-size ceiling overrun;
storing each entry as a compact pair fixed it without moving any ceiling. The
full rerun passes 1,376 tests, zero failures, 106 skipped; strict Clippy and the
three direct Node runtime/helper/execution workflows pass.

All five jobs were rerun sequentially with fresh processes and cold compiler
caches. The port still contains 996 files with combined path/content SHA-256
`b36daf80aa370322598f53f5bf776391036ab70cf993835793daa1285015ba48`.
Phase6 evidence is in `results/compiler-2026-10-16/penpot-phase6/`.

| Build | Wall seconds | Peak RSS bytes | Output |
| --- | ---: | ---: | --- |
| light-cljs render | 32.48 | 601,751,552 | 12,617,448 bytes |
| light-cljs main none | 276.60 | 1,556,643,840 | 33,583,462 bytes |
| light-cljs main advanced | 283.96 | 1,672,003,584 | 25,663,385 bytes |
| shadow-cljs release main | 60.53 | 3,956,981,760 | 1,176 files, 916 compiled, zero warnings |
| shadow-cljs compile main | 33.25 | 2,182,217,728 | 1,209 files, 1,189 compiled, zero warnings |

Main none is 7.44 times faster than the original 2,057.49-second build. Advanced
still takes 4.69 times shadow release's wall time, although its peak RSS is 42%
of shadow release's. None takes 8.32 times shadow dev's wall time, at 71% of its
peak RSS. Scope differences remain: shadow also builds render, rasterizer and
code-highlight, while light main includes its five lazy modules and render is
measured separately.

All three application bodies are byte-identical to phase2f after removing the
embedded runtime; the protocol lookup optimization changes that runtime by
30 bytes. All three complete outputs pass `node --check`. The Penpot application
has still not been bundled and run, so this is output preservation and compiler
validation, not a claim of end-to-end application correctness.

## Further diagnostic and import cleanup

The checked-array warning path had another whole-program search for each array
access, to determine whether it was an assignment target. It now builds the same
target set once per warning pass, and only when checked-array expressions exist.

An imported value also remained rooted if later canonical validation failed
before the caller received its ID. Both ordinary and environment imports now
release that temporary root on failure, preserving any pending JavaScript
exception across cleanup. The new regression repeatedly supplies invalid UUID
and instant values through the ordinary, outer-environment and active-environment
paths, checks provenance collection, and verifies subsequent successful use.
The full suite passes **1,377 tests, zero failures, 106 skipped**, and strict
Clippy passes. These two changes follow the phase6 benchmark binary; their
storage profile and timing measurements are separate.

The phase7 isolated main profile completed in 282.36 seconds, at 1,629,192,192
bytes peak RSS. Instrumentation and terminal collection make this a diagnostic
run, not another clean timing result. Its application body matches phase6b byte
for byte after excluding the instrumented runtime. Evidence is in
`results/compiler-2026-10-16/penpot-phase7/`.

Environment transport spends 2.63 seconds encoding Rust descriptors, 35.07 in
JavaScript conversion and 2.26 validating snapshots in Rust. Reader preparation
spends 35.12 seconds rebuilding aliases, 5.21 reading the remaining view fields,
1.08 registering declarations and 0.04 constructing overlays. Thus alias work
and canonical collection construction remain significant even after namespace
snapshot sharing; Rust environment construction is no longer the main cost.

Deduplicating shared arena chunks by identity across the main program and every
retained unit finds 2,421,603 expression entries occupying **470,467,984 bytes**
of table allocation (including spare capacity and chunk headers). Other tables
account for about 75 MB. These counts exclude separately allocated expression
payloads. The final main program itself has 1,220,906 expressions. After terminal
GC, live bridge data is 213,231 Datum nodes (64 bytes each including their Rc
header), 21,351 form nodes and about 2.2 MB of form buffers. QuickJS's allocated
heap is 77.2 MB; macOS malloc reports 1.07 GB still used overall. Remaining memory
is primarily compiler program storage and its payloads, not retained bridge
forms or QuickJS function source.

## Compact expression storage

The largest expression variant previously made every expression occupy 160
bytes on the benchmark machine. The constructor IDs, body, masks and fields of
a type definition now live in a boxed `TypeLayout`, reducing `Expr` to **96
bytes**. Definition cloning still clones the layout, and all readers, remappers
and optimization passes retain the same fields and ordering.

The full suite remains at 1,377 passed, zero failures, 106 skipped, with strict
Clippy passing. The isolated clean render build takes 32.50 seconds and
571,506,688 bytes peak RSS, versus phase6b's 32.48 seconds and 601,751,552 bytes.
The complete JavaScript output is byte-identical. This is a measured memory
reduction, not a speed improvement; main measurements follow separately.

## Less temporary work during macro execution

Bridge decoding now keeps its iterative work stack in one flat array and uses
direct descriptor slots and a switch, avoiding a temporary pair and tag arrays
for every node. The descriptor-to-value map, postorder, metadata handling and
snapshot validation are unchanged. Compiler declaration demand caches method
protocol names, and selector caches use nested native-type/type-name keys rather
than concatenating a string for every lookup. Registration, frontier reentry,
exceptions and declaration effects retain their existing invalidation behavior.

This phase passes all 1,377 tests, strict Clippy and the three direct Node checks.
One cold render measured 31.53 seconds and 573,931,520 bytes RSS. This small time
change needs the full main measurements before drawing a broader conclusion.

Fixed type-brand inventories also used to allocate an array and invoke a
callback for every membership probe. They now share a small predicate factory
with an indexed, short-circuit loop over one retained inventory. Metadata support
reuses those predicates. This preserves store order and performs no memoization
of mutable values or user protocol results.

The final phase10 suite passes **1,377 tests, zero failures, 106 skipped**. Strict
Clippy, formatting and all three direct Node workflows pass. The complete cold
rerun, one job at a time with no overlapping tests/builds/profiling, is:

| Build | Wall seconds | Peak RSS bytes | Output |
| --- | ---: | ---: | --- |
| light-cljs render | 30.66 | 563,331,072 | 12,617,718 bytes |
| light-cljs main none | 261.18 | 1,418,936,320 | 33,583,732 bytes |
| light-cljs main advanced | 267.48 | 1,472,495,616 | 25,663,279 bytes |
| shadow-cljs release main | 62.09 | 4,086,693,888 | 1,176 files, 916 compiled, zero warnings |
| shadow-cljs compile main | 33.02 | 2,430,648,320 | 1,209 files, 1,189 compiled, zero warnings |

The none build is 7.88 times faster than the original 2,057.49 seconds. Against
this fresh comparison, advanced remains **4.31 times slower** than shadow release
at **36% of its RSS**; none remains **7.91 times slower** than shadow dev at
**58% of its RSS**. The requested speed goal has not been reached. Scope and
unverified application execution caveats remain unchanged.

All three application bodies match phase6b byte for byte after excluding their
embedded runtimes; all complete outputs pass `node --check`. The port fingerprint
remains unchanged. Logs, output checks, binary/patch hashes and the port hash are
in `results/compiler-2026-10-16/penpot-phase10/`. The executable, complete patch,
untracked name-index source and generated JavaScript remain in
`/private/tmp/light-cljs-phase10/`.

### Native sequence cursors (phase11)

`first`, `rest` and `next` no longer redispatch ISeqable when handed an existing
native sequence. IndexedSeq and NodeSeq next cursors advance directly without
calling an overridden ISeq rest. This matches the pinned upstream definitions;
explicit ISeq and INext overrides still dispatch. The regression runs at every
optimization level and inside a source macro. A 100,000-entry QuickJS traversal
probe fell from 3,862 to 1,853 ms. The full suite passes 1,378 tests, zero failures,
106 ignored; strict Clippy, formatting and the three direct Node workflows pass.

The readable runtime grew by 767 bytes. Eight advanced fixture ceilings exceeded
their previous limits by 11–362 bytes and were raised to the next 1,000 bytes,
with the correctness/performance rationale recorded beside the fixtures for the
eventual commit message.

| Build | Wall seconds | Peak RSS bytes | Output bytes |
| --- | ---: | ---: | ---: |
| light-cljs render | 27.32 | 570,523,648 | 12,618,485 |
| light-cljs main none | 214.90 | 1,421,312,000 | 33,584,499 |
| light-cljs main advanced | 227.37 | 1,497,202,688 | 25,664,046 |

These are sequential cold processes with no overlapping CPU-heavy work. Against
the unchanged fresh phase10 shadow measurements, advanced remains **3.66 times
slower** at **37% of release RSS**, and none **6.51 times slower** at **58% of dev
RSS**. None is 9.57 times faster than the original 2,057.49-second measurement.
The speed goal is still unmet. All three application bodies match phase6b outside
the embedded runtime, and complete outputs pass `node --check`. Application
execution and scope caveats remain unchanged. Evidence is retained in
`results/compiler-2026-10-16/penpot-phase11/`; binaries, patch and generated
JavaScript remain in `/private/tmp/light-cljs-phase11/`.

### Scoped evaluator service lookup (phase12)

Active compiler callbacks now retain the evaluator's immutable service object
within their existing context lifetime. Getting a service function and checking
the bridge no longer reparses a JavaScript expression on each nested operation.
This introduces no Persistent roots and does not retain macro inputs or change
transported data. All 1,378 tests pass, strict Clippy and formatting pass.

The cold render is 26.06 seconds / 566,476,800 bytes RSS, with a byte-identical
complete output to phase11. No CPU-heavy work overlapped it. This intermediate
step did not rerun main. Evidence is in
`results/compiler-2026-10-16/penpot-phase12/`.

### Protocol selection and fresh profile (phase13)

Public protocol consumers now keep their initial membership decision while
selecting the method, avoiding a redundant satisfies? call. A method getter that
removes its marker can no longer turn a missing method into a missing-key
fallback. Explicit returned methods, native implementations and missing-method
errors remain covered at every optimization level and inside source macros.
All 1,379 tests pass, strict Clippy and all three Node workflows pass.

A cold render measured 24.93 seconds / 558,809,088 bytes RSS. The following
instrumented main measured 190.08 seconds / 1,394,130,944 bytes RSS; it is a
profile rather than a clean main benchmark. Both application bodies match
phase6b outside the runtime, and complete outputs pass node --check.

The updated profile still attributes 29.84 seconds to environment decoding,
13.19 to form import, 11.12 to argument import, 8.09 to form export, 9.73 to unit
evaluation, 7.43 to diagnostics and 24.32 to other analysis. Reader preparation
is down to 15.30 seconds (11.94 for aliases), included within macro-call times.
Instaparse's grammar macro takes 16.57 seconds; 1,033 rumext defc calls take
20.54 seconds. Deduplicated AST table storage is 284.16 MB, down from phase7's
470.47 MB, excluding payload allocations. Evidence and profiling limitations
are in `results/compiler-2026-10-16/penpot-phase13/`.
