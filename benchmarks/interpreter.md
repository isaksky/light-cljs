# Native interpreter regression measurements

The release driver measures fresh native CLI processes separately from resident
contexts, analyzed code and callable reuse. It checks every language result and
saves raw samples, medians, median absolute deviation (MAD), min/max, source and
fixture SHA-256 identities, machine/toolchain information and executable copies.
There is no SCI/JVM speed comparison or speedup target.

## Reproduce and retain a baseline

Python 3.11+ and the Rust toolchain are sufficient. No benchmark dependency,
JVM, JavaScript engine or network service is used. Run from the repository:

```sh
python3 benchmarks/run_interpreter.py \
  --output "$HOME/.cache/light-cljs-baselines/my-baseline" \
  --samples 9 --iterations 32 --frontend-ms 100

# Use the same fixtures and frontend-source options when comparing.
python3 benchmarks/run_interpreter.py \
  --output "$HOME/.cache/light-cljs-baselines/my-comparison" \
  --compare "$HOME/.cache/light-cljs-baselines/my-baseline" \
  --samples 9 --iterations 32 --frontend-ms 100
```

Output directories must be new and outside `target`. The driver builds release
CLI, interpreter, tokenizer and parser executables, then copies them into
`bin/` before any feature rebuild can replace them. A separate
`interpreter-allocations` binary enables the benchmark-only allocator. Keep the
directory: `cargo clean` will not remove these executables. The source archive
includes current uncommitted sources, tests, fixtures and examples; the Git HEAD
alone does **not** identify a build made in a dirty checkout. `manifest.json`
identifies the saved source files and executables. A saved binary must run on a
compatible OS/architecture. The driver verifies baseline hashes and rejects
changed fixtures, frontend inputs or language outcomes.

For the optional real frontend inputs, append both of these to **each** command:

```sh
--frontend-source research/clojurescript/src/main/cljs/cljs/core.cljs \
--frontend-source research/clojurescript/src/main/clojure/cljs/analyzer.cljc
```

These files are hashed in the manifest and must already exist. Their absence
does not prevent the default synthetic frontend suite. Existing
`benches/{tokenizer,parser,interpreter_gc}.rs` remain available unchanged.
The GC harness separately covers cyclic workloads at different collection
cadences; see [the reclamation contract](../docs/interpreter.md).

For individual measurements:

```sh
cargo bench --bench interpreter -- --samples 9 --iterations 32
cargo bench --bench interpreter --features count-allocations -- \
  --allocations --samples 9 --iterations 32
cargo bench --bench interpreter -- --diagnostics --samples 1 --iterations 2
```

Stdout is raw CSV; diagnostic reports go to stderr. Allocation runs set
`elapsed_ns` to zero so their instrumentation overhead cannot be mistaken for
ordinary execution speed. Diagnostic timing is also a separate run.

## What the phases include

All execution is single-threaded. There is no CPU affinity or frequency control.
Release defaults are Cargo opt-level 3, without LTO or CPU tuning; the saved
environment records explicit build overrides. No compiler/build work is timed.

| Measurement | Boundary and retained state |
| --- | --- |
| Fresh process | Parent monotonic clock around spawning the native CLI, file reads, option/context setup, required module reads, parsing/lowering/analysis/evaluation, result printing, pipe drain and process exit. Every sample gets a new process/context. First samples are retained. OS file caches are uncontrolled: this is **not** a cold-disk/cache measurement. |
| Constructor / namespace initialization | Create and drop a context; the second row also initializes the namespace registry. Contexts install many builtins on demand, so this does not mean all library Vars are eagerly installed. These microbenchmarks are primed first. |
| Parse | Existing syntax parser on resident text, including tree construction/drop; no semantic lowering. |
| Lower | Semantic lowering of one pre-parsed root from the exact resident source. The phase fixtures contain no discarded forms; direct lowering does not validate discarded subtrees removed by `parse`. |
| Read | Ordinary pull reader, including parsing, lowering and EOF check. It is an alternative to `parse` + `lower`, not an additional stage to add to them. |
| Analyze | Analyze the same retained form each time, including required macro expansion and owned code generation. Expanded/generated forms are not cached. Temporary code handles drop inside the batch; arena reclamation is separate. |
| Eval | Repeated execution of one rooted analyzed form in a primed context. No reading, expansion or analysis. |
| Read/analyze/eval | `eval_source` on the retained named source; includes incremental reading and ordinary top-level semantics. This is not a sum inferred from isolated phase clocks. |
| First load | Fresh context with an explicit in-memory source provider. Time includes require, module-source copying, analysis, initialization and computation of the report result. Context/provider/main-source setup is outside this timer; filesystem loading is in the fresh-process row. |
| Cached require | Re-evaluate the main source in the already-loaded context; verifies the same result while reusing loaded namespace state. Includes reading/analysis of the caller, not module reload. |
| Embedding invoke | Repeated `Context::invoke` of an already-created closure with a retained argument. Its loop body is analyzed once. |
| GC after phase | One explicit collection after the batch. Source/form/code roots needed by the next batch stay live. This cost is reported separately, not hidden in execution; ordinary evaluation here does not automatically collect. |

Each resident sample uses 32 operations, with explicit GC outside the timed
batch and another separately timed GC after it. Source, form, code, caches and
scratch are warmed by initial setup/evaluation. Results pass through
`black_box`; integer extraction/checksum and normal API result handling are
included; expected-checksum assertions and CSV output are outside the batch.
Sub-microsecond and small parse phases have more relative timing variation.
Frontend harnesses retain their existing calibration and 100 ms sample targets.
Comparison CLI samples alternate baseline/current order; resident binary runs
are sequential. MAD/ranges describe observed variation, not confidence intervals.

The fixtures are independently authored, committed programs: a captured
closure/512-step loop, a 256-element map/lookup/reduction workload, a bounded
consumer of an infinite lazy pipeline, a repeatedly invoked closure, and a
three-module report that builds 256 records, aggregates eight regions and uses
a user macro. They exercise useful paths but are not a large production-library
load or a claim about every supported program. Exact results are 134400,
2763520, 2796160, 85355 and 62182 respectively.

## Allocation and live-memory interpretation

The separate counting allocator forwards unchanged requests to `System`.
Allocation counts exclude reallocations; requested bytes include each complete
new realloc size. The live byte count subtracts deallocated/old realloc layouts;
peak is the maximum live requested bytes during that region. These are Rust
allocation layout bytes for the **whole benchmark process**, including retained
fixtures and harness storage. They are not RSS, allocator usable size, reserved
pages or an OS memory limit. Only counts inside the measured region contribute
to allocation/requested-byte totals; live bookkeeping runs throughout the
instrumented process. Zero byte fields in ordinary timing CSV mean unmeasured.

Every row also records interpreter heap/name/source/code/namespace/Var/root and
arena-slot counts. The GC row shows retained logical records. Slot/backing and
scratch capacity can remain high after live objects fall; that is different
from a language object leak. Compare equivalent workloads/cadences and use the
existing cyclic GC benchmark for reclamation invariants.

## Optional source attribution

`Context::set_diagnostics(true)` enables macro and code-generation reporting;
`set_macro_diagnostics` remains available for macro-only instrumentation.
`macro_expansion_stats()` identifies the fully qualified macro and its
originating interpreted fn/letfn source form. Registered native/composed
callables can have no source definition; their canonical name still identifies
them. Redefinitions with different source text are separate rows, even at the
same filename/offset. Counters include calls, generated semantic node counts
(including metadata, for analysis expansion), and invocation plus successful
realization/lowering time. Explicit data `macroexpand` calls count invocations,
but do not generate analyzed semantic nodes.

`code_generation_stats()` identifies the original source/form span and macro
origin, successful analyzed units, expression/function output counts and
inclusive analysis time. `generations - 1` exposes repeated generation of that
source form. Distinct generated forms can share one source span/origin and
contribute to that same row. It does **not** establish equivalent output or
justify caching effectful macros. Repeated execution/invocation of retained code does not add
generation records. Nested clocks can overlap; do not sum these reports as
exclusive CPU categories. They are attribution aids, not an allocation profiler.

Reports own exact source text/location snapshots and retain no interpreter
roots. Disabling stops new measurements; existing reports remain available.
`clear_diagnostics()` clears them without changing switches; context reset
drops them and disables diagnostics. The default path does not read diagnostic
clocks, copy source snapshots or search report tables. Enabled diagnostics can
be expensive and accumulate unique locations until cleared. They do not print
anything themselves, change language effects or skip required macro expansion.
Focused tests compare output, effects, errors and live records with diagnostics
on/off, repeated generation, redefinition and disabling inside a host macro.

## Recorded native baseline

The accompanying October 8, 2026 report and raw data are in
[`results/interpreter-2026-10-08`](results/interpreter-2026-10-08/).
Measurements are from an Apple M3 Ultra virtual machine, 24 visible CPUs,
32 GiB RAM, macOS 15.7.3, Rust/Cargo 1.99.0, LLVM 23.1.2,
`aarch64-apple-darwin`. See that directory's report for measured bottlenecks,
frontend comparisons, variation and the saved known-good executable location.

## Bounded reuse follow-up

The [bounded-reuse report](results/interpreter-2026-10-08/reuse/README.md) records
two saved-baseline comparisons, scalar-root allocation/GC changes, and the exact
scope of the Form-identity cache. `compare_interpreter_reuse.py` rebuilds a
hash-verified baseline archive, links identical analysis/observation harnesses
to both libraries, compares all corpus observations, and measures cold distinct
Forms separately from repeated analysis of one retained Form. It does not
assume macros or repeated source strings are safe to cache.
