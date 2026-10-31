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
  the `protocolSlots` memo; `SHARED_READER_STATE`: the reader's own state,
  declared at the top of its function) rewritten to the task's cells
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
  `hashOrderedColl` over its items, and an engine map's from the map branch
  of `hash` over its entries (light-cljs-698), cached per task
  (`formHashes` on the task's cells, as the V8 profile's WeakMap caches
  it), and any other engine form's from the engine: the engine folds a host
  object's identity
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
  `protocolVariables`, `protocolGetters`, `reifySites`, `namespaces`, the
  reader's `tags` and `builtinTags`) live on the task's
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
  through `kindHooks.isLoadable`; after boot these two are WeakMaps seeded
  from the boot's snapshot, so a reify or type a task discards is reclaimed
  (683), and only the boot task's are enumerable Maps. `ClassBrand`'s "some class is branded"
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
bounded fallback diagnostic, the weak post-boot registries (683),
`ClassBrand`'s per-task flag (684: over a copy of the runtime that exports
`defineClass` and `TypeBrand`, as the V8 profile's test does), engine
symbols, the publication of the members 666 made shareable and their use
from a task spawned separately with its own first cells, the lazy registry
getters read from such a task (688) and the boot's memory. `SHARED_EXPORTS`
names bindings `shared_profile_eval` exports from a copy of the runtime
(`rt.sharedSnapshots`).

State after light-cljs-666's second attempt (2026-10-31): the profile
boots; 738 of 748 members publish (716 after 666's first attempt, 267
after 647's third, 268 after its second, 259 after its first, 193 after
646). The causes 647
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
listed (`__rt.seal` over every export and `core` member; with
`SHARED_EVAL_FILE=<path>` the expression comes from a file). The causes
the second attempt removed (22 members): the reader's own state, the
declarations at the top of the `reader` unit's function (`streams` and
`readerMaps` as the task's WeakMaps; `tags` and `builtinTags` as per-task
registries seeded from the boot's entries; `defaultTag` and `activeDepth`
as data defaults), rewritten to the task's cells by a second rewrite list
in `build.rs` (`SHARED_READER_STATE`: the names declared in a scope
directly under the unit's top level, since `SHARED_TASK_STATE` rewrites
scope-0 declarations and free names only); the reader IIFE's argument
object and its two frozen sentinels (`absent`, `discarded`), which the
reader's functions capture and no binding reaches, sealed where they are
built (`SHARED_READER_REWRITES`); `namespaces` as a per-task registry
seeded with the boot's `cljs.user` and `cljs.core`; and `demungePattern`
as the pattern's source string, with `demunge` building its global regex
per call (a regex object is exotic data a seal cannot take, and
`__rt.formRegex` refuses the `g` flag, whose `lastIndex` would be state).
The third attempt (2026-10-31, with light-cljs-688) added the symbols
and metadata views, after which 739 of 748 members publish: `symbols` is
a view over engine symbol forms (`symbol` and the positional
`SymbolValue` build `__rt.formSymbol`), `metadataValues` reads an engine
form's metadata from the form (`__rt.formMeta`; `set` on a form throws,
since a form's metadata is given where the form is built), and
`with-meta` of a symbol (`collectionWithMeta.symbols`) builds
`__rt.formWithMeta` when the metadata is an engine map or none. The
engine's forms carry neither runtime-built metadata (a map of the
verbatim `maps`, until the maps view) nor an unqualified name holding `/`
(`(symbol nil "a/b")`; a form's spelling must split back into its pair,
the engine's `splits_back`), so `sharedSymbol` builds such a symbol as a
runtime-owned object over the views' fallbacks with the V8 profile's
prototype, which `symbols`, equality, hashing and printing read alike;
given engine metadata or none again, it is an engine symbol again. A
form has no `toString`, so `str`, `displayStr`, the printer and
`Namespace.prototype.toString` spell an engine keyword or symbol from its
payload (`sharedNamedString`). The engine's hash of a symbol is the
runtime's (`core.hash` of either agrees). With the namespaces' names
engine symbols, `currentNs`, the `Namespace` entries of the
`protocolImplementations` and `namespaces` snapshots and so every lazy
registry getter of `sharedTaskDefaults` seal (688, found by 648: a task
that read any getter was rejected through them), and the builtin tag
parsers' location object is sealed where it is built
(`SHARED_READER_REWRITES`). As with keywords, an engine symbol has no
`SymbolValue` prototype: `type` of one is `Object`, and `(.-name sym)`
and `(sym map)` are not supported on this profile. The remaining
rejections, by cause:

| Members | Cause | Example | What removes it |
| ---: | --- | --- | --- |
| 8 + 1 | the V8 form bridge (`compilerFormBridge`): its WeakMaps and the `let`s its `decode` assigns, reached from `rt.compilerForms` and, through `readerJsLiterals.array` (`compilerForms.jsArray`, which the `compilerForms` unit installs for `#js` literals read for macros), from every reader entry point (`core.read`, `core.ednReadString`, `core.readerInitialTagTable`, ...), and from the builtin `js` tag parser (`jsLiteral`), which the tag tables' boot snapshots hold, so the task defaults (`Object.getPrototypeOf(core)`), the hub and `core.sharedTaskCells` (which captures the hub) are rejected through it too; `shared_profile_registry_getters_keep_the_profile_published` boots a copy of the runtime without that one registration | `rt.compilerForms`, `core.read` | 648 decides what the Pants build boots of the V8 bridge; the options are below |

What the Pants build boots of the V8 form bridge, for light-cljs-648 (the
user decides; the options, found while closing the other causes):

1. Leave `readerJsLiterals` plain on this profile: the `compilerForms`
   unit's `jsArray`/`jsObject` installation is skipped, so a `#js` literal
   read for a macro is a plain array or object (what the V8 profile reads
   outside the compiler's descriptor mode), and `rt.compilerForms` stays
   the one rejected member, used by nothing a macro task reaches. Smallest
   change; the reader entry points, the tag tables, the defaults, the hub
   and `sharedTaskCells` publish at once.
2. Swap the `compilerFormBridge` unit for an engine-forms bridge: `decode`
   returns the engine form itself (the Pants build's macro arguments are
   engine forms already), so the unit keeps no WeakMaps and no `let`s, and
   `compilerForms` publishes as well. Right by construction for this
   profile, but it duplicates what 648's boot does on the compiler side.
3. Keep the bridge and make its registries per-task cells
   (`SHARED_TASK_STATE`), as the dispatch registries are. Keeps the V8
   profile's descriptor mode alive on this profile, which nothing needs;
   most code for the least value.

Recommendation: 1 now (a two-line assembly rewrite, no new unit), with 2
if 648 finds a macro that reads `rt.compilerForms` through the hub.
Taken by light-cljs-666 as a `compilerForms` swap that reads a `#js`
literal as the port's engine compiler literal ("light-cljs-666: imported
forms, publication and the builders" below); the table's last row is
closed and every member publishes.

The fourth attempt (2026-10-31, after the review fixes light-cljs-691 to
693) added the maps view, the first half of it: `maps` recognizes an
engine map form and reads it as an array map of its entries
(`sharedMapEntries`: a frozen `{key, value, hash}` array in the engine's
print order, `formPrintItems`, built once per task and map on the task's
cells, `formEntries`), and `mapKinds` answers `array-map` for a form, so
the verbatim units take their positional paths (never a hash trie) for
`map?`, `count`, `get`, `contains?`, `find`, `keys`, `vals`, `seq`,
printing, equality and hashing with a runtime-built map of the same
entries, `reduce-kv`, `into` and the updates, and engine metadata (a
symbol's, a vector's) is readable throughout
(`shared_profile_engine_maps_are_maps`). An engine map hashes as the
runtime hashes it, through the map branch of `hash` over its entries,
cached per task with the sequences' hashes (`sharedFormHashes`): the
engine's `formHash` folds a host object's identity, so a map holding a
Date, a symbol or a list, equal to a runtime-built map, hashed apart from
it and missed it as a hash-map key (light-cljs-698, from the review of
this attempt). The same review found that `preserveMeta` returned an
operation's engine result as it was when the input had no metadata: the
input may be the wrapper `with-meta` of nil builds over an engine vector
that carries metadata, so `conj`, `pop` and `assoc` of the bare wrapper
brought the removed metadata back; a result now gets the input's nil
metadata as well (light-cljs-699). Runtime-owned maps (sorted maps,
records, and every map the verbatim builders still make: an update of an
engine map is a runtime-owned array map) keep the V8 profile's kinded
store, a property of the view so the boot seals it and its kinds; a
`kindedStore()` in a profile-only unit would run before the runtime's
`weakMapStores` is initialized. A map entry is a runtime-owned pair over
the vectors' fallback with the vectors' prototype, as the V8 profile's
lean entry (`mapEntry.lean`) is, which fixes the `seq` of a runtime-built
map below. The engine's `arguments` object gained its `Symbol.iterator`
(`Array.prototype.values`, as CreateUnmappedArgumentsObject gives it):
`conj` of a map entry onto a map reaches `indexedNth` through protocol
dispatch, whose `nth(...arguments)` threw. The second half, left: swap the
builders (`makeMap`, `mapFromEntries`, `hashTrie`, `updatedMap`,
`kindHooks.updatedRecord`, the transient paths, `empty`) so an update of
an engine map is an engine map (`formAssoc`, `formDissoc`, `formArrayMap`
for the array-map kind, `formMap` for the hash-map kind) and the hot
reads (`lookup`, `kindTables[5][6]`, `mapCount`, `kindTables[5][4]`)
read `formGet` and `formCount` without building the entries; the kinded
payload's `Array.isArray` tests and `hashRoots` then matter only to the
runtime-owned kinds. Two cautions found: engine key comparison
(`formGet`) hashes a runtime-owned key (a set, a sorted map, a symbol of
`sharedSymbol`'s fallback) by identity, as noted for `formHashes`, so the
swapped lookups must keep the verbatim path for such keys or convert
them; and `formMap` is a hash map, whose entries print in trie order
(`{:b 2, :a 1}` for `(formMap :a 1 :b 2)`), while a runtime-built array
map keeps insertion order, so a literal that must print in source order
needs `formArrayMap`.

Left for the next attempt besides the table: the second half of the maps
view above, the chunks view and the `mapSeq` thunk over sealed inputs
(acceptance 3), the engine-kind views for atoms, Vars, multimethods,
regex, structs and records (acceptance 4), and the boot's memory below.

The boot's memory (`shared_profile_boots_with_four_mib_heap`, ignored with
the numbers): peak 53 MB of managed bytes and 50 MB live after the boot
under `Runtime::new`; 46 MB peak and 39 MB live without the seal walk; the
boot fails with `HeapLimit` under a hard limit of 32 MiB or less.
Re-measured after the third attempt under a 256 MiB hard limit (the
engine accounts bytes only under a limit): 69 MB peak with 10
collections during the boot, and 5.4 MB live after a collection at the
end of it, so the live profile is near the budget and the peak is the
boot's garbage (the memos, snapshots and seal walk allocate and drop),
which a limit of 4 MiB cannot absorb between collections. The 4 MiB
budget of acceptance 4 is open until that garbage is understood (the
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

Also for light-cljs-666, found by the third attempt and fixed by the
fourth: the `seq` of a runtime-built map threw, because the verbatim
`mapEntry` set the prototype of the entry it builds with the swapped
`makeVector`, an engine vector, which is not extensible
(`core.seq(core.hashMap(1, 2))`: "cannot set the prototype of a form
vector"); a map entry is now a runtime-owned pair over the private
fallback of `vectors` (an engine map entry form, `formNth` of
`formPrintItems`, is an option once entries need to publish).

## Phase 1 results (light-cljs-648)

`--macro-core port|real` (and `:compiler-options {:macro-core :port|:real}`
in light-cljs.edn) selects the Pants build's cljs.core. It is accepted on the
Pants build only (`MacroCore::check_available`: built with
`--no-default-features --features bbp-macros`, no V8 or QuickJS), defaults to
`port`, adds `macro-core-real` to the compiler identity when `real` (the
port's identity is unchanged), and `--stats` prints `native.core.real` (0 or 1).
`LIGHT_CLJS_MACRO_CORE=real` makes `real` the default where no flag or build
file chooses, which is how a whole test binary runs in either mode.

### How `real` runs

- **Boot once, publish what passes** (`src/bbp_evaluator/real_core.rs`).
  `BbpRuntime::new` runs `SHARED_RUNTIME` in a boot task, then offers every
  member of its `core` hub, and every property of the per-task defaults
  under it, to `Runtime::publish` one by one. The ones that publish go on
  the published hub: a sealed object over a sealed copy of the defaults that
  published. Each macro task gets `[hub, boot, unpublished names, private
  boots]` as `nativeMacroTask`'s seventh parameter.
- **A task's cells over the hub** (`src/bbp_runtime.js`, the `real` block
  before the export object). The task makes `globalThis.__lightSharedCells`
  over the hub, with the hub's cell defaults as its own writable properties
  (`sharedTaskCells`'s contract, made from the published hub, since the
  profile's own `sharedTaskCells` captures the unpublished hub). A member
  that did not publish is a getter there: the first read boots a private
  copy of the profile in that task (the boot function captures nothing, so
  it publishes with the hub), takes the member from it and counts the boot
  in `native.core.private-boots`. A per-task default whose getter did not
  publish is resolved the same way, with the task's cells as receiver.
- **Bindings.** `rt.nativeCoreBinding(name)` answers the hub's member as a
  value (so a helper that captures it can still publish), a getter for an
  unpublished member, and the port's descriptor for a name the real hub does
  not have. `rt.core` and `globalThis.__lightNativeRuntime.core` are the
  task's cells themselves, with the port's own names the real core lacks
  (`invokeMulti`, `invokeVar` and the other call adapters emitted code
  reaches, the evaluator's members) added to them. So the emitted code's
  writes to a core cell (a `binding` of `*print-readably*`, `*print-length*`,
  `with-out-str`'s `*print-fn*`) and their restoration in its `finally`
  land on the cells the real functions read; a first version made `rt.core`
  a child of the cells, where those writes shadowed the cells and the real
  printer never saw them
  (`real_core_dynamic_bindings_reach_runtime_functions`). The port's data
  cells of the same names forward to the task's cells, so the port's
  functions that still run (the adapters' printing) see the same bindings.
  `globalThis.__lightNativeRuntime.port` is always the port's table, and
  the port's runtime reaches its own adapters through it, so its Var,
  multimethod and struct calls are unchanged in both modes.
- **`*ns*` stays the evaluator's.** `currentNs` is not offered for
  publication (`evaluator_member`), and a task's cells take the port's
  `currentNs` accessor, which answers from the compiler's namespace frames
  and which the port's reader context and namespace machinery bind. A
  first version published the profile's boot value, so `*ns*` in a macro
  called from `app` read `cljs.user`
  (`real_core_preserves_macro_namespace_context`).
- **What stays the evaluator's** (`evaluator_member`). The members the
  manifest marks `compile-time` (the namespace registry, `create-ns`,
  `find-ns`, the macro reader context, the form bridge, declaration demand),
  `gensym`, whose names on the Pants build come from the compiler's
  counters and gensym scopes and are logged for the cache, and `currentNs`
  (above) resolve to the port's in `real` mode too. The command loop, root registry, catalog and
  `decl` are unchanged.

### Numbers

On master 91d9e540 with the review fixes, 697 of the 725 offered members
publish (`macro_core_boot_and_task_bytes`): `currentNs` is the
evaluator's now (above) and no longer offered, and the 28 listed below
after `currentNs` remain per task. The test runs and the differences below
are from the same tree.

Members on master 20ce6bfb (the profile at 738 of its 748 members): the
boot offers 726 members (the hub's members and the per-task defaults,
without the evaluator's), of which 697 publish and 29 take the per-task
fallback (`macro_core_boot_and_task_bytes` lists them): `currentNs`; the
reader's eight entry points (`readString`, `read`, `readerReadString`,
`readerRead`, `ednRead`, `ednReadString`, `readerParseTimestampFields`,
`readerInitialTagTable`), which reach the V8 form bridge's
`readerJsLiterals` (below); and all 20 per-task registry getters of the
defaults (`coreDispatchers`, `gensymScopes`, `protocolSlots`,
`formHashes`, the reader's `namespaces`, `streams`, `tags` and the rest),
which reach an unsealed object in the boot's `coreDispatchers` snapshot
through `sharedRegistry` (`BBP_PUBLISH_TRAIL=1`: `get coreDispatchers >
sharedRegistry > sharedSnapshots > array > array > object > object`),
reported to light-cljs-666. Until that snapshot seals, a task that reads
one of those registries boots its private copy once; the generated 200k
project booted one (`native.core.private-boots 1`, on the base before 20ce6bfb).

### The V8 form bridge in `real` mode

The Pants build does not boot the V8 form bridge as part of the shared
core. Its forms cross between the compiler and the engine through the Rust
bridge of `src/bbp_evaluator.rs` (memoized native-form import, unchanged
subtree identity, the persistent `&env`), which is what the port's
`compilerForms` serves, and which the design keeps (section 1, "Why not
freeze on publication"). So in `real` mode:

- `compilerForms` (the runtime's export, `version: 5` `decode`/`encode` over
  JSON-like envelopes for V8 and QuickJS) is not offered for publication at
  all; the evaluator runtime's own `compilerForms` (gensym scopes,
  namespace context, `takeGensyms`) stays the one the command loop calls,
  as the manifest's `compile-time` rule already keeps every other piece of
  evaluator machinery on the port.
- The reader entry points that reach the bridge's `readerJsLiterals.array`
  (`#js [...]` in data read at macro time) stay per task: a task that calls
  `read-string` or `cljs.reader/read-string` in a macro boots its private
  copy and reads there. Making them shared is not a matter of sealing the
  bridge: the real reader builds runtime-owned lists, symbols and
  `#js` values, not engine forms, so its results would not cross the
  Rust bridge either. The fix is the profile's reader building engine
  forms, as the port's reader does today by adapting the same canonical
  `reader` unit to the engine's constructors at assembly
  (`native_reader_source` in `src/bbp_evaluator.rs`); that belongs with the
  symbol and form views of light-cljs-666, and is recorded here rather than
  done in 648, which only resolves names.

Boot cost, before (`port`) and after (`real`), from
`bbp_evaluator::tests::macro_core_boot_and_task_bytes` (debug build,
`--ignored --nocapture`) and the generated 200k project at 1 thread
(release build of this work on master 0338139d, before the rebase onto 20ce6bfb; `release bench --stats --jobs 1`, three
alternating runs on a machine at load 26 to 40, so the times are noisy):

| | port | real |
| --- | ---: | ---: |
| Runtime boot (debug) | 416 ms; 466 ms on 91d9e540 | 2,672 ms (of which the profile's boot and publication); 2,637 ms on 91d9e540 |
| Runtime live bytes after a collection (debug) | 1.07 MB | 18.1 MB; 20.1 MB on 91d9e540 |
| Runtime peak managed bytes during boot (debug, 512 MiB limit) | 1.10 MB | 72.0 MB; 105.3 MB on 91d9e540 |
| Bytes a task holds after its boot (8 tasks, debug) | 1.58 MB | 1.45 MB; 1.41 MB on 91d9e540 |
| 200k: `native.core.boot-ms` | | 307 to 329 ms |
| 200k: `native.producers.setup-ms` (57 producers) | 13.7, 22.2, 103.1 | 20.2, 24.8 |
| 200k: `macro.gc.peak-managed-bytes` | 164.5 MB | 214.2 MB |
| 200k: peak memory (`memory.peak-rss-bytes`) | 1.15 to 1.18 GB | 1.18 to 1.19 GB |
| 200k: wall | 15.5 to 31.5 s | 21.5 to 24.5 s |
| 200k: output | `1a75bc87…` | identical |

A task still runs the port's factory in `real` mode (its command loop,
registry, catalog, `decl` and adapters, which phase 3 slims to the evaluator
runtime), so the per-task bytes do not fall yet: the real core adds one
boot of about 0.3 s and about 50 MB of managed bytes at its peak per
compilation (18 MB live after a collection, 20 MB on 91d9e540), which
the aggregate `--macro-memory-limit` must hold. Under the default 64 MiB
the boot fits, with collections; a limit of 32 MiB or less cannot hold it
(the profile's own measurement above), and a build that runs several
tasks at once under the default runs out in a task ("heap byte limit
exceeded" below).

### Differences between the modes

`cargo test --features bbp-macros --test bbp_macros` runs whole in either
mode (`LIGHT_CLJS_MACRO_CORE=real`). On master 91d9e540 plus this work
(debug build, `cargo nextest run --no-default-features --features
bbp-macros --test bbp_macros -j 4`, a loaded machine): with `port` all 335
pass (9 ignored); with `real` 181 pass and 154 fail (the first attempt,
on 20ce6bfb: 165 and 160). The Pants build's whole suite (`cargo nextest
run --no-default-features --features bbp-macros -j 8`, 2,234 tests) has
142 failures with `port`, the baseline, and 473 with `real` (472 failed, 1
timed out; 508 on the first attempt), led by the same causes as the list
below: `cannot convert object to primitive value` in about 110, changed
answers in about 65, then the collection views on engine values (`count`,
`seq`, `assoc`, `nth` on engine maps and structs, `WrongType`), the
memory limit (17), interop with engine forms and protocol extension of a
hub function.

No failure is a missing name ("unsupported operation: native macro runtime
core.X" no longer occurs). Every failing test of the `real` run follows,
under its cause, with the first engine error of its log (or the kind of
assertion that failed) and the number of tests with that error. The list is
regenerated by running the binary with `LIGHT_CLJS_MACRO_CORE=real` and
grouping the first error of each failure; a test's full message is in the
run's log.

26 tests. **A real function strings or compares an imported form** (`cannot convert object to primitive value`). A real core function that converts its argument to a string or number (`str`, `name`, `pr-str`, `partition`, `=` over a macro argument or a name the compiler supplied) gets a form the Rust bridge imported, which the profile's views do not recognize yet (`(str (ns-name *ns*))` in a macro called from a namespace the compiler named fails the same way). Owner: the view of imported forms, light-cljs-666.

- `TypeError: cannot convert object to primitive value` (25): `native_imported_queue_seq_protocols_match_quickjs`, `build_config::a_custom_data_reader_reads_configuration_and_declares_views`, `build_config::config_answers_a_set_key_an_unset_key_and_a_collection_value`, `build_config::configuration_preserves_floating_negative_zero`, `compiler_literal_first_entry_helper_matches_quickjs`, `compiler_literal_operations_match_quickjs_at_every_pool_size`, `explicit_compiler_literals_match_quickjs`, `explicit_compiler_readers_keep_definition_and_read_gensyms_apart_from_macros`, `function_combinators_match_the_legacy_compiler_and_run`, `lazy_sequence_macros_match_the_legacy_compiler_and_run`, `native_delay_values_match_quickjs`, `native_explicit_nil_metadata_keeps_following_reify_ids`, `native_generated_scalar_mappings_match_legacy`, `native_hash_collection_emission_matches_legacy`, `native_mutable_deftype_published_helper_matches_quickjs_at_all_threads`, `native_namespace_context_matches_quickjs_across_helpers_and_jobs`, `native_namespace_registry_preserves_utf16_names`, `native_printing_matches_quickjs_for_every_native_value_kind`, `native_protocol_value_kind_table_matches_quickjs_at_all_threads`, `native_transducer_call_shapes_match_legacy`, `native_transducer_helpers_publish_and_resume_without_replay`, `native_verbatim_cuerdas_unsafe_concat_pipeline`, `producer_catalog_work_does_not_grow_with_the_catalog`, `seq_preserves_list_identity_and_existing_element_occurrences`, `sequence_predicate_table_matches_quickjs_at_all_jobs`
- `Error: mixed: cannot convert object to primitive value` (1): `flatten_sequence_tree_table_matches_quickjs_at_all_jobs`

17 tests. **The real core does not fit the macro memory limit** (`heap byte limit exceeded`). The real core holds about 20 MB live after its boot, and 105 MB at the boot's peak without a limit (`macro_core_boot_and_task_bytes`); two tests set 16 MiB or less and fail in the boot, and the others run several tasks or pool slots under the default 64 MiB and run out in a task. Owner: the boot's footprint, light-cljs-666 acceptance 4.

- `heap byte limit exceeded` (17): `an_earlier_unit_demanding_a_parked_initializer_lends_it_its_lane`, `demanded_helper_bindings_keep_once_only_effects_at_one_two_four_and_eight_slots`, `effect_lanes_keep_one_slot_atom_effects_under_seeded_scheduling_stress`, `indirect_initializers_use_the_earlier_callers_bindings_under_seeded_scheduling`, `inherited_initializers_use_the_earlier_callers_bindings_under_seeded_scheduling`, `keep_helpers_publish_and_resume_at_seeded_pool_sizes`, `native_atom_watchers_follow_the_shared_atom_across_tasks`, `native_definition_groups_publish_all_products_without_replaying_effects`, `native_integer_predicates_work_as_shared_atom_validators`, `native_iteration_views_publish_without_replaying_their_initializer`, `native_namespace_completed_lazy_frontier_matches_reference`, `native_namespace_frontier_counts_completed_lazy_realizations`, `native_namespace_registration_frontier_matches_reference`, `native_profile_stats_cover_engine_bridge_and_helpers`, `published_once_cells_keep_source_order_bindings`, `the_macro_memory_limit_denies_allocation_catchably_and_the_build_recovers`, `wide_js_obj_bootstrap_matches_quickjs_at_every_pool_size`

20 tests. **A macro returns a runtime-owned value** (`Unsupported macro result arbitrary object` or `function`, at the path shown). The real core built the result as a runtime object (a list of `listFromArray`, a hash map, a sorted map or set, a deftype instance, a queue), which the Rust bridge cannot export to the compiler. Owner: light-cljs-666's views (engine forms for those values); queues, phase 4.

- `Error: Unsupported macro result arbitrary object (including promise) at $[1][0]` (5): `native_compile_time_code_reader_uses_macro_visibility`, `native_find_preserves_comparator_equal_stored_keys`, `native_map_selection_and_updates_keep_metadata_and_missing_values`, `native_nested_updates_and_key_projections_keep_callback_contracts`, `native_sorted_set_by_stays_an_explicit_boundary`
- `Error: Unsupported macro result arbitrary object (including promise) at $` (3): `native_macro_result_errors_identify_the_value_and_container_path`, `native_mutable_deftype_member_store_preserves_operand_order`, `unsupported_operations_are_explicit_and_do_not_fall_back`
- `Error: Unsupported macro result arbitrary object (including promise) at $[1][4]` (2): `native_compile_time_empty_primitive_sequences_and_scalar_predicates`, `native_name_munging_nested_association_and_merge_with_match_core`
- `Error: Unsupported macro result arbitrary object (including promise) at $[1][1]` (1): `native_mutable_deftype_shared_stores_reject_host_objects_catchably`
- `Error: Unsupported macro result arbitrary object (including promise) at $[1][2]` (1): `native_struct_call_fields_override_ifn_in_callbacks`
- `Error: Unsupported macro result arbitrary object (including promise) at $[1][16]` (1): `native_struct_host_interop_matches_quickjs_at_every_pool_size`
- `Error: Unsupported macro result arbitrary object (including promise) at $[0]` (1): `native_helpers_ignore_public_core_rebindings`
- `Error: Unsupported macro result arbitrary object (including promise) at $[1][12]` (1): `native_ifn_object_member_precedence_matches_legacy`
- `Error: Unsupported macro result arbitrary object (including promise) at $[1]` (1): `native_core_bindings_keep_unsupported_getters_at_the_original_read`
- `Error: Unsupported macro result arbitrary object (including promise) at $[1][1][6]` (1): `fnext_and_nfirst_match_quickjs_at_all_threads`
- `Error: Unsupported macro result function at $` (1): `memoize_rejects_task_owned_captures_keys_and_results`
- `Error: Unsupported macro result arbitrary object (including promise) at $[4][2]` (1): `native_transformed_sequences_honor_host_iterable_extensions`
- `Error: Unsupported macro result arbitrary object (including promise) at $[1][4][0][1].value[0]` (1): `memoize_supports_first_class_construction_and_independent_instances`

36 tests. **The program's answer differs.** A real function answers for an engine value or a port-built value through its generic path: printing engine maps, sets and records; `satisfies?`, `instance?` and the reference-type predicates; Vars, multimethods, regexes and delays the port built; hash order; `with-redefs` of a core function; a host property read as a protocol. The assertion's left and right are in the run's log. Owner: light-cljs-666's views; dispatch over engine tables, light-cljs-649.

- `changed answer (assertion left == right)` (32): `build_resources::service_arguments_with_lone_surrogates_are_refused_in_both_engines`, `compiler_literal_array_and_object_predicates_match_quickjs`, `compiler_literal_sequential_classification_does_not_realize_values`, `delay_reentry_and_constructors_publish_at_all_threads`, `delays_publish_and_run_once_at_all_threads`, `demanded_compiler_literals_publish_and_keep_source_order_at_all_jobs`, `demanded_multimethods_reject_task_owned_dispatch_without_replaying_effects`, `memoize_preserves_apply_receiver_semantics`, `memoize_supports_first_class_core_callbacks`, `memoize_uses_argument_equality_and_caches_falsey_results`, `native_atom_metadata_and_throwing_watch_preserve_committed_state`, `native_compile_time_exception_uuid_and_array_helpers_match_application_values`, `native_compiler_literal_protocols_match_quickjs`, `native_core_bindings_preserve_with_redefs_and_restoration`, `native_core_replacements_reanalyze_helpers_from_earlier_artifacts`, `native_core_writes_through_javascript_holes_observe_replacements`, `native_js_call_and_apply_read_struct_fields`, `native_nested_sorted_map_by_equality_matches_quickjs`, `native_object_operation_table_matches_quickjs`, `native_protocol_extensions_and_saved_protocols_match_quickjs`, `native_protocol_predicates_read_host_properties_like_quickjs`, `native_protocol_satisfies_demands_before_mask_short_circuit`, `native_regex_constructor_calls_keep_macro_argument_identity`, `native_regex_property_mutations`, `native_resolve_reflects_a_declared_var`, `native_scalar_predicates_and_binding_probes_match_legacy`, `native_shared_sequence_and_map_protocols_match_quickjs`, `native_sorted_map_by_equality_direction_matches_quickjs`, `native_struct_core_protocol_operation_matrix_matches_legacy`, `nested_helper_macros_keep_generations_and_execute_effects_once`, `protocol_values_publish_and_match_quickjs_at_all_threads`, `regex_helpers_and_parse_thunks_publish_at_seeded_pool_sizes`
- `assertion failed: std::ptr::eq(result.data().kind(), original.data().kind())` (1): `extracted_native_metadata_reuses_the_original_datum_and_occurrence`
- `QuickJS reference: [[[false false true false false false false false 1 true] [false false false 1]] [false fal` (1): `native_protocol_redeclaration_identity_matches_quickjs_at_all_threads`
- `called 'Result::unwrap_err()' on an 'Ok' value: Program { namespaces: ["cljs.user"], reader_contexts: Layered` (1): `atoms_still_reject_host_regexps`
- `missing "map set-field => \"error: cannot add property 'x': the object is not extensible\"" in` (1): `native_form_object_operations_match_quickjs`

9 tests. **The published hub is immutable, or a runtime type is not the port's.** Writes to properties of the sealed, published hub functions (`cannot assign to read-only property`, `cannot set property 'call'`), protocol extension of a hub function, `instance?` against a runtime type the port's value is not (`invalid 'instanceof' right operand`), a number's prototype in the real IFn path. Owner: light-cljs-649 (engine tables for dispatch and extension) and 666.

- `TypeError: invalid 'instanceof' right operand` (2): `native_mutable_deftype_closure_in_atom_matches_quickjs`, `native_mutable_deftype_publishes_current_fields_and_user_protocols`
- `TypeError: cannot read property 'get' of undefined` (1): `native_regexp_adapters_preserve_callee_before_arguments`
- `TypeError: cannot assign to read-only property 'isEven'` (1): `generated_core_writes_in_demanded_values_match_quickjs_at_every_pool_size`
- `TypeError: cannot set property 'call' of a published function: functions shared between tasks are immutable` (1): `native_core_property_writes_preserve_call_overrides`
- `UnsupportedOperand: Object.getPrototypeOf of a number needs a wrapper object, which is not supported (in ifnMe` (1): `native_core_protocol_predicate_matrix_matches_quickjs`
- `TypeError: cannot assign to read-only property 'inc'` (1): `native_core_reads_observe_replacements_in_demanded_helpers`
- `TypeError: undefined is not a function` (1): `compiler_literal_string_slices_and_runtime_literals_match_quickjs`
- `Error: cljs.user:58:12: extension: unsupported operation: native protocol extension of [object Function]` (1): `native_member_precedence_table_matches_quickjs`

3 tests. **A port-built multimethod has no method for the real core's dispatch.** `defmulti` and `defmethod` lower to the port's MultiFn; the real core's dispatch reads its own registry. Owner: light-cljs-649.

- `Error: No method in multimethod 'helpers/m' for dispatch value: x` (1): `demanded_multimethod_dispatch_shapes_match_quickjs_at_every_pool_size`
- `Error: No method in multimethod 'helpers/m' for dispatch value: :x` (1): `demanded_multimethod_effects_run_once_across_suspension_and_lending`
- `Error: No method in multimethod 'helpers/mf' for dispatch value: :still-callable` (1): `native_multifn_host_coercions_match_legacy`

2 tests. **A private copy of the profile booted inside a macro's deadline** (`deadline exceeded`, debug build, loaded machine), for a member that did not publish. Owner: publication of every member, light-cljs-666 (below).

- `deadline exceeded` (2): `native_raw_call_apply_matrix_matches_quickjs`, `native_raw_callable_collections_match_quickjs`

2 tests. **A shared lazy sequence's value cannot publish** (`a shared lazy sequence realized a value it cannot share`): the real `map` captures its rest-argument array, or a regex state is task-owned. Owner: light-cljs-666.

- `TypeError: a shared lazy sequence realized a value it cannot share: cannot publish: value.rest.thunk.capture(1` (1): `for_and_doseq_over_lazy_helpers_match_the_legacy_compiler_at_one_two_and_four_slots`
- `TypeError: a shared lazy sequence realized a value it cannot share: cannot publish: value[0] is a task-owned o` (1): `published_regex_state_matches_quickjs`

1 tests. **The engine lacks `DataView`**, which the real `hash` of a double uses. Owner: the engine's intrinsics, light-cljs-666's list.

- `ReferenceError: DataView is not defined` (1): `native_public_hash_helpers_preserve_numeric_and_unordered_contracts`

2 tests. **Downstream of a failure above in a dependency or a helper's artifact** (a dependency cell failed with one of the errors above; a macro-provider unit's artifact).

- `Error: dependency world#0kind#3 failed with String(` (1): `native_compiler_macro_fixtures_keep_their_legacy_results`
- `native macro-provider artifact: "a unit declared a macro"` (1): `native_installed_macros_expand_in_demanded_helpers`

36 tests. **A collection operation on an engine value takes the generic path.** `seq`, `count`, `conj`, `assoc`, `nth`, `pop`, `rseq`, `name`, `deref`, `realized?`, `reduce-kv`, `IWriter` and the iterator protocols on engine maps, sets, structs, reduced values, symbols and Vars, which the maps, sets and kind views do not answer yet. Owner: light-cljs-666's views, 649.

- `Collection(WrongType { expected:` (5): `memoize_shares_results_and_once_only_effects_across_tasks_and_cold_helpers`, `native_empty_sorted_maps_do_not_call_the_comparator`, `native_ifn_apply_uses_direct_invoke_entries`, `native_instance_ifn_apply_keeps_captures_and_direct_receivers`, `native_nil_metadata_preserves_collection_identity`
- `Error: [object Object] is not ISeqable` (4): `native_custom_reducers_do_not_require_a_sequence_implementation`, `native_predicate_apply_observes_count_before_traversal`, `nthrest_identity_realization_and_callbacks_match_quickjs_at_all_jobs`, `protocol_type_and_record_macros_match_the_legacy_compiler_and_run`
- `TypeError: unsupported operation: native macro runtime conj on reduced` (3): `native_core_services_match_quickjs`, `native_iteration_reduction_stops_before_requesting_a_continuation_key`, `rumext_property_filters_resume_reduce_kv_callbacks`
- `TypeError: unsupported operation: native macro runtime count on object` (3): `native_reduce_kv_matches_quickjs`, `native_var_identity_apply_and_metadata_match_quickjs`, `native_zipmap_matches_legacy_collection_and_realization_semantics`
- `Error: Cannot create iterator from [object Object]` (2): `native_transformed_sequences_keep_source_and_mapper_call_member_order`, `native_transformed_sequences_preserve_seqability_and_deferred_methods`
- `Error: Doesn't support name on this type` (2): `compiler_literal_string_keys_keep_exact_names`, `native_interned_keyword_mappings_match_legacy`
- `TypeError: unsupported operation: native macro runtime seq on object` (2): `multifns_sharing_constructor_tables_observe_each_others_mutations`, `native_positional_literal_matrix_matches_legacy`
- `Error: (:w :w :w) is not ISeqable` (1): `native_reference_typed_sequences_export_like_quickjs`
- `TypeError: unsupported operation: native macro runtime assoc on object` (1): `sorted_map_by_helpers_publish_and_resume_at_seeded_pool_sizes`
- `Error: Index out of bounds` (1): `native_map_entry_calls_match_legacy`
- `Error: No protocol method IWriter/-write defined for this value` (1): `native_reify_keeps_captures_metadata_and_instance_protocol_methods`
- `Error: assoc not supported on this type` (1): `compiler_literal_property_updates_keep_callbacks_and_metadata`
- `Error: No protocol method IStack.-pop defined for this value` (1): `native_pop_matches_quickjs_at_every_pool_size`
- `Error: No protocol method IReversible.-rseq defined for this value` (1): `native_rseq_protocol_dispatch_matches_quickjs`
- `Error: conj not supported on this type` (1): `collection_core_lowering_matches_the_legacy_compiler_and_runs`
- `Error: count not supported on this type` (1): `native_array_helpers_preserve_nested_writes_holes_and_checked_failures`
- `Error: value does not support deref` (1): `native_var_reflection_and_namespace_views_match_quickjs_at_every_pool_size`
- `Error: light.build/depend!: expected {:resource path}, {:namespace sym} or {:provider token :fingerprint strin` (1): `build_resources::depend_declares_resources_namespaces_and_providers_without_reading`
- `Error: no conversion to symbol` (1): `native_var_symbol_conversion_and_raw_apply_match_quickjs`
- `TypeError: unsupported operation: native macro runtime realized? on object` (1): `native_repeat_realized_and_native_drop_match_quickjs`
- `Error: keyword expects strings or keywords and an optional nil namespace` (1): `native_dynamic_constructors_match_legacy`
- `Error: {3 [:z], 2 [:w], 1 [:x :y]} is not ISeqable` (1): `native_sorted_map_by_matches_quickjs`

The regressions this pellet adds, failing before it:
`real_core::real_core_resolves_names_the_port_lacks` (the port's
`core.reductions` getter throws; the real hub answers),
`real_core::real_core_expands_as_the_port_where_both_answer`,
`real_core::real_core_stats_report_the_boot_and_publication`,
`real_core::real_core_dynamic_bindings_reach_runtime_functions` (a
`binding` of `*print-readably*` or `*print-length*`, `with-out-str`, and the
restoration after a throw, against the real printer: "No *print-fn* fn set"
before the fix) and `real_core::real_core_preserves_macro_namespace_context`
(`*ns*` in a macro, in a helper initialized on demand and after a `binding`
of `*ns*`: `cljs.user` before the fix) (tests/bbp_macros/real_core.rs),
`commands::macro_core_selects_the_build_runtime` (tests/config_file: a
build file's `:macro-core :real` reaches a release build whose macro calls
`reductions`, and `--macro-core port` after the command selects the port),
`cache_key::tests::the_macro_core_changes_the_compiler_identity`, and
`native_core_macros::tests::bootstrap_definitions_expand_on_the_real_core`:
the bootstrap definitions of `doseq`, `when` and `lazy-cat` run on the real
core and expand as the Rust expansion does, where the port lacked what they
use. That test selects the Pants build's engine itself, so it checks the
real core in every feature combination that has it, not V8 or QuickJS.

### Left to light-cljs-666

The review of the first attempt found two parts of this pellet's acceptance
that need the profile's publication work, which light-cljs-666 owns; 648
closes without them:

- **`for` on the real core.** `for`'s bootstrap definition does not expand
  yet (`bootstrap_for_expands_on_the_real_core`, ignored with that reason):
  it partitions its binding vector, an imported form, with the real
  `partition`, which fails on it with "cannot convert object to primitive
  value" (`(count (partition 2 xs))` over a macro argument `[1 2]` does,
  over a vector the macro built it answers). The fix is the view of
  imported forms, the first cause above.
- **Publish instead of booting private copies.** The 28 members and
  per-task defaults that do not publish (the reader's entry points and the
  per-task registry getters, "Numbers" above) are still taken from a
  private copy of the profile that the reading task boots, counted in
  `native.core.private-boots`; the once-per-compilation contract holds only
  once they publish, and then the fallback and its counter go.

For light-cljs-650: `bb benchmarks/pants-core-coverage.bb --real-core`
runs the Pants CLI with `--macro-core real` and only checks (the doc's table
records the port), so the inventory can be compared between the modes
before its `real` column exists.

### light-cljs-666: imported forms, publication and the builders

The fifth attempt (2026-10-31, on master e2494875 with 648's
`--macro-core` flag) was driven by the real-mode failures rather than by
the publication walk alone: `LIGHT_CLJS_MACRO_CORE=real cargo nextest run
--no-default-features --features bbp-macros --test bbp_macros` failed 155
of 335 on the base (the doc's 154 plus one timeout; `-j 4`, a loaded
machine), and the counts below are after each commit.

**The view of imported forms** (the largest cause, `cannot convert object
to primitive value`). The Rust bridge imports every node of a macro's
arguments as an occurrence (docs/native-macros.md: the identity by which
an unchanged subtree exports as its original Datum), which the engine
treats as an object: property reads see through it, `typeof`, `===`,
arithmetic and `ToPrimitive` do not, and `formCount`, `formNth` and the
other form operations reject it. The port's lowering copes with
`__rt.formValue` at every adapter; the real core is the V8 profile's
JavaScript (`x + 1`, `typeof x === "number"`, `n === count(p)` in
`partition`) and cannot. Two changes, by what the value is:

- A scalar (nil, a boolean, a number, a string, a character) imports as
  the plain primitive in `real` mode (`Bridge::plain_scalars`,
  src/bbp_evaluator.rs): the V8 build's bridge gives the same, and the
  export already maps a scalar back to the argument's exact Datum by value
  (`scalar_origins`), so a ratio or an integer the macro returns unchanged
  still exports exactly. The port keeps every wrapper.
- A collection, symbol or keyword keeps its occurrence, and the profile's
  views read through it: `sharedFormKind` answers the kind of the form
  behind an occurrence and `sharedFormOf` the form, used by `sharedView`
  (`keywords`, `symbols`, `vectors`, `cells`, `lazyValues`, `delays`),
  `maps`, `sets`, `metadataValues`, `constructorHashes`,
  `sharedNamedString`, `preserveMeta`, `sharedSymbol` and `cons`. The
  occurrence stays where the value is kept (an item, a tail, a value
  returned as it is), so identity export still finds it.

With that, `for`'s bootstrap definition expands on the real core
(`bootstrap_for_expands_on_the_real_core`, no longer ignored) and
`real_core_reads_imported_forms_through_their_occurrences` runs `partition`,
`str`, `name`, `=`, `inc`, `nth`, `get`, `keys`, `assoc`, `meta`, `pr-str`,
`into` and `frequencies` over imported arguments with the port's answers.

**Publication.** The reader entry points and every registry getter failed
to publish through one path: the `compilerForms` unit installs the V8 form
bridge's `jsArray`/`jsObject` (a WeakMap-branded array or object) into
`readerJsLiterals`, which the builtin `js` tag parser holds, which the tag
tables' boot snapshots hold, which the task defaults hold. The profile now
swaps `compilerForms`: the bridge's `decode`/`encode` stay the evaluator's
member (not offered in `real` mode), and a `#js` literal the reader reads
is an engine compiler literal, as the Pants build's reader builds one
(option 1 of "What the Pants build boots of the V8 form bridge" above, with
the port's representation rather than a plain array). Result: 726 of 726
offered members publish (`macro_core_boot_and_task_bytes`), the per-task
fallback is never taken and `native.core.private-boots` is 0; the fallback
and its counter can go once 703/704's writable Var slots settle the task's
cells.

**The builders.** The second half of the maps view and the sets view: a
map or set the real core builds or updates is an engine form, so macro
code's intrinsics (`__rt.cljGet` on the result of `frequencies`, `=` of an
`into #{}` result) and the export read it. `makeMap` builds `formArrayMap`
for the `array-map` kind and `formMap` for the `hash-map` kind (the two
kinds the V8 profile keeps apart, promotion at the ninth entry), `makeSet`
`formLiteralSet` and `formSet`; `mapFromEntries` and the hash map statics
no longer build hash tries (the engine hashes); a map's kind is read from
the form (`formIsArrayMap`, which now answers for a positional set too) so
an update of a hash map is a hash map again (trie order, as the V8
profile's `PersistentHashMap` keeps it); `mapTraversal`, `setTraversal`
and `collectionEntryIndex` take a form's entries positionally whatever its
kind; a map entry is an engine entry (`mapEntry`: the first entry of a
one-entry map's seq, which `cljIsMapEntry`, `key`, `val` and the `vectors`
view take), so the seq of a map and `find` cross into macro code; and the
empty set and maps are engine forms built once at boot. Sorted maps and
sets, records and queues keep the V8 profile's objects. Two consequences
recorded in the tests' expected data: a runtime-built map is an engine
map, so a symbol or vector given one as metadata stays an engine form, and
`(assoc (hash-map :a 1 :b 2) :c 3)` prints in trie order. The public
constructors (`PersistentArrayMap.` with its slots) cannot brand an engine
result (`publicCollectionSlots`), which the real-mode runs below list.

**Counts.** The real-mode `bbp_macros` run (335 tests on the base, 338
with this pellet's) fails 113 after the first commit (d637fa45: the
imported-form view, the publication swap and the builders together),
from 155: 43 tests fixed, one changed (`native_environment_updates_
changed_slots_without_importing_the_namespace_again` counted a scalar the
bridge no longer retains; the count now includes plain scalars), and 110
after the second (the reduced view: a reduction a macro stops with
`(reduced x)`, an engine marker, stops on the real core; three tests), of
which two are `deadline exceeded` on the loaded machine (the debug build
ran 18 minutes at `-j 4`); rebased onto master 48a3a404 (the engine's
collector work), 109 of 338, none new against the base's list. The Pants
build's whole suite in `real` mode (`-j 8`, 41 minutes beside peers)
fails 330 of 2,244 (329 failed, 1 timed out; 473 on the base), with the
port's 142 baseline failures among them, led by the changed answers (87),
`seq`/`count` on a port-built object the port's adapters reject (21),
protocol extension of a hub function (8), interop with engine forms, and
the unsupported results above; in `port` mode it is exactly the 142
baseline and `bbp_macros` passes 338 of 338. The remaining `bbp_macros`
failures by first error (`bb` grouping of the run's log, after the first
commit):

| Tests | First error | Cause |
| ---: | --- | --- |
| 46 | changed answer (`left == right`) | printing and predicates over engine values and port-built values (Vars, multimethods, regexes, delays, records, structs), `with-redefs` of a core function, `satisfies?`/`instance?`; 649's dispatch tables for the protocol ones |
| 12 | `Unsupported macro result arbitrary object` or `function` | a macro returns a runtime-owned value: sorted maps and sets, queues, deftype instances, a reify, `memoize`'s function |
| 5 | `cannot convert object to primitive value` | a real function strings a value the views do not take yet: a struct, a port-built Var or regex, a sorted collection |
| 10 | `is not ISeqable`, `Cannot create iterator`, `count`/`seq`/`assoc`/`realized?` `on object`, `conj on reduced` | structs, records, Vars, reduced values and sorted collections reaching the generic paths; `IStack.-pop`, `IReversible.-rseq`, `IWriter/-write`, `IPrintWithWriter` on engine or port values |
| 2 | `heap byte limit exceeded` | the two tests that set 16 MiB or less (the boot itself; from 16 on the base: the published hub spares the tasks their private copies) |
| 9 | the published hub is immutable, or a type is not the port's | `cannot assign to read-only property`, `cannot set property 'call'`, `invalid 'instanceof' right operand`, protocol extension of a hub function; 649 |
| 3 | `No method in multimethod` | port-built multimethods against the real dispatch; 649 |
| 2 | `a shared lazy sequence realized a value it cannot share` | `mapSeq`'s thunk over a regex state; a task-owned value in a lazy result |
| 1 each | `DataView is not defined`; `no conversion to symbol`; `value does not support deref`; a dependency failed downstream; `cannot add property 'key'` (a record update writes an entry object) | the engine's intrinsics; Vars; records |

**Memory.** The real core's live size after boot rose from 20.1 MB to
29.8 MB with the builders (`macro_core_boot_and_task_bytes`; the peak is
106 MB in both), which the heap-limit tests feel; the cause is not
measured (the engine has no live inventory by kind; candidates: the maps
the boot builds as engine forms with their intermediate vectors retained
through `formEntries`, and the per-entry closures of `sharedMapEntries`).
Open, with the boot's garbage of the fourth attempt.
