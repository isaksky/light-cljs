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
   pinning it are rescoped to the keyed names (light-cljs-1009), written as
   members, or as raw `js*` JavaScript where a test computes a name, once
   the key is real (light-cljs-1025).
3. **Answered: yes.** `Object.keys`, `Object.entries`, `for...in` and spread
   of a runtime value may list its keyed fields; `JSON.stringify` is
   unchanged. The `runtime_value_enumeration_contract` test pins it.

What light-cljs-1009 landed, and how it differs from the sections below
while the migration is under way:

- **Keyed names (section 1).** Every protocol slot, marker and mask name ends
  in `$LKEY`, in the runtime (`protocolSlot`, `protocolMasks`, the masks of
  `makeType`, `defineType` and `typeMasks`), the shared-heap profile, the
  Pants runtime's reads, and emission (`rta::protocol_slot`; the analyses
  scan for `rta::protocol_slot_stem`, which a keyed name contains). The
  analyzer keys a member, dotted field or `js/` path component with the shape
  of a slot, marker or mask (`analyzer::keyed_js_member`); the invoke
  convention `cljs$core$IFn$_invoke$arity$N` stays unkeyed. Since
  light-cljs-1025 the output writer writes each build's key for the
  placeholder and the runtime registers it page-wide (section 1,
  "Derivation", `src/runtime_key.rs`); the compile-time runtime keeps the
  placeholder as its key.
- **Slot-first dispatch (section 3), for persistent vectors.** Their
  prototype holds the keyed kind (`kinds`) and keyed slots for `-nth`,
  `-count`, `-peek`, `-conj`, `-assoc` and `-lookup`. The wrappers `nth.slots`,
  `count.slots`, `peek.slots`, `conj.slots`, `assoc.slots` and `get.slots` are
  each operation's outermost layer: a value with a keyed kind calls its keyed
  slot first. Program types keep today's path until their recognition moves
  to the keyed kind (light-cljs-1013), which keeps the getter-order tests of
  `specify!`-ed host objects as they are. A value with the prototype and no
  store of its own (one made by reflection) has its slot answer `noBranch`
  and takes the operation, so its answers do not change. The runtime's own
  slot methods are marked (`builtinSlot`), and the older layers' lookups
  (`keyMethod`, `directMethod`) skip a marked slot, so they answer as before
  and never call a slot that serves them; `extend-type` and `specify!` write
  unmarked methods, which every layer finds. A compiler session that demands
  declarations turns the wrappers off (`slotFirst.on`).
- **The classes (section 2), with one field.** Vectors, subvectors, map
  entries and transient vectors are instances of internal constructors
  (`VectorCell`, `SubvecCell`, `EntryCell`, `TransientVectorCell`) that share
  the public types' prototypes, kinds 1 to 4 of the `kinds` unit. Each holds
  today's store object, unfrozen, in one own keyed field (`store$LKEY`,
  `state$LKEY`), which `vectorStore` and `transientState` read behind the
  keyed kind, so the store algorithms (`vectorConj`, `VectorView`, the
  transients' state) are unchanged; `payloads.vectors` and `mapEntries` are
  deleted, and the map and set transients keep the `transients` store until
  light-cljs-1012. Moving the store's `cnt`, `shift`, `root` and `tail`, and
  the metadata and hash entries, into fields of the class itself is left to a
  later pellet. The emitter's direct reads of a known vector call
  `vectorStore`; the Pants profile swaps `vectorStore` for one that also
  reads engine vector forms. The class bases have a `toJSON` that leaves the
  keyed field out (question 3; `runtime_value_enumeration_contract` in
  tests/support/js_runtime.test.js).
- **The compile-time brand (section 4).** Each cell constructor ends with
  `/*@brand*/`, which build.rs turns into `stamp(this);` in the compile-time
  runtime only; the compiler form bridge tests `stamped` before it reads a
  value's kind, so rejecting a macro result still reads no property of a
  host value, and so does `inspect`, whose vector test carries
  `/*@stamped*/`, which build.rs turns into `stamped(value) &&` there. The
  printer is not changed: printing a host value reads its protocol markers
  and masks before any kind test, as it did before this pellet. The Pants
  profile's `stamped` also accepts engine vector forms.

What light-cljs-1010 landed: the seqs over an array or a string, the
first slice of section 2's seq rows.

- **The view classes.** Every seq `view` built, `IndexedSeq` (rest
  arguments, the seqs of arrays, strings, vectors of up to 32 items,
  queues and sets), `ChunkedSeq`, and the map seqs over an array of entries
  (`NodeSeq`, `ArrayNodeSeq`, `PersistentArrayMapSeq`, `RSeq` and the
  `PersistentTreeMapSeq` that `rseq` and `subseq` give), is an instance of
  an internal constructor on its type's prototype (`IndexedSeqCell`,
  `ChunkedSeqCell`, ...), kinds 5 to 11, with four own keyed fields:
  `arr$LKEY` (the storage), `i$LKEY` (the offset), `indexed$LKEY` and
  `view$LKEY` (today's view kind). A chunked seq adds `vec$LKEY` and
  `base$LKEY` (upstream's `vec` and `i`; its `off` is `i$LKEY`), where it
  kept them in `publicCollectionSlots`. `viewOf` replaces `views.get`; the
  `views` store is deleted, and with it a view's frozen payload and the
  chunked seq's slots entry per step. A view's `tail` was always null and
  is gone. Metadata and hashes stay in `metadataValues` and
  `constructorHashes` (light-cljs-1026). `RSeq` still copies its items.
- **Slots and slot-first dispatch.** `IndexedSeq` and `ChunkedSeq` have
  keyed `-first`, `-rest`, `-next` and `-seq` slots (`IndexedSeq` also
  `-count`), a vector a `-seq` slot, and `first`, `rest`, `next` and `seq`
  have slot-first wrappers (`first.slots` and the rest), as 1009's six
  operations. Where today's operation takes a further operation (`next` of
  a chunked seq or of a queue's seq is the `seq` of its `rest`), the slot
  takes it through the operations' wrappers, so an `extend-type` of that
  protocol answers as before.
- **Rest arguments** are an `IndexedSeq` over one fresh array, neither
  frozen nor stamped in release: `fnRest` copies a dispatcher's `arguments`
  and takes a direct call's array literal as it is.
- **The compile-time brand.** The view constructors end with `/*@brand*/`.
  The marker `/*@stamped*/` (build.rs `STAMPED_MARKER`) is now any kind
  test's prefix, so `inspect`'s view test carries it too; the compiler form
  bridge tests `stamped` before `isView` and `viewOf`.
- **What is left for the seq kinds** (filed as their own pellets before
  light-cljs-1011): lazy seqs, chunked conses, array chunks and chunk
  buffers (`lazyValues`, `chunks`, `chunkBuffers`), and the cells (`Cons`,
  `List`, `EmptyList`, ranges, `Repeat`, `Iterate`, `Cycle`, the key and
  value seqs and the tree map seq, `cells`).

What light-cljs-1011 landed: keywords and symbols.

- **The classes.** A keyword is a `KeywordCell` on `KeywordValue`'s
  prototype, kind 20, with the own keyed fields `ns$LKEY`, `name$LKEY`,
  `fqn$LKEY` and `hash$LKEY`; a symbol a `SymbolCell` on `SymbolValue`'s,
  kind 21, with `ns$LKEY`, `name$LKEY`, `str$LKEY` and `hash$LKEY` (kinds 12
  to 19 are left to the seq kinds still to convert). `keywordOf` and
  `symbolOf` replace `keywords.get` and `symbols.get`: the value itself
  where its kind and its `fqn` (`str`) field are there (the public
  constructor may give it an undefined spelling), else undefined, so equality
  reads the kind and `fqn`, never `instanceof`. The `keywords` and `symbols`
  stores are deleted; `keywordCells` and `symbolCells` stand for them in the
  store inventories. A symbol's metadata is its own `meta$LKEY` field
  since light-cljs-1031 (below). Upstream's public fields
  (`.-fqn`, `.-_hash`, `.-str`) read the keyed ones. A keyword's `call`
  is `get` without a store read, and its `IFn` arity entries, which
  `apply` calls, call `call`, so an `extend-type` of `IFn` still answers.
- **Lookups and hashes.** `entryIndex` compares a keyword key with each key
  by identity, then by kind and `fqn`, with no store and no slot read per
  entry; it reads the key's own `IEquiv` slot once, and a program's
  implementation (`extend-type`, `specify!` of the key) takes `equalPair`
  per key as before. `hash` caches a keyword's or symbol's hash in its
  field. light-cljs-1011 had each keyword constant at `Advanced` carry its
  hash, which the compiler computed (`value_hash::keyword`),
  `rt$keyword(null, "a", h)`; light-cljs-1050 took it out, since its digits
  barely compress (section 7, "Constants").
- **Profiles.** The keyword and symbol constructors end with `/*@brand*/`,
  and the compiler form bridge and `inspect` test `stamped` first. The Pants
  profile swaps `keywordOf` and `symbolOf` for ones that read engine forms
  (a fresh object of their fields, with the engine's hash), and its
  runtime-owned symbols (`sharedSymbol`) are symbol cells.

What light-cljs-1029 landed: the lazy seqs, chunked conses, array chunks
and chunk buffers.

- **The classes.** `LazySeqCell`, `ChunkedConsCell`, `ArrayChunkCell` and
  `ChunkBufferCell` are internal constructors on the public types'
  prototypes, kinds 16 to 19 (the keywords and symbols of light-cljs-1011
  are 20 and 21), set with `viewType`. Their own keyed fields
  are upstream's: a lazy seq's `fn` (null once realized), `s` and `hash`; a chunked cons's `chunk`, `more` and `hash`; a
  chunk's `arr`, `off` and `end`, and a range's integer chunk also its
  `first` item and `step` (null in an array chunk); a chunk buffer's `buf`
  and `end`. Each kind has a test that also requires its first own field
  to be present (`lazyOf`, `chunkedConsOf`, `chunkOf`, `chunkBufferOf`;
  present, as a field may hold undefined), so a value made by reflection
  from the prototype is none of them. `lazyValues`, `chunks`
  and `chunkBuffers` are deleted, and a chunked cons is no longer a cell
  (`cells`): every reader takes the test, and the store inventories take
  `lazyCells` and `chunkedConsCells`. `lazyStep` replaces `deferredValue`
  for lazy seqs (delays keep it). Metadata stays in `metadataValues`.
  The chunked cons's prototype holds no kind table number (`kindOps`): its
  operations are its slots, and the old layers' fallbacks read it through
  `chunkedConsOf`. The seq classes' shared base and the chunk prototypes
  have a `toJSON` that leaves the keyed fields out.
- **Slots.** A lazy seq has keyed `-seq`, `-first`, `-rest` and `-next`
  slots, which realize it through `seq` (so an `extend-type` of
  `ISeqable` answers as before) and take the operation of what it
  realized; a chunked cons `-seq`, `-first`, `-rest`, `-next` (the seq of
  its rest, through the wrappers) and the chunk protocols; a chunk
  `-count`, `-nth` and `-drop-first`; a vector's chunked seq the chunk
  protocols. The chunk operations the lazy pipeline calls (`chunkFirst`,
  `chunkRest`, `chunkNext` and `chunkDropFirst`) read a value's slot
  first, as the slot-first wrappers do; the core dispatchers
  (`coreMethod`) stay as they are.
- **The Pants build.** The shared-heap profile's `lazyOf` and
  `chunkedConsOf` answer the engine's lazy seqs and chunked conses with a
  view of the same keyed fields, its `cells` no longer holds chunked
  conses, its `delays` is a unit of its own, and its `stamped` accepts the
  engine's lazy seqs and conses, which the form bridge reads.

What light-cljs-1026 landed: the vector classes hold their data themselves,
replacing 1009's one `store$LKEY` (`state$LKEY`) field.

- **A vector is its own store.** `VectorCell` has the own keyed fields
  `meta`, `cnt`, `shift`, `root`, `tail` and `hash` (null until computed),
  in upstream's order, and `callerTail` only where the public constructor
  gave the tail. `vectorStore` returns the value itself (a value of kinds 1
  to 3 with a `cnt` field), and the store algorithms read and build these
  fields: `vectorTrie`, `vectorConj`, `vectorAssoc` and `vectorPop` return a
  new `VectorCell`, so `conj` and `assoc` allocate the vector and its new
  nodes and no store object. A map entry (`EntryCell(key, val)`) has the
  same fields, its metadata always nil.
- **Views are values too.** The `VectorView` class is gone: a subvector is
  a `SubvecCell` with `meta`, `cnt`, `sub` (the vector it views, upstream's
  `v`), `begin` (upstream's `start`, a name the tests' `rt$` scans would
  take for a runtime reference), `built` (the trie of its items, built once by `viewTrie`
  for the operations that read a trie's fields) and `hash`; `vectorView`
  builds one. `assoc` and `pop` of a view give a view of kind 1, a
  `VectorViewCell` on `PersistentVectorValue`'s prototype, as they gave a
  vector holding a view before. A subvector of items a protocol
  implementation answers, or the public constructor's subvector of
  getters, is a trie with the subvector prototype (`subvecOfTrie`).
- **Transient vectors** are their own state: `TransientVectorCell` has
  `cnt`, `shift`, `root`, `tail`, `owned` (the nodes it may edit) and
  `editable`; `transientState` returns it, and the transient operations tell
  it from a map or set transient's state by `state === value`. The map and
  set transients keep the `transients` store until light-cljs-1012.
- **Metadata and hashes.** `ownMeta` tells a value that holds them in its
  own fields (these classes, never an engine form of the Pants profile);
  `meta`, `preserveMeta`, `conjMeta`, `collectionWithMeta` (through
  `setMeta`), `hash`, `hashed`, the sequential hash and the public
  `__hash` field read and write the fields for such a value and the
  `metadataValues` and `constructorHashes` stores for the rest. The
  compiler form bridge reads a vector's field behind `stamped`.
- **Enumeration.** `Object.keys` of these values lists the fields above
  (`runtime_value_enumeration_contract`); their `toJSON` drops every keyed
  field (`unkeyedJSON`).
- **Profiles.** The Pants profile's engine vector stores have keyed fields
  too (`cnt`, `form`, a `tail` getter); `with-meta` of an engine vector is a
  `VectorViewCell` of the whole form's store, which `sharedVectorForm` gives
  back as the form; its swapped storage units take views as the V8
  profile's do. The emitter's `count` of a known vector reads `cnt$LKEY`.

What light-cljs-1030 landed: the lists, conses, ranges, repeats,
iterates and cycles, the key and value seqs and a sorted map's seq, which
were the last values of the `cells` store.

- **The classes.** Each is one object of an internal constructor on its
  type's prototype: `ListCell` (kind 12), `EmptyListCell` (13), `ConsCell`
  (14), `IntegerRangeCell` (15), `RangeCell` (22), `RepeatCell` (23),
  `IterateCell` (24), `CycleCell` (25), `KeySeqCell` (26, the key and value
  seqs, on a prototype of no public type, as before) and `ReSeqCell` (27,
  `re-seq`'s conses, likewise); a sorted map's seq from its public
  constructor is a `TreeMapStackCell` on the prototype and kind 11 of its
  views, told from them by its fields. Kinds 12 to 15 were the ones left
  for the seq kinds; the rest follow the keywords' and symbols' 20 and 21.
  Their own keyed fields are upstream's where the data are the same (a
  list's `first`, `rest` and `count`, a range's `first` item, `end` and
  `step` and an integer range's `count`, an iterate's `f`, `prev`, `seed`
  and `next`, a cycle's `all`, `prev`, `current` and `next`, a repeat's
  `next`), plus `cell$LKEY`, today's kind of seq ("list", "cons", "range",
  "repeat", "iterate", "cycle", "keys", "vals", "tree-seq"), as the views
  keep `view$LKEY`. Where a step computes `first` or `rest` (a range's
  next range, a repeat's, iterate's and cycle's next step, a key seq's key,
  a sorted map's seq's node), it is a getter of the prototype under the
  same keyed name, so every reader reads `first$LKEY` and `rest$LKEY`
  alike. `cellOf` (the value itself where its keyed kind and its own `cell`
  field are there) replaces `cells.get`, and `seqCells` stands for the
  store in the inventories; the `cells` store is deleted, and with it a
  cell's frozen payload, its closures and `Object.create` per step. A
  cell's metadata copy and `clone` copy its own keyed fields
  (`cellCopy`), not a program's slots on the value. Metadata and hashes
  stay in `metadataValues` and `constructorHashes`; the cells' kind table
  number (`kindOps` 2) stays until light-cljs-1015.
- **Slots.** Lists, the empty list, conses, ranges, repeats, iterates,
  cycles, key seqs, `re-seq`'s conses and sorted map seqs have keyed `-first`, `-rest`,
  `-next` and `-seq` slots, and lists, the empty list and integer ranges
  `-count`, which the slot-first wrappers serve. A list's `next` is its
  rest and a repeat's its next step, as `next` takes them; every other
  cell's `next` is the seq of its rest, through the wrappers, so an
  `extend-type` of `ISeq` answers as before. A sorted map's seq shares its
  prototype with its views, so its `-first`, `-rest`, `-next` and `-seq`
  slots answer both (the core dispatchers, `coreMethod`, call a slot for
  either); it has no `-count` slot yet. `RSeq` still copies its items.
- **Copies.** The shared-heap profile's `cellFields` unit also copies an
  engine list or cons, whose `cellOf` is a view of its fields, as a runtime
  list or cons; a seq's `toJSON` is `unkeyedJSON`.
- **The compile-time brand.** The cell constructors end with `/*@brand*/`;
  the compiler form bridge tests `stamped` before `isCell` and before it
  reads a repeat's kind, and `inspect`'s cell test carries `/*@stamped*/`.
  A seq's `toJSON` (the shared base's) now leaves out every keyed own
  field by its key, since a destructuring would run the cells' getters.
- **The Pants build.** The shared-heap profile's `cellOf` answers the
  engine's lists and conses (not its chunked ones) with a view of the same
  keyed fields, its `cons` builds a `ListCell` onto a runtime-owned tail,
  and its `stamped` accepts the engine's lists.

What light-cljs-1028 landed: the vector classes declare their protocols and
answer their operations from slots.

- **Masks and markers (section 3).** The prototypes of
  `PersistentVectorValue`, `SubvecValue`, `MapEntryType` and
  `TransientVectorType` hold the keyed masks
  (`cljs$lang$protocol_mask$partition0$$LKEY`, `...partition1$$LKEY`), as
  literals in each class's type unit, and the keyed marker of `IFind`, the
  one protocol of theirs without a mask bit. They declare exactly what the
  native tests (`builtinCoreImplementation`) answer for their values, so
  `implements?` and `satisfies?` give the same answers: a vector IFn,
  ICounted, IEmptyableCollection, ICollection, IIndexed, ILookup,
  IAssociative, IStack, IVector, IMeta, IWithMeta, IReduce, IKVReduce,
  IEquiv, IHash, ISeqable, ISequential, IReversible, IPrintWithWriter,
  IEditableCollection, IComparable, ICloneable, IIterable, IDrop and IFind;
  a subvector the same but IEditableCollection; a map entry the same but
  IKVReduce, IEditableCollection and ICloneable, and with IMapEntry; a
  transient vector IFn, ICounted, IIndexed, ILookup, ITransientCollection,
  ITransientAssociative and ITransientVector
  (`vector_family_prototype_contract` in tests/support/js_runtime.test.js
  pins them). The other converted kinds carry none yet.
- **Slots.** A subvector and a map entry have the keyed `-nth`, `-count`,
  `-peek`, `-conj`, `-assoc`, `-lookup` and `-seq` slots a vector has, and a
  transient vector `-nth`, `-count` and `-lookup`, each a unit that is an
  effect of its class and waits for its operation (`when: nth`, ...), and
  the slot-first wrappers name the classes among their targets, so a
  program that builds entries and no vector keeps the entries' slots and
  the wrappers. A map entry's slots answer from its tail at indexes 0 and 1
  only, as `nth.entries` and `get.entries` did; a subvector its public
  constructor built (`SubvecValue`) answers `count`, `conj` and `assoc` by
  its bounds and the vector it views (`kindHooks.conjSubvec`, `assocSubvec`);
  a transient vector that is no longer editable throws, through
  `transientNth` and `editableTransient`. Each slot is complete for every
  value of its class, because the compile-time runtime's protocol lookup
  (build.rs `interpreterDispatch`) does not skip the runtime's own slots and
  calls them; only a value with the prototype and none of the class's
  fields, which reflection alone makes, answers `noBranch`.
- **The older layers.** A runtime class's mask bits declare what it
  implements natively, with no slot for most operations, so the protocol
  lookups that look for a program's method (`ownMethod`, `publicMethod`,
  `contains.own`) read them only for a value without a keyed kind, and see a
  runtime class's value as before (a marker, then the payload stores' step);
  `ownDeclared` answers such a value with no method (the operation's native
  path) instead of "No protocol method"; `get.own`, which calls the
  `-lookup` slot a declared `ILookup` mask names, takes the operation where
  that slot answers `noBranch`. The membership tests (`implementsProtocol`,
  `storeProtocol`, `ownProtocol`) read the masks, with the native tests'
  answers. When program types get keyed kinds (light-cljs-1013), these
  kind tests must tell them from the runtime's classes, whose masks alone
  are native.
- **Forgery (section 5).** `Object.create` of these prototypes now
  type-tests as their kinds; the forged vector, entry and transient rows of
  `value_kind_predicates_answer_as_the_brand_stores_did` and
  `protocol_hooks_answer_runtime_values_as_the_full_lookup` (items 1 and 3)
  and the transient vector's forged candidates in
  tests/support/js_runtime.test.js (items 19 and 20) are rescoped.

What light-cljs-1012 landed: array, hash and sorted maps and the map
transients are classes, each holding today's payload in an own keyed field,
as light-cljs-1009 did for vectors before light-cljs-1026.

- **The classes.** An array map is an `ArrayMapCell` (kind 28), a hash map
  a `HashMapCell` (29) and a sorted map a `TreeMapCell` (30), on their
  public types' prototypes (`viewType`). Their own keyed field `entries`
  holds what the `maps` store held, unfrozen: an array map's entries in
  order (`ArrayMapEntry`, or the public constructor's getters), a hash map's
  trie (`{root, nil, length}`), a sorted map's entries in its comparator's
  order; a sorted map also holds its comparator, `comp`. `mapOf` tells them
  by kind and `entries`; `makeMap` builds the class its type gives
  (`Type.prototype.cljs$lang$cell$$LKEY`); `with-meta` and `clone` copy a
  map through `mapCellCopy`. A map transient is a `TransientArrayMapCell`
  (31) or `TransientHashMapCell` (32) whose own field `state` holds the
  state the `transients` store held (`transientMapState`); the set
  transients keep that store until light-cljs-1013. Upstream's fields (the
  flat key, value array, a hash map's `cnt`, `root`, `hasNil` and `nilVal`)
  are light-cljs-1034's, and a sorted map as a red-black tree of entries
  light-cljs-1035's. Metadata and hashes stay in `metadataValues` and
  `constructorHashes`; a public constructor's slots in
  `publicCollectionSlots`.
- **The stores.** `maps` and `mapKinds` are no longer stores: they read a
  map's `entries` field and its class's kind for the call sites that read
  the stores, and `maps.records` adds records, whose entries an update
  gives them in `recordMaps` (a `kindedStore`) until they become classes in
  light-cljs-1013. `sortedComparators` is deleted: `comparatorOf` reads a
  sorted map's `comp`, and the sorted sets keep theirs in `setComparators`.
  The trie's private field (`trieBrand`) is the hash sets' alone.
- **Slots.** The three map prototypes have keyed `-lookup` (both arities),
  `-count`, `-assoc`, `-contains-key?` and `-conj` slots, and array and
  hash maps `-dissoc`; `dissoc` and `contains?` have slot-first wrappers
  (`dissoc.slots`, `contains.slots`), and the others' wrappers name the map
  types. An array map's `assoc` copies its unfrozen entries once
  (`arrayMapAssoc`) where it stays an array map, and its `dissoc` removes in
  one loop without a closure (`arrayMapDissoc`); a hash map's copies its
  trie's three fields (`hashMapAssoc`); `contains?` is `get` through its
  wrapper, so an `extend-type` of `ILookup` answers as before. Each slot is
  complete for every map of its class, as the compile-time protocol lookup
  calls it (light-cljs-1028): a public constructor's map takes `mapCount`,
  `mapAssocEntries` (with the promotion of `assocArrayMapSlots`) and
  `dissocEntries`, the operations' own paths; only a value made by
  reflection answers `noBranch`.
- **Profiles.** The map and map transient constructors end with
  `/*@brand*/`; the compiler form bridge tests `stamped` before `maps.has`,
  and `inspect`'s map traversal carries `/*@stamped*/`. The Pants profile's
  `maps` reads its engine maps' forms and the classes' fields, its
  `sharedOwnedMap` builds the V8 profile's classes, its `mapKinds` is
  `maps.kinds`, and its sorted set comparators are `setComparators`.
- **Records** become classes in light-cljs-1013, as section 2 says: a
  converted map's operations go slot-first and never reach `maps.has`'s
  record test.
- **Forgery (section 5).** A map's or map transient's own fields copied onto
  its prototype make one: the `copied-map` row of
  `value_kind_predicates_answer_as_the_brand_stores_did` (item 1) and the
  maps' and map transient's copied candidates in
  tests/support/js_runtime.test.js (items 19 and 20) are rescoped, and an
  array map built in the `lightCljsWeakMapStores` runtime writes no WeakMap
  entry, where its store wrote two. A sorted map's prototype mask written by
  a program (item 23) is now ignored by the older lookups, as a vector's is
  since light-cljs-1028, so its `nth` walks its seq where it threw.

What light-cljs-1033 changed: `seq`'s slot-first wrapper sends the empty
list to the operation, as before light-cljs-1030 gave it slots. Every walk
ends there (a `rest` at the end gives the empty list, and `next` takes its
`seq`), and its slot was a fifth class at the slot read and call that each
step takes: once a program also walked lazy seqs (a vector's chunked seq, a
lazy seq, a chunked cons and the vector itself are the other four), they
went megamorphic, and lazy-walk was 10% and seq-walk-vec 18% slower timed
as a pair. The operation's protocol lookup finds a program's `-seq` of
`EmptyList`, so `extend-type` answers as before; `seq` of the empty list
costs the operation's path (about 27 ns against 11 through its slot, 36
before light-cljs-1030). The other wrappers keep the empty list's slots, as
no walk ends on them.

- **The wrappers' size.** V8's mid tier (Maglev) inlines a function of at
  most 100 bytes of bytecode, and every step of a walk calls these
  wrappers. The `seq` wrapper was 90 bytes; the empty-list test made it
  102, and seq-walk-vec alone was then 19% slower (the shape of the test
  did not matter, only the size). The wrapper takes the empty list and
  `noBranch` as parameters of the function that makes it, whose reads need
  no check for an uninitialized `const`, and is 94 bytes. An edit to a
  slot-first wrapper should keep it under 100 (`node
  --trace-turbo-inlining` prints each candidate's bytecode size), until
  light-cljs-1016 calls the slots from the call sites.

What light-cljs-1034 landed: array and hash maps hold upstream's fields,
replacing 1012's one `entries` field for them. Sorted maps keep it until
light-cljs-1035, and the map transients their `state` field (below).

- **An array map** (`ArrayMapCell`) has the own keyed fields `cnt` and
  `arr`, upstream's flat key, value array (`[k0, v0, k1, v1, ...]`), which
  nothing writes once the map is built: `assoc` copies it once
  (`arrayMapAssoc`), `dissoc` copies the pairs around each equal key
  (`arrayMapDissoc`), and the builders (`arrayMapFromArray`,
  `mapFromShape`) push their pairs, so that every such array is packed.
  `arrayIndex` searches it as `entryIndex` searches entries, reading a
  key's `fqn` before its kind test (upstream's `array-index-of-keyword?`):
  the array's mixed keys and values tell V8 nothing of a key's class. A
  public constructor's map holds the array and count it was given in its
  fields as in its slots, so `count` of an array map reads `cnt` alone.
- **A hash map** (`HashMapCell`) has `cnt`, `root`, `nil` (upstream's
  `has-nil?` and `nil-val` in one field: the nil entry, which keeps the
  first nil key, null or undefined) and `lit`, the builder's entries at
  their indexes or null, which advanced output's constant lookups read
  (`mapValueAt`, replacing `rt$maps.get(m)[i].value`). `get`, `assoc`,
  `dissoc` and `count` read and build the fields (`hashMapLookup`,
  `hashMapAssoc`, `hashMapDissoc`, `hashMapCount`): an `assoc` builds the
  map and the path of new nodes, and no trie object; leaf entries are no
  longer frozen, a trie's and a hash set's alike (`putHashEntry`,
  `hashNodeAssoc`, `shapeEntries`), since no reader writes them. The trie
  algorithms are unchanged.
- **The readers of entries.** `maps.get` still answers every map with
  entries for the call sites that read them (the older layers, public
  constructors' maps, transients, `updatedMap`): an array map's pairs as
  fresh `ArrayMapEntry`s (`arrayMapEntries`) and a hash map's fields as one
  trie per map (`hashMapTrie`, positional entries included), each behind a
  kind hook that only a program with that class keeps. `makeMap` builds
  either class from entries or a trie. The readers that walk every pair
  (`seq`, `reduce-kv`, map equality and hashing) read `mapPairs`: an array
  map's own `arr`, else its traversal's keys and values, kept per hash map.
  `with-meta` and `clone` copy the fields (`mapCellCopy`; the Pants
  profile's own also copies an engine map, a hash map's through `hashTrie`).
- **Left for later** (light-cljs-1036): the map transients'
  state as their own fields, a public hash map constructor's slots as
  fields, and removing `maps` and `mapKinds` where no caller needs them.
  Printing, `mapIterator` and the older layers read an array map's
  entries through `maps`, one object per pair.
- **Profiles.** The Pants profile's `sharedOwnedMap` and `maps` build and
  read the same fields; an array map's public constructor builds its class
  itself, so build.rs no longer swaps its allocation (`publicArrayMap`). Its
  own `arrayMapFromArray`, `mapFromShape` and `hashMapAssoc` build through
  `makeMap` and `updatedMap`, so that an ordinary map stays an engine map,
  and its `hashMapTries` and `traversalPairs` are stores that publish
  (`sharedStore`). `arrayIndex` reads a key through `keywordOf` where it has
  no `fqn` field, as the profile's engine keywords have none.

What light-cljs-1031 landed: a symbol's metadata in its own field.

- **The field.** `SymbolCell` has a fifth own keyed field, `meta$LKEY`,
  after upstream's order (`ns`, `name`, `str`, `_hash`, `_meta`): null from
  `symbol`, the given value from the positional constructor (`SymbolValue`),
  the symbol's own from `kindHooks.symbolCopy`, which `collectionWithMeta`
  then sets. `ownMeta` answers a value of kind 21, so `meta`, `setMeta`,
  `preserveMeta` and the compiler form bridge (`decode` sets a decoded
  symbol's metadata with `setMeta`, `encode` reads the field) read and
  write the field; the public `_meta` accessor reads and writes it, and a
  symbol's `toJSON` leaves it out. `metadataValues` no longer holds a
  symbol's metadata, so a copy of the build reads the metadata another copy
  gave (the Sol review of light-cljs-1011, finding 1).
- **`ownMeta`'s size.** Where a program has protocol implementations, a
  vector's `conj` inlines `ownMeta` more than once (through `conjMeta`,
  `preserveMeta` and `meta`) with 9 bytes of V8's cumulative inlining budget
  (920 bytes) to spare; past them the vector's constructor is no longer
  inlined and vec-conj is 9% slower. Testing kind 21 and the `str` field
  made the function 71 bytes (55 with one comparison for kinds 1 to 3),
  and testing a vector's `cnt` field with `in` alone (47) also lost in more
  runs, so `ownMeta` tests kind 21 alone (45 bytes, under the 53 it had):
  a value made by reflection from a symbol's prototype reads an undefined
  field as its metadata, where it read nil (deliberate reflection, which
  the group gave up).
- **The Pants build.** `sharedSymbol`'s runtime-owned symbol cells take the
  field; engine symbol forms keep the engine's metadata (`formWithMeta`),
  which `ownMeta` does not answer.

What light-cljs-1036 landed: the map transients hold their own fields, a
public hash map its count and nil entry, and no reader takes an array or
hash map's entries through `maps`.

- **Map transients.** A `TransientArrayMapCell` (kind 31) has the own keyed
  fields `editable` and `arr`, upstream's flat key, value array, which
  `assoc!` and `dissoc!` edit in place (upstream's `len` is the array's
  length; `dissoc!` moves the last pair into the key's place); its ninth
  key builds a `TransientHashMapCell` of its pairs. A `TransientHashMapCell`
  (kind 32) has `edit` (the token of the nodes it owns, null once
  persisted, as upstream's) and a hash map's `cnt`, `root` and `nil`, the
  names `HashMapCell` uses for upstream's `count`, `has-nil?` and
  `nil-val`. A map transient is its own state (`transientState`,
  `isMapTransient`), as a transient vector; `persistent!` gives the map of
  its fields, sharing them. The set transients keep the `transients` store
  until light-cljs-1013. The kind hooks `transientMap`,
  `transientMapLookup`, `assocTransientMap` and `dissocTransientMap` hold
  the map transients' paths.
- **The public hash map constructor** builds a `HashMapCell` whose `cnt` is
  the count it is given and whose `nil` is its nil entry, so `count`,
  `assoc` and `dissoc` of it read and update the fields as any hash map's;
  its slots keep `root`, `has-nil?` and `nil-val` for its public fields
  alone. Its updates have no slots, so `.-nil-val` of one without a nil
  key reads `get`'s nil where the slot held undefined.
- **The readers of entries.** `maps.get` answers only a sorted map's
  entries (and a record's, `maps.records`); `arrayMapEntries` and
  `hashMapTrie` are no longer kind hooks, and `hashMapTrie` and its store
  are deleted. Printing and `mapIterator` walk `mapPairs` (printing only
  where no protocol implementation can see an array's seq, else
  `mapTraversal`'s entries); the older layers of `assoc`, `dissoc`,
  `count`, `get`, `lookup`, `find`, `hash` and `transient` call the array
  and hash maps' slot functions or read their fields (`isHashMap`, the
  kind hooks `assocArrayMap`, `assocHashMap`, `dissocArrayMap`,
  `dissocHashMap` and `hashMapLookup`); `mapTraversal` makes an array map's
  entries of its pairs and walks a hash map's trie once per map
  (`hashMapTraversal`). A public constructor's array map still updates
  through entries of its pairs (`mapAssocEntries`, `dissocEntries`).
- **Profiles.** The Pants profile persists a map transient through
  `makeMap`, so that it stays an engine map, reads a runtime-owned map's
  entries as the V8 profile's `mapTraversal` does, and builds a public
  hash map's update as a class with slots.

What light-cljs-1013 landed: the values of the payload stores but sorted
sets and records, and the values of the WeakMap stores that section 2 gives
1013 but vars, functions with metadata and string writers, are classes.

- **The classes.** Each holds what its store held, in own fields, and its
  type's prototype holds its kind (`kinds`): a hash or array set is a
  `HashSetCell` (kind 33) with `entries` (an array set's entries, a hash
  set's trie) and `skind` (what `setKinds` answers); a set transient a
  `TransientHashSetCell` (34) whose `state` field holds its state, as the
  map transients' did after light-cljs-1012; a reduced value a
  `ReducedCell` (35) with `val`; a queue a `QueueCell` (36) with `arr`, its
  items; an iterator seq an `IteratorSeqCell` (37) with `first`, `iter` and
  `tail`; a delay a `DelayCell` (40) with `state` (its thunk and value); a
  UUID a `UUIDCell` (42) with `uuid` and `hash`; an eduction an
  `EductionCell` (43) with its read-only `xform` and `coll`. `Atom` (38),
  `Volatile` (39) and `MultiFn` (41) were classes already and keep
  upstream's unkeyed public fields (`state`, `meta`, `validator`,
  `watches`, `dispatch_fn` and the rest); their prototypes now hold the
  kind and their constructors the brand marker, where each value was put in
  a WeakMap or WeakSet. Nothing in them is frozen.
- **The stores.** `transients`, `privateStore` and `trieBrand` are deleted.
  `queues`, `reducedValues`, `iteratorSeqs`, `atoms`, `volatiles`, `delays`,
  `eductions`, `uuids` and `multiFns` keep their names as kind tests (a
  value of the class with its first own field), as `maps` did, so their
  callers and the store inventories are unchanged; `uuidHashes` is gone (a
  UUID's hash is its field). `sets` answers a `HashSetCell` by its kind and
  a sorted set by the kinded store it keeps (`sets.store`), with
  `setComparators`, until light-cljs-1035's sorted maps settle what a
  sorted set holds. `with-meta` and `clone` build a set's or queue's copy
  by its class (`setQueueCopy`) and copy only real stores.
- **What is left** (light-cljs-1037 and light-cljs-1038, before
  light-cljs-1022): records, program types and `reify` (`TypeBrand`,
  `typeFields`, `recordTypes`, `reifyValues`, and the records' updates,
  `recordMaps`, the last user of `kindedStore` beside sorted sets) through
  the emitter, with the 1028 handoff below; and sorted sets, vars
  (`vars`), functions with metadata (`metaFunctions`) and string writers
  (`stringBufferWriters`). The "lite" types (`VectorLite`, `HashMapLite`,
  `ObjMap`, `SetLite`, `ArrayList`) are upstream names with no light-cljs
  value (empty constructors a protocol can be extended to), so they hold
  nothing to convert; `Loadable`, `TaggedLiteral` and `Namespace` hold their
  own fields already.
- **`swap!`.** An atom's `swap!` and `swap-vals!` call its function with
  the arguments as they came, or, past two, with the first two and an
  indexed seq of the others (upstream's `apply`), and build no seq of them
  all for `swapState` to walk; the Pants build's engine atoms take
  `swapState`.
- **Enumeration.** `Object.keys` of a set, set transient, queue, reduced
  value, delay or UUID lists its keyed fields, and its `toJSON` (the set,
  reduced, UUID and delay prototypes', the transient and sequence bases')
  leaves them out (`runtime_value_enumeration_contract`). An iterator seq
  and an eduction define their fields as not enumerable, so an iterator
  seq still prints as `#js {}` and both stringify as `{}`, as before.
- **Profiles.** The constructors end with `/*@brand*/`. The compiler form
  bridge tests `stamped` before a set's, queue's, UUID's, atom's or
  volatile's kind (a sorted set by its store first), and `inspect` carries
  `/*@stamped*/` on those tests and the reduced test. The Pants profile's
  `stamped` also accepts the engine's sets, reduced markers and atoms; it
  drops its own `uuids`, `uuidHashes` and `eductions` stores, which the V8
  profile's classes replace, and its `volatiles` is the V8 kind test.
- **Forgery (section 5).** The own fields of a set, a set transient, a
  queue, a reduced value or an iterator seq copied onto its prototype make
  one (items 19 and 20 of tests/support/js_runtime.test.js gain those
  candidates); `Object.create` of an atom or a volatile (a value, not a
  prototype) is one, so those two checks keep host objects of their shape.
  The QuickJS session test of the reduced store
  (`quickjs_sessions_brand_class_values_with_a_weak_set`) now counts the
  compile-time stamp's one `WeakSet` entry and no WeakMap entry. The
  Proxy of `protocol_hooks_answer_runtime_values_as_the_full_lookup` is
  asked the same names more often (795 reads at none, where 553), as each
  new kind test reads the keyed kind.
- **The 1028 handoff** (the older lookups ignore mask bits on a value with
  a keyed kind) stays as it is: program types did not get keyed kinds in
  this pellet, so `ownMethod`, `publicMethod` and `contains.own` still tell
  them by having none.

What light-cljs-1035 landed: a sorted map is upstream's red-black tree,
whose nodes are map entries.

- **The map.** A `TreeMapCell` (kind 30) has the own keyed fields `comp`
  (its comparator), `tree` (its root node, null when empty) and `cnt`,
  replacing 1012's sorted array of entries (`entries`). The public
  constructor builds one of the comparator, tree and count it is given,
  as they are: `.-tree` gives that tree back, `count` that count, and an
  `assoc` of a new key one more, as upstream's; it reads no node until an
  operation does. It keeps no `publicCollectionSlots` entry, so the
  slots' `noBranch` for public constructors' maps no longer applies to it.
- **The nodes.** A node is a `TreeNodeCell` on `MapEntryType`'s prototype
  (upstream's `BlackNode` and `RedNode`, which already resolved to
  `MapEntry`): `EntryCell`'s six fields, so every reader and slot of a map
  entry reads it as one, unchanged, then `left`, `right` and `red`. A node
  is never written: an update builds the nodes of its path, which share
  the `tail` array (the key and value) of a node whose key and value they
  keep, and every node shares one frozen empty `root`. The public fields
  `key` and `val` of every map entry (upstream's `MapEntry` fields) and
  `left` and `right` of a node are getters of the entry prototype
  (`MapEntryType.fields`, after `upstreamMembers` or `resetMeta`, as the
  other types' fields), whose writes give the value an own field, as
  `publicFields`' do; `key`, `left` and `right` join `UPSTREAM_MEMBERS`.
- **The algorithms** are upstream's: `assoc` (`treePut`, `tree-map-add` and
  `tree-map-replace` in one walk: an equal value keeps the map, a replaced
  value keeps the key there), `dissoc` (`treeRemove`, with upstream's
  `balance-left-del`, `balance-right-del` and `tree-map-append`), lookup
  and `find` (`treeEntryAt`, which gives the node itself), `reduce-kv`
  (`treeKvReduce`), and the comparator called as upstream's, the given key
  first. `sorted-map` and `sorted-map-by` `assoc` each pair in turn
  (`buildSortedMap`). A tree a public constructor was given of other
  objects is read by the names `key`, `val`, `left` and `right` (a map
  entry by its items), as upstream's; an update first rebuilds it of
  nodes (`ownTree`), balanced (`treeOfPairs`, the deepest level red), as
  `makeMap` builds a sorted map of entries (`treeOfEntries`: `empty`, a
  sorted set's `tree-map`, the Pants profile's `sharedOwnedMap`).
- **Seqs.** `seq`, `rseq` and `-sorted-seq-from` (`subseq`, `rsubseq`)
  are upstream's PersistentTreeMapSeq, a `TreeMapStackCell` of a list of
  nodes (`treeStackPush`, a `ListCell` per step, or `conj` onto a stack a
  program gave), whose `first` is a node and whose `rest` pushes the next
  subtree (`treeSeqNext`); `count` reads its `cnt`, or walks where it is
  below 0. A list stack's fields stand for its `peek`, `next` and `conj`
  only where no program can give lists protocol implementations
  (`protocolHooks.extend`, which now also waits for the seq's unit); else
  every stack takes the operations, as upstream's. The view class of kind 11 (`TreeMapSeqCell`) is deleted: no
  seq of a sorted map is an array any more, and the seq slots answer the
  cells alone. A public constructor's stack, ascending flag and count are
  the cell's fields, which its public fields read.
- **The older layers.** `maps.get` answers no map of the classes (only a
  record's entries); `assoc`, `dissoc`, `count`, `get`, `lookup`, `find`,
  `hash`, `empty`, `rseq`, `reduce-kv`, printing and `mapTraversal` /
  `mapPairs` read the fields or call the sorted map's kind hooks
  (`assocTreeMap`, `dissocTreeMap`, `treeMapLookup`, `treeEntryAt`,
  `treeKvReduce`, `treeMapPairs`, `treeMapEntries`, `treeOfEntries`), each
  an effect of `TreeMapCell` or `treeMapConstructor` that waits for its
  operation. A sorted map has a `-dissoc` slot (`dissoc.slots` names its
  type). Traversals that take every pair (printing, equality, hashing, a
  map's iterator) walk the tree in order, building the pairs or entries
  they read, where 1012 read the stored array.
- **Sorted sets** keep their entry arrays and `setComparators` until
  light-cljs-1013 (`sortedSeqFrom` keeps their array path); a sorted
  set's `tree-map` builds a sorted map of its members (`treeOfEntries`).
- **Profiles.** The Pants profile builds the same classes
  (`sharedOwnedMap`, `mapCellCopy`), and its `maps.get` answers only
  engine maps. Nodes are runtime-owned entries, so build.rs no longer
  swaps `publicTree`'s allocation (`sharedTreeEntry` is deleted).
- **Tests.** A sorted map's seq from `seq` now has the stack upstream's
  has (it was an array view, whose `.-stack` was nil), so
  `(count (PersistentTreeMapSeq. nil (.-stack s) true -1 nil))` of
  `(sorted-map 1 2 3 4)`'s seq counts both nodes, 2, as upstream's
  (`builtin_types.rs`, it gave 1).

What light-cljs-1037 landed: records, program types and `reify` values are
told by their prototypes' keyed kinds, and no type brands its instances in
release or dev output.

- **The kinds.** `decorateType` (`defineType` at `Advanced`, `makeType`
  below it and in dev builds) gives a type's prototype kind 64 (a
  `deftype`, and light.lazy's `Loadable`) or 65 (a `defrecord`), and a
  record's prototype its type under the keyed `record` name, which
  `recordTypeOf` reads. `reify` gives its site's type kind 66, `makeReify`
  builds a `ReifyCell` (kind 66, its methods its own properties), and the
  functions with metadata that `makeIFn` builds have kind 66 on
  `ifnPrototype`, as they were `reifyValues` before. The kinds start at 64
  so that the runtime's classes, below 64, keep a range of their own
  (light-cljs-1038 and later add some).
- **The stores.** `typeFields`, `recordTypes`, `reifyValues` and
  `recordMaps`, with `typeFields.brands`, `recordTypes.brands`,
  `maps.records` and `mapKinds.records`, are deleted: `recordTypeOf`,
  `recordCells`, `isTypeValue` and `reifyCells` are kind tests. A record is
  no map to `maps` or `mapKinds`; the native tests that answered it as one
  (`seqableValue`, `countedValue`, `collectionValue`, `lookupValue`,
  `associativeValue`, `kvReduceValue`, `mapValue`, `metaValue`,
  `withMetaValue` and `protocolStores`) answer it by its kind
  (`recordCells`), and those that excluded it (`ifnValue`, `findValue`,
  `editableValue`, `reduceValue`, `cloneablePayload`, `emptyableValue`,
  `ownMap`, `find`) still do by the same test, as the Pants profile's
  `maps` holds its engine record forms (the Sol review of this pellet), so
  `satisfies?`, `implements?` and the host tables answer as before. Where the record's own method answers first, as it
  does for every operation a program keeps, the paths that built a record
  of entries (`updatedRecord`, the record branch of `updatedMap` and
  `dissocEntries`) are deleted; `with-meta` of a record without its
  method builds a record of its class with its fields (`recordCopy`, as
  `recordMethods.withMeta`). `kindedStore` and `payloads` remain only for
  the sorted sets (light-cljs-1038).
- **The emitter** writes no brand (the `TypeBrands` choice, the brand
  readers of `runtime_slice` and the emissions that chose a brand are
  deleted): `defineType(function Type(a0, ...) { this.a = a0; ... }, ...)`
  at `Advanced`, chunked or not. `makeType`'s constructor ends with
  `/*@brand*/`, so the compile-time runtime stamps each instance, and the
  compiler form bridge and `inspect` test `stamped` before the kind.
  `TypeBrand`, `ClassBrand`, `ClassBrand.types` and `defineClass` are
  compile-time units no other unit requires, kept with their tests.
  Calling a constructor without `new` on an instance assigns its fields
  again, as upstream's does, where a second brand threw a `TypeError`
  (`records::constructors_called_without_new_only_assign_fields`).
- **The 1028 handoff.** `ownMethod`, `publicMethod`, `contains.own` and
  `ownDeclared` take mask bits as native only for a runtime class's kind,
  below 64 (`!(kind < 64)` reads the masks of a value without a kind and
  of a program type's alike), so a program type's mask bits still declare
  its methods. The slot-first wrappers take any keyed kind, so where a
  program keeps them they call a record's or type's slot (its method)
  first, as `get.own` did for a declared `ILookup`.
- **Profiles.** The Pants profile's `maps` answers its engine record forms
  (their entries, kind "record"), where `maps.records` did, and its
  `recordTypeOf`, `recordCells` and `isTypeValue` its engine record and
  struct forms;
  its `typeFields`, `recordTypes`, `reifyValues` and brand units are
  deleted.
- **Forgery (section 5).** Items 12 to 15 are rescoped in
  tests/support/js_runtime.test.js: `inspect` of `Object.create` of a
  record's prototype, alone or with a record's fields, is a record, and a
  record moved to `Object.prototype` is a host object (`inspect` gives
  null). The copy of a record's fields onto `{}` stays no record. Item 17
  (the runtime types' public field accessors) needed no change. The Proxy
  of `protocol_hooks_answer_runtime_values_as_the_full_lookup` is asked
  828 times (790 for the second extension set) at every level, where it
  was 794 (757) below `Advanced` and 838 (798) at it, as the record and
  `reify` tests read the kind and no class brand walks the prototypes.
What light-cljs-1038 landed: sorted sets, vars, functions with metadata and
string writers are classes, so no runtime value of section 2 keeps a store
of its own (after light-cljs-1037's records).

- **Sorted sets.** A sorted set is upstream's PersistentTreeSet, a
  `TreeSetCell` (kind 44, `treeSetOf`) whose own keyed field `treeMap` is
  its `tree-map`: a sorted map (`TreeMapCell`) whose keys are its members,
  each with a nil value. `sorted-set` and `sorted-set-by` build that map as
  `sorted-map` does (`buildSortedSet` on `buildSortedMap`); `conj` and
  `disj` `assoc` and `dissoc` it (the set itself where it does not change),
  `count` reads its count, lookup, `get`, `contains?` and invoking the set
  take the key of the node that holds the member (`treeEntryAt`), `empty`
  empties it and `subseq`/`rsubseq` map `key` over its
  `-sorted-seq-from`, as upstream's; the kind hooks `treeSetCount`,
  `treeSetLookup`, `treeSetSeq`, `treeSetComparator` and `treeSetMembers`
  are effects of `TreeSetCell`. The public constructor holds the map it is given as it
  is, undefined too, which `.-tree-map` gives back; `count` is the map's
  `count`, which its protocol implementations may answer; `comparatorOf` is its
  map's comparator, or `compare` where it was given no sorted map, as
  before. Its seq is upstream's, the keys of its tree map's seq (a key
  seq, `setSeqs`, where it was an indexed seq of an array of the members),
  so `first` and `rest` read one path of the tree. The readers that take
  every member (`rseq`, printing, hashing, equality, `inspect`) take
  `sets.get`'s fresh array of the members' entries in order, walked once
  per operation.
  `sets` is no store: `sets.store`, `setComparators`, `setTypes`,
  `setType`, `treeSetMaps` and `deferredEntries` are deleted, and `makeSet`
  builds hash and array sets only.
- **Vars** are `VarCell`s (kind 45, `vars`) on `Var`'s prototype: callable
  values (`makeIFn` takes the value to fill) whose own keyed fields are
  upstream's `val`, the function giving the root, and `sym`, kept as
  given; the fields of a var and of a function with metadata are not
  enumerable, so they stringify and enumerate as before. `with-meta` of a
  var builds a var of its fields (`makeVar`, `kindHooks.varCopy`), callable
  where another load of the build made it. **Functions with
  metadata** are `MetaFnCell`s (kind 46, `metaFunctions`), callable values
  whose own keyed field `afn` is the function they call, on a prototype of
  `ifnPrototype` with the kind, not enumerable. **String writers** are
  `StringBufferWriterCell`s (kind 47, `stringBufferWriters`) whose own
  field is upstream's public `sb`; `StringBufferWriter` builds one. Their
  metadata stays in `metadataValues`, as every class's does.
- **`payloads` and `kindedStore` are deleted**, the sorted sets their last
  user once light-cljs-1037 deleted `recordMaps`: no release output has
  them. No value has a private-field kind, so the inventories
  (`storesPredicate`, `payloadStore`, `kindStore`) probe their stores in
  turn without reading one; the private fields left are the compile-time
  stamp's (`stampBrand`).
- **Functions with metadata** had a `reify`'s kind (66) on `ifnPrototype`
  since light-cljs-1037; their class's prototype now holds their own,
  a runtime class's (46, below 64), so `reifyCells` no longer answers
  them; `ownDeclared` answers their natively implemented `IMeta` as a
  runtime class's, as it did as a `reify`'s.
- **Profiles.** Each constructor ends with `/*@brand*/`; the compiler form
  bridge tests `stamped` before a set's and a var's kind, and `inspect`
  carries `/*@stamped*/` on its var and set tests. The Pants profile's
  `sets` reads its engine sets and the classes (`setCellOf`, `treeSetOf`),
  its `sharedOwnedSet` builds a `HashSetCell`, as its `setQueueCopy` does
  of an engine set's members for `with-meta` and `clone`, its `vars` reads its engine
  Vars' fields as the keyed names, its `varPrototype` reads the fields,
  its `stamped` accepts engine Vars, and its `setComparators`,
  `treeSetMaps`, `metaFunctions` and `stringBufferWriters` stores are
  deleted.
- **Tests.** Under WeakMap stores a sorted set writes no entry
  (tests/support/js_runtime.test.js), whose enumeration contract gains it
  (`treeMap`); the Proxy of
  `protocol_hooks_answer_runtime_values_as_the_full_lookup` is asked the
  same names more often (877 reads at each level, where 828), one keyed kind read
  per new kind test.

What light-cljs-1022 changed: a program protocol call on a record whose
type inference cannot tell (the gate's `protocol-on-record`) runs the
method's own dispatcher (`Emitter::protocol_fast_path`), which now tests
its receiver as upstream's protocol functions do, `r0 != null && r0[m1] !=
null`, where it read `r0?.[m1] != null`. The record's layout was not the
cost: the chained read was about 0.5 to 0.7 ns of the call (3.47 to 2.93
ns, 1.44 to 1.16 times shadow-cljs's, on master; 1.32 to 1.07 with
light-cljs-1020's bit operators, whose loop is shadow-cljs's). Nothing
about the classes, the slots or `extend-type` changes.
benchmarks/runtime-vs-shadow.md has the measurements, and the call-site
form it measured and left out.

What light-cljs-1014 landed: each class compares and hashes its own values
(section 3, "Equality and hashing").

- **Slots.** Vectors, subvectors and map entries, array, hash and sorted
  maps, hash and sorted sets, lists, conses, lazy seqs, indexed and chunked
  seqs and chunked conses have keyed `-equiv` slots; they and keywords and
  symbols have keyed `-hash` slots. `=` (`equalPair.slots`) and `hash`
  (`hash.slots`) have slot-first wrappers (87 and 68 bytes of bytecode):
  after `=`'s identity test, a value of a runtime class (a keyed kind below
  64) is compared or hashed by its slot, which is the class's own or the
  method `extend-type` or `specify!` of `IEquiv` or `IHash` wrote, as the
  operations' protocol lookups found before; records and the program's
  types (kinds from 64) keep the operations' paths. `objectEquiv` (the host
  `equiv`) and the `-equiv` and `-hash` protocol functions are unchanged.
- **The comparisons.** A vector or a map entry against another, both
  holding their items in their own trie and tail (no view, no caller's
  tail), compares counts, then items index by index, the first unequal pair
  ending it (`classEquiv`), and hashes its items index by index into its
  `hash` field (`vectorHash`), where `=` and `hash` walked their seqs. An
  array, hash or sorted map against another compares counts, then looks
  each of its keys up in the other, an array map's keys with `arrayIndex`
  (`mapEquiv`), as `equivMap` does. Sets (`equivSet`, count and
  membership), seqs (`equivSequential`, walked with `first` and `next`) and
  every mixed comparison (a vector against a list, a map against a record)
  take the structural comparison (`structuralEqualPair`) directly, and maps,
  sets, seqs, keywords and symbols the structural hash, cached where `hash`
  cached it: a keyword's and a symbol's in their field, a map's and a set's
  in `constructorHashes`. `hash` is split: `hashStructural` is what it does
  without a program's `IHash` method, as `structuralEqualPair` is to
  `equalPair`. Cycles and iterates keep identity and have no slot.
- **Extensions.** The comparisons by fields read no program method, so they
  hold only while no `extend-type` or `specify!` has reached a runtime kind
  (`kindTables` emptied, the guard the kind table branches use); after one,
  every class's slot takes the structural comparison and hash, so a
  program's `ICounted`, `ISeqable`, `ISeq` or `ILookup` of a runtime type
  is read as before (`classes_compare_and_hash_as_the_generic_walk_did`).
  A session that demands declarations (`slotFirst` off) takes the walk too,
  whose operations demand the extensions they could reach. The Pants
  build's profile throws on such an extension, so it always compares by
  fields. An `extend-type` of `js/Object` with `IEquiv` or `IHash` no
  longer reaches these classes, whose own slots now outrank the inherited
  method, as upstream's own `-equiv` and `-hash` do; before, `=` and `hash`
  took it below `Advanced` and `=` did not at it (the Sol review of this
  pellet).
- **Complete slots.** The compile-time protocol lookup (build.rs) calls the
  runtime's own slots, so each answers every value of its class: a value
  made by reflection takes the structural comparison and hash, and
  `hashStructural` does not call a runtime slot it finds as a program's
  method (`builtinSlots`), which would ask it again. The `-equiv` slots
  answer identical values true, as the `-equiv` protocol function's
  fallback (`equalPair`) does, so a macro's `(-equiv v v)` of a vector
  holding NaN is true, as the program's; `objectEquiv` (`.equiv`) takes no
  runtime slot as an implementation, so its comparison keeps no identity
  test there either.
- **Primitives.** `hash` of a number, a string or another primitive reads
  its prototype's `IHash` mask bit and marker by name, as upstream's
  `implements?` does, before the protocol lookup's computed names (a keyed
  read of a primitive each, which was most of `hash`'s time for a number);
  a session that demands declarations takes the lookup.
- **Forgery (section 5).** A value made by reflection from a class's
  prototype now compares structurally through its class's slot in release
  too: where a program extended `IEquiv` to `default` or `object`, that
  method no longer answers `=` of it. Its hash is unchanged
  (`hashStructural` still asks the protocol lookup last).

What light-cljs-1016 landed: the slot-first core operations' call sites at
`:advanced` (section 3, "Core operations").

- **Call-site entries.** `count`, `nth` (two and three arguments), `get`
  (two and three, and `(:k m)` and `(:k m d)`, which are `get`), `first`,
  `rest`, `next`, `seq`, `peek`, `conj` and `dissoc` (two), `assoc`
  (three) and `contains?` call one small runtime unit each, `countSite`,
  `getSite`, `getSite3` and the others (`Emitter::site_call`):
  `rt$getSite(m, k)` for `(:k m)`, one call. Each entry is its operation's
  slot-first wrapper for its arity without the wrapper's other reads:
  `value != null && value.cljs$core$ICounted$_count$arity$1$<key> && (result
  = value.cljs$core$ICounted$_count$arity$1$<key>(value)) !== noBranch ?
  result : count(value)`, the keyed name dotted, the slot read twice to call
  it as upstream's protocol functions read it, a slot that holds no method
  (false, nil) or answers `noBranch` (a value made by reflection) taking
  the operation through its wrappers, whose order for nil, strings, arrays
  and host tables is unchanged; `nthSite` tests a number index first and
  `seqSite` sends the empty list to the operation, as their wrappers do.
  They are 47 to 75 bytes of bytecode, which V8 inlines into each site
  (the 100 of light-cljs-1033). A slot unit names its entries in its
  `when:`.
- **When.** The entries read no keyed kind, which the wrappers read first,
  so they are called only where every value that can hold a core protocol
  slot has a kind: the core protocols are intact (only the program's types
  extend them) and the program neither writes a core protocol member by
  name (`(set! (.-cljs$core$ILookup$_lookup$arity$2 o) f)`) nor spells one
  with the key's placeholder in raw text
  (`ProgramFacts::core_slots_runtime_only`). Elsewhere, and below
  `:advanced`, the operations are called as before. Which arguments each
  operation takes is the rule, never the program.
- **What was measured and left out.** The slot test written at each call
  site, `(x != null && x.slot != null ? x.slot(x) : rt$count(x))`, was the
  fastest form without `noBranch` (`count-small` 1.6 ns against the
  entry's 2.0), but kept no answer for a value made by reflection, and with
  `noBranch` it was no faster than the entry (2.8); it would also grow
  workload 200k's output by about half (its 16,000 calls of these
  operations, about 110 bytes each with the eight-character key). Reading
  the keyed kind in the entry, as the wrappers do, cost 1.3 ns of
  `count-small`'s 2. benchmarks/runtime-vs-shadow.md has the gate.
- **What is left.** `kw-small` and `get-small` gain little (under 1 ns of
  13 and 18): an array map's lookup of a keyword reads the key's `IEquiv`
  implementation through the protocol hooks once per lookup (`arrayIndex`,
  `protocolHooks.keyMethod`), about 4.5 ns, which upstream's
  `array-index-of-keyword?` does not (light-cljs-1041).

What light-cljs-1040 landed: keywords and symbols compare by their own
`-equiv` slots too.

- **Slots.** `KeywordValue`'s prototype has a keyed `-equiv` slot,
  `keywordEquiv`, and `SymbolValue`'s `symbolEquiv`: identical values are
  equal, else two keywords by kind and `fqn` and two symbols by kind and
  `str`, as `structuralEqualPair` compares them; a value made by reflection
  from the prototype, with no `fqn` (`str`) of its own, equals no other
  value, as that comparison answers it. They need no `kindTables` guard: they
  read no program method. `=` of two keywords that are not identical is now
  `=`'s wrapper and the slot, where it took the old protocol lookup
  (`builtin`, `keyMethod`, `externalMethod`) and the structural comparison;
  the lean multimethod tables, which compare the dispatch value with each
  method's by `=` (`isaPlain`), gain the most.
- **The keyword searches.** `arrayIndex`, `entryIndex` and `arrayMapDissoc`
  read a keyword key's program `IEquiv` method once per search through
  `keywordProgramEquiv`, which takes the class's own slot for none. Where no
  session demands declarations (`slotFirst`), it reads the key's slot itself
  and compares it with `keywordEquiv`, so the common search makes no lookup
  and no test of the runtime's own slots (`builtinSlots`, a WeakSet probe in
  the V8 profile and a linear search in the Pants build's); a getter on the
  slot still runs once per search. A session that demands declarations
  takes the lookup, which demands first, and drops the class's slot from its
  answer, as the compile-time lookup (build.rs `interpreterDispatch`)
  returns the runtime's own slots. A host type's method (`externalMethod`)
  never answers a keyword. The Pants build's keywords are engine forms,
  which carry no slot: their searches take the lookup as before.
- **Extensions.** `extend-type` of `IEquiv` on `Keyword` or `Symbol`
  replaces the slot and `specify!` shadows it, so `=` and the searches call
  the program's method as before (tests/fixtures/compiler_sequence_object_equiv.tsv).
  An `extend-type` of `js/Object` with `IEquiv` no longer reaches keywords
  and symbols, whose own slot now outranks the inherited method, as
  upstream's `Keyword` and `Symbol` do and as light-cljs-1014 made it for
  the other classes: `=` and `.equiv` of two keywords or symbols compare
  them, where they called the program's method; and an array map's lookup
  of a keyword at `Advanced`, which took that method for the key's and
  matched the first key (`(get {:x 1 :y 2} :y)` gave 1), finds the key.

What light-cljs-1041 landed: an array map's keyword search small enough for
V8 to inline, and `contains?` of an array map without `get`'s layers.

- **The search's size.** `arrayIndex` keeps the keyword search alone; nil,
  strings, numbers, other keys, and a keyword whose program gave it an
  `IEquiv` method (from the first key that is not identical), are searched
  by `arrayIndexOf`. `keywordProgramEquiv` keeps the slot's read, and its
  lookups (a host type's method, a session that demands declarations) are
  `keywordLookupEquiv`'s. Their bytecode went from 381 and 180 bytes to 246
  and 71. TurboFan inlines a call of several targets only while their
  bytecode, with what each target's own optimized code inlined, fits its
  budget (920 bytes, a fifth more reserved for the candidate):
  `getSite`'s call of the `-lookup` slot, which every `(:k m)` of a program
  shares, had seen an array map's, a hash map's and a record's in the gate,
  and the array map's `-lookup` with the search was 636 bytes, so the call
  stayed a call and a record's `-lookup` ran out of line (light-cljs-1040's
  `record-kw-field` +30%). It is now about 390, and the call inlines its
  three targets. Where a call has seen array maps but takes hash maps, the
  array map's now inlined search takes budget the hash map's lookup used to
  get (`kw-hash100` after the small-map operations alone: +10%).
- **`contains?`.** An array map's `-contains-key?` slot
  (`arrayMapContains`) searches its keys with `arrayIndex` where the map's
  three-argument `-lookup` slot is its class's own (`arrayMapGet`), the
  method `get` would call; otherwise, and where a session demands
  declarations, it calls `get` as `mapContains` does, so `specify!` and
  `extend-type` of `ILookup` answer as before. Hash and sorted maps keep
  `mapContains`. Every unit that installs a `-lookup` slot names
  `arrayMapContains` in its `when:`, which changes no slice (it requires
  `get`).
- Answers are unchanged, and a getter on a key's slot runs as often.

What light-cljs-1015 landed: the old dispatch layers are gone from the
operations that release and dev output run.

- **The kind tables.** `kindOps`, `kindTables`, the nine wrappers that read
  them (`seq.branches` to `assoc.branches`) and their branch units (`seq.cells`,
  `count.vectors`, `vectorFirst`, `seqBranch`, `lazyBranch` and the rest, 61
  units) are deleted, and no prototype holds a kind number. A value whose
  class has the operation's slot takes it from the slot-first wrapper or the
  call-site entry; every other value takes the operation's own path, which
  the branches only shortened; `count` keeps two of their shortcuts on its
  own path (a cons, a range of fractions and a lazy seq walk at once, and a
  queue counts its items before the other kind tests), as without them
  `count` of a cons took three times as long. `seq` of a sorted set
  gives its tree map's keys (`setSeqs`) on that path too, which only the
  branch gave, and the seq of its members once an extension would have
  emptied the tables (`fieldComparisons`), as before.
- **The protocol wrappers.** `behindProtocol` and the wrappers of `seq`,
  `first`, `rest`, `next` and `count` (`seq.protocol` and the others) are
  deleted: each operation looks a program's implementation up itself, first
  after its nil case (`protocolHooks.programMethod`, which `ownMethod` fills
  where only `installMethods` is kept), so the order of every lookup is
  unchanged and a call takes one function less.
- **The lookup where `extend-type` or `specify!` exist** is `extendedMethod`,
  the old `publicMethod`, which only the runtime's classes' values (a keyed
  kind below 64) take through the payload stores' probes (`payloadStore`): a
  host value no longer probes the eight stores, so the Proxy of
  `protocol_hooks_answer_runtime_values_as_the_full_lookup` is asked 842 and
  809 times where 877 and 837 (`count` asks for a lazy seq's kind early).
  `publicMethodInfo` is `protocolMethodInfo` and
  `publicDeclared` `extendedDeclared`. The compile-time runtime keeps its
  demanding lookup (build.rs `interpreterDispatch`), now assigned to
  `extendedMethod`.
- **The 1014 guard** (comparisons and hashes by fields only until an
  extension reaches a runtime class) is not dead: a program's `ICounted` or
  `ILookup` of a vector or map would answer differently from the fields. It
  reads `fieldComparisons.on`, which `extend-type` of `Object` or of a
  type that had a kind table number (`tabledKind`) and `specify!` of its
  value turn off, as they emptied the tables. The shared-heap profile
  seals it, so such an extension still throws there.
- **What is left** (light-cljs-1046): a runtime class's value that an
  operation without a slot gives the lookup (`count` of a set or queue,
  `reduce`, `meta`, printing) still probes the payload stores in turn
  (`payloadStore`), and the `satisfies` hooks' inventories
  (`storesPredicate`, `ownedProtocolValue`) are unchanged; `ownMethod`'s,
  `extendedMethod`'s and `contains.own`'s mask guard for runtime classes
  stays, as their mask bits declare native implementations without slots.
  No record prototype walk is left in release units (light-cljs-1037).
- **Size.** The readable runtime is 22,083 bytes smaller (674,075 to
  651,992 without the header lines); the runtime slices of the
  measured programs are in benchmarks/runtime-vs-shadow.md (light-cljs-1015).

What light-cljs-1046 landed: no protocol lookup probes the stores in turn.

- **One store test.** Every store of the inventories is a class's kind
  test, and names the keyed kinds of its classes (`classKinds`). A table
  by kind (`kindTable`, 128 dense entries, so that no index a host gives
  `Array.prototype` is read) holds each kind's store, and a test reads the
  value's keyed kind once and asks that one store (`storeHolds`), which
  tells a value of the class from one made by reflection as before. The
  lookup of `extend-type` programs (`extendedMethod`) and the `satisfies`
  and `implements` hooks take a runtime class's value (`runtimeKind`: a
  kind below 64 whose store holds it) in one step, the native test cached
  by kind for the five protocols whose answer is the same for every value
  of a class (`kinds`, where it was by store); `payloadStore`,
  `payloadStores` and `kindStore` are deleted. The inventories
  (`ownedProtocolValue`, which `satisfies` and printing read, and
  `seqableValue`, `collectionValue`, `metaValue`, `withMetaValue`) are one
  store test each. The answers do not change: `ownedStores` is
  `protocolStores` by kind, and uuids, eductions, iterator seqs, chunks,
  chunk buffers and string writers stay outside it, so `extend-type` of
  `object` or `default` still reaches them; the Proxy of
  `protocol_hooks_answer_runtime_values_as_the_full_lookup` is asked 340
  and 345 times, where 842 and 809. The Pants build's profile, whose
  engine forms have no kind, keeps probing (its `storesPredicate` and
  `ownedProtocolValue`), and the QuickJS reference's reordering of the
  probes (build.rs) is gone.
- **Slots.** A hash or array set has `-count`, `-seq`, `-conj` and
  `-lookup` slots, a queue `-count`, `-seq` and `-conj`, and a vector, a
  subvector and a map entry `-contains-key?` (`vectorContains`), each
  complete for every value of its class (a public constructor's count
  is its slots', as a subvector's), so those operations answer slot-first
  where an `extend-type` program took the lookup before their own paths:
  in a program with one `extend-type`, `count` of a set 185 to 10 ns,
  `conj` 589 to 56, `get` 138 to 12, `contains?` of a vector 93 to 5.
- **Mask guards.** `ownMethod` and `contains.own` no longer skip a runtime
  class's mask bits: `contains.own` finds the vectors' slot, and
  `ownMethod` leaves a natively implemented protocol without a slot to
  `ownDeclared`'s kind test, as before. `extendedMethod` keeps its guard,
  where the kind's step answers. (light-cljs-1056 gave `ownMethod` its
  guard back: see below.)
- **What is left.** `reduce` of a vector or a map still takes the lookup
  before its own path in an `extend-type` program (their `-reduce` slots
  must keep the walk of an extended `-seq`, light-cljs-1055), and `count`
  of a cons walks its seq with the `satisfies` hook per step (204 ns for
  two items there, 34 without extensions, as before light-cljs-1015's 40).

What light-cljs-1053 landed: the side stores' entries are the values' own
fields.

- **Metadata and cached hashes.** `metadataValues` and `constructorHashes`
  are deleted. A value's metadata is its own keyed field `meta` and its
  cached hash its field `hash`, as a vector's and a symbol's were. Only
  the classes that held them already, and the empty list's `hash`, write
  them in their constructor; every other class is given them on demand
  (`setMeta`, `holdHash`, a public constructor's `metaFields`), so a list,
  a cons, a seq, a map or a set that never takes metadata or a hash
  allocates no more than before. A field `setMeta` gives a value that
  lacks one is not enumerable, so metadata stays out of `Object.keys` and
  JSON, as it was in the store, and a var, a function with metadata, a
  `reify` value and a host literal of the compiler form bridge need nothing
  of their own. `ownMeta` is the field's presence. The Pants build's
  profile swaps `meta`, `setMeta`, `heldHash` and `holdHash`, which also
  answer its engine forms (`formMeta`, the task's hash cache) and leave a
  value the boot sealed without a cached hash.
- **A public constructor's slots** are its value's own field `slots`
  (`constructorSlots`), which the operations read where they read the
  store: the slots keep a constructor's answers (a queue's or a set's
  count as given, a subvector's bounds, a vector's `root` token and `tail`
  as given), so those branches stay. A list needs none: the empty list is
  kind 13, so a list of kind 12 with count 0 is a public constructor's.
- **Subvectors and root tokens.** A subvector's source is its field `v`
  (`subvecSources`), a vector's canonical root token its field `token`
  (`canonicalVectorRoots`), and a token holds the root it names in its own
  field `vroot`, which is not enumerable (`vectorRoots`): still an opaque
  object, never an upstream trie node.
- **What stays.** `hashRoots` and the member side of `hashTraversals` are
  keyed by a set's or a transient's member array or trie, which is no class
  value; they go with the hash set's trie. A hash map keeps its own
  traversal in its field `traversal`. The Proxy of
  `protocol_hooks_answer_runtime_values_as_the_full_lookup` is asked
  `slots` once more (341 and 346 times): `count` reads the field where it
  probed the store.
- **Size.** The re-frame apps' whole bundles shrink 60 to 161 bytes after
  gzip, not the 600 to 900 light-cljs-1047 expected: a keyed field read
  (`v.slots$<key>`) is longer than the minified store call it replaces
  (`Xa.get(v)`), and the public constructors' branches stay
  (benchmarks/re-frame-vs-shadow.md).

What light-cljs-1055 landed: `reduce` and `reduce-kv` of the vectors and
maps call slots before any protocol lookup.

- **Slots.** A vector, a subvector, an array map and a hash map have
  `-reduce`, one method in both arity slots that reads the arguments'
  count (upstream's PersistentHashMap has no IReduce, but `reduceable?` of
  a hash map was already true here); a vector, a subvector and the three
  maps have `-kv-reduce`, as upstream's. A map entry keeps none: upstream's
  has IReduce, but no program reduces one often enough to pay its bytes.
  `reduce.slots` and `reduceKv.slots` call them first, outside
  `reduce.protocol` and `reduceKv`'s own lookup, which find a program's
  method in the same slot. `reduce.slots` calls each arity's slot through
  a function of its own, so that it stays under the 100 bytes of
  light-cljs-1033 (96; with both arities inline it was 166);
  `reduceKv.slots` is 100. No call-site entry: a reduction's cost is its
  steps, and the wrapper costs a few nanoseconds once per reduction.
- **Complete, with the walk of an extended `-seq`.** `reduce` walks a
  vector's or a map's seq, so an `-seq` that `extend-type` or `specify!`
  gave it decides what is reduced. A vector's or subvector's `-reduce`
  compares the value's `-seq` slot with its class's own, read once as the
  slot loads (`own`), and a map's tests that it has none (no map class has
  one): only then does it walk the store (`vectorReduce`) or the pairs
  (`mapReduceSlot`, an entry of each pair of `mapPairs`, as the seq holds
  them, without the seq). A public constructor's array map (its `slots`
  field, which its copies and clones keep since light-cljs-1053) is walked
  by its array and count, as its iterator walks it (`iteratorArrayMap`),
  unless a program gave it `-iterator`, and its vector of more than 32
  items in its store, as `reduce` walks them. Otherwise, and for a public
  constructor's shorter vector (`callerTail`), a vector of up to 32 items
  whose tail holds more than its count (whose seq walks the whole tail),
  and where a compiler session demands declarations (`slotFirst`
  off: `reduce`'s path demands each protocol it reads), the slot takes
  `reduce`'s own path (`reduceOwn`, the function under `reduce.protocol`,
  so that a slot the compile-time lookup calls does not find itself
  again), which then takes no lookup but for a program's `-seq`. The
  `-kv-reduce` slots read what `reduceKv` reads (the items by count, all
  of a caller's array first; the pairs; a sorted map's tree) and never
  take its path, whose own lookup would find the slot again.
- **Measured** (benchmarks/runtime-vs-shadow.md, light-cljs-1055): the
  gate's `reduce-kv-small` 50 to 7 ns and `reduce-entries-small` 357 to 90
  (light/shadow 4.63 to 1.15), no operation over 3% slower; with one
  `extend-type`, `reduce` of a 3-item vector 132 to 20 ns and of an array
  map of 3 keys 263 to 46; workload 200k's steady state 49.2 to 46.1 ms
  per iteration; the runtime slices 3.8 to 4.3 KB larger, mostly the
  slots' keyed names.
- **What is left.** The seqs' `-reduce` (upstream's ChunkedSeq, the map
  seqs, LazySeq and the rest), a map entry's, and a sorted map's or set's
  `reduce` still take the lookup first in an `extend-type` program.

What light-cljs-1056 found: `ownMethod` tests a runtime class's kind before
its mask bit again.

- **The cause.** light-cljs-1046 let `ownMethod` (the lookup of a program
  that implements protocols without `extend-type` or `specify!`) take a
  runtime class's mask bit as a declaration. A vector's mask declares
  `IMeta`, and `conj` and `assoc` of a vector read its metadata with
  `meta` wherever `installMethods` is kept (`conjMeta`, and the `-assoc`
  slot's `preserveMeta`). So each `conj` and `assoc` went on to
  `keyMethod` (`keySlot` and a megamorphic read of the absent `-meta`
  slot) and `ownDeclared` (a `reifyCells` WeakMap probe) to answer
  nothing, where the kind test had sent it to `keyMarker`'s one read. In
  readable profiles of the gate's `vec-conj-64`, `keyMethod`, `keySlot` and
  `ownDeclared` took 11% of the samples on master and none before
  light-cljs-1046 or after this fix. V8 inlined the path both ways
  (`--trace-turbo-inlining`): the loss was the extra work, not a lost
  inlining, and no unit on the operations' own paths had changed.
- **The fix.** `ownMethod` skips a runtime class's mask bits again (`!(kind
  < 64)`), so only a marker declares a program's method for its value, as
  the 1028 handoff above has it. Without `extend-type` or `specify!`, which
  install `extendedMethod`, only a member the program writes by name
  (`(set! (.-cljs$core$IMeta$_meta$arity$1 v) f)`, or raw text spelling
  one) puts a program's method in a runtime class's slot, and such a
  program now keeps `extendedMethod` at `:advanced`: the emitter roots
  `extendType` at the first such write (`ProgramFacts::core_method_write`,
  a method's name, holding `$arity$`; a marker written by name declares
  nothing). So `meta` of that vector answers the written method at every
  level, as light-cljs-1046 had made it, where `685f86dd`'s `:advanced`
  answered the vector's own metadata
  (`runtime_meta_slot_overrides_without_markers`). The runtime's own
  `installMethods` types keep their markers. `contains.own` keeps
  light-cljs-1046's reading of the mask bits, which finds the vectors'
  `-contains-key?` slot.
- **Measured** (benchmarks/runtime-vs-shadow.md, light-cljs-1056): the
  gate's `vec-conj-64` 2,404 to 2,051 ns, `vec-assoc-1024` 71.3 to 66.1 and
  `group-by-100` 10,204 to 9,868, all three below `685f86dd` (before
  light-cljs-1046) in an alternated recheck; no operation over 2% slower.
- **What is left.** `conj` and `assoc` of a vector still read its metadata
  through `meta`'s lookup (the mask, kind and marker reads, about a tenth
  of `vec-conj-64`'s samples) in every program that implements a protocol,
  though only `extend-type`, `specify!` or a method written by name could
  give a vector its own `IMeta` (light-cljs-1060).

What light-cljs-1057 landed: a hash map's trie compares a keyword key by its
spelling, and its walk takes about 9 ns less.

- **The key test.** `hashNodeFind` and `hashNodeAssoc` compare a key with a
  leaf's by `hashKeyTest`, upstream's `key-test`: a keyword by identity,
  then with a keyword by kind and `fqn` (`keyword-identical?`), its
  spelling read first as in `arrayIndex`, with no call of its `-equiv`
  slot, where `equalPair`'s wrapper called it through a call that has seen
  every class's slot, which V8 does not inline. The key's program `IEquiv`
  method is read once per comparison through `keywordProgramEquiv`, as
  `equalPair` read the slot once per comparison, and called where there is
  one (`extend-type`, `specify!`), so the program still decides which keys
  are equal, a getter on the slot runs as often, and a session that demands
  declarations takes the lookup (light-cljs-1040's rule). Any other key
  compares by `equalPair` alone: the Pants build's profile makes its
  identity test after reading a boxed scalar, so that an occurrence of NaN
  equals nothing, itself included, which an identity test before it broke
  (`shared_profile_nan_occurrence_keys_use_canonical_equality`). A
  collision node compares each entry so.
- **A slot written by name.** `keywordProgramEquiv` reads the key's slot
  without the protocol lookup (`protocolHooks.builtin`), which only its
  lookups need, as `equalPair`'s wrapper reads it: a program that writes a
  keyword's `-equiv` slot by its member name (`(set!
  (.-cljs$core$IEquiv$_equiv$arity$2 k) f)`) keeps no lookup at
  `Advanced`, where an array map's keyword search had compared the
  spelling since light-cljs-1040 (the Sol review of light-cljs-1057).
  `=` with a keyword constant is compiled at `Advanced` to an identity and
  spelling test (`keywordFqn`), which reads no slot, as before.
- **The walk.** `hashNodeFind` tells a collision node by its missing
  `slots` and an entry by its missing `kind`, and its loop tests no node:
  `node.kind === "collision"`, `!slot?.kind` and `node` in the loop's
  condition each cost a lookup 1.5 to 3.5 ns in the gate's readable build
  (`kw-hash100` and `get-hash100` with an identity test before the key
  test: 20.8 and 17.5 ns; with the slot's tests changed, 17.4 and 14.4;
  the collision node's too, 15.9 and 13.4; and no test in the loop's
  condition, 14.3 and 11.0; with the identity test in the key test's
  keyword path only, as landed, 14.4 and 13.0). Every caller tests the
  root first.
  `hashNodeFind` is 105 bytes of bytecode (131 before), `hashKeyTest` 130;
  TurboFan inlines both, with `keywordOf` and `keywordProgramEquiv`, into
  the benchmark's loop. Comparing the kind first made `hashKeyTest` 80
  bytes but `kw-hash100` 1.3 ns slower.
- **Measured** (benchmarks/runtime-vs-shadow.md, light-cljs-1057): in the
  gate `kw-hash100` 22.2 to 15.8 ns (light/shadow 1.59 to 1.17),
  `get-hash100` 20.0 to 11.4 (1.63 to 0.94), `update-hash100` -9%,
  `dissoc-hash100` -4%, no operation over 1% slower; workload 200k's steady
  state unchanged (43.6 to 43.3 ms per iteration); 267 bytes more at
  advanced where a program keeps hash maps (`keywordProgramEquiv`).
- **What is left.** `kw-hash100`'s remaining 2.3 ns over shadow-cljs's is
  before the walk: in the readable build, with the walk returning at once,
  `getSite`, the hash map's `-lookup` slot, `hashMapLookup`'s field tests
  and `hash`'s wrapper take about 9 ns, where shadow-cljs's whole lookup
  takes 13. A process whose `getSite` has seen array and hash maps can
  inline differently than before (the hash map's target is about 100 bytes
  larger): `destructure-map-3` behind the small-map and hash map
  operations alone was 7% slower, and 4% faster in the whole gate.

What light-cljs-1060 landed: `conj` and `assoc` of a vector copy its
metadata field without `meta`'s lookup.

- **Why nothing else can answer.** Without `extend-type` or `specify!`,
  `meta`'s lookup is `ownMethod` (or none, without protocol
  implementations), which answers a value of a runtime class (a kind below
  64) only with a method in its `-meta` slot, beside its marker. Only
  `installMethods` through `extendType` or `extendInstance`, or a member the
  program writes by name, which keeps `extendType`
  (`ProgramFacts::core_method_write`), puts one there; reflection that
  computes a keyed name is the forgery the group gave up. So where
  `protocolHooks.extend` is absent, `meta` of a vector, a subvector, a map
  entry or a set is its own field.
- **The change.** `conjMeta` tests `protocolHooks.extend`, where it tested
  `protocolHooks.builtin`: with it, the metadata is read with `meta`
  (`preserveMeta`), as before; without, copied from the field, as upstream's
  `-conj` and `-assoc` copy `meta`. The `-assoc` slots of a vector, a
  subvector and a map entry and a set's `-conj` slot call it where they
  called `preserveMeta`; the vectors' `-conj` slots already did. Its unit is
  among `protocolHooks.extend`'s `when:` units, so a program that keeps
  `extendType` keeps the flag. Dev builds, the macro host, the REPL, the
  evaluator, the QuickJS reference and the Pants build's profile keep every
  unit, so they take `meta` as before. A value that `specify!` or a method
  written by name gives its own `IMeta` hands what that method answers to
  its `conj` and `assoc` at every level, as before
  (`vector_and_set_updates_copy_metadata_without_the_lookup`).
- **Measured** (benchmarks/runtime-vs-shadow.md, light-cljs-1060): the
  lookup is gone from `vec-conj-64`'s readable profile; in two gate passes
  `vec-conj-64` 2,012 and 2,036 to 1,322 and 1,313 ns (light/shadow 1.52
  and 1.57 to 1.03), `vec-assoc-1024` -15% and -18%, `group-by-100` -7% and
  -14%. `swap-assoc` (+16%) and `select-keys-3` (+8 to 10%), whose code is
  the same, are slower in the whole gate's process and faster without the
  operations before them: `preserveMeta`, `meta` and `ownMethod`, which
  vectors no longer reach, take other feedback there. Workload 200k
  unchanged.

What light-cljs-1059 landed: `reduce` of a hash or array set and of an
indexed seq calls a slot before any protocol lookup; the other seqs, a map
entry, a sorted map or set and a queue keep `reduce`'s path, as measured.

- **Slots.** A hash or array set (`PersistentHashSetValue`) and an indexed
  seq (`IndexedSeqValue`: rest arguments, a short vector's seq, a set's or a
  queue's seq, a public constructor's over the caller's array or string)
  have `-reduce`, one method in both arity slots as light-cljs-1055's,
  which `reduce.slots` calls (its 96 bytes of bytecode unchanged). Both
  walk their items with one function (`itemsReduce`) as `reduce` walks
  them: a set its members' keys in traversal order, which its seq holds,
  where `reduce` took `iterator`, a `SeqIter`, a fresh array of the keys
  with an indexed seq over it, and the `IIterable` lookups; an indexed seq
  its array from its offset, read as the walk reaches each item. Upstream's
  sets have no IReduce: `reduceable?` of a set stays false, since a slot is
  no mask bit, and the lookups (`keyMethod`) take no runtime slot for a
  program's method.
- **Complete.** A slot walks only where the value's `-seq` is its class's
  own and it has no `-iterator`, which `reduce` walks for a set and a
  queue's seq, and where the indexed seqs' `-first` and `-next`, which that
  iterator calls, are their class's own (each read as the slot loads). A
  set that `specify!` or `extend-type` gave one arity of `IReduce` takes
  the lookup for the other (`noBranch`), which throws as before, since a
  set does not implement it. Otherwise an `-seq`, an `-iterator`, an
  `-first`, an `-next` or an `IReduce` that `extend-type` or `specify!`
  gave, and a compiler session demanding declarations (`slotFirst`), take
  `reduce`'s own path (`reduceOwn`) or the program's method in the slot, as
  before; a value made by reflection answers `noBranch`. The answers are
  pinned at every level by
  `reduce_slots_keep_the_answers_of_extended_seqs_and_reductions`, and slot
  against path by tests/support/js_runtime.test.js.
- **Left out, as measured.** The other seqs (lists, conses, lazy seqs,
  chunked conses and seqs, ranges, repeats, iterates, cycles, re-seq's
  conses, the map and reversed seqs) were first given one shared slot
  that took `reduceOwn`. Since light-cljs-1046 their lookup is a few kind,
  mask and marker reads, which the slot's own call and field tests cost as
  well: with one `extend-type`, a 3-item list 111 to 104 ns, a cons 145 to
  129, a 3-item lazy seq 249 to 235, but a range of 10 90 to 98 and a repeat
  of 3 57 to 67 (alternated scratch runs, load 15-18), for 76 bytes of
  TodoMVC's gzip; without the seq slots the lists, conses and lazy seqs
  measured as fast as with them. A sorted map's tree walk (5 lookups to
  none, 78 bytes) and a queue's walk (13 to none) were left out as rarely
  reduced, a map entry as light-cljs-1055 left it, and a sorted set, whose
  seq depends on `fieldComparisons`. Their `reduce` keeps the lookup; a
  map seq's and a key seq's walks take lookups of their own, as the map
  seqs have no `-first` or `-next` slots.
- **Measured** (benchmarks/runtime-vs-shadow.md, light-cljs-1059):
  `reduce` of a 3-member set with one `extend-type` 251 to 21 ns, of 3 rest
  arguments 183 to 43, `into []` of a 3-member set 452 to 173; the gate's
  `merge-small` (rest arguments) -11% and no operation over 3% slower;
  workload 200k's steady state 45.3 to 44.9 ms per iteration; the re-frame
  apps' whole bundles 233 to 257 bytes larger after gzip (0.07 to 0.20%).

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
  per-build key (section 1). The value's own fields carry the key too. At
  `:advanced` each keyed name is written short, `$<key>` and a code (section 1,
  "Short names at `:advanced`").
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
the keyed name is the unkeyed one followed by `$<key>`. At `:advanced` the
output writes each one short, `$<key>` and a code (below, "Short names at
`:advanced`").

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
  give `:advanced` short keyed names from a per-build table (light-cljs-1048
  did, under "Short names at `:advanced`").

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
"light-cljs.runtime-keys")]`, a `Map`, the runtime's only global write,
which since light-cljs-1018 also holds each key's identity-hash table, under
"Two runtimes on one page"), and a load that finds its key registered with a different hash throws at once,
naming `:runtime-key`, before any value exists. The same output loaded
twice finds its own hash ("Decisions for the user" 1). The runtime text uses a placeholder suffix
(`$LKEY`, which `src/runtime_manifest.rs` checks appears only as a keyed
name's suffix), which embedding replaces; the raw `src/js_runtime.js` that
tests evaluate runs with the literal placeholder as its key. Compiled
namespace artifacts in the compile cache carry the emitter's placeholder for
the key and the output writer substitutes it, so the key is not part of a
namespace's cache key.

As light-cljs-1025 built it (`src/runtime_key.rs`): the emitter's
placeholder is the runtime's, `$LKEY`, and the hash's is `"$LHASH"`, one
string literal of the first runtime unit, `runtimeKey`, which every
`:advanced` slice keeps and which registers the key with the hash (it
registers nothing while its key is the placeholder, at compile time). A
string literal the emitter writes spells the `$` that starts a placeholder
`\u0024`, so no string of the program (a value, a keyword's or symbol's
name, a name the runtime looks up by its spelling) holds one and the
substitution never changes it; a property name the emitter writes in
brackets (`x["cljs$core$ISeq$$LKEY"]`) and the head of a `js/` path are
code and keep the placeholder where the name is keyed (the shape of a slot,
marker or mask, `analyzer::keyed_name`), and any other is written as a
value is. The output writer replaces a placeholder
only where it ends a name, in the output of `PreparedApplication`'s
`emit_*` (one key for all the chunks of a chunked build, derived from all
of them in plan order), moving the source map's columns by the four bytes
each key adds; size reports count runtime text as written, keyed
(`runtime_key::written_len`). Raw JavaScript (`js*`) is code too: a keyed name it spells
with the placeholder is keyed. The hash registered is 16 hexadecimal digits
of the same SHA-256 the key's eight base-36 digits come from. As
light-cljs-1018 added it, `:runtime-key` is `:compiler-options
{:runtime-key "KEY"}` in light-cljs.edn and `--runtime-key KEY` on the
command line (`ApplicationOptions::runtime_key`, which the output writer
passes to `RuntimeKey::release`), a release option that dev builds ignore
(docs/build-configuration.md, "As built (light-cljs-1018)").

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
keys are caught at load too. As light-cljs-1025 built it, the identity is
the build of light-cljs.edn (`compile ID`, `watch ID`; none for the flag
commands) and the entry namespace; the output writer of a dev build is
`dev_build::runtime_file` for
`runtime.js`, the namespace units it and a watch step write, the units the
dev server's `invoke` sends and the scripts of a connected REPL's forms
(`dev_repl`), each with the one dev key; the session's own runtime keeps
the placeholder.

As light-cljs-1045 changed it, the dev key is one per build: derived from
the whole runtime source and the build's identity (`dev_build::dev_key`,
`RuntimeKey::dev`), so it still depends on neither the sources nor an edit,
`compile ID` and `watch ID` of one build share it, and two different dev
builds, two entries or two builds of light-cljs.edn of one entry, get two
keys; the hash registered is of the same inputs. The dev session keeps its
build's key (`DevSession::page_key`, which the watch sets when it starts)
and writes it into every unit of a watch step, every script of a
connected REPL's form, and `index.js`; the dev server's `invoke` takes the
key of the mount's build. Every page global of a dev build ends in the key
too: its runtime, `__lightRuntime$KEY`; its definitions,
`__lightApplicationRoots$KEY` (the emitter's application registry, whose
name ends in the placeholder, which the compile-time runtime keeps); its
client, `__lightDev$KEY`; and `index.js`'s `__lightDevLoaded$KEY` and
`__lightDevHalted$KEY`. The unkeyed `__lightDevLoaded` and `__lightDev`
stay, as the promise and the client of the dev build that loaded last (the
one build of a page with one), and the client's `runtime` and `roots` are
its build's runtime and definitions. A restart of a watch that changes its
build's entry changes the key, and a page then needs a full reload.

### Short names at `:advanced`

As light-cljs-1048 built it, `:advanced` output without `--pseudo-names`
writes every keyed name short: `$`, the key, and a code of one or two
letters (`a` to `z`, `A` to `Z`, then two of them), the most used names
taking the shortest codes, ties in the stems' order.
`x.cljs$core$ICounted$_count$arity$1$<key>(x)` is written `x.$<key>y(x)`
and `v.cnt$<key>` is `v.$<key>a`. Every kind of keyed name has one: the
runtime classes' fields, the kind, cell and record names, the masks, the
markers and the slots, of the runtime's protocols and the program's alike.
The rules of this section hold:

- **The name carries the key.** A host object or another build never holds
  a name that starts with `$<key>` by accident.
- **Two copies of one build share them** ("Decisions for the user" 1). The
  table depends on the output's text alone, as the key does
  (`RuntimeKey::shortened`, in `src/runtime_key.rs`).
- **Every protocol name is keyed** ("Decisions for the user" 2), short or
  whole.
- **Dotted property access stays.** `$<key>a` is an identifier.

**The form.** light-cljs-1047 measured two forms on master's output, bundled
with React (benchmarks/re-frame-vs-shadow.md, "What-ifs on master's
output"). `$<key>a`, the key then a code, saved 109 to 161 bytes more after
gzip than `a$<key>` in each re-frame app, so it is the form. Where the
runtime tests a name by its key, it tests whether the name starts with
`$<key>`, as short names do, or ends with it, as the whole names dev builds
keep do (`unkeyedJSON`, which leaves the keyed fields out of a value's JSON).

**What the runtime names itself.** `protocolSlot` builds the slot and
marker names from a protocol's key and arity as the program runs. It names
the methods `defineType`, `reify`, `extend-type` and `specify!` install, a
protocol function's dispatch, and the protocol lookups. So the output
writer gives it, for the `"$LNAMES"` string literal of its unit
`protocolSlotCodes`, the codes of the protocol slots and markers the text
spells: ` stem code stem code `. `protocolSlot` looks its stem up there
first. A slot or marker the text never spells keeps its whole keyed name,
which only `protocolSlot` writes and reads, so the two agree. The writer
lists every slot and marker the text spells, whether or not the runtime
will name it, because which keys reach `protocolSlot` is known only as the
program runs. The mask and field names the runtime built by concatenation
are written whole instead (`protocolMasks`, `defineType`, `typeMasks`,
`rangeField`, the keyword's upstream fields), so that each is renamed with
its uses.

**Measured.** The table holds 32 stems in simple and TodoMVC and 38 in
flow, about 380 bytes after gzip. The what-ifs left it out, and the runtime
they renamed could not have loaded. With the table, each whole bundle
shrinks 703 bytes after gzip in simple, 883 in TodoMVC and 1,001 in flow
(benchmarks/re-frame-vs-shadow.md, "2026-11-17: short keyed names at
`:advanced`"), against the what-if's 1,018, 1,188 and 1,462 for this form.

**A program protocol's slots from the text.** As light-cljs-1058 changed
it, `Advanced` output names a program protocol's slots where the runtime
needs them, and the table holds only the core protocols' slots and the
markers (`runtime_key::slot_stem`):

- a protocol function's dispatch: `protocolMethod` takes the slots of its
  arities as its last argument, `names`, and `protocolSlot` answers them
  for the key from then on (its `spelled`), so a method entry that names
  the dispatcher (`[g1, 2, function ...]`) reaches the same slot;
- a method entry of a type, a record, `reify`, `specify!` and an
  `extend-type` of a program type or a runtime type the program never
  assigns names its slot in place of its key (`["$<key>c", 2, function
  ...]`), which `installMethods` tells by the key's missing slash; an
  `extend-type` of a value that may be a host type's name as it runs
  (`js/Date`, a local, an assigned `PersistentVector`) keeps the key, which
  `extendType` reads there, and names the slot as the entry's fourth item;
  one of a host type's name (`number`) installs no slot.

Where `protocolSlot` still builds a name from a key, the table stays:

- **Core protocols' slots.** The runtime's own lookups (`methodKey` and
  `keySlot`, `coreMethod`, `ownMethod`, `extendedMethod`), its record
  methods (`recordMethods`) and the core entries of a program's types name
  them from the keys the runtime text spells, at the arity of each call. A
  name passed costs about 3 bytes after gzip (flow's 41 dispatchers' names
  cost 135), and these sites (each resolved key, each record method entry,
  each core entry of a program's type) outnumber the 25 stems, about 10
  bytes each in the table; a missed site would name in full a slot the
  text spells short.
- **Markers.** `directMethod` (`satisfies?`, `implements?`), `methodKey`
  and `protocolMethodInfo` name them from any protocol a program passes;
  two to four are spelled in an app.

Spelling the table's stems as the keys the text holds (`ICounted/-count/1`)
saved 30 to 44 bytes after gzip in the re-frame apps' output, but its
rewriting in `protocolSlot` cost about 170 bytes raw in every program that
keeps it, so the stems stay as they are. Measured in
benchmarks/re-frame-vs-shadow.md, "2026-11-18: a program protocol's slots
named by the text": the table falls to 25 stems in simple and TodoMVC and 26
in flow, about 265 bytes after gzip, and the whole bundles shrink a further
61, 122 and 108 bytes after gzip (the output as written 161, 129 and 200).

**What keeps its whole name.**

- **Not keyed, so not renamed:** every unkeyed name: `toJSON`, `toString`,
  `call`, the `IFn` invoke names, `Symbol.iterator`, and a program type's
  own fields.
- **Keyed, but kept whole:**
  - the key's own spelling, `"$<key>"`, which `protocolSlot` appends;
  - a dev build's page globals (`__lightDev$<key>`);
  - any keyed name whose stem has neither a field's shape nor a protocol
    name's.
- **Names with other characters:** a stem with a character outside ASCII,
  which a string escapes (`\u03c0`), keeps its whole name, so a member
  that spells it and the name `protocolSlot` builds agree.
- **Raw text that may reach a name by its spelling:** where the program's
  raw JavaScript (`js*`) spells a keyed name other than as a member,
  `x.name$LKEY` (in a string, or built from a stem and the placeholder),
  the whole output keeps whole names (`runtime_key::keyed_by_member`), so
  anything raw text can reach by a string it still reaches.
- **Spelled as a member in source:** member syntax and `js*` that spell a keyed
  name whole are keyed by the compiler (above), so they are renamed with
  it.

**Where names stay whole.** Dev builds, the REPL, the macro host, the
evaluator, the Pants build, the `:none` and `:prepare` levels, and
`--pseudo-names` keep the whole names, and `"$LNAMES"` stays the
placeholder there.

**Source maps and size reports.** A source map's segments after a
shortened name move by what it changed. `--size-report` counts each
runtime unit as written, short names and the table included
(`runtime_slice::shortened_bytes`).

### Two runtimes on one page

- **Two different builds** (including a dev and a release build of one app,
  and since light-cljs-1045 two dev builds): different keys. Neither
  recognises the other's values: to each, the
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

**The guarantee, as light-cljs-1018 tested it.** `tests/support/two_runtimes.mjs`
builds `tests/fixtures/two-runtimes` (an app, `two.app`, released as is,
with a `:runtime-key`, with another constant and as a dev build, and a
second app, `two.other`, and since light-cljs-1045 the dev builds of
`two.other` and of `:flavor`, a second build of `two.app`) and loads them
into one page, in Node
(`runtime_key::runtimes_on_one_page_keep_apart_unless_they_share_a_key`)
and in headless Chromium (`--chromium NPM_TOOLS`). Each runtime reports how
it reads every runtime's values, one of each kind (vector, subvector, array
and hash map, set, list, lazy seq, range, keyword, symbol, map entry, UUID,
record, `deftype`, atom, `reify` of a program protocol, `ex-info`), and its
host-value answers. What two runtimes on one page share and do not share:

- **Different builds never recognise each other's values**: two apps, one
  app and the same program with another `:runtime-key`, with another
  constant (a `goog-define`), a dev and a release build of one app, or two
  dev builds (of two apps, or two builds of one app). To
  each, the other's value is a host object: every predicate (`vector?`,
  `map?`, `keyword?`, `record?`, `satisfies?`) is false, `=` is false,
  `nth`, `count`, `name` and `key` throw and `get` answers nil as for `{}`,
  and it prints as `#object[...]`. What a host object does answer, it does:
  the other's seqs are host iterables (`Symbol.iterator` is not keyed), so
  `first`, `vec` and `reduce` iterate them as any iterable, and `str` of
  the other's UUID is its text (`toString`).
- **Host constructors**: an `extend-type js/Date` with a core protocol in
  one is invisible to the other (`count` of a date throws there,
  `satisfies?` is false), and two builds extending `js/Date` with the same
  program protocol, under the same name, each keep their own method, where
  upstream's later load would replace the earlier's.
- **A forced key collision** between two different outputs (one written
  with the other's key) makes the second load throw "Two light-cljs builds
  share the runtime key ...: give one a different :runtime-key" before any
  of its values exist; the first runtime and later different builds go on
  as before.
- **The same build loaded twice** shares the key: each copy recognises the
  other's collections, keywords, symbols, map entries, UUIDs, atoms and
  `reify` values, operates on them through the other copy's slots (an
  `extend-type` of its own `Box` in the first copy answers in the second,
  and a `reduce` or `reduce-kv` over the other's vector, map or range stops
  at its own `reduced`), and compares them equal with equal hashes, a
  collection holding a host object both copies hold included: the identity
  hash of a host object is one per key on the page (`identityHashes`, a
  `WeakMap` the key registry holds as `.identities[key]`, a compile-time
  load keeping its own), so a hash map of one copy
  keyed by its vector of the object finds the other copy's equal vector.
  Before light-cljs-1018 each load numbered host objects itself, and the
  two vectors hashed apart (found by the Sol review). State stays per copy:
  each has its own `defonce` atoms, keyword intern table (keywords equal, never
  identical) and extensions of its own types. The program's `deftype` and
  `defrecord` types are each copy's own, as a type defined again by a
  reload is: a record of the other copy answers `record?` and its fields
  but is not `=` to one of its own. An `extend-type` of a host constructor
  is shared by name, and its protocol markers are both copies': the marker
  value is one per key on the page (`protocolSentinel`,
  `Symbol.for("light-cljs.protocol" + key)`). Before light-cljs-1018 it
  was one per load, so the second copy's `extend-type js/Date` made the
  first copy's `satisfies?` false, and neither copy's `satisfies?` of a
  program protocol held for the other's `reify`. Not yet shared:
  `ExceptionInfo` is still recognised by `instanceof`, so `ex-data` of the
  other copy's ex-info is nil and it prints as a host error
  (light-cljs-1044).
- **A dev build next to a release build** works both ways round, and a dev
  unit evaluated again (a hot reload) with a release build on the page
  registers in the dev runtime, whose other namespaces keep their state.
  The release output writes no global but the key registry; a dev build
  keeps its own in page globals, each ending in its key
  (`__lightRuntime$KEY`, `__lightDev$KEY`, `__lightApplicationRoots$KEY`,
  ...).
- **Two dev builds on one page** are supported since light-cljs-1045, and
  tested in Node and headless Chromium: two dev builds of two apps, and two
  dev builds of one entry (two builds of light-cljs.edn), each load, run
  and hot-reload independently. Each has its own key, so neither
  recognises the other's values, and its own runtime, client and
  definitions, so a namespace both define (`two.shared`) keeps each
  build's `defonce` state, and a unit of one evaluated again, whether it
  loaded first or second, registers in its own runtime alone, the other
  keeping its state. Two dev builds loading at once keep their failures
  apart: `index.js` counts an error the page reports as its build's only
  while one of its own scripts is the document's `currentScript`, so a
  unit of one that throws halts that build alone (found by the Sol review;
  a scenario where the first of two builds fails while the second's loader
  is pending). Each build's client is its own (`__lightDev$KEY`),
  which reads the event stream of the dev server's mount that served it
  (`/<id>/events`) and keeps its own REPL sessions, and every unit, step
  and REPL form its build's session writes names its key, so it reaches
  that build's runtime alone: in
  `dev_server::two_dev_builds_in_one_realm_keep_their_hooks_and_repl_state`
  two clients of `compile app worker` run in one Node realm, each connected
  to its own mount, a step of each runs its own build's after-load hook of
  the same name, and each build's REPL session keeps its own references by
  number and history. The unkeyed `__lightDevLoaded`
  and `__lightDev` are the dev build that loaded last's. Two copies of one
  dev build (the same build of light-cljs.edn and entry, loaded twice)
  share its key, as two copies of release output do, and with it its dev
  globals: the second copy's `runtime.js` and `client.js` replace the
  first's, so a page loads a dev build once and reloads its units.
- **What they share** is the page: globals a program writes, the key
  registry with each key's identity-hash table, and the unkeyed members:
  `Object` protocol methods (`toString`), the `IFn` invoke entries and `Symbol.iterator`, which any
  caller may call.
- **Compile time**: two loads of the compile-time runtime in one engine
  share its fixed key, so each reads the other's values through their
  slots, but each stamps its values with its own brand, so neither's form
  bridge nor `inspect` takes the other's vector for a language value
  (`runtime_key::compile_time_runtime_loads_keep_their_own_brands`, in the
  evaluator's engine and in Node).

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
| sorted map | `PersistentTreeMap` (`treeMapConstructor`); `RedNode`, `BlackNode` as map entries | comp, tree, cnt, meta, hash; key, val, left, right, hash | `maps`, `sortedComparators` | 1012, tree 1035 |
| map transients | `TransientArrayMap`, `TransientHashMap` (`...Type`) | editable, len, arr; edit, root, count, hasNil, nilVal (as `HashMapCell`'s cnt, root, nil) | `transients` | 1012, fields 1036 |
| sets | `PersistentHashSet`, `PersistentTreeSet`, `TransientHashSet` | meta, the members as today (an array set's array, a hash set's trie), hash; meta, treeMap, hash; the transient's members | `sets` (kind 6), `setKinds` | 1013, sorted 1038 |
| queue | `PersistentQueue`, `PersistentQueueSeq` (`PersistentQueueValue`) | meta, count, front, rear, hash; meta, front, rear, hash | `queues` | 1013 |
| atom, volatile | `Atom`, `Volatile` | state, meta, validator, watches; state | `atoms`, `volatiles` | 1013 |
| reduced, delay | `Reduced`, `Delay` | val; f, value | `reducedValues`, `delays` | 1013 |
| var, multimethod | `Var`, `MultiFn` | val, sym, meta; name, dispatchFn, defaultDispatchVal, hierarchy, methodTable, preferTable, methodCache, cachedHierarchy | `vars`, `multiFns` | 1013, var 1038 |
| the rest | `UUID`, `TaggedLiteral`, `Namespace`, `Eduction`, `MetaFn`, `StringBufferWriter`, iterator seqs (`ES6IteratorSeq`), `Loadable` | uuid, hash; tag, form; obj, name; xform, coll; afn, meta; sb; value, iter, rest | `uuids`, `uuidHashes`, `eductions`, `metaFunctions`, `stringBufferWriters`, `iteratorSeqs` | 1013, `MetaFn` and `StringBufferWriter` 1038 |
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
(vectors by count and index, maps by count and lookup), and light-cljs-1040
keywords and symbols their `-equiv`; mixed comparisons keep their answers.

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
light-cljs-1015 no payload class, kind table, `behindProtocol`,
`publicMethod`, brand class or probe of the payload stores in turn is in
their units (light-cljs-1046 left one store test, `kindTable`).
Release is the gate.

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
- the bridge and `inspect` test `stamped(value)` first, which reads no
  property, and only then the keyed kind. (The compile-time printer, which
  this list also named, reads a host value's protocol markers and masks
  before it classifies it, so testing the stamp there would guard nothing.)

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
  slot-shaped members, the key derivation and the cache placeholder (the
  last two in light-cljs-1025). Program
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
  namespace, name and `fqn`, and its first `hash` computes its hash and
  caches it in its field, as upstream's `-hash` does. light-cljs-1011 wrote
  the hash the compiler computed into each constant, so that startup did no
  hashing; its digits barely compress (1.0, 1.5 and 9.2 KB of gzip in
  re-frame simple, TodoMVC and flow, benchmarks/re-frame-vs-shadow.md), and
  light-cljs-1050 measured no gate operation and no startup slower without
  them, so a constant is `rt$keyword(null, "a")` again;
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
