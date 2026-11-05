# Generated 200k acceptance target

Inventory at `c7c0b1938a15ef101111995c905f0193387ec6aa`, November 4, 2026.
This pins an experiment, **not a successful Goose build or a speed claim**.
The [bounded probe recipe and observed failures](generated-200k-probes.md)
are executable evidence. Pellets owns implementation ordering.
The later [full-project baseline](generated-200k-baseline.md) records the
implemented comparison harness and passing V8/Goose application checks; the
inventory observations below retain their original revision and scope.
The later [execution-support recheck](generated-200k-probes.md#execution-support-recheck)
finds no remaining target execution gap after the four support changes; it
does not replace full-scale safety, complete-form verification or measurement.
The [final conversation audit](#final-conversation-audit) below records the
completed milestone, its unfavorable performance result and remaining limits.

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

## Final conversation audit

November 4, 2026, audited delivery through
`b67fabbb430579fe5291f4e0d76081fad5f3f5c0` (tree
`138372cf4cce008474b960df3c714375f05b7cb2`). **The delivered milestone is faithful
within the agreed experimental scope. No unresolved material omission warrants
another implementation pellet.** The unchanged 200k target compiles correctly
through Goose at 1/2/4/8, but the practical performance hypothesis loses here:
V8 takes 6.211 s, versus Goose 20.053/14.234/11.532/18.692 s. Four workers are
best for Goose; even that cell takes 1.86 times V8's wall and 5.04 times its CPU.
This conclusion does not promote Goose to a production backend.

### Sources and adopted promises

Both exact source chats were retrieved with `read_thread` and paginated to
`hasMore=false`: 17 turns in [the current orchestration chat](codex://threads/01a19f89-4ab0-7142-93f1-421889ec10b7)
and 14 in [the original discussion](codex://threads/01a19f3b-37e9-79e3-8799-4751884be0f0).
User messages and final responses were read, not inferred from titles or
summaries. The current orchestration turn was still active during this audit;
no required conversation was unavailable.

- Original turns `01a19f41-9a3e-7021-b2b2-d99148083d57` and
  `01a19f43-9985-7c90-9bd9-cb66f1d6ecde` establish the performance motivation
  and explicit private snapshots, dependency publication and authoritative effects.
  Turn `01a19f48-7d7e-72b0-8732-a572f7e532f9` clarifies that light-cljs owns
  macro expansion; subsequent storage/provenance and worktree instructions apply.
- Current turn `01a1a474-24b5-7282-9bf0-94c74095d893` moves acceptance to the
  existing generated 200k project and preserves the smaller regression workload.
  The seven-stage proposal in `01a1a476-aa4f-7a82-ade3-10a8c85b62aa` explicitly
  makes successful compilation and trustworthy measurement the completion test;
  a speedup is desired, not required. The user adopted it in
  `01a1a477-7270-71b1-bdec-6b6e2137b1a2` and authorized execution in
  `01a1a49d-8fef-76e2-8cd3-c28651b32d68`.
- That proposal adopts Astra/xhigh planning and audit, Sol 6.1/high implementation,
  actual Pellets attributes, finite implementation children, one active agent,
  independent reviews and final gap closure. Earlier authorization preserves the
  original checkout, permits local reviewed landings and forbids push/deploy.
  The earlier request to resolve routine choices autonomously still applies.

### Promise versus delivered evidence

| Obligation | Evidence and audit finding |
| --- | --- |
| Preserve the existing target | Generator/oracle/waste sources are unchanged from the milestone base `c7c0b193`. Fresh audit generation reproduces the pinned manifest and all 653 source/shim hashes: 199,129 nonblank lines, 450 worker namespaces, 57 macro namespaces. No reduced target, macro rewrite or pre-expansion. |
| Execute actual emitted source jobs in Goose | [Execution recheck](generated-200k-probes.md#execution-support-recheck) inventories all 741 unchanged factories and covers all eleven reached families. Full builds execute 5,226 entries across 615 identities and 57 helper producers. `MacroHost::use_goose_source_macros`, the exporter and `compiler_session.goose::worker` connect actual emission to private Goose VM execution. Unsupported selected jobs propagate errors; they do not fall back. |
| Retain compiler and effect ownership | Rust still reads, resolves, orchestrates expansion, analyzes returned forms and emits application JS. The source-only boundary retains V8 core/bootstrap, separately counted and charged in the [performance report](generated-200k-performance.md). Producer authority stays outside copied heaps; reviewed one-worker helper and callback tests prove suspension without replay. This is source-macro execution with V8 core/bootstrap, not an all-Goose compiler. |
| Preserve outputs and durable provenance | Complete JS bytes and five independent oracle outputs match, with strict waste checks. [Full-form verification](generated-200k-probes.md#complete-project-forms-and-teardown-verification) checks all 5,226 expansions, scalar/UTF-16 pools, metadata, origins and history after teardown in debug/optimized native modes at 1/2/4/8. Goose cells match strictly. The 82 V8 span ambiguities are narrowly checked against original aliases/source; they are not blanket normalization. The 22 inherited Rust input collapses remain an explicit reference limitation. |
| Make scale/lifecycle safe | [Safety evidence](generated-200k-probes.md#full-session-safety-and-lifetime-recheck) retains existing quotas: about 20.8 MB peak charged state, 17,773,294 bytes at close, zero final waiters, all spawned native threads joined. The 1,058-suspension stress checks reclamation beyond target demand. Native RSS reaches about 281 MB; the 64 MiB charge is not a process-memory ceiling. Existing cancellation, poison, queue, malformed snapshot, stale identity and fresh-session tests remain valid. |
| Compare fair complete builds | [Performance evidence](generated-200k-performance.md) has one excluded warmup and five quiet interleaved samples for every V8-one/Goose1/2/4/8 cell. Optimization, target and observable work match; wall includes startup through cleanup. Compiler slots vary with the native limit and are named separately. Achieved repeat VM-body overlap is 1/2/2/3, not eight-way saturation. |
| Account for all relevant costs | Whole-command CPU and memory, sampled process-tree RSS, full native RSS and separate phase probes have explicit scopes. Overlapping phase lifetimes are never added into wall or treated as CPU. Native VM slices total only 68–76 ms; compiler transport/conversion grows sharply. A specific allocation/locking root cause remains unproven. |
| Separate cache/edit behavior and preserve the control | V8 cache/no-change and cached leaf each have five measured diagnostic samples with executed/skipped work disclosed. Goose cached installation rejects explicitly; the uncached edited-leaf V8/Goose4 diagnostic succeeds. The original proposal required separation of skipped-work scenarios, not new incremental-backend support. This audit accepts that bounded interpretation; Goose cache support remains undelivered. The 15-namespace/120-call/8192-read control passes both native modes at all limits, with historical reports and the later one-worker raw dataset preserved. |

### Completed work and review

Every pre-audit pellet below is closed and landed. These terminal commits include
their intermediate implementation commits; Pellets remains the queue of record.
The audit read all twelve independent reviewer verdicts directly through their
exact chat IDs, including both repair reviews. They report the following final
heads with no outstanding findings:

| Pellet | Delivered work | Terminal commit / independent review |
| --- | --- | --- |
| 745 | Pinned target and four evidenced support gaps | `d9980e65` / [pass](codex://threads/01a1a4b9-f56f-7263-9b89-2a779df57dff) |
| 752 | Variadic arguments/rest/receiver conventions and identity repair | `b41aeccc` / [pass](codex://threads/01a1a4d6-4685-7253-acfa-012ac3d1fab2) |
| 753 | Bounded syntax-quote assembly, membership and array lengths | `d7442b06` / [pass](codex://threads/01a1a4e3-d169-7230-a67c-d7dac220b6ce) |
| 754 | Deferred map callbacks and exclusive writable snapshot ranges | `988e557f` / [pass](codex://threads/01a1a4fc-1bac-7f01-8c47-b94bed441f81) |
| 755 | Emitted callable helper publication and relocation | `c8c8b4b7` / [pass](codex://threads/01a1a517-685b-75e2-ba2b-94c45e5e4659) |
| 746 | Actual CLI comparison, baseline and cache scenarios | `8509a79c` / [pass](codex://threads/01a1a541-030b-7070-8278-0a24c4aa8847) |
| 747 | All-family execution and boundary recheck | `730a4c70` / [pass](codex://threads/01a1a54f-5e4c-7623-a0ad-639797abb230) |
| 748 | Full-session safety/headroom and continuation stress | `b74740ca` / [pass](codex://threads/01a1a56e-cc42-70a0-88ff-51951f0f0eb5) |
| 749 | Complete-project output/forms/provenance and teardown | `6bf69b39` / [pass](codex://threads/01a1a5b3-5080-7583-af89-5d3135278e36) |
| 750 | Quiet full-build matrix, phase accounting and raw archive | `b67fabbb` / [pass](codex://threads/01a1a5da-29b8-7c60-b440-0693f0553702) |

Independent review found and fixed two substantive issues before landing:
arguments objects erroneously compared structurally (752), and a writable map
memo could overlap immutable snapshot storage (754). Each used its second and
final allowed attempt, with a passing focused review afterward. No duplicate
pellet bypassed those limits. Planners 747/748 had no evidenced implementation
gap and correctly created no speculative children.

The source chat's native agent events show at most one active child throughout
this run: each worker/reviewer finished before the next started. The run record
orders each reviewed fast-forward before the next worker. Actual Pellets
attributes match the requested model/effort split. Dispatch model/effort and
`fork_turns=none` are recorded in the orchestration schedule; the thread API
exposes agent identity/order but not complete invocation-model receipts, so
that part relies on the run record rather than an independent API attestation.
This audit's own independent review/landing remains the orchestrator's final gate.

### Verification, limitations and workflow recommendations

On the audited tree, this audit reran `bb benchmarks/generated-project/test.bb`:
**12 tests, 369 assertions, zero failures/errors**. Fresh full generation matched
the manifest byte-for-byte and all 653 hashes. Babashka checks reread all **97
baseline + 262 performance archive digests**, matched all **580 performance
execution-input hashes** to the current files, and reconciled all **56 successful
build records**, exact output/oracle results, quiet gates, per-cell sample counts
and reported primary medians. Git confirms execution/compiler sources are
unchanged since `6bf69b39`; generator/oracle and the earlier control/report are
unchanged since `c7c0b193`. Tool versions remain Goose267/runtime3025e5810379,
Rust1.99.1, Node26.12.1 and bb1.12.218.

This documentation-only audit reuses the reviewed runtime/form/lifecycle checks:
36 compiler tests per native mode, full-target forms and heavier-control checks,
plus the preceding VM/snapshot/45-row/QuickJS coverage. It does not claim fresh
native compilation or a current full-suite/strict-lint pass. Existing engine/root
Clippy, root formatting, namespace-publication and playground baseline failures
remain explicit limitations. The supported JS surface is finite; general GC,
full ECMAScript, watch/REPL, cached Goose and production migration remain outside
the completed target. A performance loss does not reopen correctness work.

One preservation failure deserves a concrete workflow correction. The optional
external backup of 745's ignored raw probe directory failed with a Babashka parse
error before that finished worktree was retired. Those original raw files are
unavailable; this audit does not claim they were preserved. The committed full
manifest/probe recipes, independent original review, later 741-factory recheck,
and successful full-project archives still support the delivered target. This
historical loss does not require rerunning obsolete failure probes as current
acceptance work. Future retirement should wait for any intended backup to finish
and its hashes to verify. Retained `evidence752`–`755`, `evidence747`–`749` and
both committed archives remain available; keep the safety/provenance raw logs.
The run record and retained directories are under
`/Users/admin/.codex/tmp/pellets-orchestrate/light-cljs-goose-200k-20261104/`.

The integration editor caches are still intact in this run's
`editor-cache-backup`; restoring them and checking original/unrelated checkout
preservation remain the orchestrator's final cleanup responsibilities. This
audit changed only its own worktree. No push/deploy is part of the delivery.

Retain independent review and focused verification reuse. Include actual model
settings in future handoffs, and favor meaningful milestone updates over repeated
unchanged-status messages. For further performance work, first profile the
measured compiler bridge boundary; choose cache support separately if incremental
use matters. No optimization catalog or additional implementation pellets were
created merely because the full-project result is slow.
