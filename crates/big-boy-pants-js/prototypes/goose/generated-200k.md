# Generated 200k acceptance target

Inventory at `c7c0b1938a15ef101111995c905f0193387ec6aa`, November 4, 2026.
This pins an experiment, **not a successful Goose build or a speed claim**.
The [bounded probe recipe and observed failures](generated-200k-probes.md)
are executable evidence. Pellets owns implementation ordering.

## Immutable input and build contract

```sh
bb benchmarks/generated-project/generate.bb target/generated-200k --seed 42 --lines 200k
```

Destination must be absent/empty. Leave the default `--namespace-lines 400`.
Generator's last change is `5ed076d3230ec89be432b5d4c9156f9ee34d298c`;
its SHA-256 is `13e707a203894b666553998dc812c9ffb7bffe2f698be50e0106d5be9d552a1d`.
The [complete, unmodified generated manifest](generated-200k-manifest.edn)
contains every canonical source and Shadow shim's path, namespace, role, byte
count and SHA-256, all operation counts and all 4,526 waste markers.
Two full generations produced identical manifests and all 653 source/shim files.

| Generated artifact | SHA-256 |
| --- | --- |
| manifest.edn | `6a4da9ff2d8682c3e524d86abb36a1628131cae16bcbb0220797b4f85c131907` |
| model.edn | `75a743002f95e38b31f7b351b512e3e8c1dd618a2b7c4ca1947d398503a33bc1` |
| light-cljs.edn | `7c14e540bed768879c75d817010bd3a09ed0c2b96119f5276c41f0b319f8906c` |
| oracle.bb | `d341b739e7db6a28d102fa88e10fc16f9235c0dfc52e8f847dec7b6256eaaebf` |
| check-waste.bb | `fc6cbedb8dcd0e89d1852aa0c619788cd1da8b4835211385f57eb69c182094ba` |
| run-light.mjs | `d55d1a350da3dd5384f5dbaaf4b55e1d929d4e4176a25206fadd3d7322a4d317` |
| shadow-cljs.edn | `0790faf267b7388aaa066ddb9efb470ad6dfb16befe0ab97a923c4efa3800c8b` |
| deps.edn | `a37f60b8e38259f9730fb7e76038dc2f83af865362afb8ea96817abf1b9d4ba8` |

Canonical sources: **199,129 nonblank / 213,984 physical lines, 9,966,430
bytes, 15,450 top-level forms, 596 namespaces**. Roles: 450 worker, 57 macro,
29 group, 29 required-but-unused idle, 29 unrequired orphan, one common support,
one entry. Another 57 `.cljs` macro-loading shims are preserved, but excluded
from light's roots (`src`, `macros`). Thus 567 generated namespaces are reachable
by the declared require graph; loaded bundled libraries are counted separately.
450 “workers” are generated namespaces, never a thread count.

Primary artifact: `out/light/workload.main.js`, ESM entry `workload.main`,
**release / advanced**. Reference invocation, from the generated directory:
`LIGHT release bench --optimize advanced --no-cache --jobs 1`.
Use the generated budgets unchanged: 10,000,000 macro expansions, 1,000,000,000
macro nodes, 3,600,000 ms timeout, 2,147,483,648-byte macro memory and
100,000,000,000 polls. Persistent cache is off. Hash sources/config before and
after every comparison. Source maps use the generated/default release settings
identically in both paths; record effective configuration. Shadow is not a
required timing cell.

Build Rust with `cargo build --release --locked --features goose-macros --bin
light-cljs`, defaults retaining V8/QuickJS, and an optimized native
`compiler_session.goose` runner. Own `target`, `GOOSE_HOME=target/goose`,
`CARGO_BUILD_JOBS=3`, `LIGHT_CLJS_V8_CACHE=require`. The baseline does **not**
yet expose a Goose CLI switch: selection belongs to the subsequent comparison
harness using `MacroHost::use_goose_source_macros`; do not invent one or measure
a different optimization/emission pipeline. V8 must use the same Rust build.

## Oracle and finite correctness acceptance

Run `node run-light.mjs INPUT ITERATIONS` and `bb oracle.bb INPUT ITERATIONS`
from the generated directory. Exact stdout, including its final newline, must
match for **every** completed build and these independently evaluated pairs:

| INPUT | ITERATIONS | checksum |
| ---: | ---: | ---: |
| 0 | 1 | 687181 |
| 1 | 2 | 371978 |
| 42 | 3 | 484264 |
| 999983 | 2 | 945410 |
| 1000002 | 1 | 463429 |

Also require `bb check-waste.bb out/light/workload.main.js --strict`, equal
complete emitted JS bytes against V8 where deterministic, and an explicitly
justified normalization if byte equality is legitimately impossible. A modular
checksum alone is not complete semantic evidence. Compare every source-macro
expansion's full form structure, scalar pools, exact UTF-16, metadata, ordered
children, source spans and expansion origins. Use the existing complete-form
comparator's numeric contract; retain and analyze representative forms after
all producing jobs/sessions die. Check distinct equal occurrences, reused input
aliases, fresh identities and fresh-session isolation. Record dynamic macro and
helper counts by qualified identity, including recursive expansions; the static
counts below are not a substitute.

Failure acceptance: unsupported selected source/helper jobs fail explicitly
with macro/call/definition and emitted instruction context; no fallback or
replay. Include wrong arity/type, compiler/helper failure, bounded bridge/output
limits, cancellation/deadline and poisoned worker cases. Each ends within a
bounded watchdog, joins owned threads/processes and allows a fresh compilation.
Correctness is required at Goose execution limits 1/2/4/8; native optimized full
builds define success. Smaller/debug probes diagnose problems and do not replace
the full target.

## Actual macro surface

All 57 generated macro files were loaded through the actual namespace compiler
in source-only Goose mode. Their **741 emitted factories** (13 per namespace)
all parse/lower: 627 definitions in the 11 intended families plus 114 unused
waste macros. All 741 were inventoried, including nested/dead functions; 285
export today, 399 first reject `new_arguments`, and 57 first reject `rest`.
For the 627 intended-family factories the split is 228 exported / 342
`new_arguments` / 57 `rest`. Export is not execution.

Static source call sites, read from the unchanged canonical source ASTs with
namespace aliases resolved, total **4,314**. These include calls in unused
application definitions; recursive `chain` adds dynamic calls later.
The representative names below are the actual `workload.fonam` declarations.
Every row was invoked with V8 and native debug/optimized Goose, with unchanged
macro bodies. V8 succeeds in all eleven; Goose currently fails as shown.

| Family / representative | Sites | Required executed behavior | First Goose boundary |
| --- | ---: | --- | --- |
| blend / kalvas | 791 | destructured arguments, nested syntax quote | CljConcatAll |
| twice / dukdos | 416 | syntax quote, compiler-generated auto-symbol, vector | CljConcat |
| chain / graidamha | 399 | rest args, first/rest/seq, self-expansion | new_arguments |
| argc / gaikmenfeik | 378 | rest wrapper, count of original &form | new_arguments |
| or-local / joktol | 376 | &env :locals membership, original symbol identity | CljContains |
| sum-all / buvaiskum | 412 | rest wrapper, mapSeq callback with nested syntax quote | new_arguments |
| doubled / gakzoultrei-grekvavus | 402 | vector destructuring and body splicing | new_arguments |
| defcalc / zalstok | 529 | body splicing, demanded wrap helper, returned defn | rest |
| defproto / stipopi | 286 | return defprotocol form | new_arguments |
| defshape / marore-jaipla | 215 | return defrecord form | new_arguments |
| defdispatch / dougri | 110 | return defmulti form, auto-symbols | CljConcatAll |

Observed declaration opcode union: `call call_method cell_get cell_init cell_set
compare_ge compare_le const dec dup form_op function get_capture get_elem
get_field get_global get_local get_this jump jump_if_false new_arguments
new_array new_cell new_form_list plus pop put_elem put_field resolve_this rest
return set_local strict_eq sub to_number_pair to_numeric undefined`.
Only `new_arguments` and `rest` are rejected by the current export allowlist.
The first real worker (`src/workload/jokoum.cljs`) compiles with V8 and fails
Goose at line 7, its unchanged `dougrorje/stipopi` call.

Required form operations present in emitted factories: `CljCallMember
CljCallable CljConcat CljConcatAll CljContains CljCount CljFirst CljGet2 CljInvoke
CljNthOr CljReadRoot CljRest CljSeq CljTruth Conj FnApplyNull Keyword Symbol Value`.
Count/first/get/nth/rest/seq/truth/conj/symbol/keyword/value and suspended root
reads already have implementations. Missing runtime behavior is verified for
concat/all, contains, fnApplyNull, callable/invoke, ordinary function `.call`,
array `.length` writes, `core.sequence` and `core.mapSeq`. Existing `vec`/`mapv`
are useful building blocks, not proof of the missing operations.

The actual demanded `favoukstun` factory in the prepared-unit path is canonical
JS, returning a closure whose body calls `(rt.core.list).call(null,
rt.symbol("workload.common", "norm"), l0)`. Its factory executes, but publishing
that callable through the live forms-only helper bridge fails
`helper-Fault.None-Error.Unsupported`. Its capture includes `rt`, and its code
adds `loose_eq` and `not` to the declaration opcode union. The body has not run
successfully in Goose. Support must cover this actual prepared representation,
code/capture relocation and callable invocation; do not assume all helper
factories are native-form emissions or misdiagnose this as missing scheduling.

## Execution and bootstrap ownership

Rust owns reading, namespace/dependency resolution, macro orchestration,
declaration/binding/destructuring expansion, analysis, optimization and emitted
application JS. All selected generated **user macro bodies and demanded helper
factories/bodies must execute in Goose**, on Goose-owned heaps, from compiler
emission. No hand-translated macros, pre-expansion, reference execution of a
Goose job, changed generator, target reduction or skipped compilation passes.

The existing `source_only()` boundary intentionally keeps compiler core services
in Rust/V8: `expand_core_javascript` filters out the Goose evaluator and uses
its bootstrap. `build_in` compiles/installs `compiler_core_macros.cljs` and
`compiler_sequence_macros.cljs`; `native_core_macros.rs` expands its supported
core shapes in Rust. `defprotocol`, `deftype`, `defrecord`, and extension forms
are analyzer responsibilities; generated macros returning them still run in
Goose. The canonical source AST also has 898 direct defprotocol, 970 defrecord,
212 deftype, 143 defmulti, 1,352 defmethod, 663 extend-protocol and 464 extend-type
heads (includes orphan sources). Their emitted **application** runtime executes
in Node, not the macro VM. These counts do not justify implementing all their
application operations in Goose.

Retain this core/bootstrap boundary for the milestone and call the result
“Goose source-macro execution with V8 core/bootstrap,” not an all-Goose compiler.
It preserves the existing compiler service and limits implementation to reached
user jobs. Auto-gensyms visible here are emitted compiler-scoped symbols; none
of these source bodies introduces a live shared gensym/atom effect. Existing
coordinator effect authority must remain intact.

Charge retained V8 work to every fresh process/build: runtime creation, first
core/sequence compilation (including the process-wide UNITS cache miss), unit
installation, later core expansion, conversions and disposal. Subsequent
in-process UNITS reuse is a separate warm-process scenario. Instrument
`build_in`, `expand_core_javascript`, existing compile traces/engine entries,
Goose exports/boots/restores/imports/execution/bridge and final cleanup; report
source-job versus retained-core entry counts. Existing `GooseSession::stats`
counts only Goose costs and cannot prove total bootstrap cost or its fraction
of the build. Full-target bootstrap share and dynamic counts remain unmeasured
here and are required baseline/measurement outputs, not zero-cost assumptions.

## Worker terminology and measurement gate

Pin the primary configurations as V8 `(compiler slots=1, user execution=1)` and
Goose `(compiler slots=W, execution workers=W)`, W=1/2/4/8. Record both numbers,
retained V8 isolate counts, achieved overlapping execution and ready backlog.
This is an end-to-end configuration comparison; its ratios alone do not isolate
VM scaling from parallel Rust analysis. A fixed-compiler-slots control, if
used, must be separately labeled. Goose's existing native process has
`2 + 2*W` spawned threads, not W total threads; Rust/V8 service threads add more.

After complete native correctness, use fresh uncached processes, an explicit
warmup per cell and at least five quiet interleaved measured rounds per cell.
Keep compiler/Goose builds, generation and Node/oracle checks outside timing;
include process startup through joined cleanup inside. Report wall, aggregate
CPU, process-tree memory scope and separate native memory. Instrumented phase
runs are separate; do not add overlapping intervals or subtract waits. Cached
no-change and leaf edits keep their own labels, cache policy, source restoration
and executed/skipped macro counts. Preserve the seed24301/120-call/8192-read
control and historical results. A performance loss is valid; unsupported builds
are failures, never timing cells.

Current capacity constants (128 waiters, eight-entry Work queue, 64 MiB retained
state, 1,024 queued requests/definitions) are investigation inputs. Persistent
workers, wakeups, waiter reclamation and reviewed poison cleanup already exist.
No target-size budget has been justified here. Exact dynamic expansion counts,
reached callable categories, retained-state high waters and achieved full-target
concurrency remain for the later execution/scale gates.

Scope was checked against the latest milestone/authorization turns in
[the orchestration chat](codex://threads/01a19f89-4ab0-7142-93f1-421889ec10b7)
and every message turn in [the original discussion](codex://threads/01a19f3b-37e9-79e3-8799-4751884be0f0),
paginated to its end. This inventory does not replace the final conversation audit.
