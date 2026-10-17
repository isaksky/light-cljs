# Penpot compiler performance investigation — October 16, 2026

The main problem is the amount of work and history retained per macro expansion
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
