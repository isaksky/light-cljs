# Running the real cljs.core in the Pants build

Status, October 30, 2026: design and plan, not a delivered contract. The
user decided that the Pants build (docs/glossary.md: macros run on the
big-boy-pants-js engine) must run the real compiled `cljs.core` instead of
the hand-written port in `src/bbp_runtime.js`. The port grew one missing
function at a time; Penpot keeps hitting the next gap (`fnext`, `delay`,
`MapEntry`, transients). Coverage must be complete by construction. The
pellets are in the Pellets group `bbp-integration`, after light-cljs-644,
whose inventory script is the acceptance harness and progress meter.

Glossary terms are used throughout: V8 build, Pants build, QuickJS reference,
N threads, peak memory, slowdown vs V8.

## What "the real cljs.core" is in light-cljs

light-cljs does not compile upstream `core.cljs`. Its `cljs.core` is
`src/js_runtime.js`: the bounded runtime that every emitted application
embeds (docs/compiler.md, "Immutable collection data and the core runtime"),
10,630 lines in 1,547 manifest units (`src/runtime_manifest.rs`), with 688
core operations (`src/core_operations.tsv`, `CoreFnId`) and 890 public
declarations (`src/core_public_bindings.tsv`). The compile-time variant,
`COMPILER_RUNTIME`, is the same text with hooks `build.rs` inserts (gensym
scopes, reader context, encode budget, environment extensions, interpreter
dispatch). On top of it the Rust emitter compiles the bootstrap macro
definitions (`src/compiler_core_macros.cljs`, `src/compiler_sequence_macros.cljs`).

The V8 build and the QuickJS reference run this runtime at compile time: each
isolate evaluates `const rt = <COMPILER_RUNTIME>` once (`src/evaluator.rs`,
`src/v8_evaluator.rs`), then runs compiled macro units against `rt`.

The Pants build does not. It runs `src/bbp_runtime.js` (4,373 lines): a
command loop and root registry for the engine, a `decl` object holding
protocol tables, and a `core` table of 253 entries that re-implements core
functions over engine intrinsics (`__rt.clj*`, `__rt.form*`). The emitter's
`native_forms` mode lowers literals to engine forms and about fifty fixed
core calls to intrinsics (docs/native-macros.md, "Native core lowering"); every
other core reference becomes `nativeCore$X ?? nativeCoreGetter$X()`, where a
missing entry's getter throws `unsupported operation: native macro runtime
core.X`. That getter is where Penpot stops, one name at a time.

So "run the real cljs.core" means: the Pants build runs `src/js_runtime.js`'s
definitions, the same code the V8 build and emitted applications run, and the
253-entry port goes away. Upstream `core.cljs` is not a candidate: it needs
the Closure Library, which the user has excluded, and a different compiler.

## 1. How the real core runs on the engine with shareable values

### The constraint

The engine shares values between threads only by category
(`crates/big-boy-pants-js/src/value/share.rs`): persistent values (engine
vectors, maps, forms, structs, lazy cells, functions whose captures are
shareable) and concurrent references (atoms, cells, Promises) publish;
ordinary objects, arrays, Maps, WeakMaps, private fields and mutable binding
cells never do, frozen or not ("an object literal is rejected like a
builder"). Realm intrinsics such as `Object.prototype` are shared and
immutable; defining a property on one throws.

`src/js_runtime.js` is written against the opposite model. A vector is an
ordinary object with a private-field store (`#s`) holding a trie; keywords,
symbols, cells, lazy values, metadata, hash caches, atoms, delays and Vars
are recognized through 55 brand stores (WeakMaps or private fields); the
definitions hub `coreCells` is an object literal read in 66 places; protocol
methods and the mask ABI (`cljs$lang$protocol_mask$partition0$`) are
properties written on prototypes, and `extend-type` writes them after boot
(30 prototype writes, 25 `extendType`/`specify` sites); 13 module-level
`let` bindings (gensym counter, identity-hash counter, printing depth, tap
state, the global hierarchy) are mutated by ordinary functions. Every
runtime function closes over that state, so under the engine's rules none
of it publishes, and a second thread cannot even read it.

### The four options

| Option | What it means here | Verdict |
| --- | --- | --- |
| Lower `deftype`/`defrecord` to engine structs | Already done for user types (light-cljs-478). The runtime's own collections are hand-written JavaScript classes, not `deftype`s, so this does not reach them. | Keep; not the answer by itself |
| Freeze on publication | The engine would gain a "published ordinary object" category with sealed private storage and published prototypes; `extend-type` would need a non-prototype dispatch model; the bridge would build runtime objects from Rust instead of engine forms. | Rejected (reasons below) |
| Compile the core once into shared immutable code | Executable code is already shared (`Executable` is runtime-independent). The problem is data, not code. | Necessary, not sufficient |
| A thin native layer only where the engine must own the representation | The engine keeps owning values; the runtime's definitions run above them. | **Recommended** |

### Recommendation: the real core over engine-owned values

Keep `src/js_runtime.js` as the one definition of `cljs.core`, and give it a
third host profile next to the V8 profile (private-field stores) and the
QuickJS profile (`lightCljsWeakMapStores`, light-cljs-292): the
**shared-heap profile**. In that profile:

1. **The engine owns every value that crosses tasks.** Vectors, lists, maps,
   sets, keywords, symbols, metadata (engine forms, light-cljs-466/477),
   records and immutable `deftype` instances (structs, 478), lazy sequences
   and delays (once-cells, 479/613), atoms (engine atoms), regular
   expressions (563), Vars and multimethods (keyed structs, 589/521) keep
   the representations the Pants build has today. Nothing the port built in
   the engine is discarded.
2. **The runtime's representation units are swapped, the rest is verbatim.**
   `build.rs` assembles `js_shared_runtime.js` from the manifest by replacing,
   by unit name, the units that build or read a representation (the brand
   stores, `makeVector`, `hashTrie`, `makeMap`, `makeSet`, `cells`,
   `lazyValues`, `delays`, `atoms`, `metadataValues`, `constructorHashes`,
   `makeType`, `recordTypes`, `vars`, the kind tables' branches) with units
   of the same names and contracts written over `__rt`. The manifest's
   `requires:`/`effect:` validation checks the swap. Every other unit, which
   is where the 688 operations live (`fnext` is `first(next(x))`,
   `partition-by`, `group-by`, `MapEntry`, printing, `delay`, transducers),
   is the same text the V8 build runs. Coverage is complete by construction:
   a name exists because the manifest has its unit.
3. **Brand stores become read-only views over engine tags.** A store's
   `has`/`get` read `__rt.formKind`, `__rt.formReferenceType`
   (light-cljs-592's reference-type brand) or a struct field; `set` is only
   called by the swapped constructors. The runtime already funnels every
   brand test through `privateStore`/`kindedStore`/`storesPredicate`, so the
   seam exists; 44 `vectors.has` reads and 2 `vectors.set` writes are typical.
4. **Dispatch reads engine tables, not prototypes.** The `interpreterDispatch`
   hooks (`keyMethod`, `builtin`, `publicMethod`, `externalMethod`,
   `satisfies`, `extendType`) already exist as the one place protocol
   lookups are made (light-cljs-439). In the profile they read
   `__rt.cljProtocolLookup`/`cljDispatch` tables at the task's frontier
   (light-cljs-515) and `extend-type` installs into those tables; the mask
   ABI of the runtime's own kinds is answered from the kind, as 592 does
   now. Prototypes are frozen at the end of boot and never written again.
5. **The hub and registries are published data, read through the task.**
   `coreCells.X` reads are rewritten at assembly to a read of the published
   hub through the task's globals (`bbp_evaluator.rs` already does this for
   the scalar helpers it borrows: `coreCells.` to
   `globalThis.__lightNativeRuntime.core.`). Boot-time registries
   (`kindTables`, `methodKeys`, `protocolSlots`, `mapTypes`) are built, then
   frozen into engine vectors and maps. Lazily filled memos on method keys
   (`key.slots[arity]`, `key.info`) are precomputed at boot; there are
   about 55 keys.
6. **Task-local state lives on `globalThis`**, which the engine keeps per
   task ("globals remain those of the executing task"). The 13 `let`
   bindings move there or to engine services: the gensym counter is already
   the compiler's (`__lightGensym`); the identity-hash counter becomes an
   engine identity hash; the global hierarchy becomes an engine atom (its
   cross-task order is the effect lanes'); printing depth, tap and
   declaration-journal state are per task.
7. **Boot once, publish, share.** The profile's factory runs once per
   build in a boot task; `Runtime::publish` walks the result and reports
   the first path that cannot publish; the migration pellets drive that list
   to zero. Every macro task then reads one shared `rt`. Today every
   producer task runs the whole `nativeMacroTask` factory (about 1.5 MB per
   task, docs/native-macros.md "Memory limit"); the V8 build boots the
   runtime once per isolate.
8. **Kinds the engine does not own stay task-owned, not unsupported.**
   Persistent queues, sorted sets, transients, eductions, iterator
   sequences, chunk buffers, volatiles and string writers keep the
   runtime's own objects. Inside a task they work exactly as on V8. Across
   tasks they take the existing per-task fallback (`native.helpers.per-task`):
   a helper whose value holds one initializes in each consuming task, and a
   shared atom or delay that would hold one fails with the publication path,
   as today. 644's ranking decides which of these to promote to engine
   kinds later (phase 4).
9. **The emitter changes little.** Literal lowering to engine forms and the
   fifty fixed intrinsic calls stay (they are what made k fall from 20.5 to
   3.4 on the churn workload, benchmarks/native-macros.md "Per-thread factor
   k"); `nativeCore$X` reads resolve to the published hub's members and no
   getter throws. The bootstrap macro definitions compile as on V8, so the
   text-mode entry points that today expand `for`/`doseq` "through the
   JavaScript bootstrap, which this backend does not have" gain it; the
   Rust core-macro expansions (`src/native_core_macros.rs`, 611/618/620)
   stay as the fast path.

### Why not "freeze on publication" with the runtime's own objects

- It needs a new engine category: published ordinary objects with sealed
  private storage, publishable prototypes, and a prototype-free extension
  model anyway (a prototype write by one task at its frontier is a data race
  and the wrong answer for another task). That is most of the work of the
  recommended seam plus a second persistent object kind in the engine.
- The bridge would build runtime objects from Rust and lose the memoized
  native-form import, the unchanged-subtree identity and the `&env`
  persistent map (light-cljs-462/466/558), which the one-thread profile
  shows are already cheap (0.7 s of 22.6 s at 200k).
- Collection operations would be interpreted JavaScript tries instead of
  Rust tries. The strategy's own measurement is that a competent
  interpreter alone lands at k of 5 to 8, and that the native collections are
  what bring k under 4 (docs/shared-heap-compilation.md, "What has to be
  true for the bet to pay off").

It stays the fallback for a specific kind if the seam cannot express it
(section 6, risk 1).

## 2. What stays native, and why

| Layer | Owner | Contents | Why it stays |
| --- | --- | --- | --- |
| Engine intrinsics | Rust, `crates/big-boy-pants-js` | Forms (vector, list, map, set, keyword, symbol, metadata), structs and struct types, lazy cells and delays, atoms, regex values, sorted-map-by positions, protocol dispatch tables with frontiers, `cljRealizeAll`, publication, identity hash, the one-object-in-both-worlds member resolver (`jsMember`) | The representation must be shareable without copying and readable from Rust; equality and hashing of keys must never run user code; realization and delay bodies must run once across threads |
| Fixed-call lowering | Emitter, `native_forms` mode | Literals as forms; `count`, `seq`, `first`, `rest`, `next`, `nth`, `get`, `assoc`, `conj`, `=`, `hash`, arithmetic and the rest of the fifty | Measured speed; the real function remains the reference the harness checks each lowered call against |
| Core macros in Rust | `src/native_core_macros.rs` | `when`, `and`, `for`, `doseq`, `case`, threading and the others 611 and 618 expand | Expansion never enters the engine; the bootstrap definitions now exist as a fallback for shapes Rust declines |
| Evaluator runtime | `src/bbp_runtime.js` minus its `core` table | Command loop, root registry and catalog (light-cljs-619's name resolution), `decl` protocol bookkeeping, reader service, raw-member canonicalization | Compiler-side contracts, not `cljs.core`; unchanged by this work and renamed when the port is deleted |
| Scheduling, lanes, budgets | Compiler, `src/scheduler.rs`, `src/bbp_evaluator.rs` | Pool, deferred units, helper producers, effect lanes (480), the aggregate memory budget, instruction budgets, timeouts | Unchanged; the real core is just more shared code the tasks run |

The minimal intrinsic surface the real core needs from the engine beyond
what exists is small and will be listed exactly by the spike and by 644's
"what the port provides that the real core would need from the engine":
an identity hash for host objects, read access to a lazy cell's state
from the runtime's `LazySeq`/`Delay` handles (realize, claim, `realized?`),
a CAS-built interning map for keywords made at run time (`canonicalMember`
in the port is the pattern), and whatever the publication walk rejects in
a booted profile.

## 3. Semantics that must stay identical

- **Output.** Every build's JavaScript and source maps are byte-identical
  to the V8 build's, at every thread count, with the stress seeds. Today
  that holds for Penpot `app.render` (SHA-256 `d67471d1…`); it must hold
  for `app.main` once it builds.
- **Threads.** `--jobs N` means exactly what `--jobs 1` means. Effect lanes
  (light-cljs-480) keep source order for shared atoms; the real core adds
  one shared atom (the global hierarchy) that takes the same contract. A
  lazy sequence or delay still realizes once per build, in the earliest
  task in source order that needs it (the engine's realization gate in
  `Realize::step`, light-cljs-676, which the real core's once-cells pass
  through too), with other readers parked (613).
- **Function semantics.** Every operation answers as the V8 build, because
  it is the V8 build's code. The port's approximations (its own `satisfies?`,
  `drop`, `partition`, printing, Var call adapters) disappear with it.
- **Representation-level differences stay, and stay documented.** A native
  struct is a frozen object inheriting from `Object.prototype`
  (light-cljs-558, round 2); persistent collections never call user `IEquiv`
  or `IHash` for keys and members (478); hashed seq order follows the
  engine's trie, with printers restoring the legacy traversal; built-in
  prototypes cannot be extended by `extend-type` on `String.prototype`
  (592). These are the engine's value model, not the port's, and the real
  core does not change them. `tests/bbp_macros.rs`'s differential tables
  against the QuickJS reference remain the record of each.
- **Errors.** "Unsupported operation" at a missing name no longer exists.
  What remains explicit is "cannot share", reported with the publication
  path for a shared atom, delay or published helper, never for ordinary
  per-task use.
- **Cache identity** distinguishes the port and the real core (as it already
  distinguishes the backend and its native-form ABI) and hashes the
  assembled profile text.

## 4. Performance: estimate and what to measure

Where the Pants build is today (docs/native-macros.md, light-cljs-632;
docs/shared-heap-compilation.md, light-cljs-575):

| Workload | Pants build | V8 build | Slowdown vs V8 |
| --- | ---: | ---: | ---: |
| Generated 200k, 1 thread, wall | about 1.6x V8 (635) | 8.6 s | 1.6x |
| 200k engine time inside that | 13.2 s | 1.2 s | 11x |
| Penpot `app.render`, 1 thread | 8.68 s | 2.48 s | 3.5x |
| Penpot `app.render`, 4 threads | 7.09 s | 1.66 s | 4.3x |
| Penpot `app.main`, 1 thread (stops at `MapEntry`) | 87–97 s | 13.3 s | about 7x, slower at 4 threads |
| k, churn workload, n = 100 | 3.4 after lowering (20.5 before) | 1 | |

Estimated effect of the real core, by cost:

- **Per-task boot goes away.** Each helper producer runs the port's factory
  today; with the real core a build boots once. Expect lower per-task setup
  and peak memory. The one boot is larger (the full runtime, as V8 pays per
  isolate, tens of milliseconds) and is paid once.
- **Lowered calls are unchanged.** The fixed calls that dominate macro
  bodies on 200k stay intrinsic. Expect engine time on 200k within about
  25% of today's and wall within 10%, since most of 200k's 22 s is catalog
  application, Rust analysis and collection, not core functions.
- **Non-lowered calls take the real dispatch path.** `map`, `filter`,
  `partition`, `into`, printing and protocol calls go through
  `publicMethod`/kind tables instead of the port's direct intrinsic
  wrappers. On QuickJS this generic path is what cost 5.1x (200k) and 8.2x
  (Penpot) against V8 (light-cljs-435). The profile's dispatch units read
  engine tables, which is what the port's `decl` does, so the per-call cost
  should end near the port's; before that re-targeting it may be 1.5–3x
  worse on protocol-heavy code. That is the risk for Penpot, whose
  expensive macros (instaparse's grammar precompile, `add-properties!`) run
  helper code over strings, maps, sets and protocols.
- **Engine work compounds.** Shapes and inline caches (473) and dispatch
  overhead (474) speed up the real core's generic JavaScript; they did not
  help a port that bypassed it.
- **Publication share rises.** More helper values publish once because the
  real core's functions capture only shareable state, so
  `native.helpers.published` should rise against `native.helpers.per-task`.

What to measure, both cores on the same commit, release builds, quiet
machine, three alternating runs:

1. k on the `churn`, `protocol` and `lazy` workloads at n = 0/100/400
   (`bb benchmarks/native-macros-k.bb`), with the QuickJS reference as a
   third column for the generic-path ceiling.
2. Generated 200k and 800k at 1 thread: wall, `time.analysis-ms`, engine
   share of `trace.top-level` and `trace.unit`, collection count and pause,
   `native.producers.setup-ms`, peak memory.
3. Penpot `app.render` and `app.main` at 1 and 4 threads: wall, user CPU,
   peak memory, output SHA-256; `app.main` until it builds.
4. `native.helpers.published` against `native.helpers.per-task`, and the
   count of `nativeCore$` references resolved through lowering against
   through the hub.
5. Boot: time and bytes of the one boot and the publication walk; bytes a
   task holds after boot (today about 1.5 MB).

Gate for flipping the default (section 5): real core within 1.25x of the
port on k and on Penpot `app.render` wall at 1 thread, and not slower at 4
threads; otherwise the causes are filed with numbers, as the group's rule
for missed gates says.

## 5. Migration without a long broken period

Both cores run behind one flag in the Pants build: `--macro-core port|real`
on the command line and `:macro-core` in `light-cljs.edn` (shadow-cljs has
no counterpart; the name follows the existing `--macro-*` family). The
default stays `port` until the gates pass. The cache identity includes the
value, so switching never serves the other core's artifacts.

Phases and gates:

| Phase | Pellets | Gate |
| --- | --- | --- |
| 0. Spike | light-cljs-645 | Real `map` over a vector built by compiled code runs in a Pants macro, shared by two tasks on two threads, result identical to V8; the seam's size is counted; go/no-go recorded |
| 1. Profile and boot | 646 (engine primitives), 647 (profile assembly), 648 (boot once, `--macro-core`) | The assembled profile boots and publishes with zero rejected paths; `bbp_macros` passes with `--macro-core real` or lists each difference |
| 2. Parity | 649 (dispatch and extensions), 650 (the 644 harness in `real` mode), 651 (Penpot) | Protocol × kind matrix equal to the reference in both cores; every var, type and protocol the port supports is supported by the real core; `app.render` byte-identical at 1/2/4/8 threads, `app.main` builds or stops outside the core |
| 3. Performance and flip | 652 (measure), 653 (flip default, delete the port) | Section 4's gate; then `bbp_runtime.js` keeps only the evaluator runtime, the flag goes, docs are rewritten |
| 4. Promote kinds | 654 (maybe-later) | Queues, sorted sets, transients, eductions become engine kinds by 644's ranking |

The pellets are queued in that order after light-cljs-644 in the
`bbp-integration` group; 653 waits for the user's agreement on 652's
numbers.

Comparison during phases 1 to 3 is the 644 harness run in both modes, plus
the existing differential tables against the QuickJS reference and the
Penpot and 200k output hashes. No hand-port pellet is filed while the flag
exists; a gap found in `port` mode is answered by the `real` core, not by a
new port entry.

What happens to recent port work:

| Work | Fate |
| --- | --- |
| 558 one object in both worlds, `jsMember` resolver | Stays. The resolver answers struct fields and `call`/`apply`; the real core's prototypes do not reach engine forms |
| 561 sorted-map-by, 563 regex, 564 mutable deftypes | Stay; engine-owned kinds the profile uses |
| 589/602/603 Vars, 521 multimethods | Representation stays (keyed structs); the port's call adapters are replaced by the runtime's own `Var`/`MultiFn` units over them |
| 592 protocol values and predicates | The reference-type brand (`formReferenceType`) and the engine side stay; the port's JavaScript `satisfies`/`implements?` are deleted and the real units run over the brand; its matrix becomes harness rows |
| 611/618/620 Rust core macros | Stay; the real bootstrap definitions become the fallback for declined shapes |
| 613 delays | The once-cell stays; `delay`, `force`, `realized?`, printing come from the real core over it |
| 619/640 name resolution, catalog | Unchanged; evaluator-side, not `cljs.core` |
| 622 linear realization, 632 rope charges | Engine; unchanged |
| 633 `fnext`/`nfirst`, 623 constructors and the other hand entries | Deleted with the port; the real units replace them |
| 642 transients | Not implemented by hand. Transients stay task-owned runtime objects (section 1, point 8) and are promoted in phase 4 if 644 ranks them |
| 644 inventory | Becomes the harness. Its category pellets are not filed as port work; its "what the port provides that the real core needs from the engine" list feeds the engine-primitives pellet |
| 635 Penpot 7x profile | Continues; its numbers are the baseline for section 4 |

## 6. Risks and open questions

1. **The seam leaks: runtime units read representation internals beyond the
   stores.** Counted today: 45 `vectors.get`, 72 `cells.get`, 23
   `maps.get`; some callers walk a trie or cons chain directly. If a kind's
   readers are too entangled, the fallback is to keep that kind
   runtime-owned and task-owned (point 8) until promoted; the spike counts
   the readers per kind and the inventory pellet lists them. Recommendation:
   start with the six form kinds, structs, lazy, delay, atom, Var,
   multimethod and regex, which the engine already owns; promote nothing
   else before the harness is green.
2. **The booted profile will not publish at first.** Closures capturing an
   object literal or a mutated `let` are rejected at that edge with a path.
   Recommendation: treat the rejection list as the work list; the publish
   diagnostic is exact, so each item is one edit, and the gate is zero.
3. **Generic dispatch is slower than the port's direct calls** (section 4).
   Recommendation: re-target the dispatch units to engine tables in phase 2
   before measuring; keep the fifty lowered calls; add to the lowering list
   only from measured call counts.
4. **Two profiles of one file drift.** Recommendation: the profile is unit
   swaps validated by the manifest, with no edits to shared units; the
   manifest tests assert that every shared unit's text is identical in
   both assembled runtimes; the harness compares all three builds.
5. **Keyword identity across tasks.** `(identical? :a :a)` must hold for
   literals in every task; keywords made at run time are fresh, as upstream.
   Recommendation: literal keywords are engine forms interned by the
   emitter per site today; confirm the interning table is runtime-wide, and
   use a CAS map for `keyword`-made ones only where the runtime caches them.
6. **Compile-time hooks assume one runtime per isolate.** Reader context,
   gensym scope, encode peak and environment-extension notices are closure
   state in the V8 profile. Recommendation: in the shared profile they read
   and write the task's globals, which is where the Pants build already
   keeps them (`__lightGensym`, `__lightBuildService`).
7. **The global hierarchy and other shared mutable core state.** `derive`
   at compile time mutates one atom. Recommendation: an engine atom under
   effect lanes gives the `--jobs 1` order; document it next to the atoms
   contract.
8. **Memory.** One published runtime is counted once in the aggregate
   budget instead of per task; the 4 MiB starting-heap tests must still
   pass. Recommendation: measure the boot's bytes in the spike and keep
   lazy setup for tables the runtime builds on demand.
9. **Decisions for the user.**
   - Confirm that "real cljs.core" means `src/js_runtime.js` (recommended;
     upstream `core.cljs` needs the Closure Library).
   - Confirm the representation: engine-owned values under the real core
     (recommended) rather than the runtime's own objects frozen on
     publication, accepting that the documented representation-level
     differences remain.
   - Confirm the flag name `--macro-core` / `:macro-core`, the default
     `port` until the phase-3 gate, and that flipping and deleting the port
     is one pellet the user approves when the numbers are in.
   - Confirm that no further hand-port pellets are filed (642 is deferred
     into phase 4; 644 files no category pellets as port work).

## Spike

light-cljs-645, phase 0, on `df7531fa`. Code: `tests/bbp_macros/real_core_spike.rs`
(part of the `bbp_macros` test binary); counts: `bb benchmarks/real-core-seam.bb`
(add `--no-walk` to skip the Pants-build tests it runs for the walk).

### What ran

- **The real `map` in a Pants-build macro.** `RuntimeManifest::reached_by_uses`
  from `mapSeq` (the runtime binding of `map`) keeps 152 of the 1,547 units,
  56.8 KB. Seven of them are swapped by name for the spike's shared-heap
  profile: `vectors`, `cells` and `lazyValues` become read-only views over
  `__rt.formKind` and the engine's `cljFirst`/`cljRest`/`cljCount`/`cljSeq`
  (a lazy payload's `value` is `__rt.cljSeq(value)`, its `thunk` is `null`),
  `LazySeq` builds `__rt.lazySeq(thunk)`, `cons` builds `__rt.cljCons`,
  `emptyList` is `__rt.formList()`, and `kindHooks.seqVector` is
  `__rt.cljSeq`. The other 145 units, `mapSeq`, `seq`, `first`, `rest`,
  `next`, `seq.branches`, `lazyBranch`, `LazySeq.seq` among them, are the
  runtime's own text. A macro namespace defines `map` as a helper whose
  initializer boots that slice (`js*`), and `(defmacro m [] (vec (map inc
  [1 2 3])))` expands to `[2 3 4]`: the helper is `mapSeq`, its result an
  engine `lazy-seq`, and the program is byte-identical to the V8 build's and
  the QuickJS reference's at 1 and 2 threads, seeded.
- **Realized once, in one task.** Walking the lazy result twice calls the
  mapped function three times: the engine's once-cells do the realization the
  runtime's `deferredValue` did, through the unchanged `seq` units.
- **Without the swaps** the verbatim units fail on the first engine vector:
  `seq` finds no brand it knows and no iterator, and its error message cannot
  stringify the vector.
- **Boot once, publish, share between two threads (after light-cljs-646).**
  As first run, neither the verbatim nor the swapped `mapSeq` published
  (`value.capture(2) is a task-owned binding cell`), so the helper took the
  per-task fallback (`native.helpers.per-task` 1, `published` 0). With the
  engine primitives of 646 (below) the slice seals itself at the end of its
  boot (`shared_map_slice` in the test): `__rt.seal` on the registries,
  sentinels, constructors and prototypes it declares (deeply through the
  arrays and objects holding them), then `__rt.sealBindings` for the
  bindings it reassigned while loading. Three more units are swapped to get
  there, all profile edits of the kind 647 owns: `coreMethod` seals its
  `arities` array and resolves its method key at boot instead of memoizing
  it in a `let` its dispatcher assigns, `storesPredicate` seals its rest
  array of stores, and the slice's WeakMap and WeakSet brand stores, which
  never hold an engine value on this path, become one sealed empty store.
  Then `mapSeq` publishes; the macro helper is published once
  (`native.helpers.published` 1, `per-task` 0) and the program is still
  byte-identical to the V8 build's at 1 and 2 threads; and two tasks on two
  threads call the one published `mapSeq` together (a barrier holds each
  inside its first call of the mapped function until both are there), each
  realizing its result once (three calls each for two walks). A lazy
  result of the real `map` does not itself publish: `mapSeq`'s thunk
  captures its rest-argument array (`value.thunk.capture(0) is a
  task-owned array`), so a helper whose value is such a sequence still
  initializes per task. That is profile work (a thunk over a sealed copy,
  or the engine's own `cljCons` for one collection), not an engine
  primitive.

### Seam counts

Static, from `src/js_runtime.js` (textual: a unit names a binding outside
the unit declaring it; the manifest's own analysis is exact):

| Store | Units reading the payload (`get`) | Units calling `set`/`reset` | Units writing the payload (any way) | Units testing the brand only (`has`) |
| --- | ---: | ---: | ---: | ---: |
| `vectors` | 40 | 2 | 4 | 32 |
| `cells` | 55 | 16 | 18 | 8 |
| `maps` | 21 | 3 | 3 | 28 |
| `sets` | 17 | 2 | 2 | 20 |
| `views` | 24 | 2 | 2 | 11 |
| `lazyValues` | 12 | 1 | 9 | 15 |

"Writing the payload (any way)" adds the units that mutate a payload they
retrieved: through the `get` call (`LazySeq.fields` assigns
`lazyValues.get(this)[key]`), through a local holding it (`ChunkedCons.fields`
sets `cells.get(this).chunkedState.hash`), or by passing it to a helper that
mutates its first parameter (`deferredValue`, `kindHooks.realizeLazy`), which
counts the helper's unit too (`bb benchmarks/real-core-seam.bb --writers`
lists them; `--self-test` checks the analysis on fixtures). It is an upper
bound: the two extra `vectors` units call a subvector payload's own `pop`
function, which writes nothing. The real writers are the representation
units the profile swaps (constructors, `.fields` setters, the lazy
realization in `deferredValue`/`LazySeq.seq`/`lazyBranch`/`seq`/`next.cells`,
`makeVector.hash`, the compiler form bridge): for `lazyValues` the spike's
view already answers them, as its `thunk` is `null` and its `value` setter
drops the write while the engine cell holds the value.

| Captured state | Units naming it |
| --- | ---: |
| `coreCells` | 31 |
| `kindTables` | 57 |
| `methodKeys` | 179 |
| `protocolHooks` | 156 |
| `kindHooks` | 153 |
| a module-level `let` (18 bindings in 13 statements) | 44 |
| a binding reassigned at load time (13: `assoc`, `conj`, `contains`, `count`, `first`, `get`, `lookup`, `next`, ...) | 291 |
| any of the above | 632 |

Publication walk of a naive boot of the whole `COMPILER_RUNTIME` on the
engine (it boots: 748 members, every export and every `core` hub member,
each published on its own; 181 publish). Rejections grouped by cause, the
member's first edge and the kind at the end of the path, with the first
path:

| Members | Cause | First path |
| ---: | --- | --- |
| 448 | closure capture, binding cell | `rt.setDeclarationDemand`: `value.capture(0)` |
| 65 | closure capture, array | `core.removeAllMethods`: `value.capture(0)` |
| 32 | function own properties, object | `core.NodeSeq`: `value.properties.property(prototype)` |
| 17 | closure capture, object | `core.uncheckedAdd`: `value.capture(0).properties.property(prototype)` |
| 5 | member is an ordinary object | `rt.compilerForms`: `value` |

The same walk over the 119 top-level bindings of the swapped `map` slice:
12 publish; 76 fail at a binding cell (first `privateStore`), 10 are
WeakMaps (`keywords`, `symbols`, `metadataValues`, ...), 10 ordinary objects
(`vectors`, `cells`, `lazyValues` as the spike wrote them, `protocolHooks`,
`kindHooks`, the sentinel `noBranch`, `sequencePrototype`), 2 arrays
(`kindTables`, `methodKeys`), 2 Maps (`nativeMethods`, `coreDispatchers`),
1 WeakSet, 5 through a captured array or object, and `LazySeq` through its
prototype.

**After light-cljs-646** (same script, Pants build). The naive boot now
publishes 193 of the 748 members (181 before), and no rejection is a
binding the engine kept in a cell for want of a primitive: fixed bindings
publish once initialized, and the 143 members that still fail at an
*unsealed* binding (the 13 load-time wrappers) pass that edge once the boot
task calls `__rt.sealBindings(rt)`. The walk after that seal
(`full-sealed`), first path per cause:

| Members | Cause | First path | Who removes it |
| ---: | --- | --- | --- |
| 287 | closure capture, object | `rt.symbol`: `value.capture(0).properties.property(prototype)` | 647: `__rt.seal` the runtime's prototypes and object registries |
| 99 | function own properties, object | `rt.get`: `value.properties.property(prototype)` | 647: `__rt.seal` the constructors and their prototypes |
| 79 | closure capture, array | `rt.extendInstance`: `value.capture(2).edge(0)[0]` | 649: multimethod and extension tables move to engine tables |
| 41 | closure capture, WeakMap | `rt.compilerExceptionMessage`: `value.capture(0).edge(0)` | 647: brand stores become views over engine tags |
| 30 | closure capture, binding cell (assigned from a function) | `rt.setDeclarationDemand`: `value.capture(0)` | 647: the module-level `let`s move to task globals or engine services |
| 6 | closure capture, WeakSet | `rt.makeReify`: `value.capture(1).edge(0)` | 647: brand stores |
| 5 | member is an ordinary object | `rt.compilerForms`: `value` | 647: compile-time hooks read task globals |
| 5 | closure capture, Map | `rt.keyword`: `value.capture(0).edge(0)` | 647 (`keyword` over `formKeyword`), 649 (reader and namespace registries) |
| 3 | closure capture, Set | `core.munge`: `value.capture(2).edge(0)` | 647: tap state per task |

Every remaining cause is an object literal, array, prototype, brand store
or `let` capture in the runtime's units; none needs a further engine
feature. The `map` slice walk (each of its 119 bindings on its own, after
`sealBindings`) publishes 16; its rejections are the brand stores and
registries the test's `shared_map_slice` seals or swaps, after which
`mapSeq` publishes.

### Engine primitives the spike needed that do not exist (input to light-cljs-646)

1. **Initialized bindings that are never assigned publish as their value.**
   The engine keeps a binding in a task-owned cell when a closure captures
   it before its initialization: a function declaration called before it,
   or a later `const` (`closures_over_forward_or_reassigned_bindings_do_not_publish`).
   Every mutually recursive pair of runtime units is such a capture; this is
   448 of the 567 rejected members. Once the boot task has initialized the
   binding and no code can assign it, the cell is immutable and can be
   published as its value.
2. **Sealing at the end of boot.** The 13 bindings the runtime reassigns
   while loading (the branch and protocol wrappers, `seq =
   behindProtocol(seq, ...)`) are written only by load-time code. A seal at
   the end of the boot task makes their cells immutable (a later write
   throws) and publishable. The 18 module-level `let`s that functions
   assign after boot (gensym and identity counters, tap and printing state,
   the global hierarchy, declaration-journal hooks) cannot be sealed; they
   move to task globals or engine services, as point 6 of section 1 says.
3. **Sealed boot-time registries.** The runtime's shared state is read
   through captured identity (`methodKeys` by 179 units, `protocolHooks`
   156, `kindHooks` 153, `kindTables` 57), so rewriting every reader to
   engine vectors and maps, as point 5 of section 1 proposes, would edit
   hundreds of units. Of that state, two kinds need different treatment:
   - *Complete after boot, never written again:* `kindHooks` and
     `protocolHooks` (their members are assigned only by load-time effect
     units), the sentinels (`noBranch`, `missing`, `protocolSentinel`),
     `sequencePrototype`, `mapTypes`/`setTypes`, the `methodKeys` array
     itself, and the prototypes of the runtime's own types (49 members fail
     through `.properties.prototype`). An identity-preserving seal, which
     turns a frozen ordinary object, array or Map whose values all publish
     into a persistent value and rejects later writes, keeps these units
     verbatim. This is the "freeze on publication" option section 1
     rejects, but only for boot-time registries and prototypes, not for
     values: values stay engine-owned, so its objections (prototype
     dispatch for values, the bridge, interpreted tries) do not apply.
   - *Dispatch state that changes after boot,* which a seal would break:
     `kindTables` (`extend-type` and `specify!` on a runtime kind empty
     it), `nativeMethods` (`extend-type` of host types, set and undone by
     the declaration journal), `coreDispatchers` and the dispatch cells
     `dispatchCell` creates on first use, the protocol type tables and
     protocol values `extend-type` patches (`ICounted.number = true`), the
     runtime types' prototypes when a program extends them, the method
     keys' lazily filled memos (`key.slots[arity]`, `key.cell`,
     `key.info`), and the multimethod tables (`defmethod`; the 65 members
     failing through a captured array). These are not sealed: the memos
     are filled at boot (about 55 keys), and the rest move to the engine's
     dispatch tables read at the task's frontier (point 4 of section 1,
     `cljProtocolLookup`/`cljDispatch`, light-cljs-515) or to the keyed
     structs Vars and multimethods already use (589/521), with cross-task
     order from the effect lanes. The units that write them are the
     dispatch and extension units light-cljs-649 re-targets; the seal
     applies only once they no longer write captured objects.
4. **Not needed for `map`:** an identity hash, a lazy cell's raw state, or a
   CAS interning map (section 2's list). Realization through
   `__rt.cljSeq`, `__rt.lazySeq`, `__rt.cljCons`, `__rt.formList` and
   `__rt.cljWithMeta` existed.

Profile work, not engine work, found on the way: the brand-store views must
themselves publish (the spike's object literals do not; primitive 3 or a
struct covers them), and the spike's `cells` view reports an engine vector's
chunked seq as a plain `cons`, so the real `map` walks a vector one element
per step instead of 32. That is observable when the mapped function has
side effects; a `chunks` view over `__rt.cljChunkFirst`/`cljChunkRest`
(which exist) restores the runtime's chunked path.

### Go/no-go

**Go for the representation seam, no-go for sharing until 646 lands the
three primitives above.** The seam the design worried about is bounded: 7
swapped units ran 145 verbatim ones, and the payload readers per store are
12 to 55 units, each a view over intrinsics that already exist. What the
design underestimated is publication of the runtime's closure environment:
it fails before any representation is reached, at binding cells, for almost
every function. Without primitives 1 to 3, phase 1's gate (the profile
boots and publishes with zero rejected paths) is not reachable by profile
edits, and the real core would run as today's port does, booted once per
task. With them, 647 (profile assembly) can start from this spike's swap
list; the swap mechanism and the manifest slicing it needs work now.

**Update after light-cljs-646: go for sharing.** The three primitives exist
(`crates/big-boy-pants-js/docs/architecture.md`, "Shared-heap profile
primitives": fixed bindings, `__rt.sealBindings`, `__rt.seal`); the
identity hash and keyword interning map of section 2 were not needed by any
unit the spike or the naive boot reaches, and that table says why. The
real `map` now boots once, publishes and runs on two threads; what stops
the whole runtime from publishing is the profile work listed in the table
above, and one finding for 647: lazy results of the real sequence
functions capture per-call arrays (`mapSeq`'s rest arguments), so they stay
task-owned unless the profile changes how those thunks hold their inputs.

## Profile assembly (light-cljs-647)

Phase 1's first pellet assembled the shared-heap profile and booted it.
The pieces, all in the tree:

- `src/js_shared_profile.js`: the profile's units in the manifest format of
  `src/js_runtime.js`. `build.rs` (`shared_runtime`) replaces the runtime's
  units of the same names, places the profile-only units first, takes the
  profile's export section (which returns the exports through
  `sharedProfileBoot`), and writes two files: the annotated result, which
  `tests/compiler/runtime_manifest.rs` parses and validates like the runtime
  itself (`__rt` allowed as the one extra global, every swapped unit
  declaring the bindings of the unit it replaces, every other unit the V8
  profile's identical text), and `SHARED_RUNTIME`, the text to boot: no
  headers, every `coreCells.X` and every per-task binding
  (`SHARED_TASK_STATE` in `build.rs`: the module-level `let`s, the
  per-task registries next to them, the compile-time hooks' own `let`s and
  the `protocolSlots` memo) rewritten to the task's cells
  `globalThis.__lightSharedCells`, the two immutable vocabularies built by
  `sharedWords`, and the compile-time hooks of `COMPILER_RUNTIME` inserted
  (rewritten the same way). The V8 profile's embedded runtime is untouched.
- Task-local state (point 6): a task's cells are an object whose prototype
  is the published `coreCells` hub, whose prototype is `sharedTaskDefaults`;
  writes land on the task's object, reads fall through. The boot task boots
  over such an object and `sharedProfileBoot` moves its writes to hub cells
  (`currentNs`, `hostClock`, ...) into the hub before sealing it.
- Representation units swapped: `vectors` is a view over `formKind` whose
  payload (`sharedVectorStore`) carries the engine vector and a `tail`
  getter of the items from the tail offset, which the verbatim units that
  read a small vector's or a map entry's items directly take; the storage
  units `vectorLeaf`, `vectorSlice`, `vectorNth`, `vectorConj`,
  `vectorAssoc`, `vectorPop`, `makeVector` and `subvec` read and build
  engine vectors (`formNth`, `formConj`, `formAssoc`, `cljPop`), and a
  transient edits a trie of the items. `cells`, `lazyValues` and `delays`
  are views over `formKind` with a private-field fallback for the kinds the
  engine does not own (point 8: `cells` also holds the runtime's own
  `repeat`, `cycle`, `iterate`, `tree-seq` and range cells); a lazy or
  delay payload's `thunk` is null once `cljRealized` says so, else a
  function that realizes the cell, so the verbatim `deferredValue` and
  `realized?` read the engine's state, and a delay's `value` reads
  `cljDelayState` without running the body. `cons`, `emptyList`, `LazySeq`
  (a computed result with no thunk realizes the cell at once) and `Delay`
  build engine forms; a runtime-owned sequence that reaches the engine
  there (a `cons` tail, a lazy thunk's result: a list of the verbatim
  `listFromArray`, an indexed seq, the runtime's own `iterate`) goes
  through `sharedSeqForm`, a lazy engine seq that takes one element at a
  time through the verbatim `seq`, `first` and `rest`. A fractional vector
  index in bounds truncates (`vectorNth`, `vectorAssoc`), as the V8
  profile's bitwise indexing has it. `keywords` is a view over engine
  keywords and `keyword`/`makeKeyword` build `formKeyword` (a run-time
  keyword is fresh, as upstream's). `constructorHashes` answers an engine
  sequence's hash (vector, list, cons, lazy seq) from the runtime's own
  `hashOrderedColl` over its items, cached per task (`formHashes` on the
  task's cells, as the V8 profile's WeakMap caches it), and any other
  engine form's from the engine: the engine folds a host object's identity
  hash, which is the runtime's hash of a function or a plain object but not
  of a symbol, a list of `listFromArray`, a map or a Date, which the
  runtime hashes by value and equality compares by value (light-cljs-677);
  `identityHashes` reads `__rt.identityHash`, the engine's runtime-wide
  identity service, so `[f]` and `(f)` hash alike. A delay's view reads
  the value of `cljDelayState` in every state (the seed while pending or
  after a failed realization) and reports a `false` thunk for a realized
  delay that prints as pending, as a `(Delay. false x)` does in the V8
  profile (678). `cons` normalizes a tail that is not a seq through `seq`
  when it is called, as the V8 profile's does (680); one that normalizes to
  nil keeps the cons kind (an engine cons over the empty list, not counted),
  as the V8 profile's `Cons` with a nil tail is, and only an originally nil
  tail makes a counted list (682); and a `conj` onto a
  list stays a counted list: the engine's prepend onto an engine list, a
  `ListValue` with its count onto a runtime-owned one (681). Every other
  WeakMap and WeakSet brand store (`symbols`, `metadataValues`, `atoms`,
  `vars`, `recordTypes`, `multiFns`, `treeRoots`, ... 34 stores) is a
  `sharedStore`, a private-field class with a WeakMap's API that seals and
  publishes and never brands an engine value; `coreMethod`,
  `storesPredicate` and `callable` seal what they capture (`callable`'s
  arity table, held only by its closure, stood between 454 members and
  publication; 666); the method-key memos (`slots` for every arity a
  dispatcher declares, 0 to 22 with `invokeProtocol`, `cell`,
  `cell.protocol`, `info`) are filled at the end of the boot for the keys
  of `methodKeys` and the dispatchers' own keys (point 5; 679).
- Registries complete after boot (`protocolMasks`, `coreProtocolNames`,
  `coreProtocolsByName`, `nativeImplementations`) are `sharedMap`s: a Map's
  `get`, `has`, `size` and entry iteration over a sealed entry array. The
  vocabularies (`reservedNames`, `readerCoreNames`, `specialSymbols`) are
  sealed word arrays. The registries written after boot (`coreDispatchers`,
  `nativeMethods`, `protocolValues`, `protocolFunctions`,
  `protocolVariables`, `protocolGetters`, `reifySites`) live on the task's
  cells: each task's own Map, seeded with the boot's entries
  (`sharedSnapshots`, sealed with the rest), so a registration after boot is
  the registering task's, as the per-task fallback has it, and a write to a
  sealed boot entry throws, until light-cljs-649 moves them to engine
  tables. Two brand stores join them as registries keyed by identity
  (`sharedRegistryStore`; 666): `protocolImplementations`, what
  `installMethods` records per value (read back only by itself and by
  `collectionWithMeta`; its boot entries are sealed arrays of the method
  pairs), and `typeNames`, because a private field on a prototype or on a
  type's function (`Loadable`'s, installed while booting) keeps the engine
  from sealing it, and an unsealed `Loadable` stood behind every path
  through `kindHooks.isLoadable`. `ClassBrand`'s "some class is branded"
  flag is the task's cell `classBrands`, not a private static of the
  class, for the same reason. The `EMPTY` statics of the kinds the runtime
  still builds as its own objects (hash sets, array and hash maps, queues)
  are getters that build the task's own on first read.
- `sharedProfileBoot` fills the memos and the registry snapshots, moves the
  boot's writes to hub cells into the hub, seals, deeply through properties
  and prototypes, what every top-level binding holds (the list is generated
  by `build.rs`, with the names beside it for the diagnostic of the bindings
  a seal cannot take), then calls `__rt.sealReachable` (light-cljs-666:
  `sealBindings` plus, through the closure captures no JavaScript walk
  follows, the own-property records and untouched default prototypes of
  the functions made while booting, the prototype inspected even behind
  a record a shallow `seal` already took), and gives the boot task cells
  of its own (`sharedTaskCells`: the hub's cells and the defaults' data,
  read from the hub itself, as writable own properties, which every task
  makes before it reads the runtime; `core.sharedTaskCells` for the tasks
  light-cljs-648 starts).

`tests/bbp_macros/shared_profile.rs` boots `SHARED_RUNTIME` in a boot task
and publishes it; on failure it prints the first rejected path, the walk
of the 748 members by cause and the top-level bindings a seal cannot take
(`bb benchmarks/real-core-seam.bb` prints the same walk);
`shared_profile_publication_path_of` prints one member's path, and with
`BBP_PUBLISH_TRAIL=1` the engine prints the kind and function name of every
object on it. The other tests there cover the task's writable cells, the
vector contract across the 32-element boundary, the lazy and delay states,
engine keywords, identity hashes (an engine vector and the runtime's list
of one host function hash alike), a task's first cells (the boot's
defaults, with no cells needed), runtime-owned sequences crossing into
the engine's `cons` and lazy realization, fractional vector indexes, the
bounded fallback diagnostic and the boot's memory.

State after light-cljs-666's first attempt (2026-10-31): the profile
boots; 716 of 748 members publish (267 after 647's third attempt, 268
after its second, 259 after its first, 193 after 646). The causes 647
removed: the default `prototype` objects of the functions made while
booting (425 members, the reachable seal), the captured Maps (the
registries above), the frozen empty array the record wrapper of `maps`
captured and the `EMPTY` statics. The causes 666 removed, each found with
`BBP_PUBLISH_TRAIL=1` and `shared_profile_publication_path_of` (the trail
names the kind and function name of every object on the path): the arity
table `callable`'s function captures (454 members: `concat` and the other
combinators built while booting, reached from almost every operation
through the kind tables); then `Loadable`'s function and prototype, which
`typeNames` and `protocolImplementations` had branded with private fields
(an object with private elements does not seal: `__rt.seal` reports
"internal slots"), behind every path through `kindHooks.isLoadable`
(407); then `ClassBrand`'s private static flag (the same, through
`cloneablePayload` and the brand stores). A `shared_profile_eval`
diagnostic (`SHARED_EVAL="<js>"`) evaluates an expression after the boot
and prints its publication path, which is how the unsealable members were
listed (`__rt.seal` over every export and `core` member). The remaining
rejections, by cause:

| Members | Cause | Example | What removes it |
| ---: | --- | --- | --- |
| 11 | the reader's WeakMaps (`streams`, `readerMaps`), declared at the top of the `reader` unit's IIFE (scope 1, which `build.rs`'s `rewrite_task_state` passes over: `SHARED_TASK_STATE` rewrites scope-0 declarations and free names only) | `core.readLine`, `core.compilerInputWindow` | a second rewrite list in `build.rs` for named declarations of the reader's scope 1, rewritten to the task's cells like the module-level `let`s, with lazy WeakMap defaults in `sharedTaskDefaults` (the declaring tokens stay as dead locals) |
| 8 | the reader's `tags`/`builtinTags` Maps (filled while booting by `readerInitialTagTable`) and `namespaces` | `core.readerRegisterTagParser`, `core.createNs` | the same rewrite, with the tag tables as per-task registries seeded from the boot's entries (`sharedSnapshots`) and the builtin table sealed; namespaces as sealed registries (648) |
| 6 | `demungePattern`, a top-level `RegExp` (exotic data, which `seal` cannot take; the walk's `TypeError` is silent), and the reader's nested tables (`readerInitialTagTable`'s, the timestamp parser's) | `core.demunge`, `core.readerInitialTagTable`, `core.ednRead` | `demungePattern` as `__rt.formRegex(source, "g")` in the profile, if `String.prototype.replace` takes one; the reader's tables through the rewrite above |
| 5 | the reader's nested `let`s (`defaultTag`, `activeDepth`) | `core.ednReadString` | the same rewrite, with data defaults (`null`, `0`) in `sharedTaskDefaults` |
| 1 + 1 | `rt.compilerForms` (the V8 form bridge's `decode` keeps a `let` a nested function assigns), `core.currentNs` (a `Namespace` instance, which `installMethods` brands) | | 648 decides what the Pants build boots of the V8 bridge; namespaces as sealed registries |

Left for the next attempt besides the table: the chunks view and the
`mapSeq` thunk over sealed inputs (acceptance 3), the engine-kind views
for symbols, metadata, atoms, Vars, multimethods, regex, structs and
records (acceptance 4), and the boot's memory below.

The boot's memory (`shared_profile_boots_with_four_mib_heap`, ignored with
the numbers): peak 53 MB of managed bytes and 50 MB live after the boot
under `Runtime::new`; 46 MB peak and 39 MB live without the seal walk; the
boot fails with `HeapLimit` under a hard limit of 32 MiB or less. The 4 MiB
budget of acceptance 4 is open until that footprint is understood (the
Pants build's port boots in about 1.5 MB).

Not done here, carried by light-cljs-666: the remaining causes above; the
lazy result of the real `map` still captures its rest-argument array; the
chunks view (`cljChunkFirst`/`cljChunkRest`) for chunked seqs; the
engine-kind views for symbols, metadata, atoms, Vars, multimethods, regex,
structs and records (their stores are private-field stores for now, so a
runtime-built value works inside a task and an engine form of the kind is
not recognized by the brand; the global hierarchy stays on the task's cells
until atoms are engine atoms); moving the port's representation code
(`src/bbp_runtime.js`) into profile units.

Also for light-cljs-666, found by the third attempt: the `seq` of a
runtime-built map throws, because the verbatim `mapEntry` sets the
prototype of the entry it builds with the swapped `makeVector`, an engine
vector, which is not extensible (`core.seq(core.hashMap(1, 2))`: "cannot
set the prototype of a form vector"); the maps view decides what a map
entry is on this profile (an engine map entry, or a runtime-owned pair
over the private fallback of `vectors`).
