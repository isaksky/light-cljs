# Value layout: classes with secret-key protocol slots

Design of light-cljs-1008 (2026-11-14), for the output-speed-parity group
(pl group 1661): every later pellet of the group follows it. It replaces the
runtime's private-field payloads (`payloads`, `privateStore`, `kindedStore`
and the `WeakMap` stores at the top of `src/js_runtime.js`) and its layered
dispatch (`behindProtocol`, `publicMethod`, kind tables) with upstream-style
classes: data in the value's own fields, protocol methods on the prototype,
mask bits. What keeps a runtime's values apart from host objects and from
other runtimes is a secret short string in every property name the runtime
gives a value or a prototype, as the user chose on 2026-11-14.

It is read against research/output-speed/investigation-6ceeb23b/REPORT.md
(the causes and the representation-versus-dispatch prototype),
docs/advanced-optimization.md (Phase 2 builtin types) and upstream's
`cljs/core.cljs` (research/clojurescript). The key measurements come from
the prototype in research/output-speed/value-layout-1008/ (section 1).

## Decisions for the user

Answered by the user on 2026-11-15; the questions below are kept as asked.

1. **Answered: yes.** The per-build key is accepted; two copies of the same
   build on one page share it and recognise each other's values.
2. **Answered: key every protocol name**, program protocols included.
   String-named slot interop (`aget`/`aset`/`defineProperty` of a slot name,
   JavaScript implementing a protocol by name) stops working; the test cases
   pinning it are rescoped to the keyed names (light-cljs-1009).
3. **Answered: yes.** `Object.keys`, `Object.entries`, `for...in` and spread
   of a runtime value may list its keyed fields; `JSON.stringify` is
   unchanged. The `runtime_value_enumeration_contract` test pins it.

What light-cljs-1009 has landed so far, and how it differs from the sections
below while the migration is under way:

- **Keyed names (section 1).** Every protocol slot, marker and mask name ends
  in `$LKEY`, in the runtime (`protocolSlot`, `protocolMasks`, the masks of
  `makeType`, `defineType` and `typeMasks`), the shared-heap profile, the
  Pants runtime's reads, and emission (`rta::protocol_slot`; the analyses
  scan for `rta::protocol_slot_stem`, which a keyed name contains). The
  analyzer keys a member, dotted field or `js/` path component with the shape
  of a slot, marker or mask (`analyzer::keyed_js_member`); the invoke
  convention `cljs$core$IFn$_invoke$arity$N` stays unkeyed. The key is the
  placeholder itself everywhere for now: the derivation, the registry and the
  output writer's substitution are still to come.
- **Slot-first dispatch (section 3), for persistent vectors.** Their
  prototype holds the keyed kind (`kinds`) and keyed slots for `-nth`,
  `-count`, `-peek`, `-conj`, `-assoc` and `-lookup`. The wrappers `nth.slots`,
  `count.slots`, `peek.slots`, `conj.slots`, `assoc.slots` and `get.slots` are
  each operation's outermost layer: a value with a keyed kind calls its keyed
  slot first. Program types keep today's path until their recognition moves
  to the keyed kind (light-cljs-1013), which keeps the getter-order tests of
  `specify!`-ed host objects as they are. The vectors still keep their store
  in the private-field brand (`payloads.vectors`), so a slot reads it with
  `vectors.get`; a value with the prototype and no store (one made by
  reflection) has its slot answer `noBranch` and takes the operation, so its
  answers do not change. The runtime's own slot methods are marked
  (`builtinSlot`), and the older layers' lookups (`keyMethod`,
  `directMethod`) skip a marked slot, so they answer as before and never call
  a slot that serves them; `extend-type` and `specify!` write unmarked
  methods, which every layer finds.

1. **Two copies of one build on a page.** With the per-build key that
   section 1 recommends, two loads of the *same* output file share the key,
   so each recognises the other's collections and seqs as its own (and
   operates on them correctly, since their runtime text is identical);
   two *different* builds never do. Keeping two copies of one build apart
   needs a per-load key, which costs about 6 ns (48%) on every megamorphic
   slot read and 3 ns on every protocol test of a host value, so the generic
   operations would stay slower than shadow-cljs's. Today's private fields
   keep two copies apart. **Question: do you accept that two copies of the
   same build on one page recognise each other's values (different builds
   never do), in exchange for a per-build key that measures as fast as
   shadow-cljs's names?** Recommended: yes. Anyone who needs one program
   twice in isolation builds it twice with different `:runtime-key`s
   (section 1).

2. **Protocol names written as strings.** Source that names a slot, marker
   or mask as a member (`(.-cljs$core$ISeqable$ x)`, `(set! (.. T -prototype
   -cljs$user$P$f$arity$1) f)`) keeps working: the analyzer keys it. A name
   given as a string does not: `(aget v "cljs$user$P$f$arity$1")`, `(aset o
   "cljs$user$P$" PROTOCOL_SENTINEL)`, `(js/Object.defineProperty o slot ...)`
   with a computed `slot`, and JavaScript outside the program that
   implements a protocol by its upstream names. For core protocols this
   follows from your choice (those are exactly the names upstream
   ClojureScript's and other runtimes' values carry). For program protocols
   (`defprotocol` in the program or its libraries) it could be kept by
   keying only core protocol names, at the same speed; the cost is that a
   library protocol extended onto a host constructor (`extend-type js/Date
   lib/P`) is then shared by name between two builds that both contain the
   library, so one build's method can run on the other's calls and hand it
   values it does not recognise. Today's tests of the string interop are
   `protocols::protocol_dispatch_uses_public_slots_and_native_tables` (its
   `aget` and `aset` cases) and
   `protocols::protocol_dispatchers_call_host_slots_and_keep_their_length`.
   **Question: should every protocol name be keyed, so that string-named
   protocol interop (`aget`/`aset`/`defineProperty` of a slot name, and
   JavaScript implementing a protocol by name) stops working for program
   protocols too, or should program protocols keep their unkeyed upstream
   names?** Recommended: key every name (your wording, "all the prototype
   extensions", and the safer one for two runtimes); the two tests' string
   cases are then rescoped.

3. **Enumerating a runtime value.** The fields are own string-named
   properties (`cnt$<key>`), so `Object.keys`, `Object.entries`,
   `for...in` and object spread of a runtime collection list them, as
   upstream ClojureScript's list theirs; today they list nothing.
   `JSON.stringify` keeps today's `{}` through a `toJSON` on each runtime
   class's prototype or base, which costs nothing per operation. Hiding the
   fields otherwise means `Symbol`-keyed fields, measured at +8.5% on a
   mixed seq walk (section 1). **Question: do you accept that
   `Object.keys`, `Object.entries`, `for...in` and spread of a runtime
   value list its keyed internal fields, with `JSON.stringify` unchanged?**
   Recommended: yes. Whatever the answer, the first converting pellet adds a
   `runtime_value_enumeration_contract` check to
   tests/support/js_runtime.test.js that pins it.

## The rules in brief

- Every runtime value is an instance of a class of its kind (section 2). Its
  data are own fields; nothing in it is frozen or held in a side table.
- Protocol slots, protocol masks, protocol markers and the kind number are
  properties of the class prototype whose names end in `$<key>`, the
  per-build key (section 1). The value's own fields carry the key too.
- A core operation reads the keyed mask bit or marker and calls the keyed
  slot (section 3). `extend-type` and `specify!` overwrite slots, so no
  operation checks whether a slot was replaced.
- Release, dev builds and the target-side REPL use this layout and nothing
  else. The compile-time runtime (macro host, evaluator) uses it too and
  adds a per-load brand for the compiler form bridge; the Pants build keeps
  its engine forms for the kinds it swaps today (section 4).
- Deliberate reflection (reading the prototype's keys, `Object.create` of a
  runtime prototype, copying fields onto one) can forge a value. Nothing an
  accident can produce does (section 5). String-named protocol interop and
  enumeration of a value's fields wait for the user (questions 2 and 3).
- Each conversion deletes its kind's payload stores in the same pellet; the
  types not yet converted keep today's path (section 6).

## 1. The secret key

### What carries it

Every property name that recognises or dispatches a value carries the key:
the keyed name is the unkeyed one followed by `$<key>`.

- protocol method slots, `cljs$core$ICounted$_count$arity$1$<key>`, for the
  runtime's classes, program types, `reify`, `extend-type`, `extend-protocol`
  and `specify!` alike;
- protocol markers, `cljs$core$ISeqable$$<key>`;
- the two mask properties, `cljs$lang$protocol_mask$partition0$$<key>` and
  `...partition1$$<key>`;
- the kind number, `cljs$lang$kind$$<key>` (section 2);
- the runtime classes' own data fields, `cnt$<key>`, `tail$<key>`.

Not keyed: the functions' arity convention (`cljs$core$IFn$_invoke$arity$N`,
`cljs$lang$maxFixedArity`, `cljs$lang$applyTo`), which host callers and
other runtimes call through `.call` anyway, and the `IFn` invoke entries and
`call`/`apply` of a type implementing `IFn` (its `IFn` marker and mask bit
are keyed); `Object` protocol members (`toString`, `equiv`, `indexOf`);
well-known symbols (`Symbol.iterator`); and a program type's or record's own
fields, which keep their munged source names (`(.-x p)`), and a record's
`__meta`, `__extmap` and `__hash`.

The upstream field names of the runtime's types (`(.-cnt v)`, `(.-tail v)`,
`(.-root v)`) stay the public accessors they are today (`publicFields`): an
accessor on the prototype that reads the keyed field, so a read answers as
today and a write to an immutable field still makes a shadow property rather
than changing the value.

Source that names a keyed member, `(.-cljs$lang$protocol_mask$partition0$ x)`,
`(.cljs$core$ILookup$_lookup$arity$2 m k)` or a marker, is keyed by the
compiler at every level (the analyzer recognises the three shapes, with the
protocol's namespace, as it munges members today), so interop that names
slots keeps working. A name given as a string, literal or built at run time
(`(aget x "cljs$core$...")`, `goog.object/get`, `Object.defineProperty`),
is not keyed and sees nothing ("Decisions for the user" 2, which also asks
whether program protocols keep unkeyed names).

### Measured

`node run.mjs 7 21` in research/output-speed/value-layout-1008/ (`gen.mjs`
generates one runtime-like script per variant: the same eleven classes, the
same generic operations, the same benches; `bench.mjs` measures one bench in
a fresh process, so each bench's inline caches see only its own receivers;
`run.mjs` alternates the variants). Node v26.12.1 (V8), 24 CPUs, load
average 3.6-5.1 (unreserved); ns per operation, median of 7 rounds, range in
brackets (`results-key.json`, `table-key.md`).

- **U**: upstream names, dotted, as shadow-cljs's output has them (the
  baseline).
- **S**: the per-build string, dotted: `o.cljs$core$ICounted$_count$arity$1$q7Kx()`.
- **Y**: a per-load `Symbol` per slot, computed: `o[s_count1]()`.
- **L**: a per-load random string per slot, computed (the constant built at
  load).
- **YF**: Y with the fields also per-load `Symbol`s.

| op | U | S | Y | L | YF |
|---|---:|---:|---:|---:|---:|
| count, one class (monomorphic) | 4.3 [4.3-4.4] | 4.3 [4.3-4.4] | 4.3 [0.8-4.5] | 4.3 [4.3-4.4] | 4.3 [4.2-4.4] |
| count, four classes (polymorphic) | 3.3 [3.3-3.3] | 3.3 [3.3-3.3] | 3.3 [3.3-3.3] | 3.3 [3.2-3.3] | 3.3 [3.3-3.3] |
| count, ten classes (megamorphic) | 13.3 [13.0-13.4] | 13.1 [13.0-13.5] | 19.6 [19.5-19.9] | 19.6 [19.5-20.0] | 19.4 [19.3-19.5] |
| get of a keyword, small map | 8.9 | 8.9 | 8.9 | 8.9 | 9.0 |
| direct slot call (light-cljs-1016's shape) | 8.8 | 8.8 | 8.8 | 8.8 | 8.8 |
| assoc, small map | 22.4 | 22.5 | 22.4 | 22.4 | 22.4 |
| 64 conj onto a vector | 1387 [1348-1511] | 1456 [1371-1500] | 1464 [1370-1522] | 1471 [1379-1499] | 1470 [1372-1494] |
| seq walk, vector of 1000 | 7577 | 7503 | 7506 | 7515 | 7549 |
| seq walk, four seq kinds | 10027 [8077-10344] | 9935 [9848-9996] | 10602 [10337-10705] | 10640 [9707-10817] | 10670 [10498-10830] |
| = of two 10-item vectors | 36.4 | 36.4 | 36.4 | 36.5 | 36.4 |
| satisfies? ICounted of host values (miss) | 9.3 [9.2-9.5] | 9.3 [9.2-9.5] | 12.3 [12.2-12.7] | 12.6 [12.6-12.9] | 12.4 [12.3-12.6] |
| startup: load and first call of 400 functions (ms, 21 runs) | 7.64 [6.69-7.97] | 7.52 [7.18-7.81] | 7.61 [6.99-7.82] | 7.65 [7.16-7.80] | 7.67 [7.06-7.85] |
| emitted-program text, bytes / gzip | 352,448 / 6,145 | 378,698 / 6,273 | 213,098 / 5,397 | 213,098 / 5,397 | 213,098 / 5,397 |

A second run (`node run.mjs 5 11 results-fields.json U,S,SF`, load
4.4-5.0) added **SF**, the per-build string with `Symbol`-keyed fields: equal
to S everywhere except the four-kind seq walk, 10,860 [10,742-10,983] against
S's 10,011 (+8.5%).

What the numbers say:

- A per-build string costs nothing: S is U within noise everywhere. Dotted
  names are what V8's named inline caches and its megamorphic stub cache are
  built for, whatever the name.
- A computed key costs nothing where a site sees one to four classes, and
  6.3 ns (48%) where it sees more, which is exactly the generic operations'
  case (`count`, `first`, `next`, `get` in a real program see every
  collection and seq kind), plus 3 ns on every protocol test of a host value.
  The cost is the keyed access, not the `Symbol`: the per-load string L costs
  the same. V8's megamorphic keyed load does more work before
  it reaches the property cache than a named load does (not profiled
  further; the numbers are what decide).
- Startup does not separate them: building the prototypes and compiling the
  call sites costs the same in every variant.
- A computed key's text is smaller (a local name per slot, which minifies),
  13% less gzip on the emitted part. A per-build suffix adds 2% gzip there.
  Size is recorded, not decisive (group rules); light-cljs-1016 may later
  give `:advanced` short keyed names from a per-build table.

### Choice: a per-build string, dotted

The key is a per-build string: 8 characters of `[0-9a-z]`, appended as
`$<key>`. It is the user's choice and the fast one; a per-load key would
leave the generic operations 48% above shadow-cljs's at their megamorphic
sites, which the group's goal forbids. Switching to a `Symbol` is not
recommended. Fields use keyed string names too: `Symbol`-keyed fields (SF)
cost 8.5% on the mixed seq walk.

**Derivation.** For release output, `key = base36(hash(the output's text
with the key placeholder in it, optimization level, :runtime-key))[0..8]`:
the output's text includes its runtime slice and its whole program, so two
outputs that differ in anything (a constant, a version, a different app, the
same app at another level) get different keys, and the same output always
the same key. `:runtime-key` is an optional build option (any string;
default none). The key is deterministic, so the same inputs give
byte-identical output on every machine. Eight base-36 characters make an
accidental match between two different builds a one in 2.8 x 10^12 event,
and the runtime still detects it: as it loads it registers `key -> hash of
its output` in one page-wide registry (`globalThis[Symbol.for(
"light-cljs.runtime-keys")]`, a `Map`, the runtime's only global write), and
a load that finds its key registered with a different hash throws at once,
naming `:runtime-key`, before any value exists. The same output loaded
twice finds its own hash ("Decisions for the user" 1). The runtime text uses a placeholder suffix
(`$LKEY`, which `src/runtime_manifest.rs` checks appears only as a keyed
name's suffix), which embedding replaces; the raw `src/js_runtime.js` that
tests evaluate runs with the literal placeholder as its key. Compiled
namespace artifacts in the compile cache carry the emitter's placeholder for
the key and the output writer substitutes it (light-cljs-1009 picks a
placeholder that no emitted string literal can contain, for example one
written only through an escape the emitter always applies), so the key is
not part of a namespace's cache key.

**Dev builds, the REPL and hot reload.** A dev build derives its key from the
whole runtime source instead of its slice, so its key depends on neither the
sources nor an edit (one that makes the build keep more runtime units
included), and every
recompiled namespace, every hot reload and every form the REPL compiles
against the running page uses the key the page loaded with; the dev session
keeps it with the build. A change to the runtime text (a light-cljs upgrade)
changes the key, and a page with the old runtime then needs a full reload,
which it needs anyway. A dev build registers its build identity (build id
and entry namespaces) as its hash, so two different dev builds that match
keys are caught at load too.

### Two runtimes on one page

- **Two different builds** (including a dev and a release build of one app):
  different keys. Neither recognises the other's values: to each, the
  other's vector is a host object (`vector?` false, `count` throws as for
  `{}`, printing shows a host object). An `extend-type` of a host constructor
  (`js/Date`, `js/Object`) in one writes keyed names the other never reads,
  where upstream's would clash. `extend-type` of `object`, `default` or
  `number` goes to each runtime's own tables, as today. What they share is
  only the page: globals the program itself writes.
- **The same build loaded twice** (the question under "Decisions for the
  user"): the same key and the same runtime text, so each recognises the
  other's values and calls their slots, which are the other copy's methods
  over the other copy's fields, and answers correctly. State stays per copy:
  atoms, the keyword intern table (keywords from the two copies are equal,
  never identical: equality is by keyed kind and `fqn`, never `instanceof`,
  section 3), protocol extensions of each copy's own classes, `*print-fn*`
  and the rest. An `extend-type` of a host constructor is shared by name,
  and both copies install the same code. A direct field read of another
  value is guarded by `instanceof` of this copy's class, never by the kind
  alone, so a copy only reaches the other's fields through the other's
  slots.
- **Compile time**: each compile-time runtime has a fixed key and a per-load
  brand (section 4); it never shares a realm with output.

light-cljs-1018 tests these statements in Node and headless Chromium and
records the final guarantee here.

## 2. The classes

A conversion moves where a value's data live, not the algorithms: each
class's fields hold what its payload, its metadata entry
(`metadataValues`), its hash entry (`constructorHashes`) and its public
constructor slots (`publicCollectionSlots`) hold today, named after
upstream's fields where the data are the same. Nothing is frozen. Every
field below is a keyed name (`cnt` is `cnt$<key>`); `hash` is `null` until
computed, as upstream's `__hash`.

Each class keeps today's public name and `builtinType` base: its prototype
is `Object.create` of today's frozen shared base (`sequencePrototype` and
the others), which holds `call`, `apply`, `toString`, `equiv`, `indexOf`,
`lastIndexOf` and `Symbol.iterator`; the class prototype holds the keyed
slots, masks, markers and kind and the public field accessors, and stays
extensible for `extend-type`. Each value class has a kind number, a small
integer that one unit (`kinds`) assigns, on its prototype under the keyed
kind name: it says "a value of this runtime, of this class" (printing, the
compile-time bridge, the host-table exclusion of section 3, a record's
map-ness). Protocol membership is the masks and markers, as upstream's.

Internal helpers (vector and trie nodes, tree nodes, chunk buffers,
iterator state, transformer buffers) are plain classes with no kind, no
slots and plain field names: only the runtime's own methods reach them, and
`.-root` of a vector stays an opaque token.

| kind | class (today's name) | fields | replaces | pellet |
|---|---|---|---|---|
| vector | `PersistentVector` (`PersistentVectorValue`) | meta, cnt, shift, root, tail, hash | `vectors` (kind 3) | 1009 |
| subvec | `Subvec` (`SubvecValue`) | meta, v (the first vector, as today's view), start, end, hash | `vectors` (`VectorView`) | 1009 |
| map entry | `MapEntry` (`MapEntryType`) | key, val, hash | `vectors` kind 8, `mapEntries` | 1009 |
| transient vector | `TransientVector` (`TransientVectorType`) | cnt, shift, root, tail (the root's edit is the owner) | `transients` | 1009 |
| indexed seq | `IndexedSeq` (`IndexedSeqValue`) | arr (an array or a string), i, meta | `views` | 1010 |
| chunked seq | `ChunkedSeq` (`ChunkedSeqValue`) | vec, node, i, off, meta, hash | `views` | 1010 |
| reversed seq | `RSeq` (`RSeqType`) | ci, i, meta (O(1) as upstream's, where today copies leaf by leaf; same answers) | `views` | 1010 |
| list | `List`, `EmptyList` (`ListValue`, `EmptyListValue`) | meta, first, rest, count, hash; the empty list: meta | `cells` | 1010 |
| cons | `Cons` | meta, first, rest, hash | `cells` | 1010 |
| lazy seq | `LazySeq` | meta, fn, s, hash | `lazyValues` | 1010 |
| chunked cons, chunk | `ChunkedCons`, `ArrayChunk`, `ChunkBuffer` | chunk, more, meta, hash; arr, off, end; buf, end | `cells`, `chunks`, `chunkBuffers` | 1010 |
| ranges | `IntegerRange`, `Range` (`IntegerRangeValue`, `RangeValue`) | meta, start, end, step, cnt, hash; meta, start, end, step, chunk, chunkNext, hash | `cells` (`IntegerRangeCell`, `RangeCell`) | 1010 |
| repeat, iterate, cycle | `Repeat`, `Iterate`, `Cycle` (`...Value`) | meta, count, val, next, hash; meta, f, prevSeed, seed, next; meta, all, prev, current, next | `cells` | 1010 |
| rest arguments | `IndexedSeq` over a fresh array of the arguments | arr, i, meta | `fnRest`'s slice, freeze and stamp | 1010 |
| map seqs | `PersistentArrayMapSeq`, `NodeSeq`, `ArrayNodeSeq`, `PersistentTreeMapSeq`, `KeySeq`, `ValSeq` | arr, i, meta; meta, nodes, i, s, hash; meta, stack, ascending, cnt, hash; mseq, meta | `views` | 1010 as far as their maps allow, else 1012 |
| keyword | `Keyword` (`KeywordValue`) | ns, name, fqn, hash | `keywords` | 1011 |
| symbol | `Symbol` (`SymbolValue`) | ns, name, str, hash, meta | `symbols` | 1011 |
| array map | `PersistentArrayMap` (`PersistentArrayMapValue`) | meta, cnt, arr (upstream's flat key, value array), hash | `maps` (kind 4), `mapKinds` | 1012 |
| hash map | `PersistentHashMap` (`PersistentHashMapValue`); nodes `BitmapIndexedNode`, `ArrayNode`, `HashCollisionNode` | meta, cnt, root, hasNil, nilVal, hash; the nodes keep today's fields (edit, bitmap, arr, occupied count, capacity) | the trie's private fields | 1012 |
| sorted map | `PersistentTreeMap` (`treeMapConstructor`); `RedNode`, `BlackNode` as map entries | comp, tree, cnt, meta, hash; key, val, left, right, hash | `maps`, `sortedComparators` | 1012 |
| map transients | `TransientArrayMap`, `TransientHashMap` (`...Type`) | editable, len, arr; edit, root, count, hasNil, nilVal | `transients` | 1012 |
| sets | `PersistentHashSet`, `PersistentTreeSet`, `TransientHashSet` | meta, the members as today (an array set's array, a hash set's trie), hash; meta, treeMap, hash; the transient's members | `sets` (kind 6), `setKinds` | 1013 |
| queue | `PersistentQueue`, `PersistentQueueSeq` (`PersistentQueueValue`) | meta, count, front, rear, hash; meta, front, rear, hash | `queues` | 1013 |
| atom, volatile | `Atom`, `Volatile` | state, meta, validator, watches; state | `atoms`, `volatiles` | 1013 |
| reduced, delay | `Reduced`, `Delay` | val; f, value | `reducedValues`, `delays` | 1013 |
| var, multimethod | `Var`, `MultiFn` | val, sym, meta; name, dispatchFn, defaultDispatchVal, hierarchy, methodTable, preferTable, methodCache, cachedHierarchy | `vars`, `multiFns` | 1013 |
| the rest | `UUID`, `TaggedLiteral`, `Namespace`, `Eduction`, `MetaFn`, `StringBufferWriter`, iterator seqs (`ES6IteratorSeq`), `Loadable` | uuid, hash; tag, form; obj, name; xform, coll; afn, meta; sb; value, iter, rest | `uuids`, `uuidHashes`, `eductions`, `metaFunctions`, `stringBufferWriters`, `iteratorSeqs` | 1013 |
| records, program types | the emitter's constructors (`defineType`, `makeType`) | the program's munged fields; a record's `__meta`, `__extmap`, `__hash` (not keyed) | `TypeBrand`, `typeFields`, `recordTypes` in release and dev | 1013 |
| reify | the per-site type `reify` makes | captured locals under `members`, as today | `reifyValues` | 1013 |
| iterators | `StringIter`, `ArrayIter`, `SeqIter`, `RangedIterator`, `HashMapIter` and the rest | upstream's | (helpers, no kind) | with their collection |

The "lite" types the runtime also defines (`VectorLite`, `HashMapLite`,
`ObjMap`, `SetLite`, `ArrayList`) convert in their kinds' pellets, with
upstream's fields.

**Records become keyed-kind classes in light-cljs-1013, not 1012.** They are
already constructor functions with prototype methods, so what changes is
their brand: `TypeBrand` and `ClassBrand` become compile-time only, and the
release and dev emitters write `TypeBrands::Unread`. That is the same change
as for program types and `reify`, and 1013's gate is where record operations
are measured. 1012 does not need it: once a converted map's `get` goes
slot-first it never reaches `maps.has`'s record walk.

## 3. Dispatch

### Core operations

A core operation over a core protocol method reads the keyed mask bit (where
the protocol has one), else the keyed marker, and calls the keyed slot with
the receiver as `this` and first argument, as the dispatchers call slots
today:

```js
function count(x) {
  if (x == null) return 0;
  if ((x.cljs$lang$protocol_mask$partition0$$LKEY & 2) !== 0 || x.cljs$core$ICounted$$LKEY === protocolSentinel) {
    const method = x.cljs$core$ICounted$_count$arity$1$LKEY;
    if (method !== undefined) return method.call(x, x);
  }
  return countFallback(x); // today's path: strings, arrays, host tables, errors
}
```

- **No replacement check.** A runtime class's built-in method *is* its slot.
  `extend-type` overwrites it on the prototype (through `patchDeclaration`,
  so the compile-time declaration journal can undo it), `specify!` shadows it
  on the instance, and the next call reads whatever the slot holds. Runtime
  internals call another value's method through its slot too, never through
  a direct reference to the built-in function, so an extension is honoured
  everywhere. A slot call at a monomorphic site costs what a direct call
  costs (section 1).
- **Order.** Each operation keeps today's order of its nil, string and array
  fast paths relative to the protocol lookup; the slot read takes the
  lookup's place. `(count "ab")` stays 2 whatever an `extend-type` of
  `string` says, as today.
- **Declared but missing.** A value that declares a protocol (mask bit or
  marker) without the slot falls through, and the fallback throws today's
  "No protocol method ... defined for this value".
- **Units.** Each slot assignment is its own unit, an effect of its class
  that waits for the operations reading it (`when:`), including the protocol
  function itself (`-count`), so a program keeps only the slots of the
  operations it keeps, as with today's kind-table branches. Masks, markers
  and the kind go with the class.

### Host values

- **nil**: each operation's nil case (`(count nil)` is 0, `(seq nil)` nil),
  then the `nil` table of `extend-type nil`, as today.
- **Numbers, strings, booleans, JavaScript symbols, bigints, arrays,
  functions**: the operation's own fast path where it has one (`count`,
  `nth`, `get` and `seq` of strings and arrays; functions are `IFn` and have
  `IMeta` natively), then the per-protocol table that `extend-type` of
  `number`, `string`, `boolean`, `symbol`, `bigint`, `array` or `function`
  fills, then `default`, through today's dispatch cells, else today's error.
- **`object`**: plain host objects reach the `object` table, then `default`.
  A host constructor's prototype extended with `extend-type js/Date` or
  `js/Object` holds keyed slots and markers, so its instances take the
  slot-first path, as today's slots are found first.
- **Runtime values never reach the core protocols' host tables**: a value
  with a keyed kind is the runtime's own, and its own implementation of a
  core protocol outranks `object` and `default` (today's rule,
  `builtinCoreImplementation`); a core operation's fallback tests the kind
  before those tables. Program protocols are different, as today: a runtime
  value whose class does not implement one reaches its `object` table (its
  `nativeType` is `"object"`), then `default`, so `(extend-type object P
  ...)` covers vectors and maps too.
- **Protocol functions** of program protocols (`(-foo x)`) keep today's
  order: metadata methods for `:extend-via-metadata` protocols, then the
  keyed slot, then the host tables, then `default`.

### `satisfies?` and `implements?`

- `implements?`: the keyed mask bit or the keyed marker, own or inherited.
- `satisfies?` of a **program protocol**: the same; else the host table of
  the value's type (`nil`, `number`, ..., `object`, which a runtime value's
  type is), then `default`, for every value, as today's `satisfies` does.
- `satisfies?` of a **core protocol**: the mask bit or marker; else, only
  for a value without a keyed kind, the host table of its type, then
  `default`, then the core protocol's `extend-type` implementations; else
  false.
- Today's special cases keep their answers (`IMeta` of a function, `reify`'s
  natively implemented protocols). light-cljs-1009 adds
  `protocols::user_protocol_tables_cover_runtime_values`: `extend-type` of
  `object` and of `default` with a program protocol, and its method calls,
  `satisfies?` and `implements?` on vectors, maps, seqs and host values, at
  every level, with today's answers.
- The runtime classes declare every protocol they implement: mask bits for
  the protocols that have one, keyed markers on the prototype for the others
  (`IFind`, `IUUID`, `Inst`, `IEncodeJS` and the rest), as upstream's
  deftypes do. The predicates (`vector?`, `map?`, `seqable?`, `counted?`)
  answer what they answer today; most of them are protocol tests.

### A program's methods on runtime types

`(extend-type PersistentVector ICounted (-count [_] 5))` writes the keyed
slot and marker on `PersistentVector.prototype`, so `(count v)` is 5 for
every vector, existing ones included. `(extend-type PersistentVector ISeq
...)`, a protocol vectors lack, sets the marker and slots, so `first` (mask
bit absent, marker present) calls it, while `nth` is unchanged. Extending a
subset of a method's arities keeps the other arity slots. `specify!` writes
the instance's own slots and markers. These are today's answers (the tables
of `protocol_hooks_answer_runtime_values_as_the_full_lookup`), which today
take emptied kind tables and a lookup per call; the class layout gives them
by construction. An `extend-type` of `IFn` replaces `call` on the class
prototype (shadowing the frozen base's), as it does today.

### Equality and hashing

`=` is identity, else the value's keyed `-equiv` slot (host values by
identity, nil and undefined equal, as today). Keywords compare by keyed kind
and `fqn`, symbols by kind and `str`, never by `instanceof`, so values of two
copies of one build agree. `hash` calls the keyed `-hash` slot, which caches
in the value's `hash` field; host objects keep today's weak identity
bookkeeping. light-cljs-1014 gives each class its own `-equiv` and `-hash`
(vectors by count and index, maps by count and lookup); mixed comparisons
keep their answers.

## 4. Profiles

| profile | layout | key | brand |
|---|---|---|---|
| release (`:advanced` and the other levels' output) | classes | per build | none |
| dev builds (and the target-side REPL, hot reload) | classes | per build, stable across reloads | none |
| macro host, compile-time runtime (V8 build) | classes | fixed (`$LKEY`) | per-load private field |
| evaluator: `--eval`, `--repl`, folding (V8 build) | classes | fixed | per-load private field |
| QuickJS reference (`lightCljsWeakMapStores`) | classes | fixed | per-load `WeakSet` |
| Pants build, shared-heap profile (`src/js_shared_profile.js`) | engine forms for the kinds it swaps; classes for the kinds it inherits | fixed | engine forms; per-load brand for inherited classes |

**Release and dev builds** adopt the class layout and nothing else: after
light-cljs-1015 no payload store, kind table, `behindProtocol`,
`publicMethod` or brand class is in their units. Release is the gate.

**The compile-time runtime** (the macro host's `compilerFormBridge` and the
evaluator, which load the same factory with the `compile-time` units) adopts
the class layout too: keeping two layouts would keep every slow path alive
and double every collection's code, and compile speed matters as much as
output speed. It needs one thing release does not: the form bridge must tell
a runtime value from an arbitrary host value without reading any property of
the host value, so that rejecting a macro result never runs a getter,
`toString` or a Proxy trap (docs/evaluator.md, kept unchanged, and
`macros::rejection_never_calls_arbitrary_getters_or_to_string`). A keyed
read is a property read: a Proxy's `get` trap runs for it. So in the
compile-time runtime each runtime class's constructor stamps its instance
with a per-load brand:

- every runtime class constructor ends with a marker comment
  (`/*@brand*/`) that the release and dev embeddings drop and that
  `compiler_runtime` (build.rs), which already inserts the compile-time
  hooks, replaces with `stamp(this);`;
- `stamp` is a compile-time unit: a return-override class adding one private
  field in V8 (the cost today's stores pay, about 9 ns per value, so no
  regression for the macro host), a `WeakSet` add where the host states
  `lightCljsWeakMapStores` (QuickJS, where private fields cost more than
  entries, light-cljs-292);
- the bridge, `inspect` and the compile-time printer test `stamped(value)`
  first, which reads no property, and only then the keyed kind.

**The one emitter.** Program types already have the switch: `TypeBrands`
(src/emitter.rs). Release and dev emission use `TypeBrands::Unread`, no
brand, at every level, and `defineType`/`makeType` set the keyed kind (type,
record or reify) on the prototype, which `record?`, `map?` of a record and
the other record tests read instead of `TypeBrand.type`. Compile-time
emission keeps `Shared` or `Own` by today's rules, so `TypeBrand`,
`ClassBrand` and `defineClass` stay, as compile-time units, with their tests
(section 5). `makeType`'s constructor, which dev builds and the lower levels
use, carries the `/*@brand*/` marker in place of its `new TypeBrand(...)`.

**The Pants build** keeps its engine forms (`__rt` intrinsics) for the kinds
`src/js_shared_profile.js` swaps today (vectors and map entries, maps, sets,
keywords, symbols, list cells), and takes the class layout for every kind it
inherits from the V8 profile. Its boot seals the prototypes, so `extend-type`
or `specify!` of a runtime kind keeps throwing there, as it does today
(`sharedProfileBoot`, until light-cljs-649). A generic operation's
slot-first path finds no slot on an engine form and falls through to the
host-value fallback (section 3), where the profile's units answer engine
forms; so a converted operation keeps a hook (as `kindHooks` are today) that
only `src/js_shared_profile.js` fills. Where a conversion changes an
interface a swapped unit or one of build.rs's pattern rewrites
(`shared_owned_allocations`, `shared_persistent_updates`,
`shared_scalar_reads`) relies on, the converting pellet changes them in the
same commit; an old-layout unit the profile still needs moves into
`src/js_shared_profile.js` rather than staying in `src/js_runtime.js`. Every
conversion runs `cargo nextest run --locked --no-default-features --features
bbp-macros --test bbp_macros`.

## 5. Tests

The rule: a test that a value no accident can produce is no language value
stays ("never by accident"). A test that pins what happens to a value made
by deliberate reflection over the runtime's prototypes or keys, which the
user gave up, is rescoped. Here *deliberate reflection* is
`Object.create` of a runtime prototype (or of a value), `Object.setPrototypeOf`
to one, copying a value's own properties onto a runtime prototype, reading
the prototype's keys (`Object.getOwnPropertyNames`/`Symbols`) and writing a
keyed name. Each rescoping lands in the pellet that converts the kind, with
its line in the commit message.

### Stay

- `builtin_types::value_kind_predicates_answer_as_the_brand_stores_did`: the
  rows of runtime values, host values (`string` to `fn`) and the Proxy, and
  the pollution loop, extended with the unkeyed upstream names
  (`cljs$lang$protocol_mask$partition0$`, `cljs$core$ICounted$`,
  `cljs$core$ICounted$_count$arity$1`, `cljs$lang$kind$`) defined on
  `Object.prototype`: a host that loaded upstream ClojureScript, or
  another light-cljs build, must change no answer.
- `builtin_types::protocol_hooks_answer_runtime_values_as_the_full_lookup`:
  the rows of runtime, host and `specify!`-ed values and the Proxy row, for
  both extension sets.
- `macros::rejection_never_calls_arbitrary_getters_or_to_string`, all four
  cases, unchanged: the compile-time brand keeps it (section 4).
- `bbp_macros::shared_profile::shared_profile_class_brands_are_task_local`,
  unchanged: `defineClass`, `ClassBrand` and `TypeBrand` stay compile-time
  units with task-local flags.
- tests/support/execution.js (lines 4-5 and 71), the harness rule: language
  values are recognised only through the backend's `inspect`, "never ... by a
  public property or a tag-shaped host object", and plain objects are
  tagged. It stays as written: `inspect` reads keyed names, which no host
  object holds by accident.
- In tests/support/js_runtime.test.js: the host-object checks (`host`, `{...host}`,
  `[10, 20, 30]`, `{val: v}`, `{}`, arrays, functions, `Object.create(null)`),
  the `TypeBrand.type` loop at 1410-1414 (it tests the compile-time unit
  itself), the copies of a record's fields onto `{}` (1377: own fields
  without the prototype have no kind), and "a pipeline writes no WeakMap
  entry" (1592-1600), which the class layout keeps trivially.

### Rescoped to deliberate reflection

Each item is removed, or keeps only its host-object candidates; the reason
is the same for all, so the line says what each forges.

`builtin_types.rs`:

1. `value_kind_predicates...`: the 16 `forged-*` and `copied-*` rows
   (`Object.create` of the value's prototype, alone or with its fields): a
   value made from a runtime prototype now type-tests as its kind.
2. `value_kind_predicates...`: the Proxy line keeps its rule (only keyed
   names, which the test filters as `get:cljs$`, well-known symbols and the
   prototype are asked), and its expected text is updated to what the class
   layout asks; it may lose `proto`, since a record is told by its keyed kind
   rather than by walking the prototype chain for a brand.
3. `protocol_hooks...`: the `forged-*` rows, both extension sets.
4. `protocol_hooks...`: the expected list of the Proxy's reads: the same
   kinds of names, now keyed (`get:cljs$core$ICounted$<key>`); the key is
   normalised to `<key>` before comparing. A rename, not a semantic change.

tests/support/js_runtime.test.js (line numbers at master 6ceeb23b):

5. 183: `inspect` of `Object.create` of a keyword's prototype.
6. 184: the same of a vector's prototype.
7. 272: the same of each list, seq and cons value (the loop's
   `Object.create` line; its frozen-prototype lines stay, see below).
8. 415: the same of a hash map's prototype.
9. 607: the same of a hash set's prototype.
10. 784: `isReduced` of `Object.create` of a reduced's prototype.
11. 800-802: writing `val` on, and `isReduced` of, the same forged reduced.
12. 1376: `inspect` of `Object.create(Record.prototype)`.
13. 1378: a record's fields copied onto `Object.create(Record.prototype)`.
14. 1379-1381 (a record whose prototype is changed keeps its brand): a
    record moved to `Object.prototype` (`Object.setPrototypeOf`) is a host
    object to release and dev builds.
15. 1416: `inspect(Object.create(Record.prototype))` after the brand loop.
16. 1539: `conjBang` of `Object.create` of a transient's prototype.
17. 1579-1581: writing a public field on, and `inspect` of,
    `Object.create(Type.prototype)` for the 14 types listed: a forged value
    is accepted by the field accessor.
18. 1605-1610: the forged and copied candidates of lazy, chunked, chunk,
    vector seq, cons and list values.
19. 1674-1679: the forged and copied candidates of the fresh runtime's
    built values.
20. 1736-1741: the same in the `lightCljsWeakMapStores` runtime; the
    section's heading claim ("keeps its payloads in WeakMaps") becomes "a
    host that states WeakMap stores brands compile-time values with a
    WeakSet" when its kinds convert.
21. 1779-1809 (kind tables are private): removed with the kind tables
    (light-cljs-1015); every candidate holds a kind under a key read by
    reflection (`Object.getOwnPropertySymbols` of a prototype).
22. 1810-1838 (vectors', maps', sets' and queues' kind numbers): the same.

What stays of their neighbours: the frozen-shared-base checks
(`Object.isFrozen(Object.getPrototypeOf(Object.getPrototypeOf(value)))`,
`Object.getPrototypeOf(m0).call = ...` throws), because the class prototypes
keep inheriting from today's frozen bases that hold `call`, `equiv`,
`toString`, `indexOf` and `lastIndexOf` (section 3).

Not forgery, and therefore not rescoped here but put to the user:
enumeration of a runtime value ("Decisions for the user" 3) and string-named
protocol interop ("Decisions for the user" 2). If the user keys every
protocol name, the `aget` and `aset` cases of
`protocols::protocol_dispatch_uses_public_slots_and_native_tables` and the
host-slot cases of
`protocols::protocol_dispatchers_call_host_slots_and_keep_their_length` are
rescoped to the keyed names written as members; otherwise they stay. No other
test is rescoped for either question without the user's answer.

One more row is rescoped as a deliberate write to the runtime's prototypes:

23. `protocol_hooks...`, the third extension set ("Slots JavaScript writes
    on runtime prototypes"): `(set! (.. PersistentTreeMap -prototype
    -cljs$lang$protocol_mask$partition0$) 64)` replaces the class's whole mask
    number, built-in bits included, so the `sorted-map` row's `map?` and the
    other mask tests change, as writing a program type's prototype mask does
    since light-cljs-264. The marker and slot writes of that set keep their
    answers.

### Documentation rules that change

docs/evaluator.md keeps its rules: the macro host is still "not an untrusted
code sandbox" (line 154), and rejecting a macro result still does "not call
arbitrary getters or `toString`" (282-283).

docs/compiler.md, each changed by the pellet that converts the kinds it
names (the last by light-cljs-1015):

- 1028: "They preserve JavaScript Number, UTF-16 and private collection-brand
  semantics." becomes "They preserve JavaScript Number and UTF-16 semantics
  and recognise only values of this runtime's classes (docs/value-layout.md)."
- 1077-1083: "Owned arrays/cells and base callable storage are frozen;
  per-type extension prototypes and public value wrappers remain extensible
  for protocol and JavaScript property writes. Those properties do not
  replace the private collection payloads; private brands prevent host
  arrays or objects from impersonating language values." becomes "A runtime
  value is an instance of its kind's class: its data are own fields, its
  protocol methods, masks and kind are properties of the class prototype,
  and every such name ends in the build's secret key
  (docs/value-layout.md), so no host array or object is a language value by
  accident. Nothing in a value is frozen; the shared bases holding `call`,
  `toString` and `equiv` are."
- 1089-1092: "Its brand and store are a private class field rather than a
  WeakMap entry, which no host object can have or copy either, and its trie
  nodes stay private" becomes "Its count, shift, root, tail, metadata and
  hash are its own keyed fields, and its trie nodes stay private".
- 1183-1191 (`lightCljsWeakMapStores` keeps "all of these stores as
  `WeakMap`s instead ..., as unforgeable a brand") becomes "a host that
  states `globalThis.lightCljsWeakMapStores` before a runtime loads, as the
  evaluator does in its QuickJS sessions, has its compile-time values
  branded by a `WeakSet` rather than a private field".
- 1192-1204 (the `payloads` brand class and `payloadStore`) is removed; its
  replacement is one sentence: "A kind test reads the value's keyed kind,
  one property of its prototype."
- 1205-1213: "Unforgeability is kept everywhere: every kind test still reads
  a private field that no host object can have, inherit or copy, so a value
  forged with `Object.create` of a runtime type's prototype, or by copying a
  value's own properties, is no vector, map, seq or map entry, as before"
  becomes "No host value is a language value by accident: every kind test
  reads a property whose name ends in the build's secret key, which no host
  object holds or inherits, and copying a value's own fields onto another
  object copies no kind. Deliberate reflection can forge a value
  (`Object.create` of a runtime prototype, reading the prototype's keys);
  as in upstream ClojureScript, that is outside the guarantee
  (docs/value-layout.md, section 5)."
- 1218-1240 (kind numbers "under a private symbol", the kind tables and
  their wrappers) is replaced by section 3's dispatch in two sentences:
  "`seq`, `first`, `rest`, `next` and `count` call the value's keyed slot;
  `extend-type` and `specify!` overwrite slots."
- 1254-1257: "An array map ... stores a frozen entry array, and its kind, in
  class-private fields" becomes "An array map ... holds its keys and values
  in one array of its own, unfrozen, as upstream's".
- 1284-1287: "A hash map keeps its trie in a class-private field rather than
  a `WeakMap` entry: as unforgeable a brand" becomes "A hash map holds its
  root, count and nil entry in keyed fields".
- 2878-2906 (what makes an instance a type instance is `TypeBrand`) becomes
  "What makes an instance a type instance, and a record's type, is its
  prototype's keyed kind. Below `Advanced` the runtime's `makeType` makes
  the constructor; at `Advanced` each type's constructor is a function of its
  own. The compile-time runtime also brands each instance (`TypeBrand`, or a
  class's private field where every constructor runs only through `new`), so
  that the compiler form bridge tells it from a host value without reading a
  property", keeping the rest of the paragraph (the light-cljs-346 class
  `TypeError` applies only where the compile-time `Own` brand is used).
- 2975-2977: "Built-in types have distinct public extension prototypes while
  their immutable payloads and private brands remain protected." becomes
  "Built-in types are classes whose prototypes hold their protocol methods;
  `extend-type` replaces a method in place."

## 6. Migration order

Every step lands with the full suite, the QuickJS reference and the Pants
build tests passing, while converted and unconverted kinds coexist.

**What coexists, and how.**

- **Names are global from the first step.** light-cljs-1009 keys every slot,
  marker and mask name at once: the runtime's `protocolSlot` and mask table,
  `rta::protocol_slot` and the emitter's mask names, the analyzer's keying of
  slot-shaped members, the key derivation and the cache placeholder. Program
  types, `reify`, `extend-type` and `specify!` then write keyed names, so the
  slot-first path serves them from that step on. Expected data that only
  spell the names change with it (renames, listed in the commit).
- **A converted kind** has its class, keyed mask bits, markers and kind on
  the prototype, and its slots. It never gets today's private `kindOps`
  number or a store entry, so the old path cannot mistake it for a host
  object only if the old path never sees it: every function that reads the
  kind's store today (`grep` of the store's name is the checklist) is changed
  in the same pellet to call the slot or to read the class's fields behind
  an `instanceof` test, and **the kind's store is deleted in that pellet**,
  so that a forgotten reader fails the runtime manifest (`requires:`) or the
  load, never answers wrongly.
- **An unconverted kind** keeps its store, its kind-table branches and
  today's path. The slot-first test finds no keyed mask bit or marker on it
  (unless a program extended it, when the slot is the extension, which is
  today's precedence) and falls through.
- **Equality, hashing and printing** of a converted kind work from its step
  on: the generic structural code reads its fields (or its slots) until
  light-cljs-1014 gives each class its own `-equiv` and `-hash`.
- **The compile-time bridge** recognises each converted kind by `stamped`
  plus the kind from its step on, so the compile-time brand mechanism
  (section 4) lands with the first converted kind, in 1009.
- **Program types** keep `TypeBrand` until 1013 changes their recognition to
  the keyed kind.
- **Measurement.** Each pellet reports the gate (benchmarks/core-ops) before
  and after for its operations, workload 200k steady state, and advanced
  size (the size ratchet with a note), as the group's rules say.

**The order.** The group's order holds; what this design changes in it:

1. light-cljs-1009, vectors, map entries, subvecs, vector transients, and
   the mechanism: also the global key (above), the `kinds` unit, the
   compile-time `/*@brand*/` stamp and the bridge's `stamped` test, the
   slot-first path in the operations those types answer, and the vector
   seqs' constructors reading the class's fields until 1010 converts the
   seqs. It waits for the answers under "Decisions for the user", and
   also lands the key registry, the `toJSON` of question 3 and
   `protocols::user_protocol_tables_cover_runtime_values` (section 3).
2. light-cljs-1010, the seq kinds and rest arguments (section 2's rows);
   `RSeq` may become O(1). The map and set seqs convert as far as their
   collections allow.
3. light-cljs-1011, keywords and symbols; equality by keyed kind and `fqn`
   (section 3); the emitter writes literal keywords' hashes, computed at
   compile time, as upstream's constants carry them (section 7).
4. light-cljs-1012, array, hash and sorted maps, their transients and seqs;
   flat key, value arrays for array maps. Records stay on their brand.
5. light-cljs-1013, every remaining kind, and records, program types and
   `reify`: their recognition by keyed kind, and `TypeBrands::Unread` for
   release and dev emission.
6. light-cljs-1014, per-class `-equiv` and `-hash`.
7. light-cljs-1015, removal of the old layers and stores from the release
   and dev units; what stays is section 4's: the compile-time `stamp`,
   `TypeBrand`, `ClassBrand`, `defineClass`, and whatever
   `src/js_shared_profile.js` holds.
8. light-cljs-1016, direct slot calls at `:advanced` (dotted keyed names,
   which section 1 measured as fast as upstream's).
9. light-cljs-1017, startup (section 7).
10. light-cljs-1018, two runtimes on one page, with the `:runtime-key`
    build option (section 1), testing the guarantee the user's answer fixes.
11. light-cljs-1019, the confirmation run.

light-cljs-1002 (lazy map-seq entries) stays superseded.

## 7. Startup

Startup is 1.2-2.5x shadow-cljs's today because the runtime builds its
stores, resolves its method keys (`methodKeys`, `methodKey`), fills its kind
tables and wraps its operations as it loads (REPORT.md). The class layout
makes setup cheap rather than lazy:

- **Nothing to resolve.** Slot, marker and mask names are literals in the
  text, so no method key, kind table, store or wrapper is built at load.
  Defining a class is its constructor plus one assignment per kept slot,
  mask, marker and kind on its prototype, the shape of shadow-cljs's output.
  The key adds nothing at load (section 1, startup row).
- **Protocol machinery only where needed.** `installMethods`,
  `protocolSlot`'s munging, the dispatch cells and the host tables stay
  units that only `extend-type`, `specify!`, `reify` and program protocols
  keep, as today; a program without them keeps none, and what remains of
  `methodKey`'s resolution is done on the first extension, not at load.
- **Constants.** A literal keyword is constructed once at load with its
  namespace, name, `fqn` and its hash computed at compile time (light-cljs-1011);
  literal collections are direct constructor calls (`new PersistentVector(...)`
  of the class), hoisted as today.
- **Code V8 compiles at load.** No unit is a top-level function expression
  called at once (V8 compiles parenthesized functions eagerly) where a
  function declaration would do; tables that today are built by an
  immediately called arrow (`payloadStores`, `payloadStore`, `kindTables`)
  go with the stores. Printing's and the reader's tables are built on first
  use.
- **Measured by light-cljs-1017** with `--pseudo-names`, `node --cpu-prof`
  and the Chrome DevTools Protocol for TodoMVC, against shadow-cljs's first
  output at one iteration on workload 200k and 800k and the re-frame-shaped
  app, and TodoMVC's page load to interactive, including parse and compile
  time per byte of output.
