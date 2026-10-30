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
  lazy sequence or delay still realizes once per build, in the first task
  that needs it, with other readers parked (613).
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
