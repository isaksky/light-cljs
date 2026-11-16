// Original bounded runtime for emitted JavaScript. This file is a factory
// expression, embedded once in each compilation unit that needs owned values.
(() => {
  "use strict";

  //@unit runtimeKey
  // The build's key (docs/value-layout.md, section 1): every keyed name ends
  // in it, `$` and eight characters of [0-9a-z] that the output writer
  // writes for this text's placeholder (src/runtime_key.rs), with the hash
  // of the output (of a dev build's identity) for the hash's. As it loads,
  // before any value exists, the runtime registers the key with the hash in
  // the page's registry, its only global write, and stops when another
  // build registered the key with another hash. The compile-time runtime
  // and this file as tests evaluate it keep the placeholder as their key and
  // register nothing.
  const runtimeKey = ((key, hash) => {
    if (key.slice(1) === "LKEY") return key;
    const name = Symbol.for("light-cljs.runtime-keys"), keys = globalThis[name] || (globalThis[name] = new Map()), known = keys.get(key);
    if (known === undefined) keys.set(key, hash);
    else if (known !== hash) throw new Error(`Two light-cljs builds share the runtime key ${key.slice(1)}: give one a different :runtime-key`);
    return key;
  })("$LKEY", "$LHASH");
  //@unit weakMapStores
  // A host whose WeakMap entries cost less than private fields, as the
  // evaluator's reference-counting QuickJS, states so before the runtime
  // loads; a `kindedStore` is then a WeakMap, as
  // unforgeable a brand. The engine is never sniffed (light-cljs-292).
  // Where it is not, a store tests that a value can hold a private field
  // with `canHoldField`, which V8 inlines.
  const weakMapStores = globalThis.lightCljsWeakMapStores === true,
    canHoldField = value => typeof value === "object" ? value !== null : typeof value === "function";
  //@unit payloads requires: canHoldField
  // The private-field brand of the runtime's collections and seqs: `#s` a
  // value's payload, `#k` its kind, the number of the store holding it
  // (`kindedStore`: a sorted set's and a record's), and `#m` a map's or set's
  // kind. One read of `#k` (`kind`) names the store, where kind tests
  // probed each store's field in turn; no host object can have or copy
  // them (light-cljs-990). A value is in one store, so `set` requires a new
  // object. `kindBits` are a store's kinds; `exact` that its `has` is the
  // kind test alone (hash tries and records widen the maps' and sets').
  // Kinds 1 to 7 are `payloadStore`'s (3, the vectors', is no longer a
  // payload: `vectorStore`), 9 to 12 the rest.
  const payloads = class extends (function (value) { return value; }) {
    #s; #k; #m;
    static kind(value) { return canHoldField(value) && #k in value ? value.#k : 0; }
    // A store of `kind` with a WeakMap's methods; `reset` replaces a payload
    // (light-cljs-237), and a kinded store's `kinds` keep `#m`. The stores
    // share this code and so V8's feedback, which seq operations do not feel.
    static store(kind, kinded) {
      return class {
        static kindBits = 1 << kind;
        static exact = !kinded;
        static has(value) { return canHoldField(value) && #k in value && value.#k === kind; }
        static get(value) { if (canHoldField(value) && #k in value && value.#k === kind) return value.#s; }
        static set(value, payload) { const brand = new payloads(value); brand.#k = kind; brand.#s = payload; }
        static reset(value, payload) { value.#s = payload; }
        static kinds = kinded && {
          has: value => canHoldField(value) && #k in value && value.#k === kind && value.#m !== undefined,
          get(value) { if (canHoldField(value) && #k in value && value.#k === kind) return value.#m; },
          set(value, mapKind) { value.#m = mapKind; return this; }
        };
      };
    }
  };
  //@unit keywordOf
  // A keyword, the class of kind 20 (`kinds`, `KeywordCell`): the value
  // itself, whose own keyed fields are upstream's `ns`, `name`, `fqn` and
  // `hash` (null until computed), else undefined. Equality reads only the
  // kind and `fqn`, never `instanceof`, so the keywords of two copies of one
  // build are equal (docs/value-layout.md, section 3). One made by
  // reflection from a keyword's prototype has no `fqn` field, and is no
  // keyword; the public constructor may give the field an undefined `fqn`.
  // The Pants build's profile swaps it for one that also reads engine
  // keyword forms, whose fields it gives in a fresh object: a caller reads
  // the fields of what it returns and never hands that on as a value.
  function keywordOf(value) {
    return value != null && value.cljs$lang$kind$$LKEY === 20 && "fqn$LKEY" in value ? value : undefined;
  }
  //@unit keywordCells requires: keywordOf
  // `keywordOf` as an entry of the store inventories (`storesPredicate`,
  // `protocolStores`), as `vectorCells`.
  const keywordCells = Object.freeze({has: value => keywordOf(value) !== undefined});
  //@unit KeywordCell
  // A keyword (see `keywordOf`) with `KeywordValue`'s prototype. Nothing in
  // it is frozen. The compile-time runtime stamps it (`stamp`).
  function KeywordCell(ns, name, fqn, hash) { this.ns$LKEY = ns; this.name$LKEY = name; this.fqn$LKEY = fqn; this.hash$LKEY = hash; /*@brand*/ }
  //@unit symbolOf
  // A symbol, the class of kind 21 (`SymbolCell`), as `keywordOf`: its own
  // keyed fields are upstream's `ns`, `name`, `str`, `hash` and `meta` (its
  // metadata, light-cljs-1031). The Pants build's profile swaps it as
  // `keywordOf`.
  function symbolOf(value) {
    return value != null && value.cljs$lang$kind$$LKEY === 21 && "str$LKEY" in value ? value : undefined;
  }
  //@unit symbolCells requires: symbolOf
  const symbolCells = Object.freeze({has: value => symbolOf(value) !== undefined});
  //@unit namedOf requires: keywordOf, symbolOf
  // A keyword's or a symbol's fields (`ns`, `name`, `hash`), else undefined.
  const namedOf = value => keywordOf(value) ?? symbolOf(value);
  //@unit SymbolCell
  // A symbol (see `symbolOf`) with `SymbolValue`'s prototype. Its metadata is
  // its own `meta` field (`ownMeta`), so a copy of the build reads it too.
  function SymbolCell(ns, name, str, hash, meta) { this.ns$LKEY = ns; this.name$LKEY = name; this.str$LKEY = str; this.hash$LKEY = hash; this.meta$LKEY = meta; /*@brand*/ }
  //@unit internedKeywords
  const internedKeywords = new Map();
  //@unit kinds
  // The kind number of each runtime class converted to the class layout
  // (docs/value-layout.md, section 2), which its prototype holds under the
  // keyed name `cljs$lang$kind$$LKEY`: a value of this runtime, of this
  // class. Kinds 1 to 3 are their own vector store (`vectorStore`), 4 a
  // transient vector, its own state (`transientState`). Kinds 5 to 11 are the seqs
  // over an array or a string (`viewOf`), each set by its type's unit with
  // `viewType`: 5 `IndexedSeq`, 6 `ChunkedSeq`, 7 `NodeSeq`, 8
  // `ArrayNodeSeq`, 9 `PersistentArrayMapSeq`, 10 `RSeq`, 11
  // `PersistentTreeMapSeq`; numbers, so that a program that keeps one of
  // them keeps no table of the others. Kinds 16 to 19 are the lazy seqs
  // and chunks (light-cljs-1029), each read by its own test: 16 `LazySeq`
  // (`lazyOf`), 17 `ChunkedCons` (`chunkedConsOf`), 18 `ArrayChunk`
  // (`chunkOf`), 19 `ChunkBuffer` (`chunkBufferOf`). Kinds 20 and 21 are
  // keywords and symbols (`keywordOf`, `symbolOf`). The lists, conses and
  // the other seqs of `cellOf` (light-cljs-1030) are 12 `List`, 13
  // `EmptyList`, 14 `Cons`, 15 `IntegerRange`, 22 `Range`, 23 `Repeat`, 24
  // `Iterate`, 25 `Cycle`, 26 the key and value seqs and 27 `re-seq`'s
  // conses; a sorted map's seq has kind 11, the views' last. The maps
  // (light-cljs-1012, `mapOf`) are 28 `PersistentArrayMap`, 29
  // `PersistentHashMap` and 30 `PersistentTreeMap`, and the map transients
  // (`isMapTransient`) 31 `TransientArrayMap` and 32 `TransientHashMap`.
  // The hash and array sets (light-cljs-1013, `setCellOf`) are 33
  // `PersistentHashSet`, and the set transients (`transientSetOf`) 34
  // `TransientHashSet`. The rest of light-cljs-1013's classes are 35
  // `Reduced`, 36 `PersistentQueue`, 37 the iterator seqs, 38 `Atom`, 39
  // `Volatile`, 40 `Delay`, 41 `MultiFn`, 42 `UUID` and 43 `Eduction`.
  const kinds = Object.freeze({vector: 1, subvec: 2, entry: 3, transientVector: 4});
  //@unit unkeyedJSON
  // The JSON of a value of the class bases that have a `toJSON`: its own
  // properties but its keyed fields, as before it had them
  // (docs/value-layout.md, question 3).
  function unkeyedJSON(value) {
    const json = {};
    // Own data properties, as an object rest gives, so that no setter of
    // `Object.prototype` (`__proto__`) runs.
    for (const key of Object.keys(value)) if (!key.endsWith("$LKEY")) Object.defineProperty(json, key, {value: value[key], writable: true, enumerable: true, configurable: true});
    return json;
  }
  //@unit vectorStore
  // The store (see `vectorTailOffset`) of a vector, a subvector or a map
  // entry, the classes of kinds 1 to 3 (`kinds`): the value itself, whose
  // own keyed fields are the store's (`VectorCell`, `VectorViewCell`,
  // `EntryCell`), else undefined. A value is told by its prototype's keyed
  // kind, which no host object has by accident (docs/value-layout.md,
  // section 5); one made by reflection from a vector's prototype has no
  // `cnt` field, and is no vector.
  function vectorStore(value) {
    if (value == null) return undefined;
    const kind = value.cljs$lang$kind$$LKEY;
    return (kind === 1 || kind === 2 || kind === 3) && (value.cnt$LKEY !== undefined || "cnt$LKEY" in value) ? value : undefined;
  }
  //@unit ownMeta
  // Whether a value holds its metadata and its cached hash in its own keyed
  // fields `meta` and `hash` (docs/value-layout.md, section 2), which
  // `meta`, `withMeta` and `hash` read, rather than in the `metadataValues`
  // and `constructorHashes` stores: a vector, a subvector or a map entry of
  // this runtime's classes (`VectorCell`, `VectorViewCell`, `SubvecCell`,
  // `EntryCell`), or a symbol (`SymbolCell`, any value of kind 21), but no
  // engine vector or symbol form of the Pants build's profile. Its bytecode
  // stays under 53 bytes, or a vector's `conj` loses an inlining
  // (docs/value-layout.md, light-cljs-1031); kinds 1 to 3 are those below 4.
  function ownMeta(value) {
    if (value == null) return false;
    const kind = value.cljs$lang$kind$$LKEY;
    return kind < 4 ? value.cnt$LKEY !== undefined || "cnt$LKEY" in value : kind === 21;
  }
  //@unit holdsVector requires: vectorStore
  // Whether a value is a vector, a subvector or a map entry, as the
  // vectors' brand store answered.
  const holdsVector = value => vectorStore(value) !== undefined;
  //@unit isEntryCell
  // Whether a value is a map entry of the runtime's class (`EntryCell`).
  const isEntryCell = value => value != null && value.cljs$lang$kind$$LKEY === 3 && (value.cnt$LKEY !== undefined || "cnt$LKEY" in value);
  //@unit vectorCells requires: holdsVector
  // `holdsVector` as an entry of the store inventories (`storesPredicate`,
  // `payloadStores`), which probe it as a store that is not exact.
  const vectorCells = Object.freeze({has: holdsVector});
  //@unit vectorCellCopy
  // A new value of a vector cell's prototype with its fields, its metadata
  // and hash included, built by a constructor (so the compile-time runtime
  // stamps it), for `withMeta` and `clone`; the copy constructors keep no
  // vector type. A view's copy shares the trie it built.
  function vectorCellCopy(value) {
    const Type = function () {};
    Type.prototype = Object.getPrototypeOf(value);
    return value.sub$LKEY !== undefined ? Reflect.construct(VectorViewCopy, [value], Type) : Reflect.construct(VectorCellCopy, [value], Type);
  }
  function VectorCellCopy(v) {
    this.meta$LKEY = v.meta$LKEY; this.cnt$LKEY = v.cnt$LKEY; this.shift$LKEY = v.shift$LKEY; this.root$LKEY = v.root$LKEY; this.tail$LKEY = v.tail$LKEY; this.hash$LKEY = v.hash$LKEY;
    if (v.callerTail$LKEY) this.callerTail$LKEY = true;
    /*@brand*/
  }
  function VectorViewCopy(v) {
    this.meta$LKEY = v.meta$LKEY; this.cnt$LKEY = v.cnt$LKEY; this.sub$LKEY = v.sub$LKEY; this.begin$LKEY = v.begin$LKEY; this.built$LKEY = v.built$LKEY; this.hash$LKEY = v.hash$LKEY; /*@brand*/
  }
  //@unit VectorCell
  // A persistent vector, upstream's PersistentVector: its own keyed fields
  // are its metadata, its store's `cnt`, `shift`, `root` and `tail` (see
  // `vectorTailOffset`) and its hash, null until computed; `callerTail`,
  // where it is there, marks a tail the caller gave. Its prototype is
  // `PersistentVectorValue`'s (`PersistentVectorValue.type`). Nothing in it
  // is frozen. The compile-time runtime stamps it (`stamp`).
  function VectorCell(meta, cnt, shift, root, tail, hash) { this.meta$LKEY = meta; this.cnt$LKEY = cnt; this.shift$LKEY = shift; this.root$LKEY = root; this.tail$LKEY = tail; this.hash$LKEY = hash; /*@brand*/ }
  //@unit VectorViewCell
  // A view (`vectorView`) as a vector, with `PersistentVectorValue`'s
  // prototype: what `assoc` and `pop` of a subvector give.
  function VectorViewCell(meta, cnt, sub, start, hash) { this.meta$LKEY = meta; this.cnt$LKEY = cnt; this.sub$LKEY = sub; this.begin$LKEY = start; this.built$LKEY = null; this.hash$LKEY = hash; /*@brand*/ }
  //@unit SubvecCell
  // A subvector (`subvec`), upstream's Subvec: a view (`vectorView`) of
  // `cnt` items of the vector store `sub` from its index `begin` (upstream's `start`), with
  // `SubvecValue`'s prototype, its metadata and its hash.
  function SubvecCell(meta, cnt, sub, start, hash) { this.meta$LKEY = meta; this.cnt$LKEY = cnt; this.sub$LKEY = sub; this.begin$LKEY = start; this.built$LKEY = null; this.hash$LKEY = hash; /*@brand*/ }
  //@unit EntryCell
  // A map entry: a vector store of its key and value, which its tail holds,
  // with `MapEntryType`'s prototype. Its metadata is always nil (`withMeta`
  // of one is a vector).
  function EntryCell(key, val) { this.meta$LKEY = null; this.cnt$LKEY = 2; this.shift$LKEY = 5; this.root$LKEY = []; this.tail$LKEY = [key, val]; this.hash$LKEY = null; /*@brand*/ }
  //@unit viewOf
  // A seq over an array or a string, the classes of kinds 5 to 11 (`kinds`):
  // the value itself, whose own keyed fields are its storage `arr` (an array
  // or a string), the offset `i` of its first item, whether `nth` reads the
  // storage by absolute index (`indexed`) and the seq's kind (`view`:
  // "indexed", "chunked", "queue", "set", "reverse" or a map's kind), else
  // undefined. As `vectorStore`, one made by reflection holds no storage.
  function viewOf(value) {
    if (value == null) return undefined;
    const kind = value.cljs$lang$kind$$LKEY;
    return kind >= 5 && kind <= 11 && value.arr$LKEY !== undefined ? value : undefined;
  }
  //@unit isView requires: viewOf
  const isView = value => viewOf(value) !== undefined;
  //@unit viewCells requires: isView
  // `isView` as an entry of the store inventories, as `vectorCells`.
  const viewCells = Object.freeze({has: isView});
  //@unit IndexedSeqCell
  // A seq over an array or a string with `IndexedSeqValue`'s prototype (see
  // `viewOf`); rest arguments are one over a fresh array of their own.
  // Nothing in it is frozen. The compile-time runtime stamps it (`stamp`).
  function IndexedSeqCell(arr, i, indexed, kind) { this.arr$LKEY = arr; this.i$LKEY = i; this.indexed$LKEY = indexed; this.view$LKEY = kind; /*@brand*/ }
  //@unit ChunkedSeqCell
  // A vector's chunked seq, with `ChunkedSeqValue`'s prototype: its leaf
  // `node` is the view's storage; besides a view's fields, `vec` is the
  // vector and `base` the index of the leaf's first item, so upstream's `i`
  // and `off` are its `base` and `i`. Only a chunked seq has `vec`.
  function ChunkedSeqCell(node, off, vec, base) { this.arr$LKEY = node; this.i$LKEY = off; this.indexed$LKEY = false; this.view$LKEY = "chunked"; this.vec$LKEY = vec; this.base$LKEY = base; /*@brand*/ }
  //@unit NodeSeqCell
  // The seqs of maps over an array of their entries, one class per map seq
  // type, with the fields of `viewOf`.
  function NodeSeqCell(arr, i, indexed, kind) { this.arr$LKEY = arr; this.i$LKEY = i; this.indexed$LKEY = indexed; this.view$LKEY = kind; /*@brand*/ }
  //@unit ArrayNodeSeqCell
  function ArrayNodeSeqCell(arr, i, indexed, kind) { this.arr$LKEY = arr; this.i$LKEY = i; this.indexed$LKEY = indexed; this.view$LKEY = kind; /*@brand*/ }
  //@unit ArrayMapSeqCell
  function ArrayMapSeqCell(arr, i, indexed, kind) { this.arr$LKEY = arr; this.i$LKEY = i; this.indexed$LKEY = indexed; this.view$LKEY = kind; /*@brand*/ }
  //@unit RSeqCell
  function RSeqCell(arr, i, indexed, kind) { this.arr$LKEY = arr; this.i$LKEY = i; this.indexed$LKEY = indexed; this.view$LKEY = kind; /*@brand*/ }
  //@unit LazySeqCell
  // A lazy seq with `LazySeq`'s prototype (`lazyOf`), upstream's fields:
  // `fn` the thunk until the seq is realized, then null (one given as
  // undefined stays so), `s` what the thunk gave and `hash` the cached hash. Nothing in it
  // is frozen; a step of a lazy pipeline builds this one object. The
  // compile-time runtime stamps it (`stamp`).
  function LazySeqCell(fn, s, hash) { this.fn$LKEY = fn; this.s$LKEY = s; this.hash$LKEY = hash; /*@brand*/ }
  //@unit lazyOf
  // A lazy seq, kind 16 (`kinds`): the value itself, whose own keyed fields
  // are its state (`LazySeqCell`), else undefined. As `viewOf`, one made by
  // reflection from the prototype has no such own field, and is no lazy seq
  // (a test of the field's presence, as any of them can be undefined). The
  // shared-heap profile's also answers the engine's lazy seqs, with a view
  // of the same fields.
  function lazyOf(value) {
    if (value == null) return undefined;
    return value.cljs$lang$kind$$LKEY === 16 && "fn$LKEY" in value ? value : undefined;
  }
  //@unit isLazy requires: lazyOf
  const isLazy = value => lazyOf(value) !== undefined;
  //@unit lazyCells requires: isLazy
  // `isLazy` as an entry of the store inventories, as `viewCells`.
  const lazyCells = Object.freeze({has: isLazy});
  //@unit lazyStep
  // What the thunk of a lazy seq's state (`lazyOf`) gave, calling it on the
  // first call only. The state changes only after the call returns, so a
  // failure leaves the thunk in place, and a reentrant call is an ordinary
  // call, as upstream's `sval`.
  function lazyStep(lazy) {
    const fn = lazy.fn$LKEY;
    if (fn != null) { lazy.s$LKEY = fn.call(null); lazy.fn$LKEY = null; }
    return lazy.s$LKEY;
  }
  //@unit ChunkedConsCell
  // A chunked cons with `ChunkedCons`'s prototype (`chunkedConsOf`): its
  // `chunk`, the seq `more` after it and its cached `hash`.
  function ChunkedConsCell(chunk, more, hash) { this.chunk$LKEY = chunk; this.more$LKEY = more; this.hash$LKEY = hash; /*@brand*/ }
  //@unit chunkedConsOf
  // A chunked cons, kind 17: the value itself (`ChunkedConsCell`), else
  // undefined; the shared-heap profile's also answers the engine's.
  function chunkedConsOf(value) {
    if (value == null) return undefined;
    return value.cljs$lang$kind$$LKEY === 17 && "chunk$LKEY" in value ? value : undefined;
  }
  //@unit isChunkedCons requires: chunkedConsOf
  const isChunkedCons = value => chunkedConsOf(value) !== undefined;
  //@unit chunkedConsCells requires: isChunkedCons
  // `isChunkedCons` as an entry of the store inventories, as `viewCells`.
  const chunkedConsCells = Object.freeze({has: isChunkedCons});
  //@unit ArrayChunkCell
  // A chunk with `ArrayChunk`'s prototype (`chunkOf`): the items of `arr`
  // from `off` to `end`; a range's chunk (an integer chunk) has no items but
  // its `first` item and its `step`, which an array chunk has null.
  function ArrayChunkCell(arr, off, end, first, step) { this.arr$LKEY = arr; this.off$LKEY = off; this.end$LKEY = end; this.first$LKEY = first; this.step$LKEY = step; /*@brand*/ }
  //@unit chunkOf
  // A chunk, kind 18: the value itself (`ArrayChunkCell`), else undefined.
  function chunkOf(value) {
    if (value == null) return undefined;
    return value.cljs$lang$kind$$LKEY === 18 && "arr$LKEY" in value ? value : undefined;
  }
  //@unit ChunkBufferCell
  // A chunk buffer with `ChunkBuffer`'s prototype (`chunkBufferOf`): the
  // array `buf` it fills (null once `chunk` took it) and its `end`.
  function ChunkBufferCell(buf, end) { this.buf$LKEY = buf; this.end$LKEY = end; /*@brand*/ }
  //@unit chunkBufferOf
  // A chunk buffer, kind 19: the value itself, else undefined.
  function chunkBufferOf(value) {
    if (value == null) return undefined;
    return value.cljs$lang$kind$$LKEY === 19 && "end$LKEY" in value ? value : undefined;
  }
  //@unit cellOf
  // A list, the empty list, a cons, a range, a repeat, an iterate, a cycle,
  // a map's key or value seq or a sorted map's seq (light-cljs-1030): the
  // value itself, of one of the classes below, whose own keyed field
  // `cell` is today's kind of seq ("list", "cons", "range", "repeat",
  // "iterate", "cycle", "keys", "vals" or "tree-seq"), else undefined.
  // Each class reads `first` and `rest` under keyed names, its own fields
  // where they are data, getters of its prototype where a step computes
  // them. As `viewOf`, one made by reflection from a prototype has no own
  // `cell`, and copying the fields onto a host object copies no kind. The
  // shared-heap profile's also answers the engine's lists and conses, with
  // a view of the same fields.
  function cellOf(value) {
    if (value == null) return undefined;
    return value.cljs$lang$kind$$LKEY !== undefined && value.cell$LKEY !== undefined ? value : undefined;
  }
  //@unit isCell requires: cellOf
  const isCell = value => cellOf(value) !== undefined;
  //@unit seqCells requires: isCell
  // `isCell` as an entry of the store inventories, as `viewCells`.
  const seqCells = Object.freeze({has: isCell});
  //@unit cellFields
  // A cell's copy (`cellOf`), for its metadata: a value of its prototype
  // with its own keyed fields, in their order, and none of the slots or
  // fields a program gave the value itself.
  // The names are a frozen array, which the Pants build's shared heap can
  // publish, where a `Set` stays its task's.
  const cellFields = Object.freeze(["cell$LKEY", "first$LKEY", "rest$LKEY", "count$LKEY", "end$LKEY", "step$LKEY", "integer$LKEY", "remaining$LKEY", "next$LKEY",
    "f$LKEY", "prev$LKEY", "seed$LKEY", "realized$LKEY", "all$LKEY", "current$LKEY", "mseq$LKEY", "stack$LKEY", "ascending$LKEY", "cnt$LKEY"]);
  // The shared-heap profile's copies engine lists and conses too.
  function cellCopy(value) {
    const Type = function () {};
    Type.prototype = Object.getPrototypeOf(value);
    return Reflect.construct(CellCopy, [value], Type);
  }
  function CellCopy(value) { for (const name of Object.keys(value)) if (cellFields.includes(name)) this[name] = value[name]; /*@brand*/ }
  //@unit ListCell
  // A list with `ListValue`'s prototype, kind 12: upstream's `first`,
  // `rest` and `count`. Nothing in a cell is frozen; the compile-time
  // runtime stamps it (`stamp`).
  function ListCell(first, rest, count) { this.cell$LKEY = "list"; this.first$LKEY = first; this.rest$LKEY = rest; this.count$LKEY = count; /*@brand*/ }
  //@unit EmptyListCell
  // The empty list, `EmptyListValue`'s, kind 13: a list of count 0.
  function EmptyListCell() { this.cell$LKEY = "list"; this.count$LKEY = 0; /*@brand*/ }
  //@unit ConsCell
  // A cons with `Cons`'s prototype, kind 14: its `first` and its `rest`, a
  // seq or nil.
  function ConsCell(first, rest) { this.cell$LKEY = "cons"; this.first$LKEY = first; this.rest$LKEY = rest; /*@brand*/ }
  //@unit ReSeqCell
  // A step of `re-seq`: a cons on a prototype of its own (kind 27) over the
  // shared seq base, as its values had no type before.
  function ReSeqCell(first, rest) { this.cell$LKEY = "cons"; this.first$LKEY = first; this.rest$LKEY = rest; /*@brand*/ }
  //@unit IntegerRangeCell
  // A range of integers with `IntegerRangeValue`'s prototype, kind 15: its
  // `first` item, `end`, `step` and `count`; `rest` is a getter of the
  // prototype, so a step builds one object.
  function IntegerRangeCell(first, end, step, count) { this.cell$LKEY = "range"; this.first$LKEY = first; this.end$LKEY = end; this.step$LKEY = step; this.integer$LKEY = true; this.count$LKEY = count; /*@brand*/ }
  //@unit RangeCell
  // Another range, `RangeValue`'s, kind 22: no count.
  function RangeCell(first, end, step) { this.cell$LKEY = "range"; this.first$LKEY = first; this.end$LKEY = end; this.step$LKEY = step; this.integer$LKEY = false; /*@brand*/ }
  //@unit RepeatCell
  // A repeat with `RepeatValue`'s prototype, kind 23: the items `remaining`
  // (-1 without end), the item `first` and the `next` step once built.
  function RepeatCell(remaining, first, next) { this.cell$LKEY = "repeat"; this.remaining$LKEY = remaining; this.first$LKEY = first; this.next$LKEY = next; /*@brand*/ }
  //@unit IterateCell
  // An iterate with `IterateValue`'s prototype, kind 24: upstream's `f`,
  // `prev` (its previous seed) and `seed`, whether the seed is computed
  // (`realized`) and the `next` step once built.
  function IterateCell(f, prev, seed, realized, next) { this.cell$LKEY = "iterate"; this.f$LKEY = f; this.prev$LKEY = prev; this.seed$LKEY = seed; this.realized$LKEY = realized; this.next$LKEY = next; /*@brand*/ }
  //@unit CycleCell
  // A cycle with `CycleValue`'s prototype, kind 25: upstream's `all`,
  // `prev`, `current` (nil until a step needs it) and `next`.
  function CycleCell(all, prev, current, next) { this.cell$LKEY = "cycle"; this.all$LKEY = all; this.prev$LKEY = prev; this.current$LKEY = current; this.next$LKEY = next; /*@brand*/ }
  //@unit KeySeqCell
  // A map's key seq ("keys") or value seq ("vals"), kind 26, on a prototype
  // of its own over the shared seq base: the seq `mseq` of the map's entries.
  function KeySeqCell(kind, mseq) { this.cell$LKEY = kind; this.mseq$LKEY = mseq; /*@brand*/ }
  //@unit TreeMapStackCell
  // A sorted map's seq with `treeMapSeqConstructor`'s prototype (kind 11,
  // the views' last, but with no `arr`), upstream's PersistentTreeMapSeq:
  // the `stack` of nodes, a list, `ascending` and the count `cnt`, below 0
  // where unknown (light-cljs-1035).
  function TreeMapStackCell(stack, ascending, cnt) { this.cell$LKEY = "tree-seq"; this.stack$LKEY = stack; this.ascending$LKEY = ascending; this.cnt$LKEY = cnt; /*@brand*/ }
  //@unit ArrayMapCell
  // An array map, upstream's PersistentArrayMap, with
  // `PersistentArrayMapValue`'s prototype, kind 28 (`mapOf`): its own keyed
  // fields are its count `cnt` and `arr`, its keys and values in order,
  // flat (`[k0, v0, k1, v1, ...]`, light-cljs-1034). Nothing in it is
  // frozen, and nothing writes `arr` once the map is built: the map
  // operations copy it (`arrayMapAssoc`). A public constructor's map holds
  // the array and count it was given, as upstream's. The compile-time
  // runtime stamps it (`stamp`).
  function ArrayMapCell(cnt, arr) { this.cnt$LKEY = cnt; this.arr$LKEY = arr; /*@brand*/ }
  //@unit HashMapCell
  // A hash map, upstream's PersistentHashMap, with `PersistentHashMapValue`'s
  // prototype, kind 29 (light-cljs-1034): its own keyed fields are its count
  // `cnt`, its trie's `root` node (null without a key but nil), its `nil`
  // entry or null (upstream's `has-nil?` and `nil-val`, in the entry that
  // keeps the first nil key, null or undefined) and `lit`, the entries of
  // the builder that made it at their indexes, or null (`mapValueAt`).
  // Nothing in it is frozen, its leaf entries included.
  function HashMapCell(cnt, root, nil, lit) { this.cnt$LKEY = cnt; this.root$LKEY = root; this.nil$LKEY = nil; this.lit$LKEY = lit; /*@brand*/ }
  //@unit TreeMapCell
  // A sorted map, upstream's PersistentTreeMap, with `treeMapConstructor`'s
  // prototype, kind 30 (light-cljs-1035): its own keyed fields are its
  // comparator `comp`, its red-black `tree`, null when empty, and its count
  // `cnt`. Its nodes are map entries (`TreeNodeCell`); a public
  // constructor's map holds the tree and count it was given, as upstream's.
  function TreeMapCell(comp, tree, cnt) { this.comp$LKEY = comp; this.tree$LKEY = tree; this.cnt$LKEY = cnt; /*@brand*/ }
  //@unit TreeNodeCell
  // A node of a sorted map's tree, upstream's BlackNode and RedNode: a map
  // entry, of `EntryCell`'s own keyed fields on `MapEntryType`'s prototype
  // (`TreeNodeCell.type`), so that every reader of a map entry reads it as
  // one, whose `tail` holds its key and value; and its `left` and `right`
  // subtrees, null where none, and whether it is `red`. Nothing writes a
  // node: an update builds the nodes of its path, which share the `tail` of
  // a node whose key and value they keep, and every node shares one empty
  // `root` (`treeNodeRoot`), a map entry's vector root.
  function TreeNodeCell(tail, left, right, red) { this.meta$LKEY = null; this.cnt$LKEY = 2; this.shift$LKEY = 5; this.root$LKEY = treeNodeRoot; this.tail$LKEY = tail; this.hash$LKEY = null; this.left$LKEY = left; this.right$LKEY = right; this.red$LKEY = red; /*@brand*/ }
  const treeNodeRoot = Object.freeze([]);
  //@unit mapOf
  // A map of the class layout (`ArrayMapCell`, `HashMapCell`,
  // `TreeMapCell`): the value itself where its prototype's keyed kind is
  // 28, 29 or 30 (`kinds`) and its own field is there (an array map's
  // `arr`, the others' `cnt`, which a public constructor may give
  // undefined), else undefined. A value made by reflection from a map's
  // prototype has none, and is no map; a record is no map of this test
  // either.
  function mapOf(value) {
    if (value == null) return undefined;
    const kind = value.cljs$lang$kind$$LKEY;
    return kind === 28 ? value.arr$LKEY !== undefined ? value : undefined
      : kind === 29 || kind === 30 ? (value.cnt$LKEY !== undefined || "cnt$LKEY" in value) ? value : undefined
      : undefined;
  }
  //@unit isArrayMap
  // Whether a value is an array map of the class layout (`ArrayMapCell`).
  const isArrayMap = value => value != null && value.cljs$lang$kind$$LKEY === 28 && value.arr$LKEY !== undefined;
  //@unit isHashMap
  // Whether a value is a hash map of the class layout (`HashMapCell`).
  const isHashMap = value => value != null && value.cljs$lang$kind$$LKEY === 29 && (value.cnt$LKEY !== undefined || "cnt$LKEY" in value);
  //@unit isTreeMap
  // Whether a value is a sorted map of the class layout (`TreeMapCell`).
  const isTreeMap = value => value != null && value.cljs$lang$kind$$LKEY === 30 && (value.cnt$LKEY !== undefined || "cnt$LKEY" in value);
  //@unit arrayMapEntries requires: ArrayMapEntry
  // An array map's pairs (`arr`) as entries, for the few readers that take
  // entries of every map (`mapTraversal`) and the updates of a public
  // constructor's array map (`mapAssocEntries`): one fresh `ArrayMapEntry`
  // per pair, which a caller may change. The other readers of an array
  // map's pairs read `arr` (`mapPairs`). Each array the maps build is packed
  // (pushed, never `new Array(n)`), so that a read of it needs no test for a
  // hole.
  function arrayMapEntries(arr) {
    const entries = [];
    for (let i = 0; i < arr.length; i += 2) entries.push(new ArrayMapEntry(arr[i], arr[i + 1]));
    return entries;
  }
  //@unit flatPairs
  // The keys and values of `entries`, flat, as an array map holds them.
  function flatPairs(entries) {
    const arr = [];
    for (let i = 0; i < entries.length; i++) { const entry = entries[i]; arr.push(entry.key, entry.value); }
    return arr;
  }
  //@unit mapCellCopy requires: mapKinds, mapTypes
  // A map of the class of `value`'s kind with its fields (an array map's
  // count and keys and values, another's entries and comparator), for
  // `with-meta` and `clone` of a map, which copy the rest (metadata, hashes,
  // a public constructor's slots) from the stores. The Pants build's
  // profile has its own, which also copies an engine map.
  function mapCellCopy(value) {
    const Cell = mapTypes[mapKinds.get(value)].prototype.cljs$lang$cell$$LKEY, kind = value.cljs$lang$kind$$LKEY;
    return kind === 28 ? new Cell(value.cnt$LKEY, value.arr$LKEY)
      : kind === 29 ? new Cell(value.cnt$LKEY, value.root$LKEY, value.nil$LKEY, value.lit$LKEY)
      : new Cell(value.comp$LKEY, value.tree$LKEY, value.cnt$LKEY);
  }
  //@unit setQueueCopy requires: queues, setCellOf
  // A copy of a hash or array set's or a queue's fields, built by its class
  // (`HashSetCell`, `QueueCell`), for `with-meta` and `clone`, which copy
  // the rest (metadata, hashes, a public constructor's slots) from the
  // stores; else undefined.
  function setQueueCopy(value) {
    const set = setCellOf(value);
    return set !== undefined ? new set.cljs$lang$cell$$LKEY(set.entries$LKEY, set.skind$LKEY)
      : queues.has(value) ? new value.cljs$lang$cell$$LKEY(value.arr$LKEY) : undefined;
  }
  //@unit TransientArrayMapCell
  // A transient array map, upstream's TransientArrayMap (light-cljs-1036),
  // with `TransientArrayMapType`'s prototype, kind 31: its own keyed fields
  // are whether it is still `editable` and `arr`, its keys and values, flat,
  // as an array map holds them (`ArrayMapCell`), which it edits in place and
  // `persistent!` gives the map it builds. Upstream's `len` is the array's
  // length. It is its own state (`transientState`), as a transient vector.
  function TransientArrayMapCell(arr) { this.editable$LKEY = true; this.arr$LKEY = arr; /*@brand*/ }
  //@unit TransientHashMapCell
  // A transient hash map, upstream's TransientHashMap, with
  // `TransientHashMapType`'s prototype, kind 32: its own keyed fields are its
  // `edit`, the token of the trie nodes it owns and changes in place
  // (`hashNodeAssoc`), null once `persistent!` ran (upstream's `edit`), and
  // a hash map's `cnt`, `root` and `nil` (`HashMapCell`), which it changes
  // in place.
  function TransientHashMapCell(edit, cnt, root, nil) { this.edit$LKEY = edit; this.cnt$LKEY = cnt; this.root$LKEY = root; this.nil$LKEY = nil; /*@brand*/ }
  //@unit isMapTransient
  // Whether a value is a map transient of the runtime's classes
  // (`TransientArrayMapCell`, `TransientHashMapCell`): its prototype's keyed
  // kind and its own first field, as `isTransientCell` tells a transient
  // vector.
  function isMapTransient(value) {
    if (value == null) return false;
    const kind = value.cljs$lang$kind$$LKEY;
    return kind === 31 ? value.arr$LKEY !== undefined : kind === 32 && (value.cnt$LKEY !== undefined || "cnt$LKEY" in value);
  }
  //@unit viewType mutator
  // Makes `Type` a class of kind `kind` (`kinds`), whose values `Cell`
  // builds, which its prototype holds under the keyed name `cljs$lang$cell$`
  // for `view` and `viewAt`: the views (`viewOf`), and the lazy seqs and
  // chunks (`lazyOf` and the others), which read only the kind; and for
  // `makeMap` and `makeTransient`, the maps and map transients.
  function viewType(Type, Cell, kind) {
    Cell.prototype = Type.prototype;
    Type.prototype.cljs$lang$cell$$LKEY = Cell;
    Type.prototype.cljs$lang$kind$$LKEY = kind;
    return Type;
  }
  //@unit stampBrand compile-time requires: canHoldField, weakMapStores
  // The compile-time brand of the runtime's class values (docs/value-layout.md,
  // section 4): the compiler form bridge, `inspect` and the compile-time
  // printer test it (`stamped`) before they read a value's keyed kind, so
  // that rejecting a macro result reads no property of a host value, and
  // runs no getter or Proxy trap. build.rs (`compiler_runtime`) turns each
  // class constructor's `/*@brand*/` into a `stamp`; output never has it. A
  // private field in V8, a `WeakSet` where the host states WeakMap stores.
  const stampBrand = weakMapStores ? (members => ({add(value) { members.add(value); }, has: value => members.has(value)}))(new WeakSet())
    : class extends (function (value) { return value; }) {
      #b;
      static add(value) { new stampBrand(value); }
      static has(value) { return canHoldField(value) && #b in value; }
    };
  //@unit stamp compile-time requires: stampBrand
  function stamp(value) { stampBrand.add(value); }
  //@unit stamped compile-time requires: stampBrand
  function stamped(value) { return stampBrand.has(value); }
  //@unit kindedStore requires: payloads, weakMapStores
  // The store of the values that are not classes yet, the sorted sets
  // (`sets.store`) and the records' updates (`recordMaps`), in the private
  // fields of `payloads`: a value's payload and its kind (`setKinds`), each
  // in a private field of its own, so that a value gets both with one
  // construction (light-cljs-291). A value's kind is set after its payload.
  // Every other runtime value is a class with its own fields
  // (docs/value-layout.md, section 2; the stores that held them before
  // light-cljs-1013 were `privateStore`s of these private fields).
  function kindedStore(kind) {
    if (weakMapStores) { const store = new WeakMap(); store.reset = store.set; store.kinds = new WeakMap(); return store; }
    return payloads.store(kind, true);
  }
  //@unit maps requires: mapOf
  // The maps (light-cljs-1012): no longer a store, but what tells a map of
  // the class layout (`mapOf`), as its `has`. No map of the classes has
  // entries (light-cljs-1036, and a sorted map's tree since light-cljs-1035):
  // their readers read their fields (`mapPairs`, `mapTraversal`, the map
  // slots), so `get` answers undefined for them. A record, a map until it
  // becomes a class (light-cljs-1013), is added by `maps.records`, with the
  // entries an update gave it (`recordMaps`).
  const maps = {has: value => mapOf(value) !== undefined, get: () => undefined};
  //@unit mapKinds requires: mapOf
  // A map's kind, which its class gives: "array-map", "hash-map" or
  // "sorted-map" (and a record's, `mapKinds.records`).
  const mapKinds = {
    has: value => mapOf(value) !== undefined,
    get(value) {
      const kind = mapOf(value)?.cljs$lang$kind$$LKEY;
      return kind === 28 ? "array-map" : kind === 29 ? "hash-map" : kind === 30 ? "sorted-map" : undefined;
    }
  };
  //@unit recordMaps requires: kindedStore
  // The entries of a record that an update or `with-meta` built
  // (`updatedRecord`), and their kind, until records become classes
  // (light-cljs-1013).
  const recordMaps = kindedStore(4);
  //@unit setComparators
  // The comparators of the sorted sets, until they become classes
  // (light-cljs-1013); a sorted map holds its own (`TreeMapCell`).
  const setComparators = new WeakMap();
  //@unit comparatorOf requires: setComparators
  // A sorted map's comparator (its own `comp` field) or a sorted set's,
  // else undefined: whether a value is sorted.
  function comparatorOf(value) {
    if (value == null) return undefined;
    const kind = value.cljs$lang$kind$$LKEY;
    return kind === 28 || kind === 29 ? undefined : kind === 30 && (value.cnt$LKEY !== undefined || "cnt$LKEY" in value) ? value.comp$LKEY : setComparators.get(value);
  }
  //@unit HashSetCell
  // A hash or array set, upstream's PersistentHashSet, with
  // `PersistentHashSetValue`'s prototype, kind 33 (`setCellOf`,
  // light-cljs-1013): its own keyed fields are its members, `entries`, as
  // the `sets` store held them (an array set's entries, a hash set's trie,
  // see `putMember`), unfrozen, and `skind`, the kind `setKinds` answers
  // ("array-map" or "hash-map", or a public constructor's source map's).
  // The compile-time runtime stamps it (`stamp`).
  function HashSetCell(entries, kind) { this.entries$LKEY = entries; this.skind$LKEY = kind; /*@brand*/ }
  //@unit setCellOf
  // A hash or array set of the class layout (`HashSetCell`): the value
  // itself where its prototype's keyed kind is 33 and its own `entries`
  // field is there, else undefined; a value made by reflection from a set's
  // prototype has none, and is no set.
  function setCellOf(value) {
    return value != null && value.cljs$lang$kind$$LKEY === 33 && value.entries$LKEY !== undefined ? value : undefined;
  }
  //@unit sets requires: kindedStore, setCellOf
  // The sets: a hash or array set is a `HashSetCell`, which `has` tells by
  // its kind and `get` reads the members of; a sorted set keeps its members
  // in the kinded store (`sets.store`) until it becomes a class too.
  const sets = (store => ({
    store,
    has: value => setCellOf(value) !== undefined || store.has(value),
    get: value => setCellOf(value)?.entries$LKEY ?? store.get(value),
    set: (value, payload) => store.set(value, payload),
    reset: (value, payload) => store.reset(value, payload),
    kinds: {
      has: value => setCellOf(value) !== undefined || store.kinds.has(value),
      get: value => setCellOf(value)?.skind$LKEY ?? store.kinds.get(value),
      set(value, kind) { store.kinds.set(value, kind); return this; }
    }
  }))(kindedStore(6));
  //@unit setKinds requires: sets
  const setKinds = sets.kinds;
  //@unit mapTypes
  // The type of each map kind that a kept map type builds, for building a map
  // of an existing map's kind; `setTypes` likewise for sorted sets.
  const mapTypes = {}, setTypes = {};
  //@unit TransientHashSetCell
  // A transient set, with `TransientHashSetType`'s prototype, kind 34
  // (`transientSetOf`, light-cljs-1013): its own keyed field `state` holds
  // what the `transients` store held, the members it edits (`entries`), their
  // kind (`mapKind`), whether it is `editable` and the `edit` token of the
  // trie nodes it owns. A transient vector or map is its own state.
  function TransientHashSetCell(state) { this.state$LKEY = state; /*@brand*/ }
  //@unit transientSetOf
  // A transient set of the class layout (`TransientHashSetCell`), else
  // undefined.
  function transientSetOf(value) {
    return value != null && value.cljs$lang$kind$$LKEY === 34 && value.state$LKEY !== undefined ? value : undefined;
  }
  //@unit TransientVectorCell
  // A transient vector, upstream's TransientVector: its own keyed fields are
  // the store it edits (`cnt`, `shift`, `root`, `tail`, see
  // `vectorTailOffset`), the nodes it owns (`owned`, see `vectorEditor`)
  // and whether it is still `editable`, so that it is its own state
  // (`transientState`). Its prototype is `TransientVectorType`'s
  // (`TransientVectorCell.type`).
  function TransientVectorCell(cnt, shift, root, tail, owned) { this.cnt$LKEY = cnt; this.shift$LKEY = shift; this.root$LKEY = root; this.tail$LKEY = tail; this.owned$LKEY = owned; this.editable$LKEY = true; /*@brand*/ }
  //@unit isTransientCell
  // Whether a value is a transient vector of the runtime's class, by its
  // prototype's keyed kind (`kinds`), as `vectorStore` tells a vector.
  const isTransientCell = value => value != null && value.cljs$lang$kind$$LKEY === 4 && value.cnt$LKEY !== undefined;
  //@unit transientState requires: isMapTransient, isTransientCell, transientSetOf
  // The state of a transient (`makeTransient`): a transient vector or map
  // itself, a set transient's own `state` field, else undefined.
  const transientState = value => isTransientCell(value) || isMapTransient(value) ? value : transientSetOf(value)?.state$LKEY;
  //@unit isTransient requires: isMapTransient, isTransientCell, transientSetOf
  // Whether a value is a transient of the runtime's classes.
  const isTransient = value => isTransientCell(value) || isMapTransient(value) || transientSetOf(value) !== undefined;
  //@unit transientCells requires: isTransient
  // The transients, which are all classes, as an entry of the store
  // inventories (`protocolStores`).
  const transientCells = Object.freeze({has: isTransient});
  //@unit IteratorSeqCell
  // A seq of a host iterable's iterator, upstream's ES6IteratorSeq, kind 37
  // (`iteratorSeqs`, light-cljs-1013): its own keyed fields are its `first`
  // item, the `iter` it reads and its `tail`, the seq of the rest, null
  // until a rest found one (`restSeq`). It is a plain object to everything
  // else, as the payload's holder was before (`#js {}` in print, `{}` in
  // JSON, no keys to enumerate), so its fields are not enumerable, and its
  // prototype is a plain object with its kind, which is not enumerable
  // either.
  function IteratorSeqCell(first, iter) {
    Object.defineProperties(this, {first$LKEY: {value: first}, iter$LKEY: {value: iter}, tail$LKEY: {value: null, writable: true}});
    /*@brand*/
  }
  //@unit IteratorSeqCell.type effect: IteratorSeqCell requires: IteratorSeqCell
  IteratorSeqCell.prototype = Object.defineProperty({}, "cljs$lang$kind$$LKEY", {value: 37});
  //@unit iteratorSeqs
  // The iterator seqs (`IteratorSeqCell`): `has` tells one by its kind and
  // own `iter` field, `get` gives it.
  const iteratorSeqs = {
    has: value => value != null && value.cljs$lang$kind$$LKEY === 37 && "iter$LKEY" in value,
    get(value) { if (iteratorSeqs.has(value)) return value; }
  };
  //@unit ReducedCell
  // A reduced value, upstream's Reduced, with `Reduced`'s prototype, kind
  // 35 (`reducedValues`, light-cljs-1013): its own keyed field `val` is its
  // value, which the public `val` field reads and writes.
  function ReducedCell(val) { this.val$LKEY = val; /*@brand*/ }
  //@unit reducedValues
  // The reduced values (`ReducedCell`): `has` tells one by its kind and own
  // `val` field, which `get` reads and `reset` replaces.
  const reducedValues = {
    has: value => value != null && value.cljs$lang$kind$$LKEY === 35 && "val$LKEY" in value,
    get(value) { if (reducedValues.has(value)) return value.val$LKEY; },
    reset(value, val) { value.val$LKEY = val; }
  };
  //@unit stringBufferWriters
  const stringBufferWriters = new WeakSet();
  //@unit constructorHashes
  const constructorHashes = new WeakMap();
  //@unit publicCollectionSlots
  const publicCollectionSlots = new WeakMap();
  //@unit iterateUnrealizedSeed
  const iterateUnrealizedSeed = Object.freeze({});
  //@unit vectorRoots
  const vectorRoots = new WeakMap();
  //@unit canonicalVectorRoots
  const canonicalVectorRoots = new WeakMap();
  //@unit metadataValues
  const metadataValues = new WeakMap();
  //@unit deferredVarMetadata
  const deferredVarMetadata = new WeakMap();
  //@unit UUIDCell
  // A UUID, upstream's UUID, with `UUIDType`'s prototype, kind 42 (`uuids`,
  // light-cljs-1013): its own keyed fields are its text, `uuid`, and its
  // `hash`, null until computed, which the public `uuid` and `__hash`
  // accessors of the value read, as the `uuids` and `uuidHashes` stores
  // held them.
  function UUIDCell(uuid, hash) {
    this.uuid$LKEY = uuid; this.hash$LKEY = hash;
    Object.defineProperties(this, {uuid: {get() { return this.uuid$LKEY; }},
      __hash: {get() { return this.hash$LKEY; }, set(hash) { this.hash$LKEY = hash; }}});
    /*@brand*/
  }
  //@unit uuids
  // The UUIDs (`UUIDCell`): `has` tells one by its kind and own `uuid`
  // field, `get` reads its text.
  const uuids = {
    has: value => value != null && value.cljs$lang$kind$$LKEY === 42 && "uuid$LKEY" in value,
    get(value) { if (uuids.has(value)) return value.uuid$LKEY; }
  };
  //@unit vars
  const vars = new WeakMap();
  //@unit isAtomCell
  // Whether a value is an atom of this runtime's class (`Atom`, kind 38,
  // with its own `state` field, upstream's); the Pants build's engine atoms
  // are not.
  const isAtomCell = value => value != null && value.cljs$lang$kind$$LKEY === 38 && "state" in value;
  //@unit atoms requires: isAtomCell
  // The atoms and the volatiles (light-cljs-1013): instances of `Atom` and
  // `Volatile`, whose prototypes' keyed kinds are 38 and 39, with their own
  // `state` field, upstream's.
  const atoms = {has: isAtomCell},
    volatiles = {has: value => value != null && value.cljs$lang$kind$$LKEY === 39 && "state" in value};
  //@unit QueueCell
  // A queue, upstream's PersistentQueue, with `PersistentQueueValue`'s
  // prototype, kind 36 (`queues`, light-cljs-1013): its own keyed field
  // `arr` holds its items in order, as the `queues` store held them,
  // unfrozen; nothing writes it once the queue is built (`conj` copies it).
  function QueueCell(arr) { this.arr$LKEY = arr; /*@brand*/ }
  //@unit queues
  // The queues (`QueueCell`): `has` tells one by its kind and own `arr`
  // field, `get` reads its items.
  const queues = {
    has: value => value != null && value.cljs$lang$kind$$LKEY === 36 && value.arr$LKEY !== undefined,
    get(value) { if (queues.has(value)) return value.arr$LKEY; }
  };
  //@unit DelayCell
  // A delay, upstream's Delay, with `Delay`'s prototype, kind 40 (`delays`,
  // light-cljs-1013): its own keyed field `state` holds its `thunk` and
  // `value`, which `deferredValue` realizes, as the `delays` store held it.
  function DelayCell(state) { this.state$LKEY = state; /*@brand*/ }
  //@unit delays
  // The delays (`DelayCell`): `has` tells one by its kind and own field,
  // `get` reads its state.
  const delays = {
    has: value => value != null && value.cljs$lang$kind$$LKEY === 40 && value.state$LKEY !== undefined,
    get(value) { if (delays.has(value)) return value.state$LKEY; }
  };
  //@unit EductionCell
  // An eduction, upstream's Eduction, with `Eduction`'s prototype, kind 43
  // (`eductions`, light-cljs-1013): its own `xform` and `coll`, upstream's
  // fields, are read-only and not enumerable, as before.
  function EductionCell(xform, coll) { Object.defineProperties(this, {xform: {value: xform}, coll: {value: coll}}); /*@brand*/ }
  //@unit eductions
  // The eductions (`EductionCell`): `has` tells one by its kind and its own
  // `coll`, `get` gives it, whose `xform` and `coll` its readers read.
  const eductions = {
    has: value => value != null && value.cljs$lang$kind$$LKEY === 43 && Object.prototype.hasOwnProperty.call(value, "coll"),
    get(value) { if (eductions.has(value)) return value; }
  };
  //@unit ifnValues
  const ifnValues = new WeakMap(), ifnVariadics = new WeakMap();
  //@unit metaFunctions
  const metaFunctions = new WeakMap();
  //@unit typeNames
  const typeNames = new WeakMap();
  //@unit typeFields
  // Type instances, and a record's type by instance. `makeType` writes
  // neither: its brands answer them (`typeFields.brands`).
  const typeFields = new WeakMap();
  //@unit recordTypes
  const recordTypes = new WeakMap();
  //@unit reifyValues
  const reifyValues = new WeakSet();
  //@unit protocolHooks
  // Generic operations' protocol dispatch, empty unless `installMethods` is kept.
  const protocolHooks = {};
  //@unit kindHooks
  // Generic operations' branches for one kind, installed by the kind's builders.
  const kindHooks = {};
  //@unit kindOps
  // The key under which a runtime type's prototype holds its kind's number
  // (set where the type is), which picks the kind's branches of the generic
  // operations (`kindTables`, light-cljs-237).
  const kindOps = Symbol("kind");
  //@unit noBranch
  // What a branch of a generic operation gives for a value it leaves to the
  // operation (`kindTables`): a marker of its own, which no operation gives
  // and no method receives, so that every other answer, `missing` among
  // them, is the branch's (light-cljs-387).
  const noBranch = Object.freeze({});
  //@unit kindTables requires: noBranch
  // The generic operations' branches by kind number, each kind's by
  // operation: 0 `seq`, 1 `first`, 2 `rest`, 3 `next`, 4 `count`, 5 `nth`,
  // 6 `get`, 7 `conj`, 8 `assoc`. The kinds are 1 lazy seqs (`LazySeq`),
  // 2 cells (`Cons`, `ListValue`, `IntegerRangeValue`,
  // `RangeValue`), 3 views (`IndexedSeqValue`, `ChunkedSeqValue`), which have
  // a branch of `first` only and take their number only where `first`'s
  // wrapper is kept, 4 vectors (`PersistentVectorValue`, `SubvecValue`),
  // 5 maps (`PersistentArrayMapValue`, `PersistentHashMapValue`,
  // `treeMapConstructor`), 6 sets (`PersistentHashSetValue`,
  // `treeSetConstructor`), 7 queues (`PersistentQueueValue`) and 8 map
  // entries (`MapEntryType`, light-cljs-373); tables 0 and 9 to 15, which
  // the wrappers' `kind & 15` reaches, are no kind's. A branch is an effect
  // of its kind's types that waits for its operation; until then, and in
  // those tables, the branch gives `noBranch`. `extend-type` and `specify!`
  // that reach a runtime kind empty the tables, as a branch could skip the
  // implementations they add.
  const kindTables = Array.from({length: 16}, () => Array(9).fill(() => noBranch));
  //@unit protocolMaskNames
  const protocolMaskNames = "IFn ICounted IEmptyableCollection ICollection IIndexed ASeq ISeq INext ILookup IAssociative IMap IMapEntry ISet IStack IVector IDeref IDerefWithTimeout IMeta IWithMeta IReduce IKVReduce IEquiv IHash ISeqable ISequential IList IRecord IReversible ISorted IPrintWithWriter IWriter IPrintWithWriter IPending IWatchable IEditableCollection ITransientCollection ITransientAssociative ITransientMap ITransientVector ITransientSet IMultiFn IChunkedSeq IChunkedNext IComparable INamed ICloneable IAtom IReset ISwap IIterable IDrop".split(" ");
  //@unit protocolMasks requires: protocolMaskNames
  // Membership checks are on every generic collection operation. Resolve the
  // fixed ABI once as [property, bit], including the last occurrence of
  // duplicate protocol names. Every mask, marker and slot name ends in the
  // build's key, a placeholder here that the output writer replaces
  // (`runtimeKey`, docs/value-layout.md, section 1): no host object holds
  // one by accident.
  const protocolMasks = new Map(protocolMaskNames.map((name, index) => [name,
    ["cljs$lang$protocol_mask$partition" + (index >> 5) + "$$LKEY", 1 << (index % 32)]]));
  //@unit memberMunging
  const memberMunging = {"-":"_",":":"_COLON_","+":"_PLUS_",">":"_GT_","<":"_LT_","=":"_EQ_","~":"_TILDE_","!":"_BANG_","@":"_CIRCA_","#":"_SHARP_","'":"_SINGLEQUOTE_","\"":"_DOUBLEQUOTE_","%":"_PERCENT_","^":"_CARET_","&":"_AMPERSAND_","*":"_STAR_","|":"_BAR_","{":"_LBRACE_","}":"_RBRACE_","[":"_LBRACK_","]":"_RBRACK_","/":"$","\\":"_BSLASH_","?":"_QMARK_"};
  //@unit mungeMember requires: memberMunging
  function mungeMember(name) { return Array.from(name, c => memberMunging[c] ?? c).join(""); }
  //@unit protocolSentinel
  const protocolSentinel = Object.freeze({}), protocolSlots = new Map();
  //@unit protocolSlot requires: mungeMember, protocolSlots
  function protocolSlot(key, arity) {
    let slots = protocolSlots.get(key);
    if (slots?.has(arity)) return slots.get(arity);
    if (!slots) { slots = new Map(); protocolSlots.set(key, slots); }
    const slash = key.indexOf("/");
    const qualified = arity === 0 ? slash >= 0 : key.indexOf("/", slash + 1) >= 0;
    const namespace = qualified ? key.slice(0, slash) : "cljs.core";
    const remainder = qualified ? key.slice(slash + 1) : key;
    const separator = remainder.startsWith("//") ? 1 : remainder.indexOf("/");
    const name = arity === 0 ? remainder : remainder.slice(0, separator);
    const method = remainder.slice(separator + 1);
    const prefix = mungeMember(namespace).replaceAll(".", "$") + "$" + mungeMember(name) + "$";
    const stem = arity === 0 ? prefix : prefix + (method === "/" ? "_SLASH_" : mungeMember(method)) + "$arity$" + arity;
    // The functions' invoke convention is not keyed (docs/value-layout.md,
    // section 1), as `installMethods` writes it and the analyzer reads it.
    const slot = arity !== 0 && stem.startsWith("cljs$core$IFn$_invoke$arity$") ? stem : stem + "$LKEY";
    slots.set(arity, slot); return slot;
  }
  //@unit methodKeys
  // The resolved keys (`methodKey`) the generic operations dispatch, by a
  // number each: the key of `ILookup/-lookup` is number 30, which the unit
  // `installMethods.ILookupLookupKey` installs.
  const methodKeys = [];
  //@unit methodKey requires: protocolMasks, protocolSlot
  // A core protocol method's key, or a core protocol's name, resolved once
  // (light-cljs-358): the protocol lookups (`ownMethod`, `publicMethod`,
  // `builtin`, `ownProtocol` and the `satisfies` hooks) take it in place of
  // the key's text and read the protocol's mask property and bit, its marker
  // and the method's slot for each arity from it, where they sliced the text
  // and looked it up in maps on every call. The protocol machinery
  // (`installMethods`) resolves each key a generic operation dispatches as it
  // loads, into `methodKeys` under a number of its own, from which the
  // operation reads it with a constant index; a program without protocol
  // implementations resolves none, and the operation, which then finds no
  // protocol lookup, reads none. `info` is that of `publicMethod` and the
  // `satisfies` hooks, which `extend-type` and `specify!` keep, filled in on
  // their first call with the key, as it reads protocols that load later.
  function methodKey(key) {
    const name = key.split("/")[0], mask = protocolMasks.get(name) ?? ["", 0];
    // `slots` holds the method's slot name by arity, each named on its first
    // dispatch (`keySlot`): naming every arity of every key as the runtime
    // loads took more memory than the evaluator's smallest heaps allow. It
    // inherits nothing a program could give arrays.
    return {key, name, mask: mask[0], bit: mask[1], marker: protocolSlot(name, 0), meta: name === "IMeta", info: undefined, cell: undefined, slots: Object.create(null)};
  }
  //@unit keySlot requires: protocolSlot
  // The slot name of a resolved key's method for `arity`, named once.
  function keySlot(key, arity) { return key.slots[arity] ?? (key.slots[arity] = protocolSlot(key.key, arity)); }
  //@unit installMethods.methodKey effect: installMethods when: coreMethod requires: methodKey,
  //@ protocolHooks
  protocolHooks.methodKey = methodKey;
  //@unit installMethods.FnKey effect: installMethods when: isFn requires: methodKey, methodKeys
  methodKeys[0] = methodKey("Fn");
  //@unit installMethods.IAssociativeAssocKey effect: installMethods when: assoc requires: methodKey,
  //@ methodKeys
  methodKeys[1] = methodKey("IAssociative/-assoc");
  //@unit installMethods.IAssociativeContainsKeyKey effect: installMethods when: contains requires: methodKey,
  //@ methodKeys
  methodKeys[2] = methodKey("IAssociative/-contains-key?");
  //@unit installMethods.IAssociativeKey effect: installMethods when: isAssociative requires: methodKey,
  //@ methodKeys
  methodKeys[3] = methodKey("IAssociative");
  //@unit installMethods.IChunkedNextKey effect: installMethods when: chunkNext requires: methodKey,
  //@ methodKeys
  methodKeys[4] = methodKey("IChunkedNext");
  //@unit installMethods.IChunkedSeqKey effect: installMethods when: concatMany, concatTwo, filter,
  //@ indexedSequence, isChunkedSeq, mapSeq requires: methodKey, methodKeys
  methodKeys[5] = methodKey("IChunkedSeq");
  //@unit installMethods.ICloneableKey effect: installMethods when: isCloneable requires: methodKey,
  //@ methodKeys
  methodKeys[6] = methodKey("ICloneable");
  //@unit installMethods.ICollectionConjKey effect: installMethods when: conj requires: methodKey,
  //@ methodKeys
  methodKeys[7] = methodKey("ICollection/-conj");
  //@unit installMethods.ICollectionKey effect: installMethods when: isColl requires: methodKey,
  //@ methodKeys
  methodKeys[8] = methodKey("ICollection");
  //@unit installMethods.IComparableCompareKey effect: installMethods when: compareKeys requires: methodKey,
  //@ methodKeys
  methodKeys[9] = methodKey("IComparable/-compare");
  //@unit installMethods.ICountedCountKey effect: installMethods when: count requires: methodKey,
  //@ methodKeys
  methodKeys[10] = methodKey("ICounted/-count");
  //@unit installMethods.ICountedKey effect: installMethods when: accumulatingCount, boundedCount,
  //@ isCounted, isEmpty, structuralEqualPair requires: methodKey, methodKeys
  methodKeys[11] = methodKey("ICounted");
  //@unit installMethods.IDerefDerefKey effect: installMethods when: derefProtocol,
  //@ Loadable.dispatch requires: methodKey, methodKeys
  methodKeys[12] = methodKey("IDeref/-deref");
  //@unit installMethods.IDropKey effect: installMethods when: drop requires: methodKey, methodKeys
  methodKeys[13] = methodKey("IDrop");
  //@unit installMethods.IEditableCollectionKey effect: installMethods when: into requires: methodKey,
  //@ methodKeys
  methodKeys[14] = methodKey("IEditableCollection");
  //@unit installMethods.IEmptyableCollectionEmptyKey effect: installMethods when: empty requires: methodKey,
  //@ methodKeys
  methodKeys[15] = methodKey("IEmptyableCollection/-empty");
  //@unit installMethods.IEncodeClojureKey effect: installMethods when: jsToClj requires: methodKey,
  //@ methodKeys
  methodKeys[16] = methodKey("IEncodeClojure");
  //@unit installMethods.IEncodeJSKey effect: installMethods when: cljToJs, keyToJs requires: methodKey,
  //@ methodKeys
  methodKeys[17] = methodKey("IEncodeJS");
  //@unit installMethods.IEquivEquivKey effect: installMethods when: arrayIndex, arrayMapDissoc,
  //@ entryIndex, equalPair, objectEquiv requires: methodKey, methodKeys
  methodKeys[18] = methodKey("IEquiv/-equiv");
  //@unit installMethods.IFindFindKey effect: installMethods when: find requires: methodKey,
  //@ methodKeys
  methodKeys[19] = methodKey("IFind/-find");
  //@unit installMethods.IFindKey effect: installMethods when: isIFind requires: methodKey,
  //@ methodKeys
  methodKeys[20] = methodKey("IFind");
  //@unit installMethods.IFnKey effect: installMethods when: isIFn requires: methodKey, methodKeys
  methodKeys[21] = methodKey("IFn");
  //@unit installMethods.IHashHashKey effect: installMethods when: hash requires: methodKey,
  //@ methodKeys
  methodKeys[22] = methodKey("IHash/-hash");
  //@unit installMethods.IHashKey effect: installMethods when: hash requires: methodKey, methodKeys
  methodKeys[23] = methodKey("IHash");
  //@unit installMethods.IIndexedKey effect: installMethods when: isIndexed, nth requires: methodKey,
  //@ methodKeys
  methodKeys[24] = methodKey("IIndexed");
  //@unit installMethods.IIndexedNthKey effect: installMethods when: nth, subvec, SubvecValue
  //@ requires: methodKey, methodKeys
  methodKeys[25] = methodKey("IIndexed/-nth");
  //@unit installMethods.IIterableIteratorKey effect: installMethods when: iterator requires: methodKey,
  //@ methodKeys
  methodKeys[26] = methodKey("IIterable/-iterator");
  //@unit installMethods.IIterableKey effect: installMethods when: isIterable, reduce,
  //@ sequenceIterator requires: methodKey, methodKeys
  methodKeys[27] = methodKey("IIterable");
  //@unit installMethods.IKVReduceKvReduceKey effect: installMethods when: reduceKv requires: methodKey,
  //@ methodKeys
  methodKeys[28] = methodKey("IKVReduce/-kv-reduce");
  //@unit installMethods.IListKey effect: installMethods when: isList requires: methodKey,
  //@ methodKeys
  methodKeys[29] = methodKey("IList");
  //@unit installMethods.ILookupLookupKey effect: installMethods when: get requires: methodKey,
  //@ methodKeys
  methodKeys[30] = methodKey("ILookup/-lookup");
  //@unit installMethods.IMapDissocKey effect: installMethods when: dissoc requires: methodKey,
  //@ methodKeys
  methodKeys[31] = methodKey("IMap/-dissoc");
  //@unit installMethods.IMapEntryKey effect: installMethods when: isMapEntry requires: methodKey,
  //@ methodKeys
  methodKeys[32] = methodKey("IMapEntry");
  //@unit installMethods.IMapKey effect: installMethods when: isMap, structuralEqualPair requires: methodKey,
  //@ methodKeys
  methodKeys[33] = methodKey("IMap");
  //@unit installMethods.IMetaKey effect: installMethods when: printsMeta requires: methodKey,
  //@ methodKeys
  methodKeys[34] = methodKey("IMeta");
  //@unit installMethods.IMetaMetaKey effect: installMethods when: meta, metaSupported requires: methodKey,
  //@ methodKeys
  methodKeys[35] = methodKey("IMeta/-meta");
  //@unit installMethods.INamedKey effect: installMethods when: name, namespace requires: methodKey,
  //@ methodKeys
  methodKeys[36] = methodKey("INamed");
  //@unit installMethods.INamedNameKey effect: installMethods when: name requires: methodKey,
  //@ methodKeys
  methodKeys[37] = methodKey("INamed/-name");
  //@unit installMethods.INamedNamespaceKey effect: installMethods when: namespace requires: methodKey,
  //@ methodKeys
  methodKeys[38] = methodKey("INamed/-namespace");
  //@unit installMethods.INextKey effect: installMethods when: KeySeqCell requires: methodKey,
  //@ methodKeys
  methodKeys[39] = methodKey("INext");
  //@unit installMethods.INextNextKey effect: installMethods when: next requires: methodKey,
  //@ methodKeys
  methodKeys[40] = methodKey("INext/-next");
  //@unit installMethods.IPrintWithWriterPrWriterKey effect: installMethods when: printWriterImpl
  //@ requires: methodKey, methodKeys
  methodKeys[41] = methodKey("IPrintWithWriter/-pr-writer");
  //@unit installMethods.IRecordKey effect: installMethods when: isRecord, structuralEqualPair
  //@ requires: methodKey, methodKeys
  methodKeys[42] = methodKey("IRecord");
  //@unit installMethods.IReduceKey effect: installMethods when: isReduceable requires: methodKey,
  //@ methodKeys
  methodKeys[43] = methodKey("IReduce");
  //@unit installMethods.IReduceReduceKey effect: installMethods when: reduce requires: methodKey,
  //@ methodKeys
  methodKeys[44] = methodKey("IReduce/-reduce");
  //@unit installMethods.IResetResetKey effect: installMethods when: resetProtocol requires: methodKey,
  //@ methodKeys
  methodKeys[45] = methodKey("IReset/-reset!");
  //@unit installMethods.IReversibleKey effect: installMethods when: isReversible requires: methodKey,
  //@ methodKeys
  methodKeys[46] = methodKey("IReversible");
  //@unit installMethods.IReversibleRseqKey effect: installMethods when: rseq requires: methodKey,
  //@ methodKeys
  methodKeys[47] = methodKey("IReversible/-rseq");
  //@unit installMethods.ISeqFirstKey effect: installMethods when: first, firstSeq requires: methodKey,
  //@ methodKeys
  methodKeys[48] = methodKey("ISeq/-first");
  //@unit installMethods.ISeqKey effect: installMethods when: cons, isSeq, nth requires: methodKey,
  //@ methodKeys
  methodKeys[49] = methodKey("ISeq");
  //@unit installMethods.ISeqRestKey effect: installMethods when: next, rest, restSeq requires: methodKey,
  //@ methodKeys
  methodKeys[50] = methodKey("ISeq/-rest");
  //@unit installMethods.ISeqableKey effect: installMethods when: count, isSeqable requires: methodKey,
  //@ methodKeys
  methodKeys[51] = methodKey("ISeqable");
  //@unit installMethods.ISeqableSeqKey effect: installMethods when: lazyBranch, reduce, rseq, seq,
  //@ seqBranch, vectorFirst, vectorRest requires: methodKey, methodKeys
  methodKeys[52] = methodKey("ISeqable/-seq");
  //@unit installMethods.ISequentialKey effect: installMethods when: isSequential, nth,
  //@ structuralEqualPair requires: methodKey, methodKeys
  methodKeys[53] = methodKey("ISequential");
  //@unit installMethods.ISetDisjoinKey effect: installMethods when: disj requires: methodKey,
  //@ methodKeys
  methodKeys[54] = methodKey("ISet/-disjoin");
  //@unit installMethods.ISetKey effect: installMethods when: isSet, structuralEqualPair requires: methodKey,
  //@ methodKeys
  methodKeys[55] = methodKey("ISet");
  //@unit installMethods.ISortedKey effect: installMethods when: isSorted requires: methodKey,
  //@ methodKeys
  methodKeys[56] = methodKey("ISorted");
  //@unit installMethods.IStackPeekKey effect: installMethods when: peek requires: methodKey,
  //@ methodKeys
  methodKeys[57] = methodKey("IStack/-peek");
  //@unit installMethods.IStackPopKey effect: installMethods when: pop requires: methodKey,
  //@ methodKeys
  methodKeys[58] = methodKey("IStack/-pop");
  //@unit installMethods.ISwapSwapKey effect: installMethods when: swapProtocol, swapState requires: methodKey,
  //@ methodKeys
  methodKeys[59] = methodKey("ISwap/-swap!");
  //@unit installMethods.IUUIDKey effect: installMethods when: isUuid requires: methodKey,
  //@ methodKeys
  methodKeys[60] = methodKey("IUUID");
  //@unit installMethods.IVectorKey effect: installMethods when: isVector, mapConj requires: methodKey,
  //@ methodKeys
  methodKeys[61] = methodKey("IVector");
  //@unit installMethods.IVolatileVresetKey effect: installMethods when: vresetProtocol requires: methodKey,
  //@ methodKeys
  methodKeys[62] = methodKey("IVolatile/-vreset!");
  //@unit installMethods.IWatchableAddWatchKey effect: installMethods when: addWatchProtocol
  //@ requires: methodKey, methodKeys
  methodKeys[63] = methodKey("IWatchable/-add-watch");
  //@unit installMethods.IWatchableNotifyWatchesKey effect: installMethods when: notifyWatches
  //@ requires: methodKey, methodKeys
  methodKeys[64] = methodKey("IWatchable/-notify-watches");
  //@unit installMethods.IWatchableRemoveWatchKey effect: installMethods when: removeWatchProtocol
  //@ requires: methodKey, methodKeys
  methodKeys[65] = methodKey("IWatchable/-remove-watch");
  //@unit installMethods.IWithMetaWithMetaKey effect: installMethods when: collectionWithMeta,
  //@ withMetaSupported requires: methodKey, methodKeys
  methodKeys[66] = methodKey("IWithMeta/-with-meta");
  //@unit installMethods.InstKey effect: installMethods when: isInst requires: methodKey, methodKeys
  methodKeys[67] = methodKey("Inst");
  //@unit builtinType mutator
  // Called once per type, before any of its values exist; a type whose base
  // is `Object.prototype` has that prototype already. Extension prototypes
  // are public and mutable; immutable collection payloads and their base
  // callability remain private and frozen.
  function builtinType(Type, base) {
    Type.prototype = Object.create(base);
    Object.defineProperty(Type.prototype, "constructor", {value: Type, writable: true, configurable: true});
    return Type;
  }
  //@unit reducedPrototype requires: unkeyedJSON
  // JSON of a reduced value: its own properties but its keyed field.
  const reducedPrototype = Object.freeze({toJSON() { return unkeyedJSON(this); }});
  //@unit Reduced.type effect: Reduced requires: builtinType, Reduced, ReducedCell,
  //@ reducedPrototype, viewType
  // A reduced value is a class of its type's prototype (`ReducedCell`,
  // light-cljs-1013).
  viewType(builtinType(Reduced, reducedPrototype), ReducedCell, 35);
  //@unit ReducedCell.type effect: ReducedCell requires: Reduced, ReducedCell
  ReducedCell.prototype = Reduced.prototype;
  //@unit sequencePrototype requires: count, objectEquiv, printHooks, sequentialIndex, unkeyedJSON
  const sequencePrototype = Object.freeze({
    equiv(other) { return objectEquiv(this, other); },
    // JSON of a seq of a class (`viewOf`): its own properties but its keyed
    // fields, as before it had them (docs/value-layout.md, question 3).
    // Named by the key, not by field: a destructuring would run the getters
    // of a cell's prototype (`cellOf`), which may call a program's function.
    toJSON() { return unkeyedJSON(this); },
    toString() { return printHooks.collection(this); },
    indexOf(value, start = 0) { return sequentialIndex(this, value, start, false); },
    lastIndexOf(value, start = count(this)) { return sequentialIndex(this, value, start, true); }
  });
  //@unit ListValue.type effect: ListValue requires: builtinType, kindOps, ListCell, ListValue,
  //@ sequencePrototype, viewType
  // The lists, conses and the other seqs of `cellOf` are classes of their
  // types' prototypes (light-cljs-1030), whose kinds (`kinds`) these units
  // set; the cells' kind table number stays where it was.
  viewType(builtinType(ListValue, sequencePrototype), ListCell, 12).prototype[kindOps] = 2;
  //@unit ListCell.type effect: ListCell requires: ListCell, ListValue
  // Operations that build a cell directly keep its type, whose unit above
  // gives it its prototype, kind and slots; so for each cell class.
  ListCell.prototype = ListValue.prototype;
  //@unit EmptyListValue.type effect: EmptyListValue requires: builtinType, EmptyListCell,
  //@ EmptyListValue, sequencePrototype, viewType
  viewType(builtinType(EmptyListValue, sequencePrototype), EmptyListCell, 13);
  //@unit EmptyListCell.type effect: EmptyListCell requires: EmptyListCell, EmptyListValue
  EmptyListCell.prototype = EmptyListValue.prototype;
  //@unit Cons.type effect: Cons requires: builtinType, Cons, ConsCell, kindOps, sequencePrototype,
  //@ viewType
  viewType(builtinType(Cons, sequencePrototype), ConsCell, 14).prototype[kindOps] = 2;
  //@unit ConsCell.type effect: ConsCell requires: Cons, ConsCell
  ConsCell.prototype = Cons.prototype;
  //@unit ReSeqCell.type effect: ReSeqCell requires: ReSeqCell, sequencePrototype
  // `re-seq`'s conses keep a prototype of no public type.
  ReSeqCell.prototype = Object.create(sequencePrototype);
  ReSeqCell.prototype.cljs$lang$kind$$LKEY = 27;
  //@unit IndexedSeqValue.type effect: IndexedSeqValue requires: builtinType, IndexedSeqCell,
  //@ IndexedSeqValue, sequencePrototype, viewType
  viewType(builtinType(IndexedSeqValue, sequencePrototype), IndexedSeqCell, 5);
  //@unit ChunkedSeqValue.type effect: ChunkedSeqValue requires: builtinType, ChunkedSeqCell,
  //@ ChunkedSeqValue, sequencePrototype, viewType
  viewType(builtinType(ChunkedSeqValue, sequencePrototype), ChunkedSeqCell, 6);
  //@unit IndexedSeqCell.type effect: IndexedSeqCell requires: IndexedSeqCell, IndexedSeqValue
  // Operations that build an indexed seq directly keep its type, whose unit
  // above gives it its prototype, kind and slots.
  IndexedSeqCell.prototype = IndexedSeqValue.prototype;
  //@unit ChunkedSeqCell.type effect: ChunkedSeqCell requires: ChunkedSeqCell, ChunkedSeqValue
  ChunkedSeqCell.prototype = ChunkedSeqValue.prototype;
  //@unit ChunkedCons.type effect: ChunkedCons requires: builtinType, ChunkedCons, ChunkedConsCell,
  //@ sequencePrototype, viewType
  viewType(builtinType(ChunkedCons, sequencePrototype), ChunkedConsCell, 17);
  //@unit LazySeq.type effect: LazySeq requires: builtinType, kindOps, LazySeq, LazySeqCell,
  //@ sequencePrototype, viewType
  viewType(builtinType(LazySeq, sequencePrototype), LazySeqCell, 16).prototype[kindOps] = 1;
  //@unit arrayChunk.type effect: arrayChunk requires: arrayChunk, ArrayChunkCell, viewType
  // A chunk's JSON is its own properties but its keyed fields, as before it
  // had them (docs/value-layout.md, question 3); so is a chunk buffer's.
  viewType(arrayChunk, ArrayChunkCell, 18);
  Object.defineProperty(arrayChunk.prototype, "toJSON", {configurable: true, writable: true,
    value() { const {arr$LKEY: _a, off$LKEY: _o, end$LKEY: _e, first$LKEY: _s, step$LKEY: _t, ...json} = this; return json; }});
  //@unit ChunkBuffer.type effect: ChunkBuffer requires: ChunkBuffer, ChunkBufferCell, viewType
  viewType(ChunkBuffer, ChunkBufferCell, 19);
  Object.defineProperty(ChunkBuffer.prototype, "toJSON", {configurable: true, writable: true,
    value() { const {buf$LKEY: _b, end$LKEY: _e, ...json} = this; return json; }});
  //@unit PersistentQueueValue.type effect: PersistentQueueValue requires: builtinType, kindOps,
  //@ PersistentQueueValue, QueueCell, sequencePrototype, viewType
  // A queue is a class of its type's prototype (`QueueCell`,
  // light-cljs-1013); the kind table number stays until light-cljs-1015.
  viewType(builtinType(PersistentQueueValue, sequencePrototype), QueueCell, 36).prototype[kindOps] = 7;
  //@unit QueueCell.type effect: QueueCell requires: PersistentQueueValue, QueueCell
  QueueCell.prototype = PersistentQueueValue.prototype;
  //@unit RepeatValue.type effect: RepeatValue requires: builtinType, emptyList, RepeatCell,
  //@ RepeatValue, sequencePrototype, viewType
  // A repeat's `rest` is its `next`, built on the first step (itself for
  // one without end, the empty list after its last item).
  viewType(builtinType(RepeatValue, sequencePrototype), RepeatCell, 23);
  Object.defineProperty(RepeatValue.prototype, "rest$LKEY", {configurable: true, get() {
    const remaining = this.remaining$LKEY;
    return this.next$LKEY ?? (remaining === -1 ? this : remaining > 1
      ? (this.next$LKEY = RepeatValue(null, remaining - 1, this.first$LKEY, null, null)) : emptyList);
  }});
  //@unit RepeatCell.type effect: RepeatCell requires: RepeatCell, RepeatValue
  RepeatCell.prototype = RepeatValue.prototype;
  //@unit CycleValue.type effect: CycleValue requires: builtinType, CycleCell, cycleCurrent,
  //@ cycleNode, CycleValue, first, sequencePrototype, viewType
  // A cycle's `first` is the first item of its current seq (`cycleCurrent`)
  // and its `rest` the cycle from that seq's next, built once.
  viewType(builtinType(CycleValue, sequencePrototype), CycleCell, 25);
  Object.defineProperties(CycleValue.prototype, {
    first$LKEY: {configurable: true, get() { return first(cycleCurrent(this)); }},
    rest$LKEY: {configurable: true, get() { return this.next$LKEY ?? (this.next$LKEY = cycleNode(this.all$LKEY, cycleCurrent(this), null)); }},
    realized$LKEY: {configurable: true, get() { return this.current$LKEY != null; }}
  });
  //@unit CycleCell.type effect: CycleCell requires: CycleCell, CycleValue
  CycleCell.prototype = CycleValue.prototype;
  //@unit IterateValue.type effect: IterateValue requires: builtinType, first, IterateCell,
  //@ iterateNode, iterateUnrealizedSeed, IterateValue, sequencePrototype, viewType
  // An iterate's `first` is its seed, which the first read computes from
  // the previous one, and its `rest` the next step, built once.
  viewType(builtinType(IterateValue, sequencePrototype), IterateCell, 24);
  Object.defineProperties(IterateValue.prototype, {
    first$LKEY: {configurable: true, get() {
      if (!this.realized$LKEY) { this.seed$LKEY = this.f$LKEY.call(null, this.prev$LKEY); this.realized$LKEY = true; }
      return this.seed$LKEY;
    }},
    rest$LKEY: {configurable: true, get() { return this.next$LKEY ?? (this.next$LKEY = iterateNode(this.f$LKEY, first(this), iterateUnrealizedSeed, false)); }}
  });
  //@unit IterateCell.type effect: IterateCell requires: IterateCell, IterateValue
  IterateCell.prototype = IterateValue.prototype;
  //@unit rangeRest requires: emptyList, rangeNode
  // A range's `rest`, a getter of both range types' prototypes: the range
  // from the next item, or the empty list after the last one.
  const rangeRest = {configurable: true, get() {
    const following = this.first$LKEY + this.step$LKEY, end = this.end$LKEY, step = this.step$LKEY;
    return (step > 0 ? following < end : following > end) ? rangeNode(following, end, step, this.integer$LKEY) : emptyList;
  }};
  //@unit RangeValue.type effect: RangeValue requires: builtinType, kindOps, RangeCell, rangeRest,
  //@ RangeValue, sequencePrototype, viewType
  viewType(builtinType(RangeValue, sequencePrototype), RangeCell, 22).prototype[kindOps] = 2;
  Object.defineProperty(RangeValue.prototype, "rest$LKEY", rangeRest);
  //@unit RangeCell.type effect: RangeCell requires: RangeCell, RangeValue
  RangeCell.prototype = RangeValue.prototype;
  //@unit IntegerRangeValue.type effect: IntegerRangeValue requires: builtinType, IntegerRangeCell,
  //@ IntegerRangeValue, kindOps, rangeRest, sequencePrototype, viewType
  viewType(builtinType(IntegerRangeValue, sequencePrototype), IntegerRangeCell, 15).prototype[kindOps] = 2;
  Object.defineProperty(IntegerRangeValue.prototype, "rest$LKEY", rangeRest);
  //@unit IntegerRangeCell.type effect: IntegerRangeCell requires: IntegerRangeCell,
  //@ IntegerRangeValue
  IntegerRangeCell.prototype = IntegerRangeValue.prototype;
  //@unit NodeSeqType effect requires: builtinType, NodeSeqCell, sequencePrototype, viewType
  const NodeSeqType = viewType(builtinType(function () {}, sequencePrototype), NodeSeqCell, 7);
  //@unit PersistentArrayMapSeqType effect requires: ArrayMapSeqCell, builtinType,
  //@ sequencePrototype, viewType
  // The seq of an array map, as NodeSeqType is a hash map's.
  const PersistentArrayMapSeqType = viewType(builtinType(function () {}, sequencePrototype), ArrayMapSeqCell, 9);
  //@unit ArrayNodeSeqType effect requires: ArrayNodeSeqCell, builtinType, sequencePrototype,
  //@ viewType
  const ArrayNodeSeqType = viewType(builtinType(function () {}, sequencePrototype), ArrayNodeSeqCell, 8);
  //@unit RSeqType effect requires: builtinType, RSeqCell, sequencePrototype, viewType
  const RSeqType = viewType(builtinType(function () {}, sequencePrototype), RSeqCell, 10);
  //@unit ArrayList
  // Upstream types no light-cljs value has (lite mode's, and its collections'
  // internals): a protocol extended to one never dispatches.
  function ArrayList() {}
  //@unit ArrayNode
  function ArrayNode() {}
  //@unit ArrayNodeIterator
  function ArrayNodeIterator() {}
  //@unit BitmapIndexedNode
  function BitmapIndexedNode() {}
  //@unit Box
  function Box() {}
  //@unit HashCollisionNode
  function HashCollisionNode() {}
  //@unit HashMapIter
  function HashMapIter() {}
  //@unit HashMapLite
  function HashMapLite() {}
  //@unit HashSetIter
  function HashSetIter() {}
  //@unit NeverEquiv
  function NeverEquiv() {}
  //@unit ObjMap
  function ObjMap() {}
  //@unit ObjMapIterator
  function ObjMapIterator() {}
  //@unit SetLite
  function SetLite() {}
  //@unit VectorLite
  function VectorLite() {}
  //@unit VectorLiteIterator
  function VectorLiteIterator() {}
  //@unit VectorNode
  function VectorNode() {}
  //@unit treeMapSeqConstructor.type effect: treeMapSeqConstructor requires: builtinType,
  //@ sequencePrototype, treeMapSeqConstructor, TreeMapStackCell, viewType
  // Its values are cells of their stacks (`TreeMapStackCell`), kind 11, the
  // last of the views' kinds, as before light-cljs-1035 gave a sorted map's
  // seqs, `rseq` and `subseq` of its tree (`treeMapSeq`) and no seq of
  // this type a view of an array; a cell holds no `arr`, so is no view.
  viewType(builtinType(treeMapSeqConstructor, sequencePrototype), TreeMapStackCell, 11);
  //@unit TreeMapStackCell.type effect: TreeMapStackCell requires: emptyList, isListCell, peek,
  //@ treeMapSeqConstructor, TreeMapStackCell, treeSeqNext
  // A sorted map's seq is a cell of the constructor's prototype: its `first`
  // is the node atop its stack, its `rest` the seq of the stack past it
  // (`treeSeqNext`), or the empty list.
  TreeMapStackCell.prototype = treeMapSeqConstructor.prototype;
  Object.defineProperties(treeMapSeqConstructor.prototype, {
    first$LKEY: {configurable: true, get() { const stack = this.stack$LKEY; return isListCell(stack) ? stack.first$LKEY : peek(stack); }},
    rest$LKEY: {configurable: true, get() { return treeSeqNext(this) ?? emptyList; }}
  });
  //@unit emptyList requires: EmptyListCell
  const emptyList = new EmptyListCell();
  //@unit missing
  const missing = Object.freeze({});
  //@unit truth
  const truth = value => value != null && value !== false;
  //@unit invalidArity
  const invalidArity = n => { throw new Error("Invalid arity: " + n); };
  //@unit outOfBounds
  const outOfBounds = () => { throw new Error("Index out of bounds"); };

  //@unit keywordPrototype requires: get, invalidArity, keywordOf, objectEquiv
  // A keyword called as a function is `get` of its argument by itself
  // (upstream's `-invoke`); it reads its own `fqn`, no store. Its `IFn`
  // arity entries, which `apply` calls, are its `call`, so that an
  // `extend-type` of `IFn` replacing `call` answers through them too.
  const keywordPrototype = Object.freeze({
    cljs$core$IFn$_invoke$arity$1(collection) { return this.call(null, collection); },
    cljs$core$IFn$_invoke$arity$2(collection, notFound) { return this.call(null, collection, notFound); },
    equiv(other) { return objectEquiv(this, other); },
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    call(_self, collection, notFound) {
      if (keywordOf(this) === undefined) throw new Error("Expected keyword");
      switch (arguments.length) {
        case 2: return get(collection, this);
        case 3: return get(collection, this, notFound);
        default: return invalidArity(arguments.length - 1);
      }
    },
    // JSON of a keyword: its own properties but its keyed fields, as before
    // it had them (docs/value-layout.md, question 3).
    toJSON() { const {ns$LKEY: _n, name$LKEY: _m, fqn$LKEY: _f, hash$LKEY: _h, ...json} = this; return json; },
    toString() {
      const value = keywordOf(this);
      if (!value) throw new Error("Expected keyword");
      return ":" + value.fqn$LKEY;
    }
  });
  //@unit KeywordValue.type effect: KeywordValue requires: builtinType, keywordPrototype,
  //@ KeywordValue
  builtinType(KeywordValue, keywordPrototype);
  //@unit KeywordCell.type effect: KeywordCell requires: KeywordCell, KeywordValue
  // Keywords are kind 20 (`keywordOf`); a keyword cell has their prototype,
  // which the unit above builds.
  KeywordValue.prototype.cljs$lang$kind$$LKEY = 20;
  KeywordCell.prototype = KeywordValue.prototype;

  //@unit symbolPrototype requires: get, invalidArity, objectEquiv, symbolOf
  const symbolPrototype = Object.freeze({
    equiv(other) { return objectEquiv(this, other); },
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    call(_self, collection, notFound) {
      if (symbolOf(this) === undefined) throw new Error("Expected symbol");
      switch (arguments.length) {
        case 2: return get(collection, this);
        case 3: return get(collection, this, notFound);
        default: return invalidArity(arguments.length - 1);
      }
    },
    toJSON() { const {ns$LKEY: _n, name$LKEY: _m, str$LKEY: _s, hash$LKEY: _h, meta$LKEY: _t, ...json} = this; return json; },
    toString() {
      const value = symbolOf(this);
      if (!value) throw new Error("Expected symbol");
      return value.str$LKEY;
    }
  });
  //@unit SymbolValue.type effect: SymbolValue requires: builtinType, symbolPrototype, SymbolValue
  builtinType(SymbolValue, symbolPrototype);
  //@unit SymbolCell.type effect: SymbolCell requires: SymbolCell, SymbolValue
  // Symbols are kind 21 (`symbolOf`); a symbol cell has their prototype.
  SymbolValue.prototype.cljs$lang$kind$$LKEY = 21;
  SymbolCell.prototype = SymbolValue.prototype;

  //@unit SymbolValue requires: invalidArity, SymbolCell
  // Positional constructors retain the supplied slots; unlike cons/symbol they
  // do not normalize a tail or derive the printed spelling from name/namespace.
  // A null hash reads as none: `hash` computes it, the `_hash` field gives
  // null (an undefined one is kept, as the field gives it back).
  function SymbolValue(namespace, name, spelling, cachedHash, metadata) {
    if (arguments.length !== 5) return invalidArity(arguments.length);
    return new SymbolCell(namespace, name, spelling, cachedHash, metadata);
  }

  //@unit Cons requires: ConsCell, constructorHashes, invalidArity, metadataValues
  function Cons(metadata, head, tail, cachedHash) {
    if (arguments.length !== 4) return invalidArity(arguments.length);
    const value = new ConsCell(head, tail);
    if (cachedHash !== null) constructorHashes.set(value, cachedHash);
    metadataValues.set(value, metadata);
    return value;
  }

  //@unit constructorSlots requires: constructorHashes, metadataValues, publicCollectionSlots
  function constructorSlots(value, slots, metadata, cachedHash) {
    publicCollectionSlots.set(value, slots);
    metadataValues.set(value, metadata);
    if (cachedHash !== null) constructorHashes.set(value, cachedHash);
    return value;
  }
  //@unit ListValue requires: constructorSlots, ListCell
  function ListValue(metadata, head, tail, suppliedCount, cachedHash) {
    return constructorSlots(new ListCell(head, tail, suppliedCount), {}, metadata, cachedHash);
  }
  //@unit EmptyListValue requires: EmptyListCell, metadataValues
  function EmptyListValue(metadata) {
    const value = new EmptyListCell();
    metadataValues.set(value, metadata);
    return value;
  }
  //@unit KeywordValue requires: makeKeyword
  function KeywordValue(namespace, name, spelling, cachedHash) {
    return makeKeyword(namespace, name, spelling, cachedHash);
  }
  //@unit CycleValue requires: cycleNode, metadataValues
  function CycleValue(metadata, all, previous, current, following) {
    const value = cycleNode(all, previous, current, following);
    metadataValues.set(value, metadata);
    return value;
  }
  //@unit RepeatValue requires: constructorHashes, metadataValues, RepeatCell
  function RepeatValue(metadata, remaining, head, following, cachedHash) {
    const value = new RepeatCell(remaining, head, following);
    // Each step of `repeat` builds one: nil metadata and a nil hash are not
    // stored, and the compiler form bridge reads a repeat as having nil
    // metadata, as it reads a lazy seq (light-cljs-291).
    if (metadata !== null) metadataValues.set(value, metadata);
    if (cachedHash !== null) constructorHashes.set(value, cachedHash);
    return value;
  }
  //@unit IterateValue requires: iterateNode, iterateUnrealizedSeed, metadataValues
  function IterateValue(metadata, fn, previous, seed, following) {
    const value = iterateNode(fn, previous, seed, seed !== iterateUnrealizedSeed, following);
    metadataValues.set(value, metadata);
    return value;
  }
  //@unit PersistentVectorValue requires: makeVector, publicCollectionSlots, VectorCell, vectorRoots
  // Root slots are opaque canonical tokens, never an upstream trie-node ABI:
  // a token names the private root of the vector whose `root` field gave it.
  // The supplied tail is shared, as upstream; the metadata and the hash are
  // kept as given, an undefined one too, as the public slots keep them.
  function PersistentVectorValue(metadata, cnt, shift, root, tail, cachedHash) {
    // `callerTail` marks the tail, which vectors that keep it inherit, as
    // the caller's: no seq freezes it (`vectorChunk`).
    const store = new VectorCell(metadata, cnt, shift, vectorRoots.get(root) ?? [], tail, cachedHash);
    store.callerTail$LKEY = true;
    const value = makeVector(null, store);
    publicCollectionSlots.set(value, {cnt, shift, root, tail});
    return value;
  }
  //@unit subvecOfTrie requires: SubvecCell, VectorCell
  // A subvector whose store is the vector `trie`'s trie, not a view: of the
  // items a protocol implementation gave (`subvec`), or of getters
  // (`SubvecValue`).
  const subvecOfTrie = trie => Reflect.construct(VectorCell, [null, trie.cnt$LKEY, trie.shift$LKEY, trie.root$LKEY, trie.tail$LKEY, null], SubvecCell);
  //@unit SubvecValue requires: methodKeys, nth, protocolHooks, publicCollectionSlots, subvecOfTrie,
  //@ subvecSources, vectorStore, vectorTrie, vectorView
  function SubvecValue(metadata, source, start, end, cachedHash) {
    const store = vectorStore(source), cnt = Math.max(0, end - start);
    let value;
    // A view of a vector's store holding every index it names, as `subvec`;
    // else items that ask `nth` of the source when read.
    if (store && Number.isInteger(start) && Number.isInteger(end) && start >= 0 && start + cnt <= store.cnt$LKEY
      && !protocolHooks.publicMethod?.(source, methodKeys[25], 2, true, true)) value = vectorView(store, start, cnt);
    else {
      // Every leaf keeps the getters, and the store is marked as reading a
      // caller's array (`callerTail`), so that no copy of an item is kept.
      const getters = (from, to) => {
        const node = [];
        for (let i = from; i < to; i++) Object.defineProperty(node, i - from, {enumerable: true, get: () => nth(source, start + i)});
        return node;
      };
      value = subvecOfTrie(vectorTrie(getters(0, cnt), getters));
      value.callerTail$LKEY = true;
    }
    value.meta$LKEY = metadata;
    value.hash$LKEY = cachedHash;
    subvecSources.set(value, source);
    publicCollectionSlots.set(value, {start, end});
    return value;
  }
  //@unit ChunkedSeqValue requires: ChunkedSeqCell, constructorHashes, metadataValues
  function ChunkedSeqValue(source, node, index, offset, metadata, cachedHash) {
    const value = new ChunkedSeqCell(node, offset, source, index);
    // Without metadata or a hash, which a vector's seq has at every step,
    // no entry reads differently from a null one; with either, both are
    // kept as given, an undefined one too, as the public slots kept them.
    if (metadata == null && cachedHash == null) return value;
    metadataValues.set(value, metadata);
    if (cachedHash !== null) constructorHashes.set(value, cachedHash);
    return value;
  }
  //@unit PersistentQueueValue requires: constructorSlots, queueFromArray
  function PersistentQueueValue(metadata, cnt, front, rear, cachedHash) {
    const value = queueFromArray([]);
    return constructorSlots(value, {count: cnt, front, rear}, metadata, cachedHash);
  }
  //@unit PersistentArrayMapValue requires: typeHooks
  // The array map type, which builders instantiate with `makeMap`. Called, it
  // is `publicArrayMap`, which a program naming `PersistentArrayMap` keeps.
  function PersistentArrayMapValue(metadata, cnt, array, cachedHash) {
    return typeHooks.arrayMap(metadata, cnt, array, cachedHash);
  }
  //@unit publicArrayMap requires: ArrayMapCell, constructorSlots
  // The public constructor, whose maps keep the positional array and count it
  // received as their `arr` and `cnt`, in their fields as in the slots.
  // Positional arrays retain duplicate keys and their first-match lookup.
  function publicArrayMap(metadata, cnt, array, cachedHash) {
    // A missing array throws, as before the map held it.
    void array.length;
    return constructorSlots(new ArrayMapCell(cnt, array), {cnt, arr: array}, metadata, cachedHash);
  }
  //@unit PersistentHashMapValue requires: constructorSlots, ensureHashRoot, hash, isHashMap,
  //@ makeMap, mapPairs
  // The public constructor (light-cljs-1036): its map holds the count it is
  // given and its nil entry in its fields (`HashMapCell`), as upstream's
  // `cnt`, `has-nil?` and `nil-val`, so that its operations read them as
  // any hash map's; the slots keep the arguments for its public fields
  // alone. A hash map root shares its trie; another map's keys but nil are
  // hashed again.
  function PersistentHashMapValue(metadata, cnt, root, hasNil, nilValue, cachedHash) {
    let trie = null;
    if (isHashMap(root)) trie = root.root$LKEY;
    else {
      const pairs = root == null ? undefined : mapPairs(root), entries = [];
      // Each key and value is read once, in order, as its entries read them:
      // a public constructor's array may hold getters.
      for (let i = 0; pairs !== undefined && i < pairs.length; i += 2) {
        const key = pairs[i], value = pairs[i + 1];
        if (key != null) entries.push({key, value, hash: hash(key)});
      }
      trie = ensureHashRoot(entries);
    }
    // An undefined count stays undefined, an own field all the same.
    const nil = hasNil ? {key: null, value: nilValue, hash: 0} : null;
    return constructorSlots(makeMap(PersistentHashMapValue, {root: trie, nil, length: cnt}, "hash-map"), {root, "has-nil?": hasNil, "nil-val": nilValue}, metadata, cachedHash);
  }
  //@unit PersistentHashSetValue requires: typeHooks
  // The hash set type, which builders instantiate with `makeSet`. Called, it
  // is `publicHashSet`, which a program naming `PersistentHashSet` keeps.
  function PersistentHashSetValue(metadata, source, cachedHash) {
    return typeHooks.hashSet(metadata, source, cachedHash);
  }
  //@unit publicHashSet requires: constructorSlots, hashTrie, makeSet, mapKinds, mapTraversal,
  //@ PersistentHashSetValue
  // The public constructor: a set of a map's keys, of the map's kind (an
  // array set otherwise only comes from builders, which never build a set of
  // another kind without promoting it in `putMember`). A set of the hash-map
  // kind keeps its members in a trie, as a hash map (light-cljs-236).
  function publicHashSet(metadata, source, cachedHash) {
    const entries = (mapTraversal(source) ?? []).map(e => Object.freeze({key: e.key, hash: e.hash})), kind = mapKinds.get(source);
    return constructorSlots(makeSet(PersistentHashSetValue, kind === "hash-map" ? hashTrie(entries) : entries, kind), {"hash-map": source}, metadata, cachedHash);
  }

  //@unit Reduced requires: invalidArity, reduced
  function Reduced(value) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    return reduced(value);
  }

  //@unit symbol requires: SymbolCell
  function symbol(namespace, name) {
    return new SymbolCell(namespace, name, namespace == null ? name : namespace + "/" + name, null, null);
  }

  //@unit symbolCore requires: invalidArity, keywordOf, symbol, symbolOf, vars
  function symbolCore(namespace, name) {
    switch (arguments.length) {
      case 1: {
        if (symbolOf(namespace) !== undefined) return namespace;
        if (vars.has(namespace)) return vars.get(namespace).symbol;
        const kw = keywordOf(namespace);
        if (kw) namespace = kw.fqn$LKEY;
        if (typeof namespace !== "string") throw new Error("no conversion to symbol");
        const slash = namespace.indexOf("/");
        return slash < 1 ? symbol(null, namespace)
          : symbol(namespace.slice(0, slash), namespace.slice(slash + 1));
      }
      case 2:
        if ((namespace != null && typeof namespace !== "string") || typeof name !== "string")
          throw new Error("symbol expects strings and an optional nil namespace");
        return symbol(namespace, name);
      default: return invalidArity(arguments.length);
    }
  }

  //@unit gensymCounter
  let gensymCounter = 0;
  //@unit generatedNames
  const generatedNames = new Set();
  //@unit gensym requires: generatedNames, gensymCounter, invalidArity, str, symbolCore
  function gensym(prefix) {
    if (arguments.length > 1) return invalidArity(arguments.length);
    const base = str(arguments.length === 0 ? "G__" : prefix);
    let name; do { name = base + ++gensymCounter; } while (generatedNames.has(name));
    generatedNames.add(name);
    return symbolCore(name);
  }

  //@unit macroexpandServices
  // Evaluators install their expansion services without replacing core or
  // coupling this runtime to a particular compiler or JavaScript engine.
  let macroexpandServices = {};
  //@unit setMacroexpandServices compile-time requires: macroexpandServices
  function setMacroexpandServices(services) {
    const next = services == null ? {} : services;
    if (typeof next !== "object") throw new TypeError("Expected macroexpand services object");
    const {macroexpand, macroexpand1} = next;
    for (const service of [macroexpand, macroexpand1]) {
      if (service != null && typeof service !== "function") throw new TypeError("Expected macroexpand service function");
    }
    macroexpandServices = {macroexpand, macroexpand1};
  }
  //@unit expansionService requires: invalidArity, macroexpandServices
  function expansionService(name, displayName) {
    return function (form) {
      if (arguments.length !== 1) return invalidArity(arguments.length);
      const service = macroexpandServices[name];
      if (service == null) throw new Error(displayName + " service is not installed");
      return service.call(null, form);
    };
  }
  //@unit macroexpand requires: expansionService
  const macroexpand = expansionService("macroexpand", "macroexpand");
  //@unit macroexpand1 requires: expansionService
  const macroexpand1 = expansionService("macroexpand1", "macroexpand-1");

  //@unit cons requires: ConsCell, count, ListCell, methodKeys, protocolHooks, seq, seqValue
  function cons(head, tail, kind = "cons") {
    if (kind === "cons") {
      if (tail == null) kind = "list";
      else if (!(protocolHooks.implements?.(methodKeys[49], tail) ?? seqValue(tail))) tail = seq(tail);
    }
    return kind === "list" ? new ListCell(head, tail, count(tail) + 1) : new ConsCell(head, tail);
  }

  //@unit listFromArray requires: emptyList, ListCell
  // Each tail was just built here. Its stored count is intrinsic, independent
  // of public ICounted overrides, as in List.-conj; no protocol dispatch is
  // needed to construct it. Later public count calls still honor overrides.
  function listFromArray(items) {
    let result = emptyList, count = 0;
    for (let i = items.length - 1; i >= 0; i--) {
      result = new ListCell(items[i], result, ++count);
    }
    return result;
  }

  //@unit list requires: listFromArray
  function list(...items) { return listFromArray(items); }

  //@unit view
  // A seq over `storage` from `offset` (`viewOf`), of the type the caller
  // builds (IndexedSeq, NodeSeq or PersistentTreeMapSeq for `kind`), so that
  // a caller keeps only the types it can build: one object, of the class
  // its prototype names (`viewType`). A view rebuilt further on is of the
  // same class (`viewAt`).
  function view(Type, storage, offset, indexed, kind = "indexed") {
    const Cell = Type.prototype.cljs$lang$cell$$LKEY;
    return new Cell(storage, offset, indexed, kind);
  }
  //@unit viewAt
  // A view of `slice`'s class, storage and kind from `offset`; a chunked
  // seq's class takes its vector and base where the others take `indexed`
  // and the kind.
  function viewAt(slice, offset) {
    const Cell = slice.cljs$lang$cell$$LKEY;
    return slice.view$LKEY === "chunked" ? new Cell(slice.arr$LKEY, offset, slice.vec$LKEY, slice.base$LKEY)
      : new Cell(slice.arr$LKEY, offset, slice.indexed$LKEY, slice.view$LKEY);
  }

  //@unit seq requires: cellOf, IndexedSeqCell, isChunkedCons, iteratorSeq, iteratorSeqs, kindHooks,
  //@ lazyOf, maps, publicCollectionSlots, queues, sets, vectorStore, viewOf
  function seq(value) {
    if (value == null) return null;
    const cell = cellOf(value);
    if (cell !== undefined) return cell.cell$LKEY === "list" && cell.count$LKEY === 0 && !publicCollectionSlots.has(value) ? null : value;
    const lazy = lazyOf(value);
    if (lazy) return kindHooks.seqLazy(lazy);
    const queue = queues.get(value);
    if (queue) return kindHooks.seqQueue(value, queue);
    if (Array.isArray(value)) return value.length ? new IndexedSeqCell(value, 0, true, "indexed") : null;
    if (isChunkedCons(value)) return value;
    const v = viewOf(value);
    if (v !== undefined) return v.i$LKEY < v.arr$LKEY.length ? value : null;
    const items = vectorStore(value);
    if (items) return kindHooks.seqVector(value, items);
    if (typeof value === "string") return value.length ? new IndexedSeqCell(value, 0, true, "indexed") : null;
    if (maps.has(value)) return kindHooks.seqMap(value);
    const members = sets.get(value);
    if (members) return kindHooks.seqSet(value, members);
    if (iteratorSeqs.has(value)) return value;
    if (value[Symbol.iterator] != null) return iteratorSeq(value[Symbol.iterator]());
    throw new Error(String(value) + " is not ISeqable");
  }
  //@unit seq.branches effect: Cons, IntegerRangeValue, LazySeq, ListValue, MapEntryType,
  //@ PersistentArrayMapValue, PersistentHashMapValue, PersistentHashSetValue, PersistentQueueValue,
  //@ PersistentVectorValue, RangeValue, SubvecValue, treeMapConstructor, treeSetConstructor when: seq
  //@ requires: kindOps, kindTables, noBranch, seq
  // An operation of one value behind its branches for the kinds
  // (`kindTables`), under its name: the effects of the kinds' types wrap the
  // operation where one of them can be built, inside the protocol wrapper
  // (`behindProtocol`), whose implementations come first. A call reads the
  // value's kind once, where the operation probes payload stores in turn.
  // Reflection finds the key, so a branch trusts nothing a value says: it
  // takes the value apart from its kind's store and payload and gives
  // `noBranch` for every other value, which the operation answers. A host object
  // that inherits a type's prototype or holds a number under the key gets
  // the operation; the tables are private and have no holes, so no host
  // function stands in for a branch. Each operation has a wrapper of its own,
  // whose calls the engine sees apart from the others' (a shared one made
  // `seq-pipeline` 6% slower, and one finding the kind's table, 10%). A
  // value with no kind number calls no branch.
  seq = (operation => Object.defineProperty(function (value) {
    const kind = value?.[kindOps], result = typeof kind === "number" ? kindTables[kind & 15][0](value) : noBranch;
    return result !== noBranch ? result : operation(value);
  }, "name", {value: operation.name}))(seq);
  //@unit behindProtocol
  // A generic operation of one value behind the protocol implementation
  // `method` finds for it: the operations call protocol implementations only
  // where `installMethods` is kept, which wraps them (`seq.protocol`).
  function behindProtocol(operation, method) {
    return Object.defineProperty(function (value) {
      const implementation = value == null ? null : method(value);
      return implementation ? implementation.call(value, value) : operation(value);
    }, "name", {value: operation.name});
  }
  //@unit seq.protocol effect: installMethods when: seq requires: behindProtocol, methodKeys,
  //@ protocolHooks, seq
  seq = behindProtocol(seq, value => protocolHooks.publicMethod?.(value, methodKeys[52], 1, true));
  //@unit cellSeq requires: cellOf, isChunkedCons, noBranch
  // The seq of a cell, itself, where the operations' paths give it: the
  // branch of the kinds whose payload is a cell; a chunked cons is its own
  // seq too.
  function cellSeq(value) {
    const cell = cellOf(value);
    if (cell === undefined) return isChunkedCons(value) ? value : noBranch;
    return cell.cell$LKEY === "list" && cell.count$LKEY === 0 ? noBranch : value;
  }
  //@unit seq.cells effect: Cons, ListValue, IntegerRangeValue, RangeValue when: seq requires: cellSeq,
  //@ kindTables
  kindTables[2][0] = cellSeq;
  //@unit lazyBranch requires: kindHooks, kindTables, lazyOf, lazyRealized, methodKeys, noBranch,
  //@ protocolHooks
  // The branch of operation `index` (`kindTables`) for a lazy seq: the cells'
  // branch of the value it realizes, most often a cell, which is its own seq.
  // Every other value takes the operation, which realizes nothing again; it
  // takes the seq of the value, which the branch does not, as that can take
  // an item of a host iterator. Where protocol implementations exist, the
  // branch takes the operation's own steps (light-cljs-358): the seq of the
  // lazy seq, by an `ISeqable` implementation that JavaScript gave it or else
  // by its kind's `seq`, then the `operation` (the generic one, with its
  // wrappers) of what it realized, so that each implementation is looked up
  // once, as the operation would.
  function lazyBranch(index, operation) {
    return value => {
      const lazy = lazyOf(value), method = protocolHooks.publicMethod;
      if (lazy === undefined) return noBranch;
      if (!method) return kindTables[2][index](lazyRealized(lazy));
      const own = method(value, methodKeys[52], 1, true);
      if (own) own.call(value, value); else kindHooks.seqLazy(lazy);
      return operation(lazy.s$LKEY);
    };
  }
  //@unit LazySeq.seq effect: LazySeq when: lazyBranch, next, seq requires: cellSeq, kindHooks,
  //@ kindTables, lazyOf, lazyRealized, noBranch, protocolHooks, seq
  kindTables[1][0] = value => {
    const lazy = lazyOf(value);
    return lazy === undefined ? noBranch : kindHooks.seqLazy(lazy);
  };
  // The seq of what a lazy seq realizes: a cell is its own, which `seq`,
  // looking for an `ISeqable` implementation that JavaScript gave it, also
  // finds where protocol implementations exist.
  kindHooks.seqLazy = lazy => {
    const current = lazyRealized(lazy);
    if (!protocolHooks.publicMethod) { const result = cellSeq(current); if (result !== noBranch) return result; }
    return seq(current);
  };
  //@unit lazyRealized requires: lazyOf, lazyStep
  // The raw value a lazy seq's state (`lazyOf`) realizes, without the seq of
  // it, which can take an item of a host iterator.
  function lazyRealized(lazy) {
    let current = lazyStep(lazy);
    // Flatten nested LazySeq results iteratively, retaining the raw final
    // value. CLJS calls seq on that raw value again on each traversal.
    for (let inner = lazyOf(current); inner !== undefined; inner = lazyOf(current)) {
      current = lazyStep(inner);
      // Compress as we advance, so discarded wrappers and their captured
      // source heads can be collected even through a long empty prefix.
      lazy.s$LKEY = current;
    }
    return lazy.s$LKEY = current;
  }
  //@unit queueFromArray.seq effect: queueFromArray when: first, rest, seq requires: concat,
  //@ IndexedSeqValue, kindHooks, publicCollectionSlots, seq, view
  kindHooks.seqQueue = (value, queue) => {
    if (publicCollectionSlots.has(value)) { const q = publicCollectionSlots.get(value); return seq(concat(q.front, q.rear)); }
    return queue.length ? view(IndexedSeqValue, queue, 0, false, "queue") : null;
  };
  //@unit makeVector.seq effect: makeVector, mapEntry when: seq, vectorFirst, vectorRest requires: ChunkedSeqCell,
  //@ IndexedSeqCell, kindHooks, vectorChunk
  // A vector of at most 32 items walks its tail; a longer one is upstream's
  // chunked seq over its trie's leaves.
  kindHooks.seqVector = (value, store) => !store.cnt$LKEY ? null : store.cnt$LKEY <= 32 ? new IndexedSeqCell(vectorChunk(value, 0), 0, true, "indexed")
    : new ChunkedSeqCell(vectorChunk(value, 0), 0, value, 0);
  //@unit makeMap.seq effect: ArrayMapCell, makeMap, treeMapConstructor, recordMethods when: first,
  //@ rest, seq requires: IndexedSeqValue, isTreeMap, kindHooks, mapEntry, mapKinds, mapPairs,
  //@ PersistentArrayMapSeqType, view
  kindHooks.seqMap = value => {
    // A sorted map's seq walks its tree (light-cljs-1035).
    if (isTreeMap(value)) return kindHooks.seqTree(value, true);
    const pairs = mapPairs(value), kind = mapKinds.get(value), n = (pairs.length + 1) >> 1;
    if (!n) return null;
    const items = [];
    for (let i = 0; i < n; i++) items.push(mapEntry(pairs[2 * i], pairs[2 * i + 1]));
    return view(kind === "hash-map" ? kindHooks.hashMapSeqType(value)
      : kind === "array-map" ? PersistentArrayMapSeqType : IndexedSeqValue,
      Object.freeze(items), 0, false, kind);
  };
  //@unit treeMapConstructor.seq effect: treeMapConstructor when: first, rest, rseq, seq requires: kindHooks,
  //@ treeMapSeq
  // Only the sorted-map operations, which keep its constructor, build a map
  // of that kind.
  kindHooks.seqTree = treeMapSeq;
  //@unit makeSet.seq effect: makeSet, treeSetConstructor when: first, rest, seq requires: IndexedSeqValue,
  //@ kindHooks, setTraversal, view
  kindHooks.seqSet = (value, members) => members.length
    ? view(IndexedSeqValue, Object.freeze(setTraversal(value).map(entry => entry.key)), 0, false, "set") : null;
  //@unit seq.vectors effect: PersistentVectorValue, SubvecValue when: seq requires: kindHooks,
  //@ kindTables, noBranch, vectorStore
  // The seqs of the collections, kinds 4 to 7 (light-cljs-358), as the
  // operation gives them from their stores.
  kindTables[4][0] = value => { const store = vectorStore(value); return store ? kindHooks.seqVector(value, store) : noBranch; };
  //@unit seq.maps effect: PersistentArrayMapValue, PersistentHashMapValue, treeMapConstructor when: seq
  //@ requires: kindHooks, kindTables, maps, noBranch
  kindTables[5][0] = value => maps.has(value) ? kindHooks.seqMap(value) : noBranch;
  //@unit seq.sets effect: PersistentHashSetValue, treeSetConstructor when: seq requires: kindHooks,
  //@ kindTables, noBranch, sets
  kindTables[6][0] = value => { const members = sets.get(value); return members ? kindHooks.seqSet(value, members) : noBranch; };
  //@unit seq.queues effect: PersistentQueueValue when: seq requires: kindHooks, kindTables,
  //@ noBranch, queues
  kindTables[7][0] = value => { const queue = queues.get(value); return queue ? kindHooks.seqQueue(value, queue) : noBranch; };
  //@unit seq.entries effect: MapEntryType when: seq requires: kindHooks, kindTables, noBranch,
  //@ vectorStore
  // Map entries, kind 8 (light-cljs-373), whose store is a vector's.
  kindTables[8][0] = value => { const store = vectorStore(value); return store ? kindHooks.seqVector(value, store) : noBranch; };

  //@unit iteratorSeq requires: IteratorSeqCell
  // A host iterator's seq (upstream's ES6IteratorSeq: ISeqable and ISeq, not
  // ISequential) is a plain object that `seq`, `first`, `rest` and
  // `builtinCoreImplementation` know from `iteratorSeqs`, without protocol
  // methods. CLJS reprobes a nil tail; only successful nonempty tails are
  // memoized.
  function iteratorSeq(iterator) {
    const item = iterator.next();
    if (item.done) return null;
    return new IteratorSeqCell(item.value, iterator);
  }
  //@unit first requires: chunkOf, firstSeq, isCell, isChunkedCons, isView, iteratorSeqs, lazyOf,
  //@ publicCollectionSlots, queues, seq
  function first(value) {
    if (value == null) return null;
    if (queues.has(value) && publicCollectionSlots.has(value)) return first(publicCollectionSlots.get(value).front);
    const chunk = chunkOf(value);
    if (chunk !== undefined && chunk.step$LKEY !== null) return chunk.first$LKEY;
    const lazy = lazyOf(value);
    if (lazy !== undefined) { seq(value); return first(lazy.s$LKEY); }
    // Existing ISeq values do not redispatch ISeqable.
    return firstSeq(isCell(value) || isChunkedCons(value) || isView(value) || iteratorSeqs.has(value) ? value : seq(value));
  }
  //@unit firstSeq requires: cellOf, chunkedConsOf, iteratorSeqs, kindHooks, methodKeys,
  //@ protocolHooks, publicCollectionSlots, viewOf
  // `first`'s steps once it has the seq `current` of its value, which the
  // collections' branches take too (`seqBranch`, light-cljs-375).
  function firstSeq(current) {
    if (current === null) return null;
    const cell = cellOf(current);
    if (cell) return cell.cell$LKEY === "list" && cell.count$LKEY === 0 && !publicCollectionSlots.has(current) ? null : cell.first$LKEY;
    const chunked = chunkedConsOf(current);
    if (chunked) return kindHooks.chunkedConsFirst(chunked);
    const slice = viewOf(current);
    if (slice) return slice.i$LKEY < slice.arr$LKEY.length ? slice.arr$LKEY[slice.i$LKEY] : null;
    const own = iteratorSeqs.get(current);
    if (own) return own.first$LKEY;
    const firstMethod = protocolHooks.builtin?.(current, methodKeys[48], 1);
    if (firstMethod) return firstMethod.call(current, current);
    throw new Error("ISeq.-first not supported on this type");
  }

  //@unit first.branches effect: Cons, IntegerRangeValue, LazySeq, ListValue, MapEntryType,
  //@ PersistentArrayMapValue, PersistentHashMapValue, PersistentHashSetValue, PersistentQueueValue,
  //@ PersistentVectorValue, RangeValue, SubvecValue, treeMapConstructor, treeSetConstructor when: first
  //@ requires: first, kindOps, kindTables, noBranch
  // As `seq.branches`.
  first = (operation => Object.defineProperty(function (value) {
    const kind = value?.[kindOps], result = typeof kind === "number" ? kindTables[kind & 15][1](value) : noBranch;
    return result !== noBranch ? result : operation(value);
  }, "name", {value: operation.name}))(first);
  //@unit first.protocol effect: installMethods when: first requires: behindProtocol, first,
  //@ methodKeys, protocolHooks
  first = behindProtocol(first, value => protocolHooks.publicMethod?.(value, methodKeys[48], 1, false, true));
  //@unit first.cells effect: Cons, ListValue, IntegerRangeValue, RangeValue when: first requires: cellOf,
  //@ kindTables, noBranch
  // `first` and `rest` of a cell, which is already a seq, take no seq of it
  // and dispatch no protocol method but the one their wrappers look for
  // (`first.protocol`); `next` takes the seq of its rest (`next.cells`).
  kindTables[2][1] = value => {
    const cell = cellOf(value);
    return cell === undefined || cell.cell$LKEY === "list" && cell.count$LKEY === 0 ? noBranch : cell.first$LKEY;
  };
  //@unit LazySeq.first effect: LazySeq when: first requires: first, kindTables, lazyBranch
  kindTables[1][1] = lazyBranch(1, value => first(value));

  //@unit rest requires: emptyList, isCell, isChunkedCons, isView, iteratorSeqs, lazyOf, restSeq,
  //@ seq
  function rest(value) {
    if (value == null) return emptyList;
    const lazy = lazyOf(value);
    if (lazy !== undefined) { seq(value); return rest(lazy.s$LKEY); }
    return restSeq(isCell(value) || isChunkedCons(value) || isView(value) || iteratorSeqs.has(value) ? value : seq(value));
  }
  //@unit restSeq requires: cellOf, chunkedConsOf, emptyList, iteratorSeq, iteratorSeqs, kindHooks,
  //@ methodKeys, protocolHooks, publicCollectionSlots, viewAt, viewOf
  // `rest`'s steps once it has the seq `current` of its value, as `firstSeq`.
  function restSeq(current) {
    if (current === null) return emptyList;
    const cell = cellOf(current);
    if (cell) return cell.cell$LKEY === "list" ? cell.count$LKEY === 1 || cell.count$LKEY === 0 && !publicCollectionSlots.has(current) ? emptyList : cell.rest$LKEY : cell.rest$LKEY ?? emptyList;
    const chunked = chunkedConsOf(current);
    if (chunked) return kindHooks.chunkedConsRest(chunked);
    const slice = viewOf(current);
    if (!slice) {
      const own = iteratorSeqs.get(current);
      if (own) return own.tail$LKEY ?? (own.tail$LKEY = iteratorSeq(own.iter$LKEY));
      const restMethod = protocolHooks.builtin?.(current, methodKeys[50], 1);
      if (restMethod) return restMethod.call(current, current);
      throw new Error("ISeq.-rest not supported on this type");
    }
    if (!(slice.i$LKEY < slice.arr$LKEY.length)) return emptyList;
    if (slice.vec$LKEY) return kindHooks.restChunkedSeq(slice);
    const tail = slice.i$LKEY + 1 < slice.arr$LKEY.length ? viewAt(slice, slice.i$LKEY + 1) : emptyList;
    return slice.view$LKEY === "queue" ? kindHooks.restQueue(tail, current) : tail;
  }
  //@unit rest.branches effect: Cons, IntegerRangeValue, LazySeq, ListValue, MapEntryType,
  //@ PersistentArrayMapValue, PersistentHashMapValue, PersistentHashSetValue, PersistentQueueValue,
  //@ PersistentVectorValue, RangeValue, SubvecValue, treeMapConstructor, treeSetConstructor when: rest
  //@ requires: kindOps, kindTables, noBranch, rest
  // As `first.branches`.
  rest = (operation => Object.defineProperty(function (value) {
    const kind = value?.[kindOps], result = typeof kind === "number" ? kindTables[kind & 15][2](value) : noBranch;
    return result !== noBranch ? result : operation(value);
  }, "name", {value: operation.name}))(rest);
  //@unit rest.protocol effect: installMethods when: rest requires: behindProtocol, methodKeys,
  //@ protocolHooks, rest
  rest = behindProtocol(rest, value => protocolHooks.publicMethod?.(value, methodKeys[50], 1, false, true));
  //@unit rest.cells effect: Cons, ListValue, IntegerRangeValue, RangeValue when: rest requires: cellOf,
  //@ emptyList, kindTables, noBranch
  kindTables[2][2] = value => {
    const cell = cellOf(value);
    if (cell === undefined || cell.cell$LKEY === "list" && cell.count$LKEY === 0) return noBranch;
    return cell.cell$LKEY === "list" ? cell.count$LKEY === 1 ? emptyList : cell.rest$LKEY : cell.rest$LKEY ?? emptyList;
  };
  //@unit LazySeq.rest effect: LazySeq when: rest requires: kindTables, lazyBranch, rest
  kindTables[1][2] = lazyBranch(2, value => rest(value));
  //@unit ChunkedSeqValue.restChunked effect: ChunkedSeqValue when: restSeq requires: ChunkedSeqCell,
  //@ chunkedVectorRest, kindHooks
  // A chunked seq with a vector (`ChunkedSeqCell`) continues past its leaf.
  kindHooks.restChunkedSeq = slice => slice.i$LKEY + 1 < slice.arr$LKEY.length
    ? new ChunkedSeqCell(slice.arr$LKEY, slice.i$LKEY + 1, slice.vec$LKEY, slice.base$LKEY) : chunkedVectorRest(slice);
  //@unit queueFromArray.rest effect: queueFromArray when: restSeq requires: collectionWithMeta,
  //@ kindHooks, meta
  kindHooks.restQueue = (tail, current) => collectionWithMeta(tail, meta(current));

  //@unit next requires: cellOf, lazyOf, publicCollectionSlots, rest, seq, viewAt, viewOf
  function next(value) {
    if (value == null) return null;
    const lazy = lazyOf(value);
    if (lazy !== undefined) { seq(value); return next(lazy.s$LKEY); }
    const cell = cellOf(value);
    if (cell?.cell$LKEY === "list") return cell.count$LKEY === 1 || cell.count$LKEY === 0 && !publicCollectionSlots.has(value) ? null : cell.rest$LKEY;
    if (cell?.cell$LKEY === "repeat") return cell.next$LKEY ?? (cell.remaining$LKEY === -1 ? value : cell.remaining$LKEY > 1 ? cell.rest$LKEY : null);
    // Flat views advance directly; other kinds keep their rest/seq behavior.
    const slice = viewOf(value), kind = slice?.view$LKEY;
    if (slice && (kind === "indexed" || kind === "hash-map" || kind === "array-map" || kind === "set"))
      return slice.i$LKEY + 1 < slice.arr$LKEY.length ? viewAt(slice, slice.i$LKEY + 1) : null;
    return seq(rest(value));
  }

  //@unit next.branches effect: Cons, IntegerRangeValue, LazySeq, ListValue, MapEntryType,
  //@ PersistentVectorValue, RangeValue, SubvecValue when: next requires: kindOps, kindTables, next,
  //@ noBranch
  // As `first.branches`.
  next = (operation => Object.defineProperty(function (value) {
    const kind = value?.[kindOps], result = typeof kind === "number" ? kindTables[kind & 15][3](value) : noBranch;
    return result !== noBranch ? result : operation(value);
  }, "name", {value: operation.name}))(next);
  //@unit next.protocol effect: installMethods when: next requires: behindProtocol, methodKeys,
  //@ next, protocolHooks
  next = behindProtocol(next, value => protocolHooks.publicMethod?.(value, methodKeys[40], 1, false, true));
  //@unit next.cells effect: Cons, ListValue, IntegerRangeValue, RangeValue when: next requires: cellOf,
  //@ emptyList, kindHooks, kindTables, lazyOf, methodKeys, noBranch, protocolHooks, seq
  // A list's next is its tail; another cell's the seq of its rest, which the
  // operation takes with `rest` and `seq`: where protocol implementations
  // exist, the seq of what a `-rest` that JavaScript gave the cell gives,
  // and the rest's seq is `seq`'s, which looks for `-seq`.
  kindTables[2][3] = value => {
    const cell = cellOf(value);
    if (cell === undefined || cell.cell$LKEY === "repeat") return noBranch;
    if (cell.cell$LKEY === "list") return cell.count$LKEY === 0 ? noBranch : cell.count$LKEY === 1 ? null : cell.rest$LKEY;
    const method = protocolHooks.publicMethod;
    const own = method ? method(value, methodKeys[50], 1, false, true) : undefined;
    if (own) return seq(own.call(value, value));
    // The rest, most often a lazy seq, whose seq this is.
    const tail = cell.rest$LKEY ?? emptyList, lazy = lazyOf(tail);
    return lazy === undefined || method ? seq(tail) : kindHooks.seqLazy(lazy);
  };
  //@unit LazySeq.next effect: LazySeq when: next requires: kindTables, lazyBranch, next
  kindTables[1][3] = lazyBranch(3, value => next(value));
  //@unit seqBranch requires: methodKeys, noBranch, protocolHooks
  // The branch of `first` or `rest` for the collections, kinds 4 to 8
  // (light-cljs-375), which their operations give the seq of the value: for
  // a value in `store`, the operation's steps on that seq (`then`: `firstSeq`
  // or `restSeq`), the seq by an `ISeqable` implementation that JavaScript
  // gave the value, looked up once as `seq` does, or else by its kind's
  // `seq` (`seqOf` the value and its payload).
  function seqBranch(store, seqOf, then) {
    return value => {
      const payload = store.get(value);
      if (payload === undefined) return noBranch;
      const own = protocolHooks.publicMethod?.(value, methodKeys[52], 1, true);
      return then(own ? own.call(value, value) : seqOf(value, payload));
    };
  }
  //@unit vectorFirst requires: firstSeq, kindHooks, methodKeys, noBranch, protocolHooks,
  //@ publicCollectionSlots, vectorNth, vectorStore
  // `first` of a vector or a map entry, kinds 4 and 8, as `seqBranch`: the
  // seq the operation takes starts with the item at 0, which the branch
  // reads without the seq, unless an `ISeqable` implementation gives the
  // seq, or the value has a public constructor's slots or its store reads a
  // caller's array (`callerTail`, also of the store a subvector views),
  // which can be shorter than its count or hold getters.
  function vectorFirst(value) {
    const store = vectorStore(value);
    if (store === undefined) return noBranch;
    const own = protocolHooks.publicMethod?.(value, methodKeys[52], 1, true);
    return own ? firstSeq(own.call(value, value))
      : publicCollectionSlots.has(value) || (store.sub$LKEY ?? store).callerTail$LKEY ? firstSeq(kindHooks.seqVector(value, store))
      : store.cnt$LKEY ? vectorNth(store, 0) : null;
  }
  //@unit vectorRest requires: ChunkedSeqCell, IndexedSeqCell, kindHooks, methodKeys, noBranch,
  //@ protocolHooks, publicCollectionSlots, rest, restSeq, seq, vectorChunk, vectorStore
  // `rest` of a vector or a map entry, and with `empty` null its `next`, as
  // `vectorFirst`: the rest of its seq, built from index 1 (`kindHooks.seqVector`
  // chunks a vector of more than 32 items), which is its own seq. Where
  // protocol implementations exist or the value's seq is its kind's, `next`
  // is the seq of `rest`, as the operation takes it.
  function vectorRest(value, empty) {
    const store = vectorStore(value);
    if (store === undefined) return noBranch;
    const method = protocolHooks.publicMethod, shared = publicCollectionSlots.has(value) || (store.sub$LKEY ?? store).callerTail$LKEY;
    const own = empty === null ? undefined : method?.(value, methodKeys[52], 1, true);
    if (!shared && !own && (empty !== null || !method)) return store.cnt$LKEY < 2 ? empty : store.cnt$LKEY <= 32
      ? new IndexedSeqCell(vectorChunk(value, 0), 1, true, "indexed") : new ChunkedSeqCell(vectorChunk(value, 0), 1, value, 0);
    return empty === null ? seq(rest(value)) : restSeq(own ? own.call(value, value) : kindHooks.seqVector(value, store));
  }
  //@unit first.vectors effect: PersistentVectorValue, SubvecValue when: first requires: kindTables,
  //@ vectorFirst
  kindTables[4][1] = vectorFirst;
  //@unit rest.vectors effect: PersistentVectorValue, SubvecValue when: rest requires: emptyList,
  //@ kindTables, vectorRest
  kindTables[4][2] = value => vectorRest(value, emptyList);
  //@unit next.vectors effect: PersistentVectorValue, SubvecValue when: next requires: kindTables,
  //@ vectorRest
  kindTables[4][3] = value => vectorRest(value, null);
  //@unit first.entries effect: MapEntryType when: first requires: kindTables, vectorFirst
  kindTables[8][1] = vectorFirst;
  //@unit rest.entries effect: MapEntryType when: rest requires: emptyList, kindTables, vectorRest
  kindTables[8][2] = value => vectorRest(value, emptyList);
  //@unit next.entries effect: MapEntryType when: next requires: kindTables, vectorRest
  kindTables[8][3] = value => vectorRest(value, null);
  //@unit first.maps effect: PersistentArrayMapValue, PersistentHashMapValue, treeMapConstructor
  //@ when: first requires: firstSeq, kindHooks, kindTables, maps, seqBranch
  kindTables[5][1] = seqBranch(maps, value => kindHooks.seqMap(value), firstSeq);
  //@unit rest.maps effect: PersistentArrayMapValue, PersistentHashMapValue, treeMapConstructor
  //@ when: rest requires: kindHooks, kindTables, maps, restSeq, seqBranch
  kindTables[5][2] = seqBranch(maps, value => kindHooks.seqMap(value), restSeq);
  //@unit first.sets effect: PersistentHashSetValue, treeSetConstructor when: first requires: firstSeq,
  //@ kindHooks, kindTables, seqBranch, sets
  kindTables[6][1] = seqBranch(sets, (value, members) => kindHooks.seqSet(value, members), firstSeq);
  //@unit rest.sets effect: PersistentHashSetValue, treeSetConstructor when: rest requires: kindHooks,
  //@ kindTables, restSeq, seqBranch, sets
  kindTables[6][2] = seqBranch(sets, (value, members) => kindHooks.seqSet(value, members), restSeq);
  //@unit first.queues effect: PersistentQueueValue when: first requires: firstSeq, kindHooks,
  //@ kindTables, noBranch, publicCollectionSlots, queues, seqBranch
  // A queue that the public constructor built takes the operations, whose
  // `first` reads its front.
  kindTables[7][1] = (branch => value => publicCollectionSlots.has(value) ? noBranch : branch(value))(seqBranch(queues, (value, queue) => kindHooks.seqQueue(value, queue), firstSeq));
  //@unit rest.queues effect: PersistentQueueValue when: rest requires: kindHooks, kindTables,
  //@ queues, restSeq, seqBranch
  kindTables[7][2] = seqBranch(queues, (value, queue) => kindHooks.seqQueue(value, queue), restSeq);

  //@unit seqToMap requires: first, into, mapFromArray, next, seq, toArray
  function seqToMap(value) {
    // Upstream first probes next, then realizes exactly this argument sequence.
    // A singleton is its value (including nil); an odd trailing value is conjoined.
    if (next(value) === null) return seq(value) === null ? mapFromArray([]) : first(value);
    const values = toArray(value), trailing = values.length % 2 ? values.pop() : null;
    const base = mapFromArray(values, "array-map");
    return trailing == null ? base : into(base, trailing);
  }

  //@unit reduced requires: ReducedCell
  function reduced(value) { return new ReducedCell(value); }

  //@unit isReduced requires: reducedValues
  function isReduced(value) { return typeof value === "object" && value !== null && reducedValues.has(value); }
  //@unit ensureReduced requires: isReduced, reduced
  function ensureReduced(value) { return isReduced(value) ? value : reduced(value); }
  //@unit unreduced requires: isReduced, reducedValues
  function unreduced(value) { return isReduced(value) ? reducedValues.get(value) : value; }
  //@unit reducedValue requires: protocolHooks, reducedValues
  // The value of a `reduced`, as `deref` gives it: only a protocol
  // implementation (`protocolHooks.deref`) can give another.
  function reducedValue(value) { return protocolHooks.deref ? protocolHooks.deref(value) : reducedValues.get(value); }
  //@unit deref requires: derefProtocol
  function deref(value) { return derefProtocol(value); }
  //@unit derefProtocol requires: atoms, deferredValue, delays, isReduced, kindHooks, methodKeys,
  //@ protocolHooks, reducedValues, vars, volatiles
  function derefProtocol(value) {
    if (kindHooks.isLoadable?.(value)) return value.deref();
    const method = protocolHooks.builtin?.(value, methodKeys[12], 1);
    if (method) return method.call(value, value);
    if (delays.has(value)) return deferredValue(delays.get(value));
    if (vars.has(value)) return vars.get(value).get.call(null);
    if (atoms.has(value) || volatiles.has(value)) return value.state;
    if (!isReduced(value)) throw new Error("No protocol method IDeref.-deref defined for this value");
    return reducedValues.get(value);
  }
  //@unit Loadable
  // light.lazy's `Loadable`: `importer` loads the target's chunk (null while
  // it is resident), `getter` reads its value. The `Loadable.*` effects add
  // what a deftype has where a program can observe it, but no `TypeBrand`.
  const Loadable = class {
    constructor(importer, getter) { this.importer = importer; this.getter = getter; }
    deref() {
      if (this.importer == null) return this.getter.call(null);
      throw new Error("light.lazy: the target of this loadable is not loaded");
    }
    load() { return Promise.resolve().then(() => this.deref()); }
  };
  //@unit Loadable.deref effect: Loadable when: derefProtocol requires: kindHooks, Loadable
  // `deref` and `-deref` of a loadable, with or without protocol dispatch.
  kindHooks.isLoadable = value => value instanceof Loadable;
  //@unit Loadable.name effect: Loadable when: printHost, typeNames requires: Loadable, typeNames
  // What prints for a loadable and for its type, as for a deftype.
  Loadable.cljs$lang$type = true;
  typeNames.set(Loadable, Loadable.cljs$lang$ctorStr = "light.lazy/Loadable");

  //@unit add
  function add(x, y) {
    if (arguments.length === 0) return 0;
    if (arguments.length === 1) return x;
    let result = x + y;
    for (let i = 2; i < arguments.length; i++) result = result + arguments[i];
    return result;
  }

  //@unit numericFold
  // Numeric core functions use JavaScript Number/coercion rules. In particular,
  // single-arity CLJS functions accept missing/extra JS arguments, and the
  // variadic dispatcher falls through to its two leading slots at arity zero.
  function numericFold(operation, x, y, more) {
    let value = operation(x, y);
    for (const item of more) value = operation(value, item);
    return value;
  }
  //@unit subtract requires: numericFold
  function subtract(x, y, ...more) {
    return arguments.length === 1 ? -x : numericFold((a, b) => a - b, x, y, more);
  }
  //@unit multiply requires: numericFold
  function multiply(x, y, ...more) {
    if (arguments.length === 0) return 1;
    return arguments.length === 1 ? x : numericFold((a, b) => a * b, x, y, more);
  }
  //@unit divide requires: numericFold
  function divide(x, y, ...more) {
    return arguments.length === 1 ? 1 / x : numericFold((a, b) => a / b, x, y, more);
  }
  //@unit comparison
  function comparison(test) {
    const fn = function (x, y, ...more) {
      if (arguments.length === 1) return true;
      if (!test(x, y)) return false;
      for (const item of more) { if (!test(y, item)) return false; y = item; }
      return true;
    };
    return fn;
  }
  //@unit comparisonVariadic mutator requires: first, next, seq, variadicCore
  function comparisonVariadic(fn, test) {
    return variadicCore(fn, 2, (x, y, more) => {
      if (!test(x, y)) return false;
      for (let s = seq(more); s !== null; s = next(s)) {
        const item = first(s);
        if (!test(y, item)) return false;
        y = item;
      }
      return true;
    });
  }
  //@unit extrema requires: numericFold
  function extrema(test) {
    return function (x, y, ...more) {
      if (arguments.length === 1) return x;
      return numericFold((a, b) => isNaN(a) ? a : isNaN(b) ? b : test(a, b) ? a : b, x, y, more);
    };
  }
  //@unit isInteger
  // parseInt/parseFloat are intentional: CLJS integer? excludes numbers printed
  // in exponent notation even when Number.isInteger would accept them.
  const isInteger = x => typeof x === "number" && !isNaN(x) && x !== Infinity && parseFloat(x) === parseInt(x, 10);
  //@unit isClosureInteger
  // Closure classes belong to an explicit embedding. Resolve at call time: the
  // genuine loader can initialize them after this runtime has been created.
  function isClosureInteger(x) {
    const goog = globalThis.goog;
    const Integer = goog?.math?.Integer;
    if (typeof Integer === "function" && x instanceof Integer) return true;
    const Long = typeof goog?.module?.get === "function"
      ? goog.module.get("goog.math.Long") : goog?.math?.Long;
    return typeof Long === "function" && x instanceof Long;
  }
  //@unit fix
  const fix = x => x >= 0 ? Math.floor(x) : Math.ceil(x);
  //@unit mod
  const mod = (n, d) => ((n % d) + d) % d;
  //@unit quot requires: fix
  const quot = (n, d) => fix((n - n % d) / d);
  //@unit isEven requires: isInteger
  function isEven(n) {
    if (!isInteger(n)) throw new Error("Argument must be an integer: " + n);
    return (n & 1) === 0;
  }
  //@unit char
  function char(x) {
    if (typeof x === "number") return String.fromCharCode(x);
    if (typeof x === "string" && x.length === 1) return x;
    throw new Error("Argument to char must be a character or number");
  }
  //@unit bitOperation requires: numericFold
  function bitOperation(operation) {
    return (x, y, ...more) => numericFold(operation, x, y, more);
  }
  //@unit bitCount
  function bitCount(v) {
    v = v - ((v >> 1) & 0x55555555);
    v = (v & 0x33333333) + ((v >> 2) & 0x33333333);
    return (((v + (v >> 4)) & 0x0f0f0f0f) * 0x01010101) >> 24;
  }
  //@unit inc
  // Numeric core operations, one unit each.
  const inc = x => x + 1;
  //@unit dec
  const dec = x => x - 1;
  //@unit less requires: comparison
  const less = comparison((x, y) => x < y);
  //@unit less.variadic install: less requires: comparisonVariadic, less
  comparisonVariadic(less, (x, y) => x < y);
  //@unit lessEqual requires: comparison
  const lessEqual = comparison((x, y) => x <= y);
  //@unit lessEqual.variadic install: lessEqual requires: comparisonVariadic, lessEqual
  comparisonVariadic(lessEqual, (x, y) => x <= y);
  //@unit greater requires: comparison
  const greater = comparison((x, y) => x > y);
  //@unit greater.variadic install: greater requires: comparisonVariadic, greater
  comparisonVariadic(greater, (x, y) => x > y);
  //@unit greaterEqual requires: comparison
  const greaterEqual = comparison((x, y) => x >= y);
  //@unit greaterEqual.variadic install: greaterEqual requires: comparisonVariadic, greaterEqual
  comparisonVariadic(greaterEqual, (x, y) => x >= y);
  //@unit numericEqual requires: equal
  const numericEqual = function (...values) { return values.length !== 0 && equal(...values); };
  //@unit max requires: extrema
  const max = extrema((x, y) => x > y);
  //@unit min requires: extrema
  const min = extrema((x, y) => x < y);
  //@unit abs
  const abs = x => Math.abs(x);
  //@unit int
  const int = x => x | 0;
  //@unit long requires: fix
  const long = x => fix(x);
  //@unit uncheckedInt requires: fix
  const uncheckedInt = x => fix(x);
  //@unit uncheckedLong requires: fix
  const uncheckedLong = x => fix(x);
  //@unit byte
  const byte = x => x;
  //@unit short
  const short = x => x;
  //@unit float
  const float = x => x;
  //@unit double
  const double = x => x;
  //@unit booleans
  const booleans = x => x;
  //@unit bytes
  const bytes = x => x;
  //@unit chars
  const chars = x => x;
  //@unit shorts
  const shorts = x => x;
  //@unit ints
  const ints = x => x;
  //@unit floats
  const floats = x => x;
  //@unit doubles
  const doubles = x => x;
  //@unit longs
  const longs = x => x;
  //@unit uncheckedByte
  const uncheckedByte = x => x;
  //@unit uncheckedShort
  const uncheckedShort = x => x;
  //@unit uncheckedFloat
  const uncheckedFloat = x => x;
  //@unit uncheckedDouble
  const uncheckedDouble = x => x;
  //@unit uncheckedChar
  const uncheckedChar = x => x;
  //@unit uncheckedAdd requires: add
  const uncheckedAdd = (...xs) => add(...xs);
  //@unit uncheckedAddInt requires: add
  const uncheckedAddInt = (...xs) => add(...xs);
  //@unit uncheckedSubtract requires: subtract
  const uncheckedSubtract = (...xs) => subtract(...xs);
  //@unit uncheckedSubtractInt requires: subtract
  const uncheckedSubtractInt = (...xs) => subtract(...xs);
  //@unit uncheckedMultiply requires: multiply
  const uncheckedMultiply = (...xs) => multiply(...xs);
  //@unit uncheckedMultiplyInt requires: multiply
  const uncheckedMultiplyInt = (...xs) => multiply(...xs);
  //@unit uncheckedDivideInt requires: divide
  const uncheckedDivideInt = (...xs) => divide(...xs);
  //@unit uncheckedInc
  const uncheckedInc = x => x + 1;
  //@unit uncheckedIncInt
  const uncheckedIncInt = x => x + 1;
  //@unit uncheckedDec
  const uncheckedDec = x => x - 1;
  //@unit uncheckedDecInt
  const uncheckedDecInt = x => x - 1;
  //@unit uncheckedNegate
  const uncheckedNegate = x => -x;
  //@unit uncheckedNegateInt
  const uncheckedNegateInt = x => -x;
  //@unit uncheckedRemainderInt requires: mod
  const uncheckedRemainderInt = (n, d) => mod(n, d);
  //@unit jsMod
  const jsMod = (n, d) => n % d;
  //@unit rem requires: quot
  const rem = (n, d) => n - d * quot(n, d);
  //@unit bitXor requires: bitOperation
  const bitXor = bitOperation((x, y) => x ^ y);
  //@unit bitXor.variadic install: bitXor requires: bitXor, foldSeq, variadicCore
  variadicCore(bitXor, 2, (x, y, more) => foldSeq((x, y) => x ^ y, x, y, more));
  //@unit bitAnd requires: bitOperation
  const bitAnd = bitOperation((x, y) => x & y);
  //@unit bitAnd.variadic install: bitAnd requires: bitAnd, foldSeq, variadicCore
  variadicCore(bitAnd, 2, (x, y, more) => foldSeq((x, y) => x & y, x, y, more));
  //@unit bitOr requires: bitOperation
  const bitOr = bitOperation((x, y) => x | y);
  //@unit bitOr.variadic install: bitOr requires: bitOr, foldSeq, variadicCore
  variadicCore(bitOr, 2, (x, y, more) => foldSeq((x, y) => x | y, x, y, more));
  //@unit bitAndNot requires: bitOperation
  const bitAndNot = bitOperation((x, y) => x & ~y);
  //@unit bitAndNot.variadic install: bitAndNot requires: bitAndNot, foldSeq, variadicCore
  variadicCore(bitAndNot, 2, (x, y, more) => foldSeq((x, y) => x & ~y, x, y, more));
  //@unit bitNot
  const bitNot = x => ~x;
  //@unit bitClear
  const bitClear = (x, n) => x & ~(1 << n);
  //@unit bitFlip
  const bitFlip = (x, n) => x ^ (1 << n);
  //@unit bitSet
  const bitSet = (x, n) => x | (1 << n);
  //@unit bitTest
  const bitTest = (x, n) => (x & (1 << n)) !== 0;
  //@unit bitShiftLeft
  const bitShiftLeft = (x, n) => x << n;
  //@unit bitShiftRight
  const bitShiftRight = (x, n) => x >> n;
  //@unit bitShiftRightZeroFill
  const bitShiftRightZeroFill = (x, n) => x >>> n;
  //@unit unsignedBitShiftRight
  const unsignedBitShiftRight = (x, n) => x >>> n;
  //@unit isZero
  const isZero = x => x === 0;
  //@unit isPositive
  const isPositive = x => x > 0;
  //@unit isNegative
  const isNegative = x => x < 0;
  //@unit isOdd requires: isEven
  const isOdd = x => !isEven(x);
  //@unit isInt requires: isClosureInteger, isInteger
  const isInt = x => isInteger(x) || isClosureInteger(x);
  //@unit isPositiveInt requires: isClosureInteger, isInteger
  const isPositiveInt = x => isInteger(x) ? x > 0 : isClosureInteger(x) && !x.isNegative() && !x.isZero();
  //@unit isNegativeInt requires: isClosureInteger, isInteger
  const isNegativeInt = x => isInteger(x) ? x < 0 : isClosureInteger(x) && x.isNegative();
  //@unit isNaturalInt requires: isClosureInteger, isInteger
  const isNaturalInt = x => isInteger(x) ? !(x < 0) : isClosureInteger(x) && !x.isNegative();
  //@unit isNumber
  const isNumber = x => typeof x === "number";
  //@unit isFloat
  const isFloat = x => typeof x === "number";
  //@unit isDouble
  const isDouble = x => typeof x === "number";
  //@unit isInfinite
  const isInfinite = x => x === Infinity || x === -Infinity;
  //@unit isNaNCore
  // Printing shows function names; keep the public one.
  const isNaNCore = Object.defineProperty(x => isNaN(x), "name", {value: "isNaN"});

  //@unit isIdent requires: namedOf
  const isIdent = x => namedOf(x) !== undefined;
  //@unit isCollectionValue requires: directMethod, holdsVector, isCell, isChunkedCons, isLazy,
  //@ isView, maps, queues, sets
  const isCollectionValue = x => !!directMethod(x, "ICollection", 0) || holdsVector(x) || isCell(x) || isChunkedCons(x) || isView(x) || queues.has(x) || isLazy(x) || maps.has(x) || sets.has(x);
  //@unit boolean
  // Core predicates, one unit each.
  const boolean = x => x != null && x !== false;
  //@unit not
  const not = x => x == null || x === false;
  //@unit isBoolean
  const isBoolean = x => x === true || x === false;
  //@unit isTrue
  const isTrue = x => x === true;
  //@unit isFalse
  const isFalse = x => x === false;
  //@unit isNil
  const isNil = x => x == null;
  //@unit isSome
  const isSome = x => x != null;
  //@unit isUndefined
  const isUndefined = x => x === undefined;
  //@unit notEqual requires: equal
  const notEqual = (...xs) => !equal(...xs);
  //@unit identity
  const identity = x => x;
  //@unit isAny
  const isAny = _x => true;
  //@unit isString
  const isString = x => typeof x === "string";
  //@unit isChar
  const isChar = x => typeof x === "string" && x.length === 1;
  //@unit isKeyword requires: keywordOf
  const isKeyword = x => keywordOf(x) !== undefined;
  //@unit isSymbol requires: symbolOf
  const isSymbol = x => symbolOf(x) !== undefined;
  //@unit isSimpleIdent requires: isIdent, namespace
  const isSimpleIdent = x => isIdent(x) && namespace(x) == null;
  //@unit isQualifiedIdent requires: isIdent, namespace
  const isQualifiedIdent = x => isIdent(x) && namespace(x) != null;
  //@unit isSimpleKeyword requires: keywordOf, namespace
  const isSimpleKeyword = x => keywordOf(x) !== undefined && namespace(x) == null;
  //@unit isQualifiedKeyword requires: keywordOf, namespace
  const isQualifiedKeyword = x => keywordOf(x) !== undefined && namespace(x) != null;
  //@unit isSimpleSymbol requires: namespace, symbolOf
  const isSimpleSymbol = x => symbolOf(x) !== undefined && namespace(x) == null;
  //@unit isQualifiedSymbol requires: namespace, symbolOf
  const isQualifiedSymbol = x => symbolOf(x) !== undefined && namespace(x) != null;
  //@unit keywordIdentical requires: equalPair, keywordOf
  const keywordIdentical = (x, y) => x === y || (keywordOf(x) !== undefined && keywordOf(y) !== undefined && equalPair(x, y));
  //@unit symbolIdentical requires: equalPair, symbolOf
  const symbolIdentical = (x, y) => x === y || (symbolOf(x) !== undefined && symbolOf(y) !== undefined && equalPair(x, y));
  //@unit isFn requires: fnValue, methodKeys, protocolHooks
  // Native answers (`countedValue`) unless protocol implementations exist.
  const isFn = x => typeof x === "function" || (protocolHooks.satisfies?.(methodKeys[0], x) ?? fnValue(x));
  //@unit isIFn requires: ifnValue, isFn, methodKeys, protocolHooks
  const isIFn = x => isFn(x) || (protocolHooks.satisfies?.(methodKeys[21], x) ?? ifnValue(x));
  //@unit isArray
  const isArray = x => Array.isArray(x);
  //@unit isObject requires: isCollectionValue, isIdent, reducedValues, uuids, vars
  const isObject = x => x != null && !isCollectionValue(x) && !isIdent(x) && !uuids.has(x) && !vars.has(x) && !reducedValues.has(x) && x.constructor === Object;
  //@unit isColl requires: collectionValue, methodKeys, protocolHooks
  const isColl = x => x != null && (protocolHooks.satisfies?.(methodKeys[8], x) ?? collectionValue(x));
  //@unit isSeq requires: methodKeys, protocolHooks, seqValue
  const isSeq = x => x != null && (protocolHooks.satisfies?.(methodKeys[49], x) ?? seqValue(x));
  //@unit isList requires: listValue, methodKeys, protocolHooks
  const isList = x => protocolHooks.satisfies?.(methodKeys[29], x) ?? listValue(x);
  //@unit isSeqable requires: methodKeys, protocolHooks, seqableValue
  const isSeqable = x => x == null || (protocolHooks.satisfies?.(methodKeys[51], x) ?? seqableValue(x)) || Array.isArray(x) || typeof x === "string" || x[Symbol.iterator] != null;
  //@unit isSequential requires: methodKeys, protocolHooks, sequential
  const isSequential = x => protocolHooks.satisfies?.(methodKeys[53], x) ?? sequential(x);
  //@unit isVector requires: holdsVector, methodKeys, protocolHooks
  const isVector = x => protocolHooks.satisfies?.(methodKeys[61], x) ?? holdsVector(x);
  //@unit isMap requires: maps, methodKeys, protocolHooks
  const isMap = x => x != null && (protocolHooks.satisfies?.(methodKeys[33], x) ?? maps.has(x));
  //@unit isSet requires: methodKeys, protocolHooks, sets
  const isSet = x => x != null && (protocolHooks.satisfies?.(methodKeys[55], x) ?? sets.has(x));
  //@unit isAssociative requires: associativeValue, methodKeys, protocolHooks
  const isAssociative = x => protocolHooks.satisfies?.(methodKeys[3], x) ?? associativeValue(x);
  //@unit isIFind requires: findValue, methodKeys, protocolHooks
  const isIFind = x => protocolHooks.satisfies?.(methodKeys[20], x) ?? findValue(x);
  //@unit isCounted requires: countedValue, methodKeys, protocolHooks
  const isCounted = x => protocolHooks.satisfies?.(methodKeys[11], x) ?? (x == null || countedValue(x));
  //@unit isIndexed requires: indexedValue, methodKeys, protocolHooks
  const isIndexed = x => protocolHooks.satisfies?.(methodKeys[24], x) ?? indexedValue(x);
  //@unit isReduceable requires: methodKeys, protocolHooks, reduceValue
  const isReduceable = x => protocolHooks.satisfies?.(methodKeys[43], x) ?? reduceValue(x);
  //@unit isReversible requires: methodKeys, protocolHooks, reversibleValue
  const isReversible = x => protocolHooks.satisfies?.(methodKeys[46], x) ?? reversibleValue(x);
  //@unit isSorted requires: methodKeys, protocolHooks, sortedValue
  const isSorted = x => protocolHooks.satisfies?.(methodKeys[56], x) ?? sortedValue(x);
  //@unit isRecord requires: methodKeys, protocolHooks
  const isRecord = x => protocolHooks.satisfies?.(methodKeys[42], x) ?? false;
  //@unit isMapEntry requires: mapEntryValue, methodKeys, protocolHooks
  const isMapEntry = x => protocolHooks.implements?.(methodKeys[32], x) ?? mapEntryValue(x);
  //@unit isChunkedSeq requires: chunkedSeqValue, methodKeys, protocolHooks
  const isChunkedSeq = x => protocolHooks.implements?.(methodKeys[5], x) ?? chunkedSeqValue(x);
  //@unit isEmpty requires: countedValue, countProtocol, methodKeys, protocolHooks, seq
  const isEmpty = x => x == null || ((protocolHooks.satisfies?.(methodKeys[11], x) ?? countedValue(x)) ? countProtocol(x) === 0 : seq(x) === null);

  //@unit reduce requires: cellOf, chunkOf, first, invalidArity, isCell, isChunkedCons, isLazy,
  //@ isReduced, iterableValue, iterator, iteratorProtocol, kindHooks, methodKeys, next,
  //@ protocolHooks, publicCollectionSlots, reducedValue, reduceValue, seq, vectorReduce,
  //@ vectorStore, viewOf
  function reduce(rf, init, collection) {
    const arity = arguments.length;
    if (arity !== 2 && arity !== 3) return invalidArity(arity);
    const source = arity === 3 ? collection : init;
    if (!reduceValue(source) && !Array.isArray(source) && typeof source !== "string" && (protocolHooks.satisfies?.(methodKeys[27], source) ?? iterableValue(source))) {
      const iter = iteratorProtocol(source);
      if (arity === 2) { if (!iter.hasNext()) return rf.call(null); init = iter.next(); }
      while (iter.hasNext()) {
        init = rf.call(null, init, iter.next());
        if (isReduced(init)) return reducedValue(init);
      }
      return init;
    }
    const chunkedSlots = publicCollectionSlots.get(source), chunked = viewOf(source)?.vec$LKEY ? source : undefined;
    if (chunkedSlots?.arr) {
      const iter = iterator(source);
      if (arity === 2) { if (!iter.hasNext()) return rf.call(null); init = iter.next(); }
      while (iter.hasNext()) { init = rf.call(null, init, iter.next()); if (isReduced(init)) return reducedValue(init); }
      return init;
    }
    // A vector longer than its tail or a view (`subvec`) whose seq no
    // protocol implementation replaces, one the public constructor built
    // without `init` (with one, as upstream, its seq walks the whole tail it
    // received), and a public ChunkedSeq over one, walk its store.
    const store = vectorStore(chunked ? chunked.vec$LKEY : source);
    if (store && (chunked || chunkedSlots?.cnt != null && arity === 2
      || (store.cnt$LKEY > 32 || store.sub$LKEY) && !protocolHooks.publicMethod?.(source, methodKeys[52], 1, true)))
      return vectorReduce(store, rf, init, chunked ? chunked.base$LKEY + chunked.i$LKEY : 0, arity);
    const producer = cellOf(source);
    if (producer?.integer$LKEY && arity === 2) {
      if (producer.count$LKEY === 0) return rf.call(null);
      init = producer.first$LKEY;
      for (let i = 1; i < producer.count$LKEY; i++) {
        init = rf.call(null, init, producer.first$LKEY + i * producer.step$LKEY);
        if (isReduced(init)) return reducedValue(init);
      }
      return init;
    }
    // With an initial value, as upstream's IntegerRange: from start to end.
    if (producer?.integer$LKEY) {
      for (let i = producer.first$LKEY; producer.step$LKEY > 0 ? i < producer.end$LKEY : i > producer.end$LKEY; i += producer.step$LKEY) {
        init = rf.call(null, init, i);
        if (isReduced(init)) return reducedValue(init);
      }
      return init;
    }
    if (producer?.cell$LKEY === "repeat") {
      const n = producer.remaining$LKEY, item = producer.first$LKEY;
      let i = arity === 2 ? 1 : 0;
      if (arity === 2) init = item;
      while (n === -1 || i++ < n) { init = rf.call(null, init, item); if (isReduced(init)) return reducedValue(init); }
      return init;
    }
    if (producer?.cell$LKEY === "iterate") return kindHooks.reduceIterate(rf, init, source, producer, arity);
    if (producer?.cell$LKEY === "cycle") return kindHooks.reduceCycle(rf, init, producer, arity);
    const chunk = chunkOf(source);
    if (chunk !== undefined && chunk.step$LKEY === null) {
      let i = chunk.off$LKEY;
      if (arity === 2) init = chunk.arr$LKEY[i++];
      for (; i < chunk.arr$LKEY.length; i++) { init = rf.call(null, init, chunk.arr$LKEY[i]); if (isReduced(init)) return reducedValue(init); }
      return init;
    }
    const sequenceReduction = isCell(source) || isChunkedCons(source) || isLazy(source) || (!reduceValue(source) && !Array.isArray(source) && typeof source !== "string");
    if (arity === 2 && sequenceReduction) {
      const current = seq(source);
      return current == null ? rf.call(null) : reduce(rf, first(current), next(current));
    }
    if (sequenceReduction) {
      for (let current = seq(source); current != null; current = next(current)) {
        init = rf.call(null, init, first(current));
        if (isReduced(init)) return reducedValue(init);
      }
      return init;
    }
    let initialized = arity === 3, result = init;
    let current = seq(initialized ? collection : init);
    // A chunked seq over a vector's leaf continues past its storage.
    const walked = value => { const slice = viewOf(value); return slice?.vec$LKEY ? undefined : slice; };
    let slice = walked(current), offset = slice?.i$LKEY;
    // Traverse immutable storage directly. No recursive calls, per-step seq
    // wrappers, captured closures or persistent traversal caches are needed.
    while (current !== null) {
      const cell = cellOf(current);
      const item = cell ? cell.first$LKEY : slice ? slice.arr$LKEY[offset] : first(current);
      if (!initialized) {
        result = item;
        initialized = true;
      } else {
        // Read .call on every invocation, after the source/arguments have been
        // evaluated. Callable data and user functions share this ordinary ABI.
        result = rf.call(null, result, item);
        if (isReduced(result)) return reducedValue(result);
      }
      if (cell || !slice) {
        current = cell ? seq(cell.rest$LKEY) : next(current);
        slice = walked(current);
        offset = slice?.i$LKEY;
      } else if (++offset >= slice.arr$LKEY.length) {
        current = null;
        slice = undefined;
      }
    }
    return initialized ? result : rf.call(null);
  }

  //@unit iterateNode.reduce effect: iterateNode when: reduce requires: first, isReduced, kindHooks,
  //@ reducedValue
  kindHooks.reduceIterate = (rf, init, source, producer, arity) => {
    let value = first(source);
    if (arity === 2) { init = value; value = producer.f$LKEY.call(null, value); }
    while (true) { init = rf.call(null, init, value); if (isReduced(init)) return reducedValue(init); value = producer.f$LKEY.call(null, value); }
  };
  //@unit cycleNode.reduce effect: cycleNode when: reduce requires: cycleCurrent, first, isReduced,
  //@ kindHooks, next, reducedValue
  kindHooks.reduceCycle = (rf, init, producer, arity) => {
    let current = cycleCurrent(producer);
    if (arity === 2) { init = first(current); current = next(current) ?? producer.all$LKEY; }
    while (true) { init = rf.call(null, init, first(current)); if (isReduced(init)) return reducedValue(init); current = next(current) ?? producer.all$LKEY; }
  };
  //@unit reduce.protocol effect: installMethods when: reduce requires: methodKeys, protocolHooks,
  //@ reduce
  reduce = (operation => Object.defineProperty(function (rf, init, collection) {
    const arity = arguments.length, source = arity === 3 ? collection : init;
    const method = arity === 2 || arity === 3 ? protocolHooks.publicMethod?.(source, methodKeys[44], arity, true) : null;
    if (method) return arity === 3 ? method.call(source, source, rf, init) : method.call(source, source, rf);
    return operation.apply(null, arguments);
  }, "name", {value: operation.name}))(reduce);
  //@unit reducingFunction requires: invalidArity
  // Reducers and transforms use the same .call ABI as emitted functions and
  // custom IFn values. State is allocated when a transform receives its reducer.
  function reducingFunction(rf, step, complete = result => rf.call(null, result)) {
    return function (result, input) {
      switch (arguments.length) {
        case 0: return rf.call(null);
        case 1: return complete(result);
        case 2: return step(result, input);
        default: return invalidArity(arguments.length);
      }
    };
  }
  //@unit transform
  function transform(factory) {
    return function (rf) {
      return factory(rf);
    };
  }
  //@unit completing requires: invalidArity, reducingFunction
  function completing(rf, cf) {
    if (arguments.length === 1) cf = result => result;
    else if (arguments.length !== 2) return invalidArity(arguments.length);
    return reducingFunction(rf, (result, input) => rf.call(null, result, input), result => cf.call(null, result));
  }
  //@unit transduce requires: invalidArity, reduce
  function transduce(xf, rf, init, coll) {
    if (arguments.length === 3) { coll = init; init = rf.call(null); }
    else if (arguments.length !== 4) return invalidArity(arguments.length);
    const transformed = xf.call(null, rf);
    return transformed.call(null, reduce(transformed, init, coll));
  }
  //@unit indexedTransform requires: reducingFunction, transform
  function indexedTransform(fn, keepOnly, indexed) {
    return transform(rf => {
      let index = -1;
      return reducingFunction(rf, (result, input) => {
        const value = indexed ? fn.call(null, ++index, input) : fn.call(null, input);
        return keepOnly && value == null ? result : rf.call(null, result, value);
      });
    });
  }
  //@unit bufferedTransform requires: equalPair, isReduced, keyword, keywordIdentical,
  //@ reducingFunction, transform, unreduced, vectorFromArray
  function bufferedTransform(group) {
    return transform(rf => {
      const none = keyword("cljs.core", "none");
      let buffer = [], prior = none;
      const flush = result => {
        const value = vectorFromArray(buffer); buffer = [];
        return rf.call(null, result, value);
      };
      return reducingFunction(rf, (result, input) => {
        if (group.kind === "count") {
          buffer.push(input);
          return buffer.length === group.n ? flush(result) : result;
        }
        const previous = prior, key = group.fn.call(null, input); prior = key;
        if (keywordIdentical(previous, none) || equalPair(previous, key)) { buffer.push(input); return result; }
        const ret = flush(result);
        if (!isReduced(ret)) buffer.push(input);
        return ret;
      }, result => {
        // Clear before invoking the downstream step, including reentry/failure.
        if (buffer.length) result = unreduced(flush(result));
        return rf.call(null, result);
      });
    });
  }
  //@unit haltWhen requires: contains, get, hashMap, invalidArity, isMap, keyword, reduced,
  //@ reducingFunction, transform, truth
  function haltWhen(pred, retf) {
    if (arguments.length !== 1 && arguments.length !== 2) return invalidArity(arguments.length);
    const halt = keyword("cljs.core", "halt");
    return transform(rf => reducingFunction(rf, (result, input) =>
      truth(pred.call(null, input))
        ? reduced(hashMap(halt, truth(retf) ? retf.call(null, rf.call(null, result), input) : input))
        : rf.call(null, result, input),
      result => isMap(result) && contains(result, halt) ? get(result, halt) : rf.call(null, result)));
  }
  //@unit dedupe requires: cons, equalPair, first, invalidArity, keyword, lazySequence,
  //@ reducingFunction, rest, seq, transform
  function dedupe(coll) {
    if (arguments.length === 0) return transform(rf => {
      let prior = keyword("cljs.core", "none");
      return reducingFunction(rf, (result, input) => {
        const previous = prior; prior = input;
        return equalPair(previous, input) ? result : rf.call(null, result, input);
      });
    });
    if (arguments.length !== 1) return invalidArity(arguments.length);
    // Collection overload stays lazy; sequence's transducer overload is separate.
    const step = (values, prior) => lazySequence(() => {
      for (let s = seq(values); s !== null; s = seq(rest(s))) {
        const value = first(s);
        if (!equalPair(prior, value)) return cons(value, step(rest(s), value));
      }
      return null;
    });
    return step(coll, keyword("cljs.core", "none"));
  }
  //@unit randomSample requires: coreCells, filter, invalidArity
  function randomSample(prob, coll) {
    const pred = () => coreCells.rand.call(null) < prob;
    if (arguments.length === 1) return filter(pred);
    if (arguments.length === 2) return filter(pred, coll);
    return invalidArity(arguments.length);
  }

  //@unit reduceKv requires: holdsVector, isEntryCell, isReduced, isTreeMap, keyProtocol, kindHooks,
  //@ mapPairs, methodKeys, protocolHooks, reduce, reducedValue, valProtocol, vectorItems
  function reduceKv(rf, init, collection) {
    const method = protocolHooks.publicMethod?.(collection, methodKeys[28], 3);
    if (method) return method.call(collection, collection, rf, init);
    if (collection == null) return init;
    // MapEntry has vector storage, but no IKVReduce implementation. Its
    // elements must go through the public IMapEntry sequence fallback.
    // A sorted map walks its tree (light-cljs-1035), another map its keys
    // and values, flat (`mapPairs`).
    if (isTreeMap(collection)) {
      const tree = collection.tree$LKEY;
      if (tree == null) return init;
      const result = kindHooks.treeKvReduce(tree, rf, init);
      return isReduced(result) ? reducedValue(result) : result;
    }
    const pairs = mapPairs(collection);
    if (pairs !== undefined) {
      for (let i = 0; i < pairs.length; i += 2) {
        init = rf.call(null, init, pairs[i], pairs[i + 1]);
        if (isReduced(init)) return reducedValue(init);
      }
      return init;
    }
    const entries = isEntryCell(collection) || !holdsVector(collection) ? undefined : vectorItems(collection).map((value, key) => [key, value]);
    if (!entries) return reduce((result, entry) => rf.call(null, result, keyProtocol(entry), valProtocol(entry)), init, collection);
    for (const [key, value] of entries) {
      init = rf.call(null, init, key, value);
      if (isReduced(init)) return reducedValue(init);
    }
    return init;
  }
  //@unit subvecSources
  const subvecSources = new WeakMap();
  //@unit subvec requires: count, isVector, methodKeys, nth, protocolHooks, subvecOfTrie,
  //@ subvecSources, vectorFromArray, vectorStore, vectorView
  function subvec(value, start, end = count(value)) {
    if (start == null || end == null) throw new Error("Assert failed: subvec indices cannot be nil");
    start = start | 0; end = end | 0;
    if (start < 0 || end < start || end > count(value)) throw new Error("Index out of bounds");
    if (!isVector(value)) throw new Error("subvec requires a vector");
    // A view of a vector's store, as upstream's Subvec, unless a protocol
    // implementation answers its `nth`: then a copy of what that answers.
    const store = !protocolHooks.publicMethod?.(value, methodKeys[25], 2, true, true) && vectorStore(value);
    const result = store ? vectorView(store, start, end - start)
      : subvecOfTrie(vectorStore(vectorFromArray(Array.from({length: end - start}, (_, i) => nth(value, start + i)))));
    subvecSources.set(result, subvecSources.get(value) ?? value);
    return result;
  }
  //@unit iteratorInit
  const iteratorInit = {}, iteratorStart = {};
  //@unit SeqIter requires: first, iteratorInit, iteratorStart, next, seq
  class SeqIter {
    constructor(current, following) { this._seq = current; this._next = following; }
    hasNext() {
      if (this._seq === iteratorInit) { this._seq = iteratorStart; this._next = seq(this._next); }
      else if (this._seq === this._next) this._next = next(this._seq);
      return this._next != null;
    }
    next() {
      if (!this.hasNext()) throw new Error("No such element");
      this._seq = this._next;
      return first(this._next);
    }
    remove() { return new Error("Unsupported operation"); }
  }
  //@unit RangedIterator requires: vectorLeaf, vectorStore
  class RangedIterator {
    constructor(i, base, arr, v, start, end) {
      Object.assign(this, {i, base, arr, v, start, end});
    }
    hasNext() { return this.i < this.end; }
    next() {
      if (this.i - this.base === 32) {
        this.arr = vectorLeaf(vectorStore(this.v), this.i).slice();
        this.base += 32;
      }
      const value = this.arr[this.i & 31]; this.i++; return value;
    }
  }
  //@unit rangedIterator requires: count, RangedIterator, vectorLeaf, vectorStore
  function rangedIterator(value, start, end) {
    const base = start - start % 32;
    // Its public `arr` is a copy of a leaf, which the caller may change.
    return new RangedIterator(start, base, start < count(value) ? vectorLeaf(vectorStore(value), base).slice() : null, value, start, end);
  }
  //@unit seqIter requires: iteratorInit, SeqIter
  function seqIter(value) { return new SeqIter(iteratorInit, value); }
  //@unit iterator requires: holdsVector, iteratorInit, kindHooks, methodKeys, protocolHooks,
  //@ publicCollectionSlots, SeqIter, subvecSources
  function iterator(value) {
    const method = protocolHooks.builtin?.(value, methodKeys[26], 1);
    if (method) return method.call(value, value);
    const slots = publicCollectionSlots.get(value);
    if (slots?.arr) return kindHooks.iteratorArrayMap(slots);
    return holdsVector(subvecSources.get(value) ?? value) ? kindHooks.iteratorVector(value) : new SeqIter(iteratorInit, value);
  }
  //@unit PersistentArrayMapValue.iterator effect: PersistentArrayMapValue when: iterator requires: kindHooks,
  //@ mapEntry
  // Maps with an `arr` slot, which only the public array map constructor builds.
  kindHooks.iteratorArrayMap = slots => {
    let i = 0;
    return {hasNext: () => i < slots.cnt * 2, next() {
      if (i >= slots.cnt * 2) throw new Error("No such element");
      const entry = mapEntry(slots.arr[i], slots.arr[i + 1]); i += 2; return entry;
    }, remove() { return new Error("Unsupported operation"); }};
  };
  //@unit makeVector.iterator effect: makeVector, mapEntry when: iterator requires: count,
  //@ kindHooks, rangedIterator
  kindHooks.iteratorVector = value => rangedIterator(value, 0, count(value));
  //@unit equivMap requires: count, equalPair, first, get, isMap, missing, next, nth, seq
  function equivMap(value, other) {
    if (!isMap(other)) return false;
    let n = 0;
    for (let current = seq(value); current != null; current = next(current), n++) {
      const entry = first(current), found = get(other, nth(entry, 0), missing);
      if (found === missing || !equalPair(found, nth(entry, 1))) return false;
    }
    return n === count(other);
  }

  //@unit conj requires: cellOf, cons, emptyList, holdsVector, isCell, isChunkedCons, isLazy,
  //@ isView, kindHooks, makeVector, maps, metadataValues, methodKeys, missing, ownMeta,
  //@ preserveMeta, protocolHooks, publicCollectionSlots, queues, setMeta, sets, subvecSources,
  //@ vectorStore
  function conj(coll, item) {
    // One item onto a vector, where no protocol implementation or public
    // constructor can intervene: the hot path of building one.
    const store = arguments.length === 2 && !protocolHooks.builtin ? vectorStore(coll) : undefined;
    if (store && !subvecSources.has(coll)) {
      const built = kindHooks.conjVector(store, item);
      if (ownMeta(coll)) { if (coll.meta$LKEY !== null) setMeta(built, coll.meta$LKEY); }
      else if (metadataValues.has(coll)) metadataValues.set(built, metadataValues.get(coll));
      return built;
    }
    // Likewise one member onto a set (light-cljs-236).
    if (arguments.length === 2 && !protocolHooks.builtin && sets.has(coll)) {
      const built = kindHooks.conjSet(coll, coll, item);
      if (built === missing) return coll;
      if (metadataValues.has(coll)) metadataValues.set(built, metadataValues.get(coll));
      return built;
    }
    const args = Array.from(arguments);
    if (args.length === 0) return makeVector([]);
    let result = args[0];
    for (let i = 1; i < args.length; i++) {
      const previous = result;
      const custom = result == null ? null : protocolHooks.builtin?.(result, methodKeys[7], 2);
      if (custom) { result = custom.call(result, result, args[i]); continue; }
      const items = vectorStore(result);
      if (queues.has(result)) result = kindHooks.conjQueue(result, args[i]);
      else if (items) result = publicCollectionSlots.has(result) && subvecSources.has(result) ? kindHooks.conjSubvec(result, args[i]) : kindHooks.conjVector(items, args[i]);
      else if (maps.has(result)) result = kindHooks.conjMap(result, args[i]);
      else if (sets.has(result)) {
        const updated = kindHooks.conjSet(previous, result, args[i]);
        if (updated === missing) continue;
        result = updated;
      }
      else if (result == null) result = cons(args[i], emptyList, "list");
      else if (isCell(result) || isChunkedCons(result) || isView(result) || isLazy(result))
        result = cons(args[i], result, cellOf(result)?.cell$LKEY === "list" ? "list" : "cons");
      else throw new Error("conj not supported on this type");
      if (holdsVector(previous) || maps.has(previous) || sets.has(previous) || queues.has(previous) || cellOf(previous)?.cell$LKEY === "list") result = preserveMeta(previous, result);
    }
    return result;
  }
  //@unit queueFromArray.conj effect: queueFromArray when: conj requires: conj, kindHooks,
  //@ makeVector, meta, PersistentQueueValue, publicCollectionSlots, queueFromArray, queues
  kindHooks.conjQueue = (result, item) => {
    const q = publicCollectionSlots.get(result);
    return q ? PersistentQueueValue(meta(result), q.count + 1, q.front != null ? q.front : conj(null, item), q.front != null ? conj(q.rear ?? makeVector([]), item) : makeVector([]), null)
      : queueFromArray([...queues.get(result), item]);
  };
  //@unit makeVector.conj effect: makeVector, mapEntry when: conj requires: kindHooks, makeVector,
  //@ vectorConj
  kindHooks.conjVector = (store, item) => makeVector(null, vectorConj(store, item));
  //@unit SubvecValue.conj effect: SubvecValue when: conj requires: assoc, kindHooks, meta,
  //@ publicCollectionSlots, subvecSources, SubvecValue
  // A vector with slots and a source is a Subvec its public constructor built.
  kindHooks.conjSubvec = (result, item) => {
    const slots = publicCollectionSlots.get(result);
    return SubvecValue(meta(result), assoc(subvecSources.get(result), slots.end, item), slots.start, slots.end + 1, null);
  };
  //@unit makeMap.conj effect: ArrayMapCell, makeMap, treeMapConstructor, recordMethods when: conj
  //@ requires: kindHooks, mapConj
  kindHooks.conjMap = mapConj;
  //@unit makeSet.conj effect: makeSet, treeSetConstructor when: conj requires: assoc, copyEntries,
  //@ kindHooks, makeSet, meta, missing, PersistentHashSetValue, publicCollectionSlots, putMember,
  //@ putSortedEntry, setComparators, sets, setTypes
  // The set `conj` builds, or `missing` where it adds nothing. A hash set's
  // trie copies one path (`putMember`).
  kindHooks.conjSet = (previous, result, item) => {
    if (publicCollectionSlots.has(result)) {
      const slots = publicCollectionSlots.get(result), source = slots["tree-map"] ?? slots["hash-map"], updated = assoc(source, item, null);
      return updated === source ? result : slots["tree-map"] ? setTypes["sorted-set"](meta(result), updated, null) : PersistentHashSetValue(meta(result), updated, null);
    }
    const original = sets.get(result), comparator = setComparators.get(previous);
    let members = copyEntries(original);
    if (comparator) putSortedEntry(members, item, undefined, comparator, true);
    else members = putMember(members, item);
    if (members.length === original.length) return missing;
    return comparator ? makeSet(setTypes["sorted-set"], members, "sorted-set", comparator)
      : makeSet(PersistentHashSetValue, members, Array.isArray(members) ? "array-map" : "hash-map");
  };
  //@unit conj.branches effect: PersistentArrayMapValue, PersistentHashMapValue,
  //@ PersistentHashSetValue, PersistentQueueValue, PersistentVectorValue, SubvecValue,
  //@ treeMapConstructor, treeSetConstructor when: conj requires: conj, kindOps, kindTables,
  //@ noBranch
  // As `seq.branches`, for one item: a branch takes the collection and the
  // item, and makes the protocol lookup that `conj` makes first.
  conj = (operation => Object.defineProperty(function (coll, item) {
    const kind = coll?.[kindOps], result = typeof kind === "number" && arguments.length === 2 ? kindTables[kind & 15][7](coll, item) : noBranch;
    return result !== noBranch ? result : operation.apply(null, arguments);
  }, "name", {value: operation.name}))(conj);
  //@unit conjMeta requires: metadataValues, ownMeta, preserveMeta, protocolHooks, setMeta
  // What `conj` of a vector or a set gives `built` from `coll`: its metadata,
  // which its fast path copies and its other path reads with `meta`, which
  // protocol implementations can answer.
  function conjMeta(coll, built) {
    if (protocolHooks.builtin) return preserveMeta(coll, built);
    if (ownMeta(coll)) { if (coll.meta$LKEY !== null) setMeta(built, coll.meta$LKEY); }
    else if (metadataValues.has(coll)) setMeta(built, metadataValues.get(coll));
    return built;
  }
  //@unit conj.vectors effect: PersistentVectorValue, SubvecValue when: conj requires: conjMeta,
  //@ kindHooks, kindTables, methodKeys, noBranch, protocolHooks, subvecSources, vectorStore
  // A subvector takes the operation (`SubvecValue.conj`). Each branch calls
  // what the protocol lookup finds, as the operation would, so that it looks
  // it up once.
  kindTables[4][7] = (coll, item) => {
    const store = vectorStore(coll);
    if (!store || subvecSources.has(coll)) return noBranch;
    const custom = protocolHooks.builtin?.(coll, methodKeys[7], 2);
    return custom ? custom.call(coll, coll, item) : conjMeta(coll, kindHooks.conjVector(store, item));
  };
  //@unit conj.maps effect: PersistentArrayMapValue, PersistentHashMapValue, treeMapConstructor
  //@ when: conj requires: kindHooks, kindTables, maps, methodKeys, noBranch, preserveMeta,
  //@ protocolHooks
  kindTables[5][7] = (coll, item) => {
    if (!maps.has(coll)) return noBranch;
    const custom = protocolHooks.builtin?.(coll, methodKeys[7], 2);
    return custom ? custom.call(coll, coll, item) : preserveMeta(coll, kindHooks.conjMap(coll, item));
  };
  //@unit conj.sets effect: PersistentHashSetValue, treeSetConstructor when: conj requires: conjMeta,
  //@ kindHooks, kindTables, methodKeys, missing, noBranch, protocolHooks, sets
  kindTables[6][7] = (coll, item) => {
    if (!sets.has(coll)) return noBranch;
    const custom = protocolHooks.builtin?.(coll, methodKeys[7], 2);
    if (custom) return custom.call(coll, coll, item);
    const built = kindHooks.conjSet(coll, coll, item);
    return built === missing ? coll : conjMeta(coll, built);
  };
  //@unit conj.queues effect: PersistentQueueValue when: conj requires: kindHooks, kindTables,
  //@ methodKeys, noBranch, preserveMeta, protocolHooks, queues
  kindTables[7][7] = (coll, item) => {
    if (!queues.has(coll)) return noBranch;
    const custom = protocolHooks.builtin?.(coll, methodKeys[7], 2);
    return custom ? custom.call(coll, coll, item) : preserveMeta(coll, kindHooks.conjQueue(coll, item));
  };

  //@unit assoc requires: copyEntries, holdsVector, isArrayMap, isHashMap, isTreeMap, kindHooks,
  //@ maps, methodKeys, missing, protocolHooks, publicCollectionSlots, putCollectionEntry,
  //@ subvecSources, updatedMap, vectorStore
  function assoc(value, key, item, ...pairs) {
    const method = value == null ? null : protocolHooks.builtin?.(value, methodKeys[1], 3);
    if (method) {
      // A trailing key without its value gets nil, as upstream's `assoc`
      // takes `(second kvs)`: where the compile-time protocol lookup finds a
      // runtime class's own `-assoc` slot (light-cljs-1028), as the map
      // path below does.
      let result = method.call(value, value, key, item);
      for (let i = 0; i < pairs.length; i += 2) result = assoc(result, pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null);
      return result;
    }
    if (holdsVector(value) && subvecSources.has(value) && publicCollectionSlots.has(value)) return kindHooks.assocSubvec(value, key, item, pairs);
    const mapSlots = publicCollectionSlots.get(value);
    if (maps.has(value) && mapSlots?.arr && mapSlots.cnt >= 8) {
      const promoted = kindHooks.assocArrayMapSlots(value, mapSlots, key, item, pairs);
      if (promoted !== missing) return promoted;
    }
    // An array map's, a hash map's and a sorted map's own paths are their
    // slots' functions, which read their fields (light-cljs-1036, 1035).
    const own = isArrayMap(value) ? kindHooks.assocArrayMap : isHashMap(value) ? kindHooks.assocHashMap : isTreeMap(value) ? kindHooks.assocTreeMap : undefined;
    if (own !== undefined) {
      let result = own(value, key, item);
      for (let i = 0; i < pairs.length; i += 2)
        result = assoc(result, pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null);
      return result;
    }
    if (value == null || maps.has(value)) {
      const entries = value == null ? [] : copyEntries(maps.get(value));
      putCollectionEntry(value, entries, key, item);
      let result = updatedMap(value, entries);
      for (let i = 0; i < pairs.length; i += 2)
        result = assoc(result, pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null);
      return result;
    }
    const original = vectorStore(value);
    if (!original) throw new Error("assoc not supported on this type");
    return kindHooks.assocVector(value, original, key, item, pairs);
  }
  //@unit assoc.branches effect: PersistentArrayMapValue, PersistentHashMapValue,
  //@ PersistentVectorValue, SubvecValue, treeMapConstructor when: assoc requires: assoc, kindOps,
  //@ kindTables, noBranch
  // As `seq.branches`, for one key and item: a branch takes the collection,
  // the key and the item, and makes the protocol lookup that `assoc` makes
  // first.
  assoc = (operation => Object.defineProperty(function (value, key, item) {
    const kind = value?.[kindOps], result = typeof kind === "number" && arguments.length === 3 ? kindTables[kind & 15][8](value, key, item) : noBranch;
    return result !== noBranch ? result : operation.apply(null, arguments);
  }, "name", {value: operation.name}))(assoc);
  //@unit assoc.vectors effect: PersistentVectorValue, SubvecValue when: assoc requires: kindHooks,
  //@ kindTables, methodKeys, noBranch, protocolHooks, publicCollectionSlots, vectorStore
  // A public constructor's value, a subvector or an array map with slots,
  // takes the operation; a branch calls what the protocol lookup finds.
  kindTables[4][8] = (value, key, item) => {
    const store = vectorStore(value);
    if (!store || publicCollectionSlots.has(value)) return noBranch;
    const method = protocolHooks.builtin?.(value, methodKeys[1], 3);
    return method ? method.call(value, value, key, item) : kindHooks.assocVector(value, store, key, item, []);
  };
  //@unit assoc.maps effect: PersistentArrayMapValue, PersistentHashMapValue, treeMapConstructor
  //@ when: assoc requires: copyEntries, isArrayMap, isHashMap, isTreeMap, kindHooks, kindTables,
  //@ maps, methodKeys, noBranch, protocolHooks, publicCollectionSlots, putCollectionEntry,
  //@ updatedMap
  // An array map's, a hash map's and a sorted map's own paths are their
  // slots' functions, which also build a public constructor's maps; a
  // record's, its entries.
  kindTables[5][8] = (value, key, item) => {
    const own = isArrayMap(value) ? kindHooks.assocArrayMap : isHashMap(value) ? kindHooks.assocHashMap : isTreeMap(value) ? kindHooks.assocTreeMap : undefined;
    if (own !== undefined) {
      const method = protocolHooks.builtin?.(value, methodKeys[1], 3);
      return method ? method.call(value, value, key, item) : own(value, key, item);
    }
    const entries = maps.get(value);
    if (!entries || publicCollectionSlots.has(value)) return noBranch;
    const method = protocolHooks.builtin?.(value, methodKeys[1], 3);
    if (method) return method.call(value, value, key, item);
    const updated = copyEntries(entries);
    putCollectionEntry(value, updated, key, item);
    return updatedMap(value, updated);
  };
  //@unit SubvecValue.assoc effect: SubvecValue when: assoc requires: assoc, kindHooks, meta,
  //@ outOfBounds, publicCollectionSlots, subvecSources, SubvecValue
  // See `SubvecValue.conj`.
  kindHooks.assocSubvec = (value, key, item, pairs) => {
    const slots = publicCollectionSlots.get(value);
    if (key < 0 || slots.start + key > slots.end) return outOfBounds();
    let result = SubvecValue(meta(value), assoc(subvecSources.get(value), slots.start + key, item), slots.start, Math.max(slots.end, slots.start + key + 1), null);
    return pairs.length ? assoc(result, ...pairs) : result;
  };
  //@unit PersistentArrayMapValue.assoc effect: PersistentArrayMapValue when: assoc, mapAssocEntries
  //@ requires: assoc, collectionWithMeta, contains, kindHooks, mapFromArray, meta, missing
  // A map with an `arr` slot is an array map its public constructor built, or
  // one updated from it; it becomes a hash map at its ninth key, or else
  // gives `missing`.
  kindHooks.assocArrayMapSlots = (value, mapSlots, key, item, pairs) => {
    if (contains(value, key)) return missing;
    const normalized = mapFromArray(Array.from({length: mapSlots.cnt * 2}, (_, i) => mapSlots.arr[i]), "hash-map");
    return collectionWithMeta(assoc(normalized, key, item, ...pairs), meta(value));
  };
  //@unit makeVector.assoc effect: makeVector, mapEntry when: assoc requires: kindHooks, makeVector,
  //@ outOfBounds, preserveMeta, vectorAssoc, vectorConj
  kindHooks.assocVector = (value, store, key, item, pairs) => {
    const assign = (index, value) => {
      if (typeof index !== "number") throw new Error("Vector key must be a number");
      if (index >= 0 && index < store.cnt$LKEY) store = vectorAssoc(store, index, value);
      else if (index === store.cnt$LKEY) store = vectorConj(store, value);
      else outOfBounds();
    };
    // The fixed arity follows JS argument binding, including missing item.
    assign(key, item);
    for (let i = 0; i < pairs.length; i += 2)
      assign(pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null);
    return preserveMeta(value, makeVector(null, store));
  };

  //@unit ownMap requires: maps, protocolHooks, publicCollectionSlots, recordTypes
  // Whether a map iterates its own entries (see `hostHooks`); a record, whose
  // entries are its fields, is another type to these methods.
  function ownMap(value) { return !protocolHooks.extend && maps.has(value) && !recordTypes.has(value) && !("tree" in (publicCollectionSlots.get(value) ?? {})); }
  //@unit mapCount requires: isArrayMap, isHashMap, isTreeMap, maps, publicCollectionSlots
  // `count` of a map without protocol implementations: an array map's and
  // a hash map's `cnt`, a public constructor's too.
  function mapCount(value) {
    if (isArrayMap(value) || isHashMap(value) || isTreeMap(value)) return value.cnt$LKEY;
    const slots = publicCollectionSlots.get(value);
    if (slots && ("cnt" in slots || "count" in slots)) return "cnt" in slots ? slots.cnt : slots.count;
    return maps.get(value).length;
  }
  //@unit itemIterator
  // `es6Iterator` of a seq walking `storage` (null when empty): `item` of each.
  function itemIterator(storage, item) {
    let index = 0, more = storage !== null;
    return {next() {
      if (!more) return {value: null, done: true};
      const result = item(storage[index]);
      more = ++index < storage.length;
      return {value: result, done: false};
    }};
  }
  //@unit mapIterator requires: mapPairs, publicCollectionSlots
  // Of a map's seq (`makeMap.seq`): `item` of each key and value, which it
  // reads of the map's pairs (`mapPairs`, light-cljs-1036): a copy of a
  // public constructor's, whose caller's array can change after the
  // iterator is made, as its entries were made then.
  function mapIterator(value, item) {
    const pairs = publicCollectionSlots.has(value) ? mapPairs(value).slice() : mapPairs(value);
    let index = 0;
    return {next() {
      if (index >= pairs.length) return {value: null, done: true};
      const result = item(pairs[index], pairs[index + 1]);
      index += 2;
      return {value: result, done: false};
    }};
  }
  //@unit mapPrototype requires: contains, get, hostHooks, invalidArity, lookup, mapEntry,
  //@ mapIterator, maps, objectEquiv, ownMap, printHooks, unkeyedJSON
  const mapPrototype = Object.freeze({
    equiv(other) { return objectEquiv(this, other); },
    // JSON of a map: its own properties but its keyed fields, as before it
    // had them (docs/value-layout.md, question 3).
    toJSON() { return unkeyedJSON(this); },
    has(key) { return contains(this, key); },
    get(key, notFound = null) { return get(this, key, notFound); },
    keys() { return ownMap(this) ? mapIterator(this, key => key) : hostHooks.keys(this); },
    values() { return ownMap(this) ? mapIterator(this, (_key, value) => value) : hostHooks.values(this); },
    entries() { return ownMap(this) ? mapIterator(this, (key, value) => [key, value]) : hostHooks.entries(this); },
    [Symbol.iterator]() { return ownMap(this) ? mapIterator(this, mapEntry) : hostHooks.iterator(this); },
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    toString() { return printHooks.collection(this); },
    call(_self, key, notFound) {
      if (!maps.has(this)) throw new Error("Expected map");
      switch (arguments.length) {
        case 2: return lookup(this, key, null);
        case 3: return lookup(this, key, notFound);
        default: return invalidArity(arguments.length - 1);
      }
    }
  });

  //@unit PersistentArrayMapValue.type effect: PersistentArrayMapValue requires: ArrayMapCell,
  //@ builtinType, kindOps, mapPrototype, PersistentArrayMapValue, viewType
  // The maps are classes of their types' prototypes (light-cljs-1012), of the
  // kinds `mapOf` reads; the kind table number stays until light-cljs-1015.
  viewType(builtinType(PersistentArrayMapValue, mapPrototype), ArrayMapCell, 28).prototype[kindOps] = 5;
  //@unit ArrayMapCell.type effect: ArrayMapCell requires: ArrayMapCell, PersistentArrayMapValue
  // Operations that build a map directly keep its type, whose unit above
  // gives it its prototype, kind and slots; so for each map class.
  ArrayMapCell.prototype = PersistentArrayMapValue.prototype;
  //@unit ArrayMapCell.lookup effect: ArrayMapCell when: lookup requires: arrayMapLookup, kindHooks
  kindHooks.arrayMapLookup = arrayMapLookup;
  //@unit PersistentHashMapValue.type effect: PersistentHashMapValue requires: builtinType,
  //@ HashMapCell, kindOps, mapPrototype, PersistentHashMapValue, viewType
  viewType(builtinType(PersistentHashMapValue, mapPrototype), HashMapCell, 29).prototype[kindOps] = 5;
  //@unit HashMapCell.type effect: HashMapCell requires: HashMapCell, PersistentHashMapValue
  HashMapCell.prototype = PersistentHashMapValue.prototype;
  //@unit HashMapCell.lookup effect: HashMapCell when: lookup requires: hashMapLookup, kindHooks
  kindHooks.hashMapLookup = hashMapLookup;
  //@unit treeMapConstructor.type effect: treeMapConstructor requires: builtinType, kindOps,
  //@ mapPrototype, TreeMapCell, treeMapConstructor, viewType
  viewType(builtinType(treeMapConstructor, mapPrototype), TreeMapCell, 30).prototype[kindOps] = 5;
  //@unit TreeMapCell.type effect: TreeMapCell requires: TreeMapCell, treeMapConstructor
  TreeMapCell.prototype = treeMapConstructor.prototype;
  //@unit TreeMapCell.lookup effect: TreeMapCell when: lookup requires: kindHooks, treeMapLookup
  // A sorted map's paths, of its fields (light-cljs-1035), which the older
  // layers of the operations take, as the other maps' (`ArrayMapCell.lookup`).
  kindHooks.treeMapLookup = treeMapLookup;
  //@unit TreeMapCell.entries effect: TreeMapCell when: makeMap requires: kindHooks, treeOfEntries
  kindHooks.treeOfEntries = treeOfEntries;
  //@unit TreeMapCell.traversal effect: TreeMapCell when: mapTraversal, mapPairs requires: ArrayMapEntry,
  //@ kindHooks, treePairs
  // Its keys and values in order, flat (`mapPairs`) or as fresh entries
  // (`mapTraversal`), which a caller may change.
  kindHooks.treeMapPairs = map => treePairs(map.tree$LKEY, []);
  kindHooks.treeMapEntries = map => {
    const pairs = treePairs(map.tree$LKEY, []), entries = [];
    for (let i = 0; i < pairs.length; i += 2) entries.push(new ArrayMapEntry(pairs[i], pairs[i + 1]));
    return entries;
  };
  //@unit TreeMapCell.find effect: TreeMapCell when: find requires: kindHooks, treeEntryAt
  kindHooks.treeEntryAt = treeEntryAt;
  //@unit TreeMapCell.kvReduce effect: TreeMapCell when: reduceKv requires: kindHooks, treeKvReduce
  kindHooks.treeKvReduce = treeKvReduce;
  //@unit PersistentArrayMapValue.kind effect: PersistentArrayMapValue requires: mapTypes,
  //@ PersistentArrayMapValue
  mapTypes["array-map"] = PersistentArrayMapValue;
  //@unit PersistentHashMapValue.kind effect: PersistentHashMapValue requires: mapTypes,
  //@ PersistentHashMapValue
  mapTypes["hash-map"] = PersistentHashMapValue;
  //@unit lookup.sets effect: makeSet, treeSetConstructor when: lookup requires: kindHooks, lookup,
  //@ sets
  // The branches of `lookup` for sets, transients and sorted maps, kept only
  // where a program builds them (light-cljs-247). They wrap it in the order
  // its branches had; the hash tries take the hooks of `get.trie` and
  // `get.members` (light-cljs-302), and the last two wrappers call the hooks
  // their kinds' builders install, so that a chunk building neither keeps no
  // more than these.
  lookup = (operation => (value, key, notFound) => {
    const members = sets.get(value);
    if (!members) return operation(value, key, notFound);
    if (!Array.isArray(members)) return kindHooks.trieMember(members, key, notFound);
    const index = kindHooks.entryIndex(value, members, key);
    return index < 0 ? notFound : members[index].key;
  })(lookup);
  //@unit lookup.transients effect: makeTransient when: lookup requires: isTransient, kindHooks,
  //@ lookup
  lookup = (operation => (value, key, notFound) => isTransient(value) ? kindHooks.lookupTransient(value, key, notFound) : operation(value, key, notFound))(lookup);
  //@unit lookup.tree effect: treeMapConstructor when: lookup requires: isTreeMap, kindHooks, lookup
  // A sorted map's lookup walks its tree (light-cljs-1035), a branch only a
  // program building sorted maps keeps; the walk is a hook its class
  // installs, so that the wrapper, which shares `lookup`'s chunk, needs
  // none of it.
  lookup = (operation => (value, key, notFound) => isTreeMap(value) ? kindHooks.treeMapLookup(value, key, notFound) : operation(value, key, notFound))(lookup);
  //@unit get.trie effect: PersistentHashMapValue when: get, lookup requires: kindHooks, trieEntry
  // The item under `key` of a hash map's trie, which `get.maps` (light-cljs-358)
  // and `lookup` read, kept only where a program builds hash maps
  // (light-cljs-235; a hook rather than a wrapper of `lookup` since
  // light-cljs-302, so that a chunk places it with the hash maps).
  kindHooks.trieValue = (trie, key, notFound) => { const entry = trieEntry(trie, key); return entry ? entry.value : notFound; };
  //@unit get.members effect: publicHashSet, putMember when: get, lookup requires: kindHooks,
  //@ trieEntry
  // Likewise the member of a hash set's trie (`putMember`), which `get.sets`
  // (light-cljs-373) and `lookup.sets` read, kept only where a program builds
  // hash sets (light-cljs-236).
  kindHooks.trieMember = (trie, key, notFound) => { const entry = trieEntry(trie, key); return entry ? entry.key : notFound; };
  //@unit copyEntries.trie effect: PersistentHashMapValue, publicHashSet, putMember when: copyEntries
  //@ requires: kindHooks
  // A trie is immutable; its copy is the fields an update replaces.
  kindHooks.copyTrie = entries => ({root: entries.root, nil: entries.nil, length: entries.length});
  //@unit treeMapConstructor.kind effect: treeMapConstructor requires: mapTypes, treeMapConstructor
  mapTypes["sorted-map"] = treeMapConstructor;
  //@unit mapFromEntries requires: hashTrie, makeMap, PersistentArrayMapValue,
  //@ PersistentHashMapValue
  // A hash or array map of `entries`.
  function mapFromEntries(entries, hash) {
    return hash ? makeMap(PersistentHashMapValue, hashTrie(entries), "hash-map") : makeMap(PersistentArrayMapValue, entries, "array-map");
  }
  //@unit hashRoots
  // Routing nodes preserve observable key tests and rehashes at trie splits.
  // Immutable nodes can be shared by old collections and transient copies.
  // Transient capacity affects expansion timing.
  const hashRoots = new WeakMap();
  //@unit hashTraversals
  const hashTraversals = new WeakMap();
  //@unit copyEntries requires: hashRoots, kindHooks
  function copyEntries(entries) {
    // A trie, which is no array, has no `slice` (`copyEntries.trie`).
    const copy = entries.slice?.() ?? kindHooks.copyTrie(entries);
    if (hashRoots.has(entries)) hashRoots.set(copy, hashRoots.get(entries));
    return copy;
  }
  //@unit bitmapNode
  // A bitmap node: its slots, the edit that made it, the pairs its upstream
  // array holds (`capacity`, which times a transient's expansion) and its
  // occupied slots (`count`). An array node has the same fields, capacity 0,
  // so that every node has one shape.
  function bitmapNode(slots, edit, capacity, count) {
    return {kind: "bitmap", slots, edit, capacity, count};
  }
  //@unit hashNodeAssoc requires: bitmapNode, equalPair, hash
  function hashNodeAssoc(node, shift, keyHash, entry, edit, result, effects = true) {
    if (!node) {
      // Upstream's empty node grows to 4 pairs for an edit, 1 otherwise.
      const index = (keyHash >>> shift) & 31, slots = new Array(index + 1); slots[index] = entry;
      return bitmapNode(slots, edit, edit ? 4 : 1, 1);
    }
    if (node.kind === "collision") {
      if (keyHash === node.hash) {
        const index = node.entries.findIndex(old => effects ? equalPair(entry.key, old.key) : entry.key === old.key);
        const entries = node.entries.slice();
        if (index < 0) entries.push(entry);
        else {
          result.old = entries[index];
          // Persistent collision nodes compare values with public equality;
          // transient replacement uses identity, as does an ordinary leaf.
          if (effects && !edit && equalPair(result.old.value, entry.value)) {
            result.entry = result.old; return node;
          }
          entries[index] = {key: result.old.key, value: entry.value, hash: result.old.hash};
          result.entry = entries[index];
        }
        return {...node, entries};
      }
      const slots = []; slots[(node.hash >>> shift) & 31] = node;
      return hashNodeAssoc(bitmapNode(slots, edit, edit ? 2 : 1, 1), shift, keyHash, entry, edit, result, effects);
    }
    // A slot holds a child node, which has a `kind`, or an entry, which has
    // none; an array node's slots hold only nodes. As upstream's transients,
    // an edit changes in place the bitmap and array nodes it made (`owned`),
    // which no other version shares, and copies the rest once.
    const index = (keyHash >>> shift) & 31, slot = node.slots[index], n = node.count;
    const owned = edit && node.edit === edit, slots = owned ? node.slots : node.slots.slice();
    if (node.kind === "array") {
      slots[index] = hashNodeAssoc(slot, shift + 5, keyHash, entry, edit, result, effects);
      if (!owned) return {kind: "array", slots, edit, capacity: 0, count: slot ? n : n + 1};
      if (!slot) node.count++;
      return node;
    }
    if (!slot) {
      if (n >= 16 && (!edit || n >= node.capacity)) {
        // Canonical expansion visits retained leaves in ascending branch order,
        // into new slots, as upstream's, so a hash that throws changes nothing.
        const nodes = owned ? slots.slice() : slots;
        nodes[index] = hashNodeAssoc(null, shift + 5, keyHash, entry, edit, result, effects);
        for (let i = 0; i < nodes.length; ++i) {
          const old = nodes[i];
          if (old && !old.kind) nodes[i] = hashNodeAssoc(null, shift + 5, effects ? hash(old.key) : old.hash, old, edit, null, effects);
        }
        return {kind: "array", slots: nodes, edit, capacity: 0, count: n + 1};
      }
      slots[index] = entry;
      const capacity = !edit ? n + 1 : n < node.capacity ? owned ? node.capacity : n + 1 : n + 4;
      if (owned) { node.capacity = capacity; node.count = n + 1; return node; }
      return bitmapNode(slots, edit, capacity, n + 1);
    }
    if (slot.kind) slots[index] = hashNodeAssoc(slot, shift + 5, keyHash, entry, edit, result, effects);
    else if (effects ? equalPair(entry.key, slot.key) : entry.key === slot.key) {
      result.old = slot;
      result.entry = {key: slot.key, value: entry.value, hash: slot.hash};
      slots[index] = result.entry;
    } else {
      const oldHash = effects ? hash(slot.key) : slot.hash;
      let child;
      if (oldHash === keyHash) child = {kind: "collision", hash: keyHash, entries: [slot, entry]};
      else {
        child = hashNodeAssoc(null, shift + 5, oldHash, slot, edit, null, effects);
        child = hashNodeAssoc(child, shift + 5, keyHash, entry, edit, {}, effects);
      }
      slots[index] = child;
    }
    return owned ? node : bitmapNode(slots, edit, edit ? n + 1 : node.capacity, n);
  }
  //@unit hashNodeFind requires: equalPair
  function hashNodeFind(node, shift, keyHash, key) {
    for (; node; shift += 5) {
      if (node.kind === "collision") return node.entries.find(entry => equalPair(key, entry.key)) ?? null;
      const slot = node.slots[(keyHash >>> shift) & 31];
      if (!slot?.kind) return slot && equalPair(key, slot.key) ? slot : null;
      node = slot;
    }
    return null;
  }
  //@unit hashNodeRemove requires: bitmapNode
  function hashNodeRemove(node, shift, keyHash, removed, edit) {
    if (!node) return null;
    if (node.kind === "collision") {
      const entries = node.entries.slice(), index = entries.indexOf(removed);
      if (index < 0) return node;
      if (edit) entries[index] = entries[entries.length - 1];
      entries.splice(edit ? entries.length - 1 : index, 1);
      return entries.length ? {...node, entries} : null;
    }
    const index = (keyHash >>> shift) & 31, slot = node.slots[index];
    if (!slot) return node;
    // A child edited in place comes back as itself, under a parent the same
    // edit owns (see `hashNodeAssoc`).
    let child;
    if (slot.kind) {
      child = hashNodeRemove(slot, shift + 5, keyHash, removed, edit);
      if (child === slot) return node;
    } else if (slot !== removed) return node;
    const owned = edit && node.edit === edit, slots = owned ? node.slots : node.slots.slice();
    const n = node.count, count = child ? n : n - 1;
    if (!count) return null;
    slots[index] = child ?? undefined;
    // Capacities as upstream's arrays: a packed array node and a persistent
    // removal hold one pair fewer (`pack-array-node`, `remove-pair`), an edit's
    // copy one more than the node held (`ensure-editable`).
    if (node.kind === "array" && n <= 8 && !child) return bitmapNode(slots, edit, count, count);
    if (owned) { node.count = count; return node; }
    return node.kind === "array" ? {...node, slots, edit, count} : bitmapNode(slots, edit, edit ? n + 1 : node.capacity - !child, count);
  }
  //@unit ensureHashRoot requires: hashNodeAssoc, hashRoots
  function ensureHashRoot(entries) {
    if (!hashRoots.has(entries)) {
      let root = null;
      for (const entry of entries) if (entry.key != null) root = hashNodeAssoc(root, 0, entry.hash, entry, null, {}, false);
      hashRoots.set(entries, root);
    }
    return hashRoots.get(entries);
  }
  //@unit mapTraversal requires: arrayMapEntries, collectionTraversal, kindHooks, mapKinds, maps
  // A map's entries in its order, for the readers that take every map's
  // entries: an array map's pairs as fresh entries (`arrayMapEntries`), a
  // hash map's leaves in its trie's order, kept per map
  // (`hashMapTraversal`), else its entries (`maps`), a record's included.
  // The readers of every pair read `mapPairs`.
  function mapTraversal(value) {
    const kind = value?.cljs$lang$kind$$LKEY;
    if (kind === 28 && value.arr$LKEY !== undefined) return arrayMapEntries(value.arr$LKEY);
    if (kind === 29 && (value.cnt$LKEY !== undefined || "cnt$LKEY" in value)) return kindHooks.hashMapTraversal(value);
    if (kind === 30 && (value.cnt$LKEY !== undefined || "cnt$LKEY" in value)) return kindHooks.treeMapEntries(value);
    return collectionTraversal(maps.get(value), mapKinds.get(value));
  }
  //@unit traversalPairs
  // The flat keys and values of a hash map's traversal (`mapPairs`).
  const traversalPairs = new WeakMap();
  //@unit mapPairs requires: flatPairs, isArrayMap, isTreeMap, kindHooks, mapTraversal,
  //@ traversalPairs
  // A map's keys and values, flat (`[k0, v0, k1, v1, ...]`), in the order
  // `mapTraversal` gives its entries, which its readers walk two by two:
  // an array map's own `arr`, which no reader writes, else its entries'
  // keys and values (a hash map's kept with its traversal); undefined for a
  // value that is no map.
  function mapPairs(value) {
    if (isArrayMap(value)) return value.arr$LKEY;
    if (isTreeMap(value)) return kindHooks.treeMapPairs(value);
    const entries = mapTraversal(value);
    if (entries === undefined) return undefined;
    let pairs = traversalPairs.get(entries);
    if (pairs === undefined) {
      pairs = flatPairs(entries);
      if (value.cljs$lang$kind$$LKEY === 29) traversalPairs.set(entries, pairs);
    }
    return pairs;
  }
  //@unit setTraversal requires: collectionTraversal, setKinds, sets
  function setTraversal(value) {
    return collectionTraversal(sets.get(value), setKinds.get(value));
  }
  //@unit collectionTraversal requires: kindHooks
  function collectionTraversal(entries, kind) {
    // Array maps, positional constants, and sorted collections retain their
    // storage order. Hash-map promotion stays observable after removals.
    return kind !== "hash-map" ? entries : kindHooks.hashTraversal(entries);
  }
  //@unit PersistentHashMapValue.traversal effect: PersistentHashMapValue, publicHashSet, putMember
  //@ when: collectionTraversal, mapTraversal requires: ensureHashRoot, hashTraversals, kindHooks
  // Only hash maps and hash sets have the hash-map kind: sets that promotion
  // or the public constructor built, and hash maps, whose `entries` are a
  // trie (`hashTrie`).
  kindHooks.hashTraversal = entries => {
    if (!hashTraversals.has(entries)) {
      const trie = !Array.isArray(entries), ordered = trie ? entries.nil ? [entries.nil] : [] : entries.filter(entry => entry.key == null);
      const visit = node => {
        if (!node) return;
        if (node.kind === "collision") { ordered.push(...node.entries); return; }
        for (const slot of node.slots) {
          if (slot?.kind) visit(slot);
          else if (slot) ordered.push(slot);
        }
      };
      // Walk retained routing nodes without hashing or comparing keys again.
      visit(trie ? entries.root : ensureHashRoot(entries));
      hashTraversals.set(entries, Object.freeze(ordered));
    }
    return hashTraversals.get(entries);
  };
  // A hash map's, of its fields (`HashMapCell`): its nil entry, then its
  // trie's leaves, kept per map.
  kindHooks.hashMapTraversal = map => {
    let ordered = hashTraversals.get(map);
    if (ordered === undefined) {
      const nil = map.nil$LKEY;
      ordered = nil ? [nil] : [];
      const visit = node => {
        if (node.kind === "collision") { ordered.push(...node.entries); return; }
        for (const slot of node.slots) {
          if (slot?.kind) visit(slot);
          else if (slot) ordered.push(slot);
        }
      };
      if (map.root$LKEY) visit(map.root$LKEY);
      hashTraversals.set(map, Object.freeze(ordered));
    }
    return ordered;
  };
  //@unit PersistentHashMapValue.seqType effect: PersistentHashMapValue, publicHashSet, putMember
  //@ when: first, rest, seq requires: ArrayNodeSeqType, kindHooks, maps, NodeSeqType
  // As upstream's: an ArrayNodeSeq where the trie's root is an array node (17+
  // slots). Positional entries (an engine map's) are not hashed again: NodeSeq.
  kindHooks.hashMapSeqType = value => {
    const root = value.cljs$lang$kind$$LKEY === 29 ? value.root$LKEY : maps.get(value).root;
    return root?.kind === "array" ? ArrayNodeSeqType : NodeSeqType;
  };
  //@unit hashEntryIndex requires: ensureHashRoot, hash, hashNodeFind
  function hashEntryIndex(entries, key, keyHash) {
    if (key == null) return entries.findIndex(entry => entry.key == null);
    const root = ensureHashRoot(entries);
    if (!root) return -1;
    return entries.indexOf(hashNodeFind(root, 0, keyHash ?? hash(key), key));
  }
  //@unit removeHashEntry requires: ensureHashRoot, hashNodeRemove, hashRoots
  function removeHashEntry(entries, index, edit) {
    const entry = entries[index];
    if (entry.key != null) hashRoots.set(entries, hashNodeRemove(ensureHashRoot(entries), 0, entry.hash, entry, edit));
    entries.splice(index, 1);
  }
  //@unit promoteEntries requires: putEntry
  function promoteEntries(entries, edit) {
    const promoted = [];
    for (const entry of entries) putEntry(promoted, entry.key, entry.value, "hash-map", edit);
    return promoted;
  }
  //@unit hashTrie requires: ensureHashRoot
  // A hash map's payload (light-cljs-235): the trie of its non-nil entries,
  // its nil entry or null, and its count as `length`, so that `assoc` and
  // `dissoc` copy only a path. Built from a builder's `entries`, it also keeps
  // them, as `lit`, where advanced constant lookups read an entry by its
  // index (light-cljs-221, `mapValueAt`).
  function hashTrie(entries) {
    return !Array.isArray(entries) ? entries : {root: ensureHashRoot(entries), nil: entries.find(entry => entry.key == null) ?? null, length: entries.length, lit: entries};
  }
  //@unit trieEntry requires: hash, hashNodeFind
  // The entry of `key` in a hash map's trie, or null. As in `hashEntryIndex`,
  // a nil key or an empty trie hashes no query.
  function trieEntry(trie, key) {
    return key == null ? trie.nil : trie.root ? hashNodeFind(trie.root, 0, hash(key), key) : null;
  }
  //@unit trieDissoc requires: hashNodeRemove, trieEntry
  // A transient (`edit`) changes its own payload in place, as `putHashEntry`.
  function trieDissoc(trie, key, edit = null) {
    const entry = trieEntry(trie, key);
    if (!entry) return trie;
    const root = key == null ? trie.root : hashNodeRemove(trie.root, 0, entry.hash, entry, edit);
    if (!edit) return {root, nil: key == null ? null : trie.nil, length: trie.length - 1};
    trie.root = root; if (key == null) trie.nil = null; trie.length--;
    return trie;
  }
  //@unit PersistentHashMapValue.find effect: PersistentHashMapValue, assocTransient, publicHashSet,
  //@ putMember when: find, transientLookup requires: kindHooks, trieEntry
  // Only hash maps have a trie (see `mapTraversal`), transient maps, which
  // `assoc!` can promote to one without a persistent hash map, and transient
  // hash sets (light-cljs-236).
  kindHooks.trieEntry = trieEntry;
  //@unit PersistentHashMapValue.dissoc effect: PersistentHashMapValue when: dissocEntries requires: kindHooks,
  //@ trieDissoc, updatedMap
  kindHooks.dissocTrie = (value, trie, keys) => {
    for (const key of keys) trie = trieDissoc(trie, key);
    return updatedMap(value, trie);
  };
  //@unit putMember.disj effect: publicHashSet, putMember when: disj requires: kindHooks, trieDissoc
  // The members a hash set keeps without `keys`, or the same trie where it
  // has none of them.
  kindHooks.disjTrie = (members, keys) => {
    for (const key of keys) members = trieDissoc(members, key);
    return members;
  };

  //@unit makeMap requires: flatPairs, kindHooks
  // A map of the type the caller builds (so a caller keeps only the map types
  // it can build), an instance of the class its type's unit gives it
  // (`viewType`), which the type decides as `kind` names it: its entries,
  // unfrozen, and a sorted map's comparator. `mapTypes` gives the type of an
  // existing map's kind. An array map holds its entries' keys and values,
  // flat (`flatPairs`), a hash map the fields of its trie (`hashTrie`), a
  // sorted map the tree of its entries, in their comparator's order
  // (`treeOfEntries`).
  function makeMap(Type, entries, _kind, comparator) {
    const proto = Type.prototype, kind = proto.cljs$lang$kind$$LKEY;
    return kind === 28 ? new proto.cljs$lang$cell$$LKEY(entries.length, flatPairs(entries))
      : kind === 29 ? new proto.cljs$lang$cell$$LKEY(entries.length, entries.root, entries.nil, entries.lit ?? null)
      : new proto.cljs$lang$cell$$LKEY(comparator, kindHooks.treeOfEntries(entries), entries.length);
  }

  //@unit arrayIndex requires: equalPair, keywordOf, methodKeys, protocolHooks
  // The index of `key` in an array map's keys and values (`arr`), even, or
  // -1: upstream's `array-index-of`, as `entryIndex` searches entries; from
  // the key at `from` on.
  function arrayIndex(arr, key, from = 0) {
    const n = arr.length;
    if (key == null) {
      for (let i = from; i < n; i += 2) if (arr[i] == null) return i;
      return -1;
    }
    if (typeof key === "string" || typeof key === "number") {
      for (let i = from; i < n; i += 2) if (arr[i] === key) return i;
      return -1;
    }
    // A keyword as in `entryIndex`: by identity, then by kind and `fqn`,
    // reading its own `IEquiv` slot once.
    const keyword = keywordOf(key);
    if (keyword !== undefined) {
      const fqn = keyword.fqn$LKEY;
      let read = false;
      for (let i = from; i < n; i += 2) {
        const other = arr[i];
        if (other === key) return i;
        if (!read) {
          read = true;
          const builtin = protocolHooks.builtin, own = protocolHooks.keyMethod, k = methodKeys[18];
          const equiv = builtin && (own ? own(key, k, 2) ?? protocolHooks.externalMethod?.(key, k, 2) : builtin(key, k, 2));
          if (equiv) {
            if (equiv.call(key, key, other)) return i;
            for (i += 2; i < n; i += 2) if (equalPair(key, arr[i])) return i;
            return -1;
          }
        }
        // Its `fqn` first: the keys and values of `arr` tell V8 nothing of a
        // key's class, so the kind test runs only where the spelling matches
        // (upstream's `array-index-of-keyword?`); a value without the field
        // (another kind, or the Pants profile's engine keyword) is read
        // through `keywordOf`.
        if (other != null) {
          const spelling = other.fqn$LKEY;
          if (spelling === fqn ? keywordOf(other) !== undefined : spelling === undefined && keywordOf(other)?.fqn$LKEY === fqn) return i;
        }
      }
      return -1;
    }
    for (let i = from; i < n; i += 2) if (equalPair(key, arr[i])) return i;
    return -1;
  }

  //@unit entryIndex requires: equalPair, keywordOf, methodKeys, protocolHooks
  // The index of `key` in the entries of an array map, or -1.
  function entryIndex(entries, key) {
    // Canonical array maps compare keys without a hash guard. Keyword equality uses
    // fqn, while hashing uses namespace/name; unusual equal keywords can have
    // different hashes and therefore remain distinct in hash maps.
    // The entries there are at the start, as `findIndex` (light-cljs-989).
    const n = entries.length;
    if (key == null) {
      for (let i = 0; i < n; i++) if (entries[i].key == null) return i;
      return -1;
    }
    if (typeof key === "string" || typeof key === "number") {
      for (let i = 0; i < n; i++) if (entries[i].key === key) return i;
      return -1;
    }
    // A keyword compares with each key by identity, then by kind and `fqn`
    // (upstream's `array-index-of-keyword?`), with no read of a store or of
    // a slot per entry. Its own `IEquiv` slot is read once, at the first key
    // that is not it: where a program gave it one (`extend-type`,
    // `specify!`), that key and the rest compare as `equalPair` compares
    // them, calling it, as before light-cljs-1011.
    const keyword = keywordOf(key);
    if (keyword !== undefined) {
      const fqn = keyword.fqn$LKEY;
      let read = false;
      for (let i = 0; i < n; i++) {
        const other = entries[i].key;
        if (other === key) return i;
        if (!read) {
          read = true;
          const builtin = protocolHooks.builtin, own = protocolHooks.keyMethod, k = methodKeys[18];
          const equiv = builtin && (own ? own(key, k, 2) ?? protocolHooks.externalMethod?.(key, k, 2) : builtin(key, k, 2));
          if (equiv) {
            if (equiv.call(key, key, other)) return i;
            for (i++; i < n; i++) if (equalPair(key, entries[i].key)) return i;
            return -1;
          }
        }
        if (keywordOf(other)?.fqn$LKEY === fqn) return i;
      }
      return -1;
    }
    for (let i = 0; i < n; i++) if (equalPair(key, entries[i].key)) return i;
    return -1;
  }

  //@unit ArrayMapEntry requires: entryHashing
  // An array map's entry: its key's hash `h`, a shape's or else taken on the
  // first read of `hash`. Unlike other entries it is not frozen, so as to
  // keep it; it lives in a map's private payload (light-cljs-989).
  const ArrayMapEntry = class {
    constructor(key, value, h) { this.key = key; this.value = value; this.h = h; }
    get hash() { return this.h === undefined ? this.h = entryHashing.hash(this.key) : this.h; }
  };
  //@unit putEntry requires: ArrayMapEntry, entryIndex, kindHooks
  // Builders mutate only fresh unpublished arrays. Entry cells and retained
  // equivalent keys are shared by subsequent immutable versions.
  function putEntry(entries, key, value, kind, edit = null) {
    if (kind === "array-map") {
      const index = entryIndex(entries, key), entry = new ArrayMapEntry(index < 0 ? key : entries[index].key, value);
      if (index < 0) entries.push(entry); else entries[index] = entry;
      return;
    }
    kindHooks.putHashEntry(entries, key, value, edit);
  }
  //@unit PersistentHashMapValue.put effect: PersistentHashMapValue, makeTransient, publicHashSet,
  //@ putMember when: putEntry requires: ensureHashRoot, hash, hashNodeAssoc, hashRoots, kindHooks
  // Only hash maps, hash sets and transients have hash-map entries.
  kindHooks.putHashEntry = (entries, key, value, edit) => {
    // Its entries are not frozen (light-cljs-1034): no reader writes them.
    const keyHash = key == null ? 0 : hash(key), entry = {key, value, hash: keyHash};
    if (!Array.isArray(entries)) {
      // A hash map's trie, of which an update owns this copy (`copyEntries.trie`).
      const result = {}, nil = entries.nil;
      if (key == null) entries.nil = nil ? {key: nil.key, value, hash: keyHash} : entry;
      else entries.root = hashNodeAssoc(entries.root, 0, keyHash, entry, edit, result);
      entries.length += key == null ? !nil : !result.old;
      return;
    }
    if (key == null) {
      const index = entries.findIndex(old => old.key == null);
      if (index < 0) entries.push(entry); else entries[index] = {key: entries[index].key, value, hash: keyHash};
      return;
    }
    const result = {}, root = hashNodeAssoc(ensureHashRoot(entries), 0, keyHash, entry, edit, result);
    if (result.old) entries[entries.indexOf(result.old)] = result.entry;
    else entries.push(entry);
    hashRoots.set(entries, root);
  };

  //@unit mapFromArray requires: arrayMapFromArray, displayStr, hashTrie, makeMap,
  //@ PersistentArrayMapValue, PersistentHashMapValue, putEntry
  function mapFromArray(pairs, kind = pairs.length <= 16 ? "array-map" : "hash-map") {
    if (pairs.length % 2) throw new Error("No value supplied for key: " + displayStr(pairs[pairs.length - 1]));
    if (kind === "array-map") return arrayMapFromArray(pairs);
    const entries = [];
    const edit = kind === "hash-map" ? {} : null;
    for (let i = 0; i < pairs.length; i += 2) putEntry(entries, pairs[i], pairs[i + 1], kind, edit);
    return kind === "hash-map" ? makeMap(PersistentHashMapValue, hashTrie(entries), kind) : makeMap(PersistentArrayMapValue, entries, kind);
  }
  //@unit arrayMapFromArray requires: arrayIndex, ArrayMapCell
  // Advanced map literals of at most 8 entries keep no hash map. A key it
  // has keeps its first spelling and takes the last value, as `putEntry`.
  function arrayMapFromArray(pairs) {
    const arr = [];
    for (let i = 0; i < pairs.length; i += 2) {
      const key = pairs[i], index = arrayIndex(arr, key);
      if (index < 0) arr.push(key, pairs[i + 1]); else arr[index + 1] = pairs[i + 1];
    }
    return new ArrayMapCell(arr.length >> 1, arr);
  }

  //@unit mapValueAt
  // The value of entry `i` of a map built from a literal whose keys are
  // distinct constants (`mapFromShape`, `hashMapFromShape`), which advanced
  // output reads for a lookup of a constant key (light-cljs-221): an array
  // map's pair `i`, else the entry its trie keeps at its index (`hashTrie`).
  function mapValueAt(map, i) {
    const arr = map.arr$LKEY;
    return arr !== undefined ? arr[2 * i + 1] : map.lit$LKEY[i].value;
  }
  //@unit hashMap requires: mapFromArray
  function hashMap(...pairs) { return mapFromArray(pairs, "hash-map"); }
  //@unit arrayMap requires: arrayMapOfPairs
  function arrayMap(...pairs) { return arrayMapOfPairs(pairs); }
  //@unit arrayMapOfPairs requires: arrayMapFromArray, displayStr
  // An array map of any size, as `mapFromArray` builds one, without keeping
  // the hash map type. `apply` hands its pairs on as one array.
  function arrayMapOfPairs(pairs) {
    if (pairs.length % 2) throw new Error("No value supplied for key: " + displayStr(pairs[pairs.length - 1]));
    return arrayMapFromArray(pairs);
  }

  //@unit mapFromArrays requires: mapFromArray
  // Source hash-map literals evaluate all reader-ordered keys before values.
  // This emitter helper preserves the ordinary runtime constructor semantics.
  function mapFromArrays(keys, values) {
    const pairs = [];
    for (let i = 0; i < keys.length; i++) pairs.push(keys[i], values[i]);
    return mapFromArray(pairs, "hash-map");
  }

  //@unit mapShape
  // Map literal key shapes (light-cljs-220): distinct constant keys, hashed.
  function mapShape(keys, hashes) { return Object.freeze({keys, hashes}); }
  //@unit shapeEntries
  // The entries mapFromArray makes of a shape's keys and `values` for a hash
  // map, without key lookups.
  function shapeEntries({keys, hashes}, values) {
    const entries = [];
    for (let i = 0; i < keys.length; i++) entries.push({key: keys[i], value: values[i], hash: hashes[i]});
    return entries;
  }
  //@unit mapFromShape requires: ArrayMapCell
  // As `shapeEntries`, an array map of a shape's keys and `values`.
  function mapFromShape({keys}, values) {
    const n = keys.length, arr = [];
    for (let i = 0; i < n; i++) arr.push(keys[i], values[i]);
    return new ArrayMapCell(n, arr);
  }
  //@unit hashMapShape requires: hashNodeAssoc
  // Past 8 keys, also the trie mapFromArray builds, entry indexes as leaves.
  // Distinct keys are never equal, so `effects` changes nothing.
  function hashMapShape(keys, hashes) {
    let layout, edit = {};
    keys.forEach((key, index) => {
      if (key != null) layout = hashNodeAssoc(layout, 0, hashes[index], {key, hash: hashes[index], index}, edit, {}, false);
    });
    return Object.freeze({keys, hashes, layout});
  }
  //@unit shapeNode
  function shapeNode(node, entries) {
    return node.kind === "collision" ? {...node, entries: node.entries.map(entry => entries[entry.index])}
      : {...node, slots: node.slots.map(slot => slot.kind ? shapeNode(slot, entries) : entries[slot.index])};
  }
  //@unit hashMapFromShape requires: hashRoots, hashTrie, makeMap, PersistentHashMapValue,
  //@ shapeEntries, shapeNode
  function hashMapFromShape(shape, values) {
    const entries = shapeEntries(shape, values);
    hashRoots.set(entries, shapeNode(shape.layout, entries));
    return makeMap(PersistentHashMapValue, hashTrie(entries), "hash-map");
  }

  //@unit constantMapFromArray requires: publicArrayMap
  // The analyzer has checked distinct source constants. Character/string and
  // numeric reader categories can lower to equal JS values, so literal storage
  // retains positional entries and the array map's first-match lookup.
  function constantMapFromArray(pairs) {
    return publicArrayMap(null, pairs.length / 2, pairs, null);
  }

  //@unit constantSetFromArray requires: constantMapFromArray, publicHashSet
  function constantSetFromArray(items) {
    const pairs = [];
    for (const item of items) pairs.push(item, null);
    return publicHashSet(null, constantMapFromArray(pairs), null);
  }

  //@unit dissoc requires: dissocEntries, isArrayMap, isHashMap, isTreeMap, kindHooks, maps,
  //@ methodKeys, protocolHooks
  function dissoc(value, ...keys) {
    if (value != null && protocolHooks.builtin?.(value, methodKeys[31], 2)) return keys.reduce((result, key) => {
      const method = result == null ? null : protocolHooks.builtin?.(result, methodKeys[31], 2);
      return method ? method.call(result, result, key) : dissoc(result, key);
    }, value);
    if (arguments.length === 0) return null;
    if (keys.length === 0 || value == null) return value;
    // An array map's, a hash map's and a sorted map's own paths are their
    // slots' functions, which read their fields (light-cljs-1036, 1035), a
    // key at a time, as upstream's `dissoc`.
    const own = isArrayMap(value) ? kindHooks.dissocArrayMap : isHashMap(value) ? kindHooks.dissocHashMap : isTreeMap(value) ? kindHooks.dissocTreeMap : undefined;
    if (own !== undefined) {
      let result = value;
      for (const key of keys) result = own(result, key);
      return result;
    }
    const original = maps.get(value);
    if (!original) throw new Error("dissoc not supported on this type");
    return dissocEntries(value, original, keys);
  }
  //@unit dissocEntries requires: collectionEntryIndex, copyEntries, equalPair, hashRoots, keyword,
  //@ kindHooks, mapKinds, recordTypes, removeHashEntry, updatedMap
  // `dissoc` of `keys` from a map past its protocol lookup, of its entries
  // (`original`): also the map slots' (`arrayMapDissoc`) for a map a public
  // constructor built.
  function dissocEntries(value, original, keys) {
    if (!Array.isArray(original)) return kindHooks.dissocTrie(value, original, keys);
    const entries = copyEntries(original);
    for (const key of keys) {
      const index = collectionEntryIndex(value, entries, key);
      if (index >= 0) {
        if (mapKinds.get(value) === "array-map" && entries.length > 1) {
          // Canonical removal searches first, then copies with query-side equality.
          const retained = entries.filter(entry => !equalPair(key, entry.key));
          // The positional constructor can contain equal lowered keys. CLJS
          // allocates room for one removed pair even when the copy skips more;
          // preserve its trailing undefined pair(s) and decremented count.
          while (retained.length < entries.length - 1) retained.push(Object.freeze({key: undefined, value: undefined, hash: 0}));
          entries.splice(0, entries.length, ...retained);
        } else if (hashRoots.has(entries)) removeHashEntry(entries, index);
        else entries.splice(index, 1);
      }
    }
    const Type = recordTypes.get(value);
    const removeRecord = Type && keys.some(key => Type.fieldNames.some(name => equalPair(key, keyword(null, name))));
    return updatedMap(value, entries, removeRecord);
  }

  //@unit mapConj requires: assoc, first, holdsVector, methodKeys, next, nth, protocolHooks, seq
  function mapConj(value, member) {
    if (member == null) return value;
    // A vector's `-nth` is `nth`'s own (see `protocolHooks`).
    const vectorLike = item => protocolHooks.satisfies?.(methodKeys[61], item) ?? holdsVector(item), nthOf = protocolHooks.nthProtocol ?? nth;
    if (vectorLike(member)) return assoc(value, nthOf(member, 0), nthOf(member, 1));
    let result = value;
    for (let current = seq(member); current !== null; current = next(current)) {
      const pair = first(current);
      if (!vectorLike(pair)) throw new Error("Map conj requires vector entries");
      result = assoc(result, nthOf(pair, 0), nthOf(pair, 1));
    }
    return result;
  }

  //@unit ownSet requires: protocolHooks, sets
  // Whether a set iterates and searches its own members (see `hostHooks`).
  function ownSet(value) { return !protocolHooks.extend && sets.has(value); }
  //@unit setIterator requires: itemIterator, setTraversal
  // Of a set's seq (`makeSet.seq`).
  function setIterator(value, item) {
    const entries = setTraversal(value);
    return itemIterator(entries.length ? entries : null, item);
  }
  //@unit setPrototype requires: hostHooks, invalidArity, lookup, missing, objectEquiv, ownSet,
  //@ printHooks, setIterator, sets, unkeyedJSON
  const setPrototype = Object.freeze({
    equiv(other) { return objectEquiv(this, other); },
    // JSON of a set of a class (`HashSetCell`): its own properties but its
    // keyed fields, as before it had them (docs/value-layout.md, question 3).
    toJSON() { return unkeyedJSON(this); },
    has(key) { return ownSet(this) ? lookup(this, key, missing) !== missing : hostHooks.has(this, key); },
    keys() { return ownSet(this) ? setIterator(this, entry => entry.key) : hostHooks.iterator(this); },
    values() { return ownSet(this) ? setIterator(this, entry => entry.key) : hostHooks.iterator(this); },
    entries() { return ownSet(this) ? setIterator(this, entry => [entry.key, entry.key]) : hostHooks.entries(this, true); },
    [Symbol.iterator]() { return ownSet(this) ? setIterator(this, entry => entry.key) : hostHooks.iterator(this); },
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    toString() { return printHooks.collection(this); },
    call(_self, key, notFound) {
      if (!sets.has(this)) throw new Error("Expected set");
      switch (arguments.length) {
        case 2: return lookup(this, key, null);
        case 3: return lookup(this, key, notFound);
        default: return invalidArity(arguments.length - 1);
      }
    }
  });

  //@unit PersistentHashSetValue.type effect: PersistentHashSetValue requires: builtinType,
  //@ HashSetCell, kindOps, PersistentHashSetValue, setPrototype, viewType
  // A hash or array set is a class of its type's prototype (`HashSetCell`,
  // light-cljs-1013); the kind table number stays until light-cljs-1015.
  viewType(builtinType(PersistentHashSetValue, setPrototype), HashSetCell, 33).prototype[kindOps] = 6;
  //@unit HashSetCell.type effect: HashSetCell requires: HashSetCell, PersistentHashSetValue
  // As `ArrayMapCell.type`: a set built directly keeps its type.
  HashSetCell.prototype = PersistentHashSetValue.prototype;
  //@unit treeSetConstructor.type effect: treeSetConstructor requires: builtinType, kindOps,
  //@ setPrototype, treeSetConstructor
  builtinType(treeSetConstructor, setPrototype).prototype[kindOps] = 6;
  //@unit treeSetConstructor.kind effect: treeSetConstructor requires: setTypes, treeSetConstructor
  setTypes["sorted-set"] = treeSetConstructor;
  //@unit setType requires: PersistentHashSetValue, setTypes
  // The type of an existing set's kind.
  function setType(kind) { return kind === "sorted-set" ? setTypes[kind] : PersistentHashSetValue; }
  //@unit makeSet requires: setComparators, setKinds, sets
  // A set of the type the caller builds; `setType` gives an existing set's. A
  // hash or array set is an instance of the class its type's unit gives it
  // (`HashSetCell`, light-cljs-1013) with its members unfrozen; a sorted set
  // keeps the kinded store (`sets.store`) and its comparator
  // (`setComparators`).
  function makeSet(Type, entries, kind, comparator) {
    const Cell = Type.prototype.cljs$lang$cell$$LKEY;
    if (Cell !== undefined && kind !== "sorted-set") return new Cell(entries, kind);
    const value = Object.create(Type.prototype);
    sets.set(value, Object.freeze(entries));
    setKinds.set(value, kind);
    if (kind === "sorted-set") setComparators.set(value, comparator);
    return value;
  }

  //@unit putMember requires: entryIndex, hashRoots, putEntry
  // Adds `key` to the members of a set that a builder, `conj` or a transient
  // owns, and returns them. At most 8 are an entry array; the ninth distinct
  // member promotes them to a hash set's payload (light-cljs-236), which, as a
  // hash map's (`hashTrie`), is the trie of its non-nil members, its nil
  // member or null, and its count as `length`, so that an update copies or
  // edits one path. Promotion adds the members under one edit token, as
  // upstream's transient array map does, so that traversal order and the
  // calls that hash keys are as before. An array to which another kind's
  // lookup gave a root (see `collectionEntryIndex`) adds to that root, and
  // past 8 members becomes the trie of it.
  function putMember(members, key, edit = null) {
    if (Array.isArray(members) && !hashRoots.has(members)) {
      if (members.length < 8 || entryIndex(members, key) >= 0) {
        putEntry(members, key, null, "array-map"); return members;
      }
      const promoted = {root: null, nil: null, length: 0}, token = edit ?? {};
      for (const entry of members) putEntry(promoted, entry.key, null, "hash-map", token);
      members = promoted;
    }
    putEntry(members, key, null, "hash-map", edit);
    return !Array.isArray(members) || members.length <= 8 ? members
      : {root: hashRoots.get(members), nil: members.find(entry => entry.key == null) ?? null, length: members.length};
  }

  //@unit setFromArray requires: makeSet, PersistentHashSetValue, putMember
  function setFromArray(items) {
    let members = [];
    const edit = {};
    for (const item of items) members = putMember(members, item, edit);
    return makeSet(PersistentHashSetValue, members, Array.isArray(members) ? "array-map" : "hash-map");
  }
  //@unit arraySetFromArray requires: makeSet, PersistentHashSetValue, putEntry
  // Advanced set literals of at most 8 constants, which `setFromArray` never
  // promotes, keep no hash set.
  function arraySetFromArray(items) {
    const entries = [];
    for (const item of items) putEntry(entries, item, null, "array-map", null);
    return makeSet(PersistentHashSetValue, entries, "array-map");
  }

  //@unit literalSetFromArray requires: displayStr, makeSet, PersistentHashSetValue, putMember
  function literalSetFromArray(items, members = [], edit = {}) {
    for (const item of items) {
      const before = members.length;
      members = putMember(members, item, edit);
      if (members.length === before) throw new Error("Duplicate key: " + displayStr(item));
    }
    return makeSet(PersistentHashSetValue, members, Array.isArray(members) ? "array-map" : "hash-map");
  }
  //@unit setShape requires: putMember
  // Set literal shapes (light-cljs-220): distinct constant first members,
  // added as literalSetFromArray adds them, and its edit token.
  function setShape(members) {
    let entries = [];
    const edit = {};
    for (const member of members) entries = putMember(entries, member, edit);
    return Object.freeze({entries: Object.freeze(entries), edit});
  }
  //@unit setFromShape requires: literalSetFromArray
  // The shape's token gives the other members the capacities a literal built
  // at once gives them. An edit changes the nodes of its token in place
  // (`hashNodeAssoc`), so each set edits its own copy of the shape's trie
  // (collision nodes are always copied).
  function setFromShape({entries, edit}, items) {
    const own = node => node.kind === "collision" ? node : {...node, slots: node.slots.map(slot => slot?.kind ? own(slot) : slot)};
    return literalSetFromArray(items, Array.isArray(entries) ? entries.slice()
      : {root: entries.root && own(entries.root), nil: entries.nil, length: entries.length}, edit);
  }

  //@unit hashSet requires: setFromArray
  function hashSet(...items) { return setFromArray(items); }

  //@unit disj requires: collectionEntryIndex, copyEntries, dissoc, hashRoots, kindHooks, makeSet,
  //@ meta, methodKeys, PersistentHashSetValue, preserveMeta, protocolHooks, publicCollectionSlots,
  //@ removeHashEntry, setComparators, setKinds, sets, setType, treeSetConstructor
  function disj(value, ...keys) {
    if (value != null && protocolHooks.builtin?.(value, methodKeys[54], 2)) return keys.reduce((result, key) => protocolHooks.builtin?.(result, methodKeys[54], 2).call(result, result, key), value);
    if (arguments.length === 0) return null;
    if (keys.length === 0) return value;
    if (value == null) return null;
    const original = sets.get(value);
    if (!original) throw new Error("disj not supported on this type");
    const treeMap = publicCollectionSlots.get(value)?.["tree-map"];
    if (treeMap) return treeSetConstructor(meta(value), dissoc(treeMap, ...keys), null);
    const hashMap = publicCollectionSlots.get(value)?.["hash-map"];
    if (hashMap) return PersistentHashSetValue(meta(value), dissoc(hashMap, ...keys), null);
    // A hash set removes each member from its trie in O(log32 n).
    if (!Array.isArray(original)) {
      const members = kindHooks.disjTrie(original, keys);
      return members === original ? value : preserveMeta(value, makeSet(PersistentHashSetValue, members, "hash-map"));
    }
    const entries = copyEntries(original);
    for (const key of keys) {
      const index = collectionEntryIndex(value, entries, key);
      if (index >= 0) {
        if (hashRoots.has(entries)) removeHashEntry(entries, index); else entries.splice(index, 1);
      }
    }
    if (entries.length === original.length) return value;
    return preserveMeta(value, makeSet(setType(setKinds.get(value)), entries, setKinds.get(value), setComparators.get(value)));
  }

  //@unit transientPrototype requires: invalidArity, printHooks, transientLookup, unkeyedJSON
  // Transient payloads own a private mutable copy of canonical collection
  // storage. Persistence freezes that storage; no persistent alias can mutate it.
  const transientPrototype = Object.freeze({
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    // JSON of a class value: its own properties but its keyed fields, as
    // before it had them (docs/value-layout.md, question 3).
    toJSON() { return unkeyedJSON(this); },
    toString() { return printHooks.collection(this); },
    call(_self, key, notFound) {
      if (arguments.length !== 2 && arguments.length !== 3) return invalidArity(arguments.length - 1);
      return transientLookup(this, key, arguments.length === 2 ? null : notFound);
    }
  });
  //@unit TransientVectorType effect requires: builtinType, transientPrototype
  const TransientVectorType = builtinType(function () {}, transientPrototype);
  //@unit TransientVectorType.kind effect: TransientVectorType requires: kinds, TransientVectorType
  // Its protocols (see `PersistentVectorValue.type`): partition 0 IFn
  // ICounted IIndexed ILookup, partition 1 ITransientCollection
  // ITransientAssociative ITransientVector.
  TransientVectorType.prototype.cljs$lang$kind$$LKEY = kinds.transientVector;
  TransientVectorType.prototype.cljs$lang$protocol_mask$partition0$$LKEY = 275;
  TransientVectorType.prototype.cljs$lang$protocol_mask$partition1$$LKEY = 88;
  //@unit TransientVectorCell.type effect: TransientVectorCell requires: TransientVectorCell,
  //@ TransientVectorType
  TransientVectorCell.prototype = TransientVectorType.prototype;
  //@unit TransientArrayMapType effect requires: builtinType, TransientArrayMapCell,
  //@ transientPrototype, viewType
  // The map transients are classes too (`TransientArrayMapCell`).
  const TransientArrayMapType = viewType(builtinType(function () {}, transientPrototype), TransientArrayMapCell, 31);
  //@unit TransientHashMapType effect requires: builtinType, TransientHashMapCell,
  //@ transientPrototype, viewType
  const TransientHashMapType = viewType(builtinType(function () {}, transientPrototype), TransientHashMapCell, 32);
  //@unit TransientArrayMapCell.type effect: TransientArrayMapCell requires: TransientArrayMapCell,
  //@ TransientArrayMapType
  // The map transients' operations build them directly (light-cljs-1036),
  // keeping their types, whose units above give them their prototypes and
  // kinds, as the maps' (`ArrayMapCell.type`).
  TransientArrayMapCell.prototype = TransientArrayMapType.prototype;
  //@unit TransientHashMapCell.type effect: TransientHashMapCell requires: TransientHashMapCell,
  //@ TransientHashMapType
  TransientHashMapCell.prototype = TransientHashMapType.prototype;
  //@unit TransientHashSetType effect requires: builtinType, TransientHashSetCell,
  //@ transientPrototype, viewType
  const TransientHashSetType = viewType(builtinType(function () {}, transientPrototype), TransientHashSetCell, 34);
  //@unit TransientHashSetCell.type effect: TransientHashSetCell requires: TransientHashSetCell,
  //@ TransientHashSetType
  // A set transient's class keeps its type, whose unit above gives it its
  // prototype and kind, as the map transients' (`TransientArrayMapCell.type`).
  TransientHashSetCell.prototype = TransientHashSetType.prototype;
  //@unit makeTransient requires: TransientHashSetCell
  // A transient of `entries`: a transient vector is the `TransientVectorCell`
  // it is given (`makeVector.transient`), its own state, as a map transient
  // is (`makeMap.transient`); a set transient holds its state in its own
  // `state` field (`TransientHashSetCell`).
  function makeTransient(kind, entries, mapKind) {
    if (kind === "vector") return entries;
    return new TransientHashSetCell({kind, entries, mapKind, editable: true, edit: {}});
  }
  //@unit editableTransient requires: transientState
  function editableTransient(value, operation) {
    const state = transientState(value);
    // A transient vector or array map is its own state, with an `editable`
    // field; a transient hash map has its `edit` until it is persisted.
    if (!(state === value ? value.cljs$lang$kind$$LKEY === 32 ? value.edit$LKEY !== null : state.editable$LKEY : state?.editable)) throw new Error(operation === "persistent!" ? "persistent! called twice" : operation + " after persistent!");
    return state;
  }
  //@unit transient requires: asTransientProtocol
  function transient(value) { return asTransientProtocol(value); }
  //@unit asTransient requires: copyEntries, holdsVector, kindHooks, makeTransient, maps, setKinds,
  //@ sets, vectorStore
  function asTransient(value) {
    if (holdsVector(value)) return makeTransient("vector", kindHooks.transientVector(vectorStore(value)));
    if (maps.has(value)) return kindHooks.transientMap(value);
    return makeTransient("set", copyEntries(sets.get(value)), setKinds.get(value));
  }
  //@unit persistentBang requires: persistentBangProtocol
  function persistentBang(value) { return persistentBangProtocol(value); }
  //@unit persistTransient requires: editableTransient, isMapTransient, kindHooks
  function persistTransient(value) {
    const state = editableTransient(value, "persistent!");
    if (isMapTransient(value)) return kindHooks.persistMap(value);
    if (state === value) { state.editable$LKEY = false; return kindHooks.persistVector(state); }
    state.editable = false;
    return kindHooks.persistSet(state.entries, state.mapKind);
  }
  //@unit makeVector.persist effect: makeVector, mapEntry when: persistTransient requires: kindHooks,
  //@ makeVector, VectorCell, vectorTailOffset
  // A transient vector, map or set comes from a persistent one of its kind.
  // The vector's tail is trimmed to its count; the nodes it owned no longer
  // change, as nothing can edit them, and a leaf freezes where a seq first
  // shares it (`vectorChunk`).
  kindHooks.persistVector = store => {
    const cnt = store.cnt$LKEY;
    return makeVector(null, new VectorCell(null, cnt, store.shift$LKEY, store.root$LKEY, store.tail$LKEY.slice(0, cnt - vectorTailOffset(cnt)), null));
  };
  //@unit makeVector.transient effect: makeVector, mapEntry when: asTransient requires: kindHooks,
  //@ TransientVectorCell, viewTrie
  // A transient vector edits a copy of the tail, and copies of the trie's
  // nodes along the paths it changes, which it owns (`vectorEditor`): a weak
  // set, which keeps no node a `pop!` discarded.
  kindHooks.transientVector = store => {
    const trie = viewTrie(store), tail = trie.tail$LKEY.slice();
    return new TransientVectorCell(trie.cnt$LKEY, trie.shift$LKEY, trie.root$LKEY, tail, new WeakSet([tail]));
  };
  //@unit makeMap.persist effect: ArrayMapCell, makeMap, treeMapConstructor, recordMethods when: persistTransient
  //@ requires: ArrayMapCell, HashMapCell, kindHooks
  // `persistent!` of an editable map transient: the map of its fields, as
  // upstream's, which shares them, since nothing edits them once the
  // transient is no longer editable (an array map's `arr`, a hash map's
  // nodes that its `edit` owned).
  kindHooks.persistMap = value => {
    if (value.cljs$lang$kind$$LKEY === 31) {
      value.editable$LKEY = false;
      const arr = value.arr$LKEY;
      return new ArrayMapCell(arr.length >> 1, arr);
    }
    value.edit$LKEY = null;
    return new HashMapCell(value.cnt$LKEY, value.root$LKEY, value.nil$LKEY, null);
  };
  //@unit makeMap.transient effect: ArrayMapCell, makeMap, treeMapConstructor, recordMethods when: asTransient
  //@ requires: flatPairs, hashTrie, kindHooks, mapKinds, maps, TransientArrayMapCell,
  //@ TransientHashMapCell
  // A map transient of a map's fields (`TransientArrayMapCell`,
  // `TransientHashMapCell`): a copy of an array map's keys and values, and a
  // hash map's root, of whose nodes a fresh edit owns none, so that its first
  // change of each copies it once (`hashNodeAssoc`). A map without these
  // fields (the Pants build's engine maps) gives its entries (`maps`), of
  // its kind.
  kindHooks.transientMap = value => {
    const kind = value.cljs$lang$kind$$LKEY;
    if (kind === 28 && value.arr$LKEY !== undefined) {
      // A public constructor's array of odd length ends in a key without a
      // value, which its entries gave undefined (`arrayMapEntries`).
      const arr = value.arr$LKEY.slice();
      if (arr.length & 1) arr.push(undefined);
      return new TransientArrayMapCell(arr);
    }
    if (kind === 29 && (value.cnt$LKEY !== undefined || "cnt$LKEY" in value)) return new TransientHashMapCell({}, value.cnt$LKEY, value.root$LKEY, value.nil$LKEY);
    const entries = maps.get(value);
    if (Array.isArray(entries) && mapKinds.get(value) === "array-map") return new TransientArrayMapCell(flatPairs(entries));
    const trie = hashTrie(entries);
    return new TransientHashMapCell({}, trie.length, trie.root, trie.nil);
  };
  //@unit makeSet.persist effect: makeSet, treeSetConstructor when: persistTransient requires: kindHooks,
  //@ makeSet, PersistentHashSetValue
  kindHooks.persistSet = (entries, kind) => makeSet(PersistentHashSetValue, entries, kind);
  //@unit transientNth requires: editableTransient, missing, transientState, vectorNth
  function transientNth(value, index, fallback) {
    const state = transientState(value), store = state === value ? state : state.entries;
    // Pinned CLJS's three-argument -nth returns an out-of-range fallback
    // even after persistence; in-range access still checks ownership.
    if (fallback !== missing && !(index >= 0 && index < store.cnt$LKEY)) return fallback;
    editableTransient(value, "nth");
    return vectorNth(store, index, fallback);
  }
  //@unit transientLookup requires: editableTransient, isMapTransient, kindHooks, transientNth,
  //@ transientState
  function transientLookup(value, key, notFound) {
    if (isMapTransient(value)) return kindHooks.transientMapLookup(value, key, notFound);
    const state = transientState(value);
    // TransientHashSet retains readable lookup after persistence in both
    // pinned versions, as TransientHashMap does; vectors reject it.
    if (state.mapKind !== "hash-map") editableTransient(value, "lookup");
    if (state === value) return typeof key === "number" ? transientNth(value, key, notFound) : notFound;
    // A transient hash set edits a trie (see `asTransient`).
    if (!Array.isArray(state.entries)) return kindHooks.trieEntry(state.entries, key) ? key : notFound;
    return kindHooks.transientEntryIndex(state, key) < 0 ? notFound : key;
  }
  //@unit makeMap.transientLookup effect: ArrayMapCell, makeMap, treeMapConstructor, recordMethods
  //@ when: transientLookup requires: arrayIndex, editableTransient, hash, hashNodeFind, kindHooks
  // A map transient's lookup, of its fields: TransientHashMap retains
  // readable lookup after persistence in both pinned versions;
  // TransientArrayMap rejects it.
  kindHooks.transientMapLookup = (value, key, notFound) => {
    if (value.cljs$lang$kind$$LKEY === 31) {
      if (!value.editable$LKEY) editableTransient(value, "lookup");
      const arr = value.arr$LKEY, index = arrayIndex(arr, key);
      return index < 0 ? notFound : arr[index + 1];
    }
    let entry;
    if (key == null) entry = value.nil$LKEY;
    else { const root = value.root$LKEY; entry = root ? hashNodeFind(root, 0, hash(key), key) : null; }
    return entry ? entry.value : notFound;
  };
  //@unit makeSet.transientLookup effect: makeSet, treeSetConstructor when: transientLookup
  //@ requires: entryIndex, hashEntryIndex, kindHooks
  kindHooks.transientEntryIndex = (state, key) => state.mapKind === "array-map" ? entryIndex(state.entries, key) : hashEntryIndex(state.entries, key);
  //@unit assocTransient requires: editableTransient, isMapTransient, isTransientCell, kindHooks
  function assocTransient(value, key, item, indexed = false) {
    if (isMapTransient(value)) return kindHooks.assocTransientMap(value, key, item);
    if (isTransientCell(value) && !indexed && typeof key !== "number") throw new Error("TransientVector's key for assoc! must be a number.");
    editableTransient(value, "assoc!");
    if (isTransientCell(value)) kindHooks.assocTransientVector(value, key, item);
    return value;
  }
  //@unit makeMap.assocTransient effect: ArrayMapCell, makeMap, treeMapConstructor, recordMethods
  //@ when: assocTransient requires: arrayIndex, editableTransient, hash, hashNodeAssoc, kindHooks,
  //@ TransientHashMapCell
  // `assoc!` of a map transient, which edits its own fields, as upstream's:
  // an array map's pairs in place, keeping the key it has; at a ninth key a
  // transient hash map of its pairs (`TransientHashMapCell`) that gets the
  // key, while this one stays as it is; a hash map's trie along the key's
  // path, in the nodes its `edit` owns or copies once.
  kindHooks.assocTransientMap = (value, key, item) => {
    if (value.cljs$lang$kind$$LKEY === 31) {
      if (!value.editable$LKEY) editableTransient(value, "assoc!");
      const arr = value.arr$LKEY, index = arrayIndex(arr, key);
      if (index >= 0) { arr[index + 1] = item; return value; }
      if (arr.length < 16) { arr.push(key, item); return value; }
      const promoted = new TransientHashMapCell({}, 0, null, null);
      for (let i = 0; i < arr.length; i += 2) kindHooks.assocTransientMap(promoted, arr[i], arr[i + 1]);
      return kindHooks.assocTransientMap(promoted, key, item);
    }
    const edit = value.edit$LKEY;
    if (edit === null) editableTransient(value, "assoc!");
    if (key == null) {
      const nil = value.nil$LKEY;
      value.nil$LKEY = {key: nil ? nil.key : key, value: item, hash: 0};
      if (!nil) value.cnt$LKEY++;
      return value;
    }
    const keyHash = hash(key), result = {};
    value.root$LKEY = hashNodeAssoc(value.root$LKEY, 0, keyHash, {key, value: item, hash: keyHash}, edit, result);
    if (!result.old) value.cnt$LKEY++;
    return value;
  };
  //@unit conjTransient requires: editableTransient, isMapTransient, kindHooks
  function conjTransient(value, member) {
    const state = editableTransient(value, "conj!");
    if (isMapTransient(value)) return kindHooks.conjTransientMap(value, member);
    if (state === value) { kindHooks.conjTransientVector(state, member); return value; }
    return kindHooks.conjTransientSet(value, state, member);
  }
  //@unit conjBangOwn requires: conjBangProtocol, conjTransient, isTransient, protocolHooks
  // `conj!` onto the transient a core function's reduction holds, past the
  // protocol dispatch where no protocol implementation can answer for it.
  const conjBangOwn = (value, member) => !protocolHooks.builtin && isTransient(value) ? conjTransient(value, member) : conjBangProtocol(value, member);
  //@unit makeVector.conjTransient effect: makeVector, mapEntry when: conjTransient requires: conjVectorBang,
  //@ kindHooks
  kindHooks.conjTransientVector = conjVectorBang;
  //@unit makeVector.assocTransient effect: makeVector, mapEntry when: assocTransient requires: conjVectorBang,
  //@ kindHooks, outOfBounds, vectorAssocNode, vectorEditor, vectorTailOffset
  // `assoc!` of an index of a transient vector, or of its count.
  kindHooks.assocTransientVector = (store, key, item) => {
    if (key >= 0 && key < store.cnt$LKEY) {
      if (key >= vectorTailOffset(store.cnt$LKEY)) store.tail$LKEY[key & 31] = item;
      else store.root$LKEY = vectorAssocNode(store.shift$LKEY, store.root$LKEY, key, item, vectorEditor(store.owned$LKEY));
    } else if (key === store.cnt$LKEY) conjVectorBang(store, item);
    else outOfBounds();
  };
  //@unit makeSet.conjTransient effect: makeSet, treeSetConstructor when: conjTransient requires: kindHooks,
  //@ putMember
  // As upstream's TransientHashSet over a transient map, it edits the trie it
  // shares with the set or promotes to, as `assoc!` does (light-cljs-236).
  kindHooks.conjTransientSet = (value, state, member) => {
    state.entries = putMember(state.entries, member, state.edit);
    if (!Array.isArray(state.entries)) state.mapKind = "hash-map";
    return value;
  };
  //@unit makeMap.conjTransient effect: ArrayMapCell, makeMap, treeMapConstructor, recordMethods
  //@ when: conjTransient requires: assocBangProtocol, assocTransient, first, isMapEntry, isVector,
  //@ keyProtocol, kindHooks, next, protocolHooks, seq, truth, valProtocol
  // Past the protocol dispatch where no protocol implementation can answer
  // for the transient, as `conjBangOwn`.
  kindHooks.conjTransientMap = (value, member) => {
    const assoc = protocolHooks.builtin ? assocBangProtocol : assocTransient;
    if (isMapEntry(member)) return assoc(value, keyProtocol(member), valProtocol(member));
    if (isVector(member)) return assoc(value, member.call(null, 0), member.call(null, 1));
    for (let s = seq(member); s !== null && truth(first(s)); s = next(s)) {
      const entry = first(s);
      value = assoc(value, keyProtocol(entry), valProtocol(entry));
    }
    return value;
  };
  //@unit removeTransient requires: editableTransient, entryIndex, hashEntryIndex, hashRoots,
  //@ isMapTransient, kindHooks, removeHashEntry, trieDissoc
  function removeTransient(value, key, operation) {
    const state = editableTransient(value, operation);
    if (isMapTransient(value)) return kindHooks.dissocTransientMap(value, key);
    if (!Array.isArray(state.entries)) { state.entries = trieDissoc(state.entries, key, state.edit); return value; }
    const index = state.mapKind === "array-map" ? entryIndex(state.entries, key) : hashEntryIndex(state.entries, key);
    if (index >= 0) {
      if (state.mapKind === "array-map" && !hashRoots.has(state.entries)) state.entries[index] = state.entries[state.entries.length - 1];
      if (hashRoots.has(state.entries)) removeHashEntry(state.entries, index, state.edit);
      else state.entries.splice(state.mapKind === "array-map" ? state.entries.length - 1 : index, 1);
    }
    return value;
  }
  //@unit makeMap.dissocTransient effect: ArrayMapCell, makeMap, treeMapConstructor, recordMethods
  //@ when: removeTransient requires: arrayIndex, hash, hashNodeFind, hashNodeRemove, kindHooks
  // `dissoc!` of an editable map transient, as upstream's: an array map's
  // last pair takes the place of the key's; a hash map's trie loses the
  // key's entry along its path, in the nodes its `edit` owns.
  kindHooks.dissocTransientMap = (value, key) => {
    if (value.cljs$lang$kind$$LKEY === 31) {
      const arr = value.arr$LKEY, index = arrayIndex(arr, key);
      if (index >= 0) { const n = arr.length; arr[index] = arr[n - 2]; arr[index + 1] = arr[n - 1]; arr.pop(); arr.pop(); }
      return value;
    }
    if (key == null) {
      if (value.nil$LKEY) { value.nil$LKEY = null; value.cnt$LKEY--; }
      return value;
    }
    const root = value.root$LKEY, entry = root ? hashNodeFind(root, 0, hash(key), key) : null;
    if (entry) { value.root$LKEY = hashNodeRemove(root, 0, entry.hash, entry, value.edit$LKEY); value.cnt$LKEY--; }
    return value;
  };
  //@unit popTransient requires: editableTransient, vectorEditor, vectorLeaf, vectorPopTail
  // As upstream's TransientVector: the tail keeps a popped item until `conj!`
  // overwrites it or `persistent!` trims it; a vector whose tail empties
  // takes its last leaf for its tail.
  function popTransient(value) {
    const store = editableTransient(value, "pop!"), cnt = store.cnt$LKEY, shift = store.shift$LKEY;
    if (!cnt) throw new Error("Can't pop empty vector");
    if (cnt > 1 && !((cnt - 1) & 31)) {
      const edit = vectorEditor(store.owned$LKEY);
      store.tail$LKEY = edit(vectorLeaf(store, cnt - 2));
      const root = vectorPopTail(cnt, shift, store.root$LKEY, edit) ?? [];
      if (shift > 5 && root.length < 2) { store.root$LKEY = edit(root[0]); store.shift$LKEY -= 5; }
      else store.root$LKEY = root;
    }
    store.cnt$LKEY--;
    return value;
  }
  //@unit conjBang requires: conjBangProtocol, transient, vector
  function conjBang(value, ...members) {
    if (!arguments.length) return transient(vector());
    for (const member of members) value = conjBangProtocol(value, member);
    return value;
  }
  //@unit assocBang requires: assocBangProtocol
  function assocBang(value, key, item, ...pairs) {
    value = assocBangProtocol(value, key, item);
    for (let i = 0; i < pairs.length; i += 2) value = assocBangProtocol(value, pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null);
    return value;
  }
  //@unit dissocBang requires: dissocBangProtocol
  function dissocBang(value, key, ...keys) {
    value = dissocBangProtocol(value, key);
    for (const key of keys) value = dissocBangProtocol(value, key);
    return value;
  }
  //@unit disjBang requires: disjoinBangProtocol
  function disjBang(value, key, ...keys) {
    value = disjoinBangProtocol(value, key);
    for (const key of keys) value = disjoinBangProtocol(value, key);
    return value;
  }
  //@unit popBang requires: popBangProtocol
  function popBang(value) { return popBangProtocol(value); }

  //@unit hostHooks
  // Generic host-facing methods, for receivers that cannot answer natively.
  const hostHooks = {};
  //@unit vectorStorage requires: vectorItems, vectorNth, vectorStore
  // The items of a vector in an array, or null where it has none: the tail of
  // one with at most 32 items, which `seq` walks (`makeVector.seq`). Where
  // the store reads a caller's array (`callerTail`), whose items can change
  // or be getters, each item past one tail, or of a view, reads the store
  // when it is read, so the iterator, the printer and equality read no more
  // than they use.
  function vectorStorage(value) {
    const store = vectorStore(value), cnt = store.cnt$LKEY, sub = store.sub$LKEY;
    if (!cnt) return null;
    if (!(sub ?? store).callerTail$LKEY) return cnt <= 32 && !sub ? store.tail$LKEY : vectorItems(value);
    if (cnt <= 32 && !sub) return store.tail$LKEY;
    const items = [];
    for (let i = 0; i < cnt; i++) Object.defineProperty(items, i, {enumerable: true, get: () => vectorNth(store, i, null)});
    return items;
  }
  //@unit vectorCount requires: publicCollectionSlots, subvecSources, vectorStore
  function vectorCount(value) {
    const slots = publicCollectionSlots.get(value);
    if (slots && "cnt" in slots) return slots.cnt;
    return slots && subvecSources.has(value) ? slots.end - slots.start : vectorStore(value).cnt$LKEY;
  }
  //@unit vectorIndexOf requires: holdsVector, hostHooks, indexIn, isEntryCell, missing,
  //@ outOfBounds, protocolHooks, vectorCount, vectorNth, vectorStore
  function vectorIndexOf(coll, value, start, reverse) {
    if (protocolHooks.extend || !holdsVector(coll)) return hostHooks.index(coll, value, start, reverse);
    const store = vectorStore(coll), entry = isEntryCell(coll);
    return indexIn(vectorCount(coll), i => entry && i !== 0 && i !== 1 ? outOfBounds() : vectorNth(store, i, missing), value, start, reverse);
  }
  //@unit vectorPrototype requires: holdsVector, hostHooks, invalidArity, itemIterator, missing,
  //@ objectEquiv, outOfBounds, printHooks, protocolHooks, unkeyedJSON, vectorCount, vectorIndexOf,
  //@ vectorNth, vectorStorage, vectorStore
  const vectorPrototype = Object.freeze({
    equiv(other) { return objectEquiv(this, other); },
    // JSON of a class value: its own properties but its keyed fields, as
    // before it had them (docs/value-layout.md, question 3).
    toJSON() { return unkeyedJSON(this); },
    [Symbol.iterator]() { return protocolHooks.extend || !holdsVector(this) ? hostHooks.iterator(this) : itemIterator(vectorStorage(this), item => item); },
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    toString() { return printHooks.collection(this); },
    indexOf(value, start = 0) { return vectorIndexOf(this, value, start, false); },
    lastIndexOf(value, start) {
      if (start === undefined) start = protocolHooks.extend || !holdsVector(this) ? hostHooks.count(this) : vectorCount(this);
      return vectorIndexOf(this, value, start, true);
    },
    // The pinned fixed method accepts one index and ignores extra arguments.
    call(_self, index) {
      // The kind and store read here, where the receivers are this base's
      // few classes, rather than in `vectorStore`, which every kind test
      // calls with every kind of value.
      const kind = this.cljs$lang$kind$$LKEY;
      const store = (kind === 1 || kind === 2 || kind === 3) && this.cnt$LKEY !== undefined ? this : vectorStore(this);
      if (!store) throw new Error("Expected vector");
      if (kind === 3) {
        if (arguments.length !== 2 && arguments.length !== 3) return invalidArity(arguments.length - 1);
        if (index === 0 || index === 1) return store.tail$LKEY[index];
        return arguments.length === 3 ? arguments[2] : outOfBounds();
      }
      if (typeof index !== "number") throw new Error("Key must be integer");
      return vectorNth(store, index, missing);
    }
  });

  //@unit PersistentVectorValue.type effect: PersistentVectorValue requires: builtinType, kindOps,
  //@ kinds, PersistentVectorValue, protocolSentinel, vectorPrototype
  // A class declares the core protocols it implements (docs/value-layout.md,
  // section 3): the mask bits of `protocolMasks`, as literals, and a marker
  // for each protocol without one. A vector's are partition 0 IFn ICounted
  // IEmptyableCollection ICollection IIndexed ILookup IAssociative IStack
  // IVector IMeta IWithMeta IReduce IKVReduce IEquiv IHash ISeqable
  // ISequential IReversible IPrintWithWriter, partition 1 IEditableCollection
  // IComparable ICloneable IIterable IDrop, and the marker IFind: the
  // protocols its native tests (`builtinCoreImplementation`) answer.
  builtinType(PersistentVectorValue, vectorPrototype).prototype[kindOps] = 4;
  PersistentVectorValue.prototype.cljs$lang$kind$$LKEY = kinds.vector;
  PersistentVectorValue.prototype.cljs$lang$protocol_mask$partition0$$LKEY = -1979817185;
  PersistentVectorValue.prototype.cljs$lang$protocol_mask$partition1$$LKEY = 403460;
  PersistentVectorValue.prototype.cljs$core$IFind$$LKEY = protocolSentinel;
  //@unit VectorCell.type effect: VectorCell requires: PersistentVectorValue, VectorCell
  // A vector cell has the vectors' prototype, which the unit above builds.
  VectorCell.prototype = PersistentVectorValue.prototype;
  //@unit VectorViewCell.type effect: VectorViewCell requires: PersistentVectorValue, VectorViewCell
  VectorViewCell.prototype = PersistentVectorValue.prototype;
  //@unit SubvecValue.type effect: SubvecValue requires: builtinType, kindOps, kinds,
  //@ protocolSentinel, SubvecValue, vectorPrototype
  // A vector's protocols (`PersistentVectorValue.type`) but
  // IEditableCollection.
  builtinType(SubvecValue, vectorPrototype).prototype[kindOps] = 4;
  SubvecValue.prototype.cljs$lang$kind$$LKEY = kinds.subvec;
  SubvecValue.prototype.cljs$lang$protocol_mask$partition0$$LKEY = -1979817185;
  SubvecValue.prototype.cljs$lang$protocol_mask$partition1$$LKEY = 403456;
  SubvecValue.prototype.cljs$core$IFind$$LKEY = protocolSentinel;
  //@unit SubvecCell.type effect: SubvecCell requires: SubvecCell, SubvecValue
  SubvecCell.prototype = SubvecValue.prototype;
  //@unit entryPrototype requires: hostHooks, indexIn, invalidArity, itemIterator, objectEquiv,
  //@ outOfBounds, printHooks, protocolHooks, unkeyedJSON, vectorStore
  // The host-facing members of map entries where no vector can be built
  // (`MapEntryType.lean`): those of `vectorPrototype` for a vector of a key
  // and a value, as map entries are, the only values with items there. An
  // entry's store is a vector's of two items, which its tail holds.
  const entryPrototype = Object.freeze({
    equiv(other) { return objectEquiv(this, other); },
    // JSON of a class value: its own properties but its keyed fields, as
    // before it had them (docs/value-layout.md, question 3).
    toJSON() { return unkeyedJSON(this); },
    [Symbol.iterator]() { const items = entryItems(this); return items ? itemIterator(items, item => item) : hostHooks.iterator(this); },
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    toString() { return printHooks.collection(this); },
    indexOf(value, start = 0) { return entryIndexOf(this, value, start, false); },
    lastIndexOf(value, start) { return entryIndexOf(this, value, start, true); },
    call(_self, index) {
      const store = vectorStore(this);
      if (!store) throw new Error("Expected vector");
      if (arguments.length !== 2 && arguments.length !== 3) return invalidArity(arguments.length - 1);
      return index === 0 || index === 1 ? store.tail$LKEY[index] : arguments.length === 3 ? arguments[2] : outOfBounds();
    }
  });
  // An entry's items, unless protocol implementations could answer for it
  // or it is not one.
  function entryItems(entry) { return !protocolHooks.extend && vectorStore(entry)?.tail$LKEY; }
  // `lastIndexOf` without a start searches from the end.
  function entryIndexOf(entry, value, start, reverse) {
    const items = entryItems(entry);
    if (start === undefined) start = items ? 2 : hostHooks.count(entry);
    return items ? indexIn(2, i => i === 0 || i === 1 ? items[i] : outOfBounds(), value, start, reverse) : hostHooks.index(entry, value, start, reverse);
  }
  //@unit MapEntryType
  // The type of map entries, whose prototype has the vectors' members where
  // vectors can be built (`MapEntryType.vector`), and else the members of
  // map entries alone (`MapEntryType.lean`). The shared runtime defines it
  // twice, the second time with the vectors'.
  let MapEntryType;
  //@unit MapEntryType.lean effect: MapEntryType unless: makeVector requires: builtinType,
  //@ entryPrototype, mapEntry, MapEntryType
  MapEntryType = builtinType(function (key, value) { return mapEntry(key, value); }, entryPrototype);
  //@unit MapEntryType.vector effect: MapEntryType when: makeVector requires: builtinType, mapEntry,
  //@ MapEntryType, vectorPrototype
  MapEntryType = builtinType(function (key, value) { return mapEntry(key, value); }, vectorPrototype);
  //@unit MapEntryType.type effect: MapEntryType requires: kindOps, kinds, MapEntryType,
  //@ protocolSentinel
  // Map entries are kind 8 (light-cljs-373), with branches of their own, so
  // that a program building entries and no vector keeps no vector branch.
  // Their protocols are a vector's (`PersistentVectorValue.type`) but
  // IKVReduce, IEditableCollection and ICloneable, and with IMapEntry.
  MapEntryType.prototype[kindOps] = 8;
  MapEntryType.prototype.cljs$lang$kind$$LKEY = kinds.entry;
  MapEntryType.prototype.cljs$lang$protocol_mask$partition0$$LKEY = -1980863713;
  MapEntryType.prototype.cljs$lang$protocol_mask$partition1$$LKEY = 395264;
  MapEntryType.prototype.cljs$core$IFind$$LKEY = protocolSentinel;
  //@unit EntryCell.type effect: EntryCell requires: EntryCell, MapEntryType
  EntryCell.prototype = MapEntryType.prototype;
  //@unit TreeNodeCell.type effect: TreeNodeCell requires: MapEntryType, TreeNodeCell
  // A sorted map's node is a map entry (light-cljs-1035).
  TreeNodeCell.prototype = MapEntryType.prototype;
  //@unit makeKeyword requires: KeywordCell
  // A keyword of these fields; a null or undefined `hash` is computed on
  // first use. The public constructor's hash is kept as given, undefined
  // included, as its `_hash` field gives it back.
  function makeKeyword(namespace, name, spelling, hash) {
    return new KeywordCell(namespace, name, spelling, arguments.length < 4 ? null : hash);
  }

  //@unit freshKeyword requires: makeKeyword
  // Keywords built from data are fresh, as upstream.
  function freshKeyword(namespace, name, hash = null) {
    return makeKeyword(namespace, name,
      namespace == null ? name : namespace + "/" + name, hash);
  }

  //@unit keyword requires: freshKeyword, internedKeywords
  // Literal construction receives separately owned namespace/name strings
  // and is interned: one keyword per namespace and name. At `Advanced` a
  // literal also gives its hash, which the compiler computed
  // (`value_hash::keyword`), as upstream's constants carry it.
  function keyword(namespace, name, hash = null) {
    const key = namespace == null ? "/" + name : namespace.length + "/" + namespace + name;
    let value = internedKeywords.get(key);
    if (!value) internedKeywords.set(key, value = freshKeyword(namespace, name, hash));
    return value;
  }

  //@unit keywordFqn requires: keywordOf
  // What keyword case compares, or null.
  const keywordFqn = value => keywordOf(value)?.fqn$LKEY ?? null;

  //@unit makeVector requires: vectorTrie
  // A vector of the new array `items`, or the vector an operation built,
  // which is its own store (`vectorConj`, `vectorAssoc`, `vectorPop`); the
  // Pants build's profile swaps it for one that builds an engine vector.
  function makeVector(items, store) {
    return store === undefined ? vectorTrie(items) : store;
  }

  //@unit vectorTailOffset
  // A vector's store, the vector itself (`vectorStore`), is upstream's
  // PersistentVector: `cnt` items, the last 1 to 32 of them in the array
  // `tail` and the others in the full 32-item leaves of a 32-way trie, whose
  // `root` is `shift` / 5 levels above them; each a keyed field
  // (`VectorCell`). Nodes are arrays with no trailing holes. No operation
  // changes a persistent vector's nodes; only a transient changes those it
  // owns (`vectorEditor`). Seqs share leaves, which are frozen, and the
  // tail, which freezes when one first does (`vectorChunk`); the root and
  // the other nodes stay private. A view (`vectorView`) has `sub` and
  // `begin` instead of the trie's fields.
  // The index of the first item in the tail:
  const vectorTailOffset = cnt => cnt < 32 ? 0 : (cnt - 1) & -32;
  //@unit vectorTrie requires: VectorCell, vectorTailOffset
  // A vector of the new array `items`, which it keeps as its tail where it
  // can; `slice` gives the items of each leaf and of a shorter tail.
  function vectorTrie(items, slice = (from, to) => items.slice(from, to)) {
    const cnt = items.length, end = vectorTailOffset(cnt);
    // The frozen leaves, then each level of their parents up to the root.
    let nodes = [], shift = 5;
    for (let i = 0; i < end; i += 32) nodes.push(Object.freeze(slice(i, i + 32)));
    for (; nodes.length > 32; shift += 5) {
      const parents = [];
      for (let i = 0; i < nodes.length; i += 32) parents.push(nodes.slice(i, i + 32));
      nodes = parents;
    }
    return new VectorCell(null, cnt, shift, nodes, end ? slice(end, cnt) : items, null);
  }
  //@unit vectorLeaf requires: vectorSlice, vectorTailOffset
  // The leaf, or the tail, that holds index `i` of a store; of a view
  // (`vectorView`), a new array of its items from index `i & -32`.
  function vectorLeaf(store, i) {
    if (store.tail$LKEY === undefined && store.sub$LKEY !== undefined) {
      const from = store.begin$LKEY + (i & -32);
      return vectorSlice(store.sub$LKEY, from, Math.min(from + 32, store.begin$LKEY + store.cnt$LKEY));
    }
    if (i >= vectorTailOffset(store.cnt$LKEY)) return store.tail$LKEY;
    let node = store.root$LKEY;
    for (let level = store.shift$LKEY; level > 0; level -= 5) node = node[(i >>> level) & 31];
    return node;
  }
  //@unit vectorChunk requires: vectorLeaf, vectorStore
  // The leaf or tail holding index `i` of a vector, for a seq to share,
  // which freezes then (a leaf a transient built, the tail and a view's new
  // array), unless it is a tail the public constructor received (and
  // `assoc` kept): that one stays its caller's, as upstream.
  function vectorChunk(value, i) {
    const store = vectorStore(value), leaf = vectorLeaf(store, i);
    return !store.sub$LKEY && store.callerTail$LKEY && leaf === store.tail$LKEY ? leaf : Object.freeze(leaf);
  }
  //@unit vectorSlice requires: vectorLeaf
  // A new array of a store's items from index `from` to `to`.
  function vectorSlice(store, from, to) {
    const items = [];
    for (let i = from; i < to; i = (i | 31) + 1) items.push(...vectorLeaf(store, i).slice(i & 31, (i & 31) + to - i));
    return items;
  }
  //@unit vectorItems requires: vectorSlice, vectorStore
  // A new array of a vector's items.
  const vectorItems = value => vectorSlice(vectorStore(value), 0, vectorStore(value).cnt$LKEY);
  //@unit vectorView requires: SubvecCell
  // A subvector (`SubvecCell`), or with `Cell` another view class
  // (`VectorViewCell`), of `cnt` items of a store from its index `start`,
  // which shares that store: a view of a view is a view of the store that
  // one views. `nth`, `reduce`, `pop` and `assoc` read or update the store
  // it views; an operation that reads the fields of a trie (`shift`,
  // `root`, `tail`) reads those of one it builds of its items (`viewTrie`).
  function vectorView(store, start, cnt, Cell = SubvecCell) {
    const sub = store.sub$LKEY;
    return sub ? new Cell(null, cnt, sub, store.begin$LKEY + start, null) : new Cell(null, cnt, store, start, null);
  }
  //@unit viewTrie requires: vectorSlice, vectorTrie
  // A store with the trie's fields: itself, or for a view a vector of its
  // items, built once, or each time where the store it views reads a
  // caller's array (`callerTail`), whose items can change.
  function viewTrie(store) {
    const sub = store.sub$LKEY;
    if (!sub) return store;
    const trie = store.built$LKEY ?? vectorTrie(vectorSlice(sub, store.begin$LKEY, store.begin$LKEY + store.cnt$LKEY));
    if (!sub.callerTail$LKEY) store.built$LKEY = trie;
    return trie;
  }
  //@unit vectorReduce requires: isReduced, reducedValue, vectorLeaf, vectorNth
  // `reduce` of a store's items from index `i`: from `init` where `arity` is
  // 3, else from the first of them. A view's walks the store it views.
  function vectorReduce(store, rf, init, i, arity) {
    let end = store.cnt$LKEY;
    if (store.sub$LKEY) { i += store.begin$LKEY; end += store.begin$LKEY; store = store.sub$LKEY; }
    if (arity === 2) { if (i >= end) return rf.call(null); init = vectorNth(store, i++, null); }
    for (let leaf = vectorLeaf(store, i); i < end;) {
      init = rf.call(null, init, leaf[i & 31]);
      if (isReduced(init)) return reducedValue(init);
      if (!(++i & 31)) leaf = vectorLeaf(store, i);
    }
    return init;
  }
  //@unit vectorEditor
  // What a transient vector, which owns the nodes in `owned`, edits for
  // `node`: the node itself where it owns it, else a copy it then owns.
  const vectorEditor = owned => node => owned.has(node) ? node : (owned.add(node = node.slice()), node);
  //@unit vectorPath
  // `node` below `level` / 5 new nodes of one child each.
  const vectorPath = (level, node) => level ? [vectorPath(level - 5, node)] : node;
  //@unit vectorPushTail requires: vectorPath
  // `parent`, `level` above the leaves of a store of `cnt` items, with the
  // full tail `leaf` as its next leaf. A persistent vector copies the nodes on
  // the path; a transient's `edit` gives the ones it changes.
  function vectorPushTail(cnt, level, parent, leaf, edit) {
    const node = edit ? edit(parent) : parent.slice(), i = ((cnt - 1) >>> level) & 31;
    node[i] = level === 5 ? leaf : node[i] ? vectorPushTail(cnt, level - 5, node[i], leaf, edit) : vectorPath(level - 5, leaf);
    return node;
  }
  //@unit vectorConj requires: VectorCell, vectorPath, vectorPushTail, vectorTailOffset, viewTrie
  // A vector of `store`'s items and `item`: with a copy of the tail and
  // `item`, or else with the tail of one item after a frozen copy of the
  // full tail moves into the trie, which grows a level where its root is
  // full.
  function vectorConj(store, item) {
    if (store.sub$LKEY) store = viewTrie(store);
    const cnt = store.cnt$LKEY, shift = store.shift$LKEY, root = store.root$LKEY, tail = store.tail$LKEY;
    if (cnt - vectorTailOffset(cnt) < 32) {
      // A copying loop: spreading the tail costs more than the rest of `conj`.
      const length = tail.length, copy = new Array(length + 1);
      for (let i = 0; i < length; i++) copy[i] = tail[i];
      copy[length] = item;
      return new VectorCell(null, cnt + 1, shift, root, copy, null);
    }
    const grow = (cnt >>> 5) > (1 << shift), leaf = Object.freeze(tail.slice());
    return new VectorCell(null, cnt + 1, grow ? shift + 5 : shift,
      grow ? [root, vectorPath(shift, leaf)] : vectorPushTail(cnt, shift, root, leaf), [item], null);
  }
  //@unit conjVectorBang requires: vectorEditor, vectorPath, vectorPushTail, vectorTailOffset
  // `conj!` onto a transient vector (`TransientVectorCell`), its own store:
  // `vectorConj` in place.
  function conjVectorBang(store, item) {
    const cnt = store.cnt$LKEY, shift = store.shift$LKEY;
    if (cnt - vectorTailOffset(cnt) < 32) store.tail$LKEY[cnt & 31] = item;
    else {
      const owned = store.owned$LKEY, grow = (cnt >>> 5) > (1 << shift);
      store.root$LKEY = grow ? [store.root$LKEY, vectorPath(shift, store.tail$LKEY)] : vectorPushTail(cnt, shift, store.root$LKEY, store.tail$LKEY, vectorEditor(owned));
      if (grow) { owned.add(store.root$LKEY); store.shift$LKEY = shift + 5; }
      owned.add(store.tail$LKEY = [item]);
    }
    store.cnt$LKEY = cnt + 1;
    return store;
  }
  //@unit vectorAssocNode
  // `node`, `level` above the leaves, with `item` at index `i`: copied, a
  // leaf frozen, or given by a transient's `edit` (see `vectorPushTail`).
  function vectorAssocNode(level, node, i, item, edit) {
    const copy = edit ? edit(node) : node.slice(), at = (i >>> level) & 31;
    copy[at] = level ? vectorAssocNode(level - 5, node[at], i, item, edit) : item;
    return edit || level ? copy : Object.freeze(copy);
  }
  //@unit vectorAssoc requires: vectorAssocNode, VectorCell, vectorTailOffset, VectorViewCell
  // A vector of `store`'s items with `item` at its index `i`; of a view, a
  // view (`VectorViewCell`) of the store it views with that item. A tail the
  // caller gave (`callerTail`) stays marked where it is kept.
  function vectorAssoc(store, i, item) {
    const cnt = store.cnt$LKEY;
    if (store.sub$LKEY) return new VectorViewCell(null, cnt, vectorAssoc(store.sub$LKEY, store.begin$LKEY + i, item), store.begin$LKEY, null);
    const shift = store.shift$LKEY;
    if (i < vectorTailOffset(cnt)) {
      const result = new VectorCell(null, cnt, shift, vectorAssocNode(shift, store.root$LKEY, i, item), store.tail$LKEY, null);
      if (store.callerTail$LKEY) result.callerTail$LKEY = true;
      return result;
    }
    const tail = store.tail$LKEY.slice();
    tail[i & 31] = item;
    return new VectorCell(null, cnt, shift, store.root$LKEY, tail, null);
  }
  //@unit vectorPopTail
  // `node`, `level` above the leaves of a store of `cnt` items, without its
  // last leaf, or null where that leaves it empty: copied, or given by a
  // transient's `edit` (see `vectorPushTail`).
  function vectorPopTail(cnt, level, node, edit) {
    const i = ((cnt - 2) >>> level) & 31, child = level > 5 ? vectorPopTail(cnt, level - 5, node[i], edit) : null;
    if (child == null && !i) return null;
    const copy = edit ? edit(node) : node.slice(0, i);
    copy.length = i;
    if (child) copy.push(child);
    return copy;
  }
  //@unit vectorPop requires: VectorCell, vectorLeaf, vectorPopTail, vectorTailOffset, vectorTrie,
  //@ VectorViewCell
  // A vector of `store`'s items, of which it has at least one, without its
  // last: where the tail empties, the trie's last leaf becomes the tail and
  // the trie loses a level where its root is left with one child. Of a
  // view, a view (`VectorViewCell`) of one item fewer.
  function vectorPop(store) {
    const cnt = store.cnt$LKEY;
    if (store.sub$LKEY) return new VectorViewCell(null, cnt - 1, store.sub$LKEY, store.begin$LKEY, null);
    const shift = store.shift$LKEY, root = store.root$LKEY, tail = store.tail$LKEY;
    if (cnt === 1) return vectorTrie([]);
    if (cnt - vectorTailOffset(cnt) > 1) return new VectorCell(null, cnt - 1, shift, root, tail.slice(0, -1), null);
    const popped = vectorPopTail(cnt, shift, root) ?? [], shrink = shift > 5 && popped.length < 2;
    return new VectorCell(null, cnt - 1, shrink ? shift - 5 : shift, shrink ? popped[0] : popped, vectorLeaf(store, cnt - 2), null);
  }
  //@unit vector requires: makeVector
  function vector(...items) {
    return makeVector(items);
  }

  //@unit vectorFromArray requires: makeVector
  // Literal emission uses an array to avoid the host's argument-count limit.
  // Copy even when a test/host caller supplies its own array.
  function vectorFromArray(items) {
    return makeVector(Array.from(items));
  }

  //@unit keywordCore requires: freshKeyword, invalidArity, keywordOf, makeKeyword, namedOf,
  //@ symbolOf
  function keywordCore(namespace, name) {
    switch (arguments.length) {
      case 1: {
        if (keywordOf(namespace) !== undefined) return namespace;
        const sym = symbolOf(namespace);
        if (sym) return makeKeyword(sym.ns$LKEY, sym.name$LKEY, sym.str$LKEY);
        if (typeof namespace !== "string") return null;
        if (namespace === "/") return freshKeyword(null, namespace);
        const parts = namespace.split("/");
        return makeKeyword(parts.length === 2 ? parts[0] : null,
          parts.length === 2 ? parts[1] : parts[0], namespace);
      }
      case 2: {
        const nsKeyword = namedOf(namespace);
        const nameKeyword = namedOf(name);
        if (nsKeyword) namespace = nsKeyword.name$LKEY;
        if (nameKeyword) name = nameKeyword.name$LKEY;
        if ((namespace != null && typeof namespace !== "string") ||
            typeof name !== "string") {
          throw new Error("keyword expects strings or keywords and an optional nil namespace");
        }
        return freshKeyword(namespace, name);
      }
      default: return invalidArity(arguments.length);
    }
  }

  //@unit name requires: methodKeys, multiFns, namedOf, protocolHooks
  function name(value) {
    if (multiFns.has(value)) return name(value.name);
    const method = protocolHooks.implements?.(methodKeys[36], value) ? protocolHooks.builtin?.(value, methodKeys[37], 1) : null;
    if (method) return method.call(value, value);
    const named = namedOf(value);
    if (named) return named.name$LKEY;
    if (typeof value === "string") return value;
    throw new Error("Doesn't support name on this type");
  }

  //@unit subs requires: invalidArity
  // ClojureScript subs delegates index coercion and bounds to JS substring.
  function subs(value, start, end) {
    switch (arguments.length) {
      case 2: return value.substring(start);
      case 3: return value.substring(start, end);
      default: return invalidArity(arguments.length);
    }
  }

  //@unit namespace requires: methodKeys, multiFns, namedOf, protocolHooks
  function namespace(value) {
    if (multiFns.has(value)) return namespace(value.name);
    const method = protocolHooks.implements?.(methodKeys[36], value) ? protocolHooks.builtin?.(value, methodKeys[38], 1) : null;
    if (method) return method.call(value, value);
    const named = namedOf(value);
    if (named) return named.ns$LKEY;
    throw new Error("Doesn't support namespace on this type");
  }

  //@unit count requires: cellOf, chunkBufferOf, chunkOf, isTransient, kindHooks, maps, methodKeys,
  //@ protocolHooks, publicCollectionSlots, queues, seqableValue, sets, subvecSources, vectorStore,
  //@ viewOf
  function count(value) {
    const slots = publicCollectionSlots.get(value), cell = cellOf(value);
    // A sorted map's seq holds its count, or one below 0 where it counts its
    // nodes, as upstream's (light-cljs-1035).
    if (cell?.cell$LKEY === "tree-seq") return cell.cnt$LKEY < 0 ? kindHooks.countTreeSeq(value) : cell.cnt$LKEY;
    if (slots && "tree-map" in slots) return count(slots["tree-map"]);
    if (slots && ("cnt" in slots || "count" in slots)) return "cnt" in slots ? slots.cnt : slots.count;
    const chunked = viewOf(value);
    if (chunked?.vec$LKEY) return count(chunked.vec$LKEY) - chunked.base$LKEY - chunked.i$LKEY;
    if (slots && subvecSources.has(value)) return slots.end - slots.start;
    if (slots && sets.has(value)) return count(slots["hash-map"]);
    if (isTransient(value)) return kindHooks.countTransient(value);
    const buffer = chunkBufferOf(value);
    if (buffer !== undefined) return buffer.end$LKEY;
    const chunk = chunkOf(value);
    if (chunk !== undefined) return chunk.end$LKEY - chunk.off$LKEY;
    if (queues.has(value)) return queues.get(value).length;
    if (Array.isArray(value)) return value.length;
    if (value == null) return 0;
    const store = vectorStore(value);
    if (store) return store.cnt$LKEY;
    if (typeof value === "string") return value.length;
    // An array map's, a hash map's and a sorted map's fields, of one read of
    // the keyed kind.
    const mapKind = value.cljs$lang$kind$$LKEY;
    if (mapKind === 28 && value.arr$LKEY !== undefined) return (value.arr$LKEY.length + 1) >> 1;
    if ((mapKind === 29 || mapKind === 30) && (value.cnt$LKEY !== undefined || "cnt$LKEY" in value)) return value.cnt$LKEY;
    const entries = maps.get(value);
    if (entries) return entries.length;
    const members = sets.get(value);
    if (members) return members.length;
    if (cell && "count$LKEY" in cell) return cell.count$LKEY;
    const slice = viewOf(value);
    if (slice) return Math.max(0, slice.arr$LKEY.length - slice.i$LKEY);
    // Lazy seqs, cells without a count and other seqables walk their seqs.
    if (protocolHooks.implements?.(methodKeys[51], value) ?? seqableValue(value)) return kindHooks.countSeq(value);
    const nativeCount = protocolHooks.builtin?.(value, methodKeys[10], 1);
    if (nativeCount) return nativeCount.call(value, value);
    throw new Error("count not supported on this type");
  }

  //@unit count.branches effect: Cons, IntegerRangeValue, LazySeq, ListValue, MapEntryType,
  //@ PersistentArrayMapValue, PersistentHashMapValue, PersistentHashSetValue, PersistentQueueValue,
  //@ PersistentVectorValue, RangeValue, SubvecValue, treeMapConstructor, treeSetConstructor when: count
  //@ requires: count, kindOps, kindTables, noBranch
  // As `seq.branches`.
  count = (operation => Object.defineProperty(function (value) {
    const kind = value?.[kindOps], result = typeof kind === "number" ? kindTables[kind & 15][4](value) : noBranch;
    return result !== noBranch ? result : operation(value);
  }, "name", {value: operation.name}))(count);
  //@unit count.protocol effect: installMethods when: count requires: behindProtocol, count,
  //@ methodKeys, protocolHooks
  count = behindProtocol(count, value => protocolHooks.publicMethod?.(value, methodKeys[10], 1, true, true));
  //@unit count.host effect: count, extendType, extendInstance when: entryPrototype, resetMeta,
  //@ upstreamMembers, vectorPrototype requires: count, hostHooks
  hostHooks.count = count;
  //@unit accumulatingCount requires: count, countedValue, methodKeys, next, protocolHooks, seq
  function accumulatingCount(value) {
    let result = 0;
    for (let current = seq(value); current != null; current = next(current), result++) {
      if (protocolHooks.satisfies?.(methodKeys[11], current) ?? countedValue(current)) return result + (protocolHooks.countProtocol ?? count)(current);
    }
    return result;
  }
  //@unit count.seqs effect: ChunkedCons, compilerFormBridge, cons, listFromArray, Cons, cycleNode,
  //@ emptyList, EmptyListValue, Eduction, extendInstance, extendType, iterateNode, iteration,
  //@ iteratorSeq, ISeqable, LazySeq, ListValue, mapProjection, rangeNode, RepeatValue, reSeq,
  //@ treeMapSeqConstructor when: count requires: accumulatingCount, kindHooks
  // Only these units, and a program type implementing `ISeqable`
  // (`PROTOCOL_ROOTS` in src/runtime_slice.rs), build values `count` walks.
  kindHooks.countSeq = accumulatingCount;
  //@unit treeMapSeqConstructor.count effect: treeMapSeqConstructor when: count requires: count,
  //@ kindHooks, next
  // A sorted map's seq without a count counts its tail's.
  kindHooks.countTreeSeq = value => 1 + count(next(value));
  //@unit count.cells effect: Cons, ListValue, IntegerRangeValue, RangeValue when: count requires: cellOf,
  //@ kindHooks, kindTables, methodKeys, noBranch, protocolHooks
  // A list and an integer range hold their count (and a public constructor
  // gave it the same count it gave the range); other cells walk their seqs.
  kindTables[2][4] = value => {
    const cell = cellOf(value), kind = cell?.cell$LKEY;
    if (kind === "list" || kind === "range" && cell.integer$LKEY) return cell.count$LKEY;
    return (kind === "cons" || kind === "range") && (protocolHooks.implements?.(methodKeys[51], value) ?? true) ? kindHooks.countSeq(value) : noBranch;
  };
  //@unit LazySeq.count effect: LazySeq when: count requires: isLazy, kindHooks, kindTables,
  //@ methodKeys, noBranch, protocolHooks
  kindTables[1][4] = value => isLazy(value) && (protocolHooks.implements?.(methodKeys[51], value) ?? true) ? kindHooks.countSeq(value) : noBranch;
  //@unit count.vectors effect: PersistentVectorValue, SubvecValue when: count requires: kindTables,
  //@ noBranch, publicCollectionSlots, vectorStore
  // The counts the stores of the collections, kinds 4 to 7, hold
  // (light-cljs-358). A public constructor's value, whose slots can give
  // another count (`cnt`), a subvector's bounds or a sorted set's map, takes
  // the operation.
  kindTables[4][4] = value => { const store = vectorStore(value); return store && !publicCollectionSlots.has(value) ? store.cnt$LKEY : noBranch; };
  //@unit count.maps effect: PersistentArrayMapValue, PersistentHashMapValue, treeMapConstructor
  //@ when: count requires: isArrayMap, isHashMap, isTreeMap, kindTables, maps, noBranch,
  //@ publicCollectionSlots
  kindTables[5][4] = value => {
    if (isArrayMap(value)) return publicCollectionSlots.has(value) ? noBranch : value.cnt$LKEY;
    if (isHashMap(value) || isTreeMap(value)) return value.cnt$LKEY;
    const entries = maps.get(value);
    return entries && !publicCollectionSlots.has(value) ? entries.length : noBranch;
  };
  //@unit count.sets effect: PersistentHashSetValue, treeSetConstructor when: count requires: kindTables,
  //@ noBranch, publicCollectionSlots, sets
  kindTables[6][4] = value => { const members = sets.get(value); return members && !publicCollectionSlots.has(value) ? members.length : noBranch; };
  //@unit count.queues effect: PersistentQueueValue when: count requires: kindTables, noBranch,
  //@ publicCollectionSlots, queues
  kindTables[7][4] = value => { const queue = queues.get(value); return queue && !publicCollectionSlots.has(value) ? queue.length : noBranch; };
  //@unit count.entries effect: MapEntryType when: count requires: kindTables, noBranch,
  //@ publicCollectionSlots, vectorStore
  kindTables[8][4] = value => { const store = vectorStore(value); return store && !publicCollectionSlots.has(value) ? store.cnt$LKEY : noBranch; };
  //@unit IndexedSeqValue.viewKind effect: IndexedSeqValue when: first.branches requires: IndexedSeqValue,
  //@ kindOps
  // Views, the seqs of arrays, strings and vectors, are kind 3 where a kind
  // with types of its own (lazy seqs, cells or the collections) wraps
  // `first`: a program that builds only views keeps the operations' own
  // paths, and no wrapper.
  IndexedSeqValue.prototype[kindOps] = 3;
  //@unit ChunkedSeqValue.viewKind effect: ChunkedSeqValue when: first.branches requires: ChunkedSeqValue,
  //@ kindOps
  ChunkedSeqValue.prototype[kindOps] = 3;
  //@unit first.views effect: ChunkedSeqValue, IndexedSeqValue when: first.branches requires: kindTables,
  //@ noBranch, viewOf
  kindTables[3][1] = value => {
    const slice = viewOf(value);
    return slice === undefined ? noBranch : slice.i$LKEY < slice.arr$LKEY.length ? slice.arr$LKEY[slice.i$LKEY] : null;
  };


  //@unit vectorNth requires: missing, outOfBounds, vectorTailOffset
  // The item of a vector's store at a number `index`, truncated; a view's is
  // that of the store it views. A view is told by the trie's own `tail`
  // field, which it lacks: a vector reads only its own fields here.
  function vectorNth(store, index, notFound) {
    const cnt = store.cnt$LKEY;
    if (index >= 0 && index < cnt) {
      const tail = store.tail$LKEY;
      if (tail === undefined && store.sub$LKEY !== undefined) return vectorNth(store.sub$LKEY, store.begin$LKEY + index, notFound);
      if (index >= vectorTailOffset(cnt)) return tail[index & 31];
      let node = store.root$LKEY;
      for (let level = store.shift$LKEY; level > 0; level -= 5) node = node[(index >>> level) & 31];
      return node[index & 31];
    }
    if (notFound === missing) return outOfBounds();
    return notFound;
  }

  //@unit nth requires: cellOf, chunkOf, first, indexedValue, invalidArity, isEntryCell,
  //@ isTransient, kindHooks, methodKeys, missing, next, outOfBounds, protocolHooks, seq,
  //@ sequential, seqValue, vectorNth, vectorStore, viewOf
  function nth(value, index, notFound) {
    const arity = arguments.length;
    if (arity !== 2 && arity !== 3) return invalidArity(arity);
    if (typeof index !== "number") throw new Error("Index argument to nth must be a number");
    if (value == null) return arity === 2 ? value : notFound;
    const fallback = arity === 2 ? missing : notFound;
    // A vector first, the most common: only a map entry, of two items, has
    // no index but 0 and 1.
    const store = vectorStore(value);
    if (store) return store.cnt$LKEY === 2 && index !== 0 && index !== 1 && isEntryCell(value) ? fallback === missing ? outOfBounds() : fallback : vectorNth(store, index, fallback);
    if (isTransient(value)) return kindHooks.nthTransient(value, index, fallback);
    if (chunkOf(value) !== undefined) return kindHooks.nthChunk(value, index, fallback);
    if (cellOf(value)?.integer$LKEY) { const c = cellOf(value); return index >= 0 && index < c.count$LKEY ? c.first$LKEY + Math.floor(index) * c.step$LKEY : fallback === missing ? outOfBounds() : fallback; }
    if (Array.isArray(value)) return index > -1 && index < value.length ? value[Math.trunc(index)] : fallback === missing ? outOfBounds() : fallback;
    if (typeof value === "string") {
      if (index > -1 && index < value.length) return value.charAt(index | 0);
      if (fallback === missing) return outOfBounds();
      return fallback;
    }
    const implementing = protocolHooks.implements;
    if ((implementing?.(methodKeys[49], value) || implementing?.(methodKeys[53], value)) ?? (seqValue(value) || sequential(value))) {
      // Lists traverse to an exact zero. Indexed finite views retain the
      // pinned absolute-offset lookup, including fractional/missing slots.
      let current = value, remaining = index;
      if (!viewOf(current)?.indexed$LKEY && remaining < 0)
        return fallback === missing ? outOfBounds() : fallback;
      while (current !== null) {
        const slice = viewOf(current);
        if (slice?.indexed$LKEY) {
          const absolute = slice.i$LKEY + remaining;
          if (absolute >= 0 && absolute < slice.arr$LKEY.length) return slice.arr$LKEY[absolute];
          break;
        }
        if (remaining === 0) {
          if (seq(current) != null) return first(current);
          break;
        }
        if (protocolHooks.satisfies?.(methodKeys[24], current) ?? indexedValue(current)) {
          // A seq is never a transient, so its `-nth` is `nth`'s own.
          const indexed = protocolHooks.nthProtocol ?? nth;
          return fallback === missing ? indexed(current, remaining) : indexed(current, remaining, fallback);
        }
        if (seq(current) == null) break;
        current = next(current);
        remaining--;
      }
      return fallback === missing ? outOfBounds() : fallback;
    }
    const nativeNth = protocolHooks.publicMethod?.(value, methodKeys[25], arity);
    if (nativeNth) return nativeNth.call(value, ...arguments);
    throw new Error("nth not supported on this type");
  }

  //@unit nth.branches effect: MapEntryType, PersistentVectorValue, SubvecValue when: nth requires: kindOps,
  //@ kindTables, missing, noBranch, nth
  // As `seq.branches`, for a number `index` and two or three arguments; a
  // branch takes the value, the index and the item for an index out of
  // bounds, or `missing` to throw.
  nth = (operation => Object.defineProperty(function (value, index, notFound) {
    const kind = value?.[kindOps], arity = arguments.length;
    const result = typeof kind === "number" && typeof index === "number" && (arity === 2 || arity === 3)
      ? kindTables[kind & 15][5](value, index, arity === 2 ? missing : notFound) : noBranch;
    return result !== noBranch ? result : operation.apply(null, arguments);
  }, "name", {value: operation.name}))(nth);
  //@unit nth.vectors effect: PersistentVectorValue, SubvecValue when: nth requires: isEntryCell,
  //@ kindTables, noBranch, vectorNth, vectorStore
  // A map entry, of two items, has no index but 0 and 1 (see `nth`).
  kindTables[4][5] = (value, index, notFound) => {
    const store = vectorStore(value);
    return store && !(store.cnt$LKEY === 2 && isEntryCell(value)) ? vectorNth(store, index, notFound) : noBranch;
  };
  //@unit nth.entries effect: MapEntryType when: nth requires: isEntryCell, kindTables, missing,
  //@ noBranch, outOfBounds, vectorStore
  // A map entry's items (light-cljs-373), which its store's tail holds, read
  // first, as `nth` tests for an entry only at another index; a vector with
  // the entries' prototype takes the operation at those.
  kindTables[8][5] = (value, index, notFound) => {
    const store = vectorStore(value);
    if (store && store.cnt$LKEY === 2 && !store.sub$LKEY && (index === 0 || index === 1)) return store.tail$LKEY[index];
    return store && store.cnt$LKEY === 2 && isEntryCell(value) ? notFound === missing ? outOfBounds() : notFound : noBranch;
  };
  //@unit nth.protocol effect: installMethods when: nth requires: methodKeys, nth, protocolHooks
  nth = (operation => Object.defineProperty(function (value, index, notFound) {
    const arity = arguments.length;
    const method = value == null || typeof index !== "number" || arity !== 2 && arity !== 3 ? null : protocolHooks.publicMethod?.(value, methodKeys[25], arity, true, true);
    return method ? method.call(value, ...arguments) : operation.apply(null, arguments);
  }, "name", {value: operation.name}))(nth);

  //@unit lookup requires: isEntryCell, kindHooks, maps, vectorStore
  // The map, vector and string branches; the other kinds' wrap it.
  function lookup(value, key, notFound) {
    // An array map's and a hash map's fields, of one read of the keyed kind.
    const mapKind = value?.cljs$lang$kind$$LKEY;
    if (mapKind === 28 && value.arr$LKEY !== undefined) return kindHooks.arrayMapLookup(value, key, notFound);
    if (mapKind === 29 && (value.cnt$LKEY !== undefined || "cnt$LKEY" in value)) return kindHooks.hashMapLookup(value, key, notFound);
    const entries = maps.get(value);
    if (entries) {
      if (!Array.isArray(entries)) return kindHooks.trieValue(entries, key, notFound);
      const index = kindHooks.entryIndex(value, entries, key);
      return index < 0 ? notFound : entries[index].value;
    }
    const store = vectorStore(value);
    if (store) return isEntryCell(value) ? key === 0 || key === 1 ? store.tail$LKEY[key] : notFound : typeof key === "number" && key >= 0 && key < store.cnt$LKEY ? kindHooks.nthVector(store, key, notFound) : notFound;
    if (typeof value === "string" && key != null && key > -1 && key < value.length) {
      return value.charAt(key | 0);
    }
    // Raw object properties are not lookup entries. Public get handles array
    // indexing separately without giving host objects owned collection storage.
    return notFound;
  }
  //@unit makeTransient.lookup effect: makeTransient when: lookup requires: kindHooks,
  //@ transientLookup
  kindHooks.lookupTransient = transientLookup;
  //@unit makeVector.lookup effect: makeVector when: lookup requires: kindHooks, vectorNth
  kindHooks.nthVector = vectorNth;
  //@unit makeMap.lookup effect: ArrayMapCell, makeMap, treeMapConstructor, makeSet,
  //@ treeSetConstructor when: lookup requires: collectionEntryIndex, kindHooks
  // A record is a map too, but `get` finds its `-lookup` (`recordMethods.lookup`)
  // first.
  kindHooks.entryIndex = collectionEntryIndex;

  //@unit get requires: invalidArity, lookup, methodKeys, protocolHooks
  function get(value, key, notFound) {
    const method = value == null ? null : protocolHooks.publicMethod?.(value, methodKeys[30], arguments.length, true);
    if (method) return method.call(value, ...arguments);
    if (Array.isArray(value)) {
      const valid = key != null && key < value.length && (arguments.length === 2 || key > -1);
      return valid ? value[key | 0] : arguments.length === 2 ? null : notFound;
    }
    switch (arguments.length) {
      case 2: return lookup(value, key, null);
      case 3: return lookup(value, key, notFound);
      default: return invalidArity(arguments.length);
    }
  }

  //@unit get.branches effect: MapEntryType, PersistentArrayMapValue, PersistentHashMapValue,
  //@ PersistentHashSetValue, PersistentVectorValue, SubvecValue, treeMapConstructor,
  //@ treeSetConstructor when: get requires: get, kindOps, kindTables, missing, noBranch
  // As `seq.branches`, for two or three arguments; a branch takes the value,
  // the key, the item for a key it lacks and the arity, which the protocol
  // lookup it makes first needs. `contains?`, which asks for `missing`,
  // takes the operation.
  get = (operation => Object.defineProperty(function (value, key, notFound) {
    const kind = value?.[kindOps], arity = arguments.length;
    const result = typeof kind === "number" && (arity === 2 || arity === 3 && notFound !== missing)
      ? kindTables[kind & 15][6](value, key, arity === 2 ? null : notFound, arity) : noBranch;
    return result !== noBranch ? result : operation.apply(null, arguments);
  }, "name", {value: operation.name}))(get);
  //@unit get.vectors effect: PersistentVectorValue, SubvecValue when: get requires: isEntryCell,
  //@ kindTables, methodKeys, noBranch, protocolHooks, vectorNth, vectorStore
  // `lookup` of a vector, after the protocol lookup that `get` makes first,
  // whose method it calls as `get` would.
  kindTables[4][6] = (value, key, notFound, arity) => {
    const store = vectorStore(value);
    if (!store || isEntryCell(value)) return noBranch;
    const method = protocolHooks.publicMethod?.(value, methodKeys[30], arity, true);
    if (method) return arity === 2 ? method.call(value, value, key) : method.call(value, value, key, notFound);
    return typeof key === "number" && key >= 0 && key < store.cnt$LKEY ? vectorNth(store, key, notFound) : notFound;
  };
  //@unit get.maps effect: PersistentArrayMapValue, PersistentHashMapValue, treeMapConstructor when: get
  //@ requires: collectionEntryIndex, isArrayMap, isHashMap, isTreeMap, kindHooks, kindTables,
  //@ lookup, maps, methodKeys, noBranch, protocolHooks, publicCollectionSlots
  // Likewise of a map: a hash map's trie and entries without slots are
  // searched as `lookup` does, and others, as trees, take `lookup`.
  kindTables[5][6] = (value, key, notFound, arity) => {
    if (!maps.has(value)) return noBranch;
    const method = protocolHooks.publicMethod?.(value, methodKeys[30], arity, true);
    if (method) return arity === 2 ? method.call(value, value, key) : method.call(value, value, key, notFound);
    if (isArrayMap(value) || isHashMap(value) || isTreeMap(value)) return lookup(value, key, notFound);
    const entries = maps.get(value);
    if (!Array.isArray(entries)) return kindHooks.trieValue(entries, key, notFound);
    if (publicCollectionSlots.has(value)) return lookup(value, key, notFound);
    const index = collectionEntryIndex(value, entries, key);
    return index < 0 ? notFound : entries[index].value;
  };
  //@unit get.sets effect: PersistentHashSetValue, treeSetConstructor when: get requires: collectionEntryIndex,
  //@ kindHooks, kindTables, methodKeys, noBranch, protocolHooks, sets
  // Likewise of a set (light-cljs-373), whose member equal to the key it
  // gives: from a hash set's trie or an array of members, as
  // `lookup.sets` does.
  kindTables[6][6] = (value, key, notFound, arity) => {
    const members = sets.get(value);
    if (!members) return noBranch;
    const method = protocolHooks.publicMethod?.(value, methodKeys[30], arity, true);
    if (method) return arity === 2 ? method.call(value, value, key) : method.call(value, value, key, notFound);
    if (!Array.isArray(members)) return kindHooks.trieMember(members, key, notFound);
    const index = collectionEntryIndex(value, members, key);
    return index < 0 ? notFound : members[index].key;
  };
  //@unit get.entries effect: MapEntryType when: get requires: isEntryCell, kindTables, noBranch,
  //@ protocolHooks, vectorStore
  // Likewise of a map entry (light-cljs-373), as `lookup` reads it; where
  // protocol implementations exist, the operation, which looks them up.
  kindTables[8][6] = (value, key, notFound) => {
    const store = vectorStore(value);
    return store && !protocolHooks.publicMethod && isEntryCell(value) ? key === 0 || key === 1 ? store.tail$LKEY[key] : notFound : noBranch;
  };
  //@unit get.own effect: installMethods when: get requires: demandKey, get, keySlot, methodKeys,
  //@ noBranch, protocolHooks
  // `get` of a value whose type declares `ILookup` by its mask bit, as the
  // program's types and records do: the method its slot holds, read here, so
  // that the engine's feedback for the read is this operation's own, not that
  // of the protocol lookup every operation shares (light-cljs-358). A slot
  // without a method finishes as the protocol lookup would
  // (`protocolHooks.declaredMethod`), from the slot as read, which a getter
  // could have produced after changing the value; any other value takes the
  // operation, and so does a runtime class's value whose slot answers
  // `noBranch` (`get.slots`).
  get = (operation => Object.defineProperty(function (value, key, notFound) {
    const resolved = methodKeys[30], arity = arguments.length;
    if (value != null && (arity === 2 || arity === 3) && (value[resolved.mask] & resolved.bit) !== 0) {
      demandKey(resolved, arity, value);
      const slot = value[keySlot(resolved, arity)], own = slot ? slot : protocolHooks.declaredMethod(value, resolved, arity, slot);
      if (own) {
        const result = arity === 2 ? own.call(value, value, key) : own.call(value, value, key, notFound);
        if (result !== noBranch) return result;
      }
    }
    return arity === 2 ? operation(value, key) : arity === 3 ? operation(value, key, notFound) : operation.apply(null, arguments);
  }, "name", {value: operation.name}))(get);

  //@unit slotFirst
  // Whether the slot-first wrappers run: `off` where a compiler session
  // demands declarations (`setDeclarationDemand`), as there the operations'
  // older layers demand each protocol before they read a slot. A method, not
  // an assignment, so that the shared-heap profile, which seals this object,
  // keeps the switch on the task's cells.
  const slotFirst = {on: true, off() { slotFirst.on = false; }};
  //@unit builtinSlots
  // The runtime's own slot methods (`PersistentVectorValue.nth` and the
  // rest), which `builtinSlot` marks: the protocol lookups of the
  // operations' older layers (`keyMethod`, `directMethod`) do not take them
  // for a program's implementation, so those layers answer as before and
  // never call a slot that serves them.
  const builtinSlots = new WeakSet();
  function builtinSlot(method) { builtinSlots.add(method); return method; }
  // A persistent vector's protocol methods are slots of its prototype
  // (docs/value-layout.md, section 3), each a unit kept with the operation
  // that reads it. A slot is complete for every value with the prototype: it
  // never takes the operation it serves, whose protocol lookup finds the slot
  // too. `extend-type` overwrites a slot and `specify!` shadows it, so the
  // next call runs the program's method. A value with the prototype but no
  // store of its own (one made by reflection) answers `noBranch`, and the
  // slot-first wrapper then takes the operation, which answers it as before.
  //@unit PersistentVectorValue.nth effect: PersistentVectorValue when: nth requires: builtinSlot,
  //@ missing, noBranch, PersistentVectorValue, vectorNth
  PersistentVectorValue.prototype.cljs$core$IIndexed$_nth$arity$2$LKEY = builtinSlot(function (self, index) {
    const store = self;
    if (store.cnt$LKEY === undefined && !("cnt$LKEY" in store)) return noBranch;
    return vectorNth(store, index, missing);
  });
  PersistentVectorValue.prototype.cljs$core$IIndexed$_nth$arity$3$LKEY = builtinSlot(function (self, index, notFound) {
    const store = self;
    if (store.cnt$LKEY === undefined && !("cnt$LKEY" in store)) return noBranch;
    return vectorNth(store, index, notFound);
  });
  //@unit PersistentVectorValue.count effect: PersistentVectorValue when: count requires: builtinSlot,
  //@ noBranch, PersistentVectorValue
  PersistentVectorValue.prototype.cljs$core$ICounted$_count$arity$1$LKEY = builtinSlot(function (self) {
    const store = self;
    if (store.cnt$LKEY === undefined && !("cnt$LKEY" in store)) return noBranch;
    return store.cnt$LKEY;
  });
  //@unit PersistentVectorValue.peek effect: PersistentVectorValue when: peek requires: builtinSlot,
  //@ noBranch, PersistentVectorValue, vectorNth
  PersistentVectorValue.prototype.cljs$core$IStack$_peek$arity$1$LKEY = builtinSlot(function (self) {
    const store = self;
    if (store.cnt$LKEY === undefined && !("cnt$LKEY" in store)) return noBranch;
    return store.cnt$LKEY ? vectorNth(store, store.cnt$LKEY - 1, null) : null;
  });
  //@unit PersistentVectorValue.conj effect: PersistentVectorValue when: conj requires: builtinSlot,
  //@ conjMeta, makeVector, noBranch, PersistentVectorValue, vectorConj
  PersistentVectorValue.prototype.cljs$core$ICollection$_conj$arity$2$LKEY = builtinSlot(function (self, item) {
    const store = self;
    if (store.cnt$LKEY === undefined && !("cnt$LKEY" in store)) return noBranch;
    return conjMeta(self, makeVector(null, vectorConj(store, item)));
  });
  //@unit PersistentVectorValue.assoc effect: PersistentVectorValue when: assoc requires: builtinSlot,
  //@ makeVector, noBranch, outOfBounds, PersistentVectorValue, preserveMeta, vectorAssoc,
  //@ vectorConj
  PersistentVectorValue.prototype.cljs$core$IAssociative$_assoc$arity$3$LKEY = builtinSlot(function (self, key, item) {
    let store = self;
    if (store.cnt$LKEY === undefined && !("cnt$LKEY" in store)) return noBranch;
    if (typeof key !== "number") throw new Error("Vector key must be a number");
    if (key >= 0 && key < store.cnt$LKEY) store = vectorAssoc(store, key, item);
    else if (key === store.cnt$LKEY) store = vectorConj(store, item);
    else outOfBounds();
    return preserveMeta(self, makeVector(null, store));
  });
  //@unit PersistentVectorValue.lookup effect: PersistentVectorValue when: get requires: builtinSlot,
  //@ noBranch, PersistentVectorValue, vectorNth
  PersistentVectorValue.prototype.cljs$core$ILookup$_lookup$arity$2$LKEY = builtinSlot(function (self, key) {
    const store = self;
    if (store.cnt$LKEY === undefined && !("cnt$LKEY" in store)) return noBranch;
    return typeof key === "number" && key >= 0 && key < store.cnt$LKEY ? vectorNth(store, key, null) : null;
  });
  PersistentVectorValue.prototype.cljs$core$ILookup$_lookup$arity$3$LKEY = builtinSlot(function (self, key, notFound) {
    const store = self;
    if (store.cnt$LKEY === undefined && !("cnt$LKEY" in store)) return noBranch;
    return typeof key === "number" && key >= 0 && key < store.cnt$LKEY ? vectorNth(store, key, notFound) : notFound;
  });
  // A subvector's slots (light-cljs-1028) are a vector's, over the view it
  // is (`vectorNth` reads through it), but for a subvector its public
  // constructor built (`SubvecValue`), whose bounds give its count and whose
  // `conj` and `assoc` build a subvector of the vector it views, as the
  // operations give it (`SubvecValue.conj`, `SubvecValue.assoc`). Each slot
  // is complete for a subvector: the compile-time runtime's protocol lookup
  // finds a slot and calls it.
  //@unit SubvecValue.nth effect: SubvecValue when: nth requires: builtinSlot, missing, noBranch,
  //@ SubvecValue, vectorNth
  SubvecValue.prototype.cljs$core$IIndexed$_nth$arity$2$LKEY = builtinSlot(function (self, index) {
    if (self.cnt$LKEY === undefined && !("cnt$LKEY" in self)) return noBranch;
    return vectorNth(self, index, missing);
  });
  SubvecValue.prototype.cljs$core$IIndexed$_nth$arity$3$LKEY = builtinSlot(function (self, index, notFound) {
    if (self.cnt$LKEY === undefined && !("cnt$LKEY" in self)) return noBranch;
    return vectorNth(self, index, notFound);
  });
  //@unit SubvecValue.count effect: SubvecValue when: count requires: builtinSlot, noBranch,
  //@ publicCollectionSlots, SubvecValue
  SubvecValue.prototype.cljs$core$ICounted$_count$arity$1$LKEY = builtinSlot(function (self) {
    if (self.cnt$LKEY === undefined && !("cnt$LKEY" in self)) return noBranch;
    const slots = publicCollectionSlots.get(self);
    return slots ? slots.end - slots.start : self.cnt$LKEY;
  });
  //@unit SubvecValue.peek effect: SubvecValue when: peek requires: builtinSlot, noBranch,
  //@ SubvecValue, vectorNth
  SubvecValue.prototype.cljs$core$IStack$_peek$arity$1$LKEY = builtinSlot(function (self) {
    if (self.cnt$LKEY === undefined && !("cnt$LKEY" in self)) return noBranch;
    return self.cnt$LKEY ? vectorNth(self, self.cnt$LKEY - 1, null) : null;
  });
  //@unit SubvecValue.conjSlot effect: SubvecValue when: conj requires: builtinSlot, conjMeta,
  //@ kindHooks, makeVector, noBranch, preserveMeta, publicCollectionSlots, SubvecValue, vectorConj
  SubvecValue.prototype.cljs$core$ICollection$_conj$arity$2$LKEY = builtinSlot(function (self, item) {
    if (self.cnt$LKEY === undefined && !("cnt$LKEY" in self)) return noBranch;
    if (publicCollectionSlots.has(self)) return preserveMeta(self, kindHooks.conjSubvec(self, item));
    return conjMeta(self, makeVector(null, vectorConj(self, item)));
  });
  //@unit SubvecValue.assocSlot effect: SubvecValue when: assoc requires: builtinSlot, kindHooks,
  //@ makeVector, noBranch, outOfBounds, preserveMeta, publicCollectionSlots, SubvecValue,
  //@ vectorAssoc, vectorConj
  SubvecValue.prototype.cljs$core$IAssociative$_assoc$arity$3$LKEY = builtinSlot(function (self, key, item) {
    if (self.cnt$LKEY === undefined && !("cnt$LKEY" in self)) return noBranch;
    if (publicCollectionSlots.has(self)) return kindHooks.assocSubvec(self, key, item, []);
    if (typeof key !== "number") throw new Error("Vector key must be a number");
    let store = self;
    if (key >= 0 && key < store.cnt$LKEY) store = vectorAssoc(store, key, item);
    else if (key === store.cnt$LKEY) store = vectorConj(store, item);
    else outOfBounds();
    return preserveMeta(self, makeVector(null, store));
  });
  //@unit SubvecValue.lookup effect: SubvecValue when: get requires: builtinSlot, noBranch,
  //@ SubvecValue, vectorNth
  SubvecValue.prototype.cljs$core$ILookup$_lookup$arity$2$LKEY = builtinSlot(function (self, key) {
    if (self.cnt$LKEY === undefined && !("cnt$LKEY" in self)) return noBranch;
    return typeof key === "number" && key >= 0 && key < self.cnt$LKEY ? vectorNth(self, key, null) : null;
  });
  SubvecValue.prototype.cljs$core$ILookup$_lookup$arity$3$LKEY = builtinSlot(function (self, key, notFound) {
    if (self.cnt$LKEY === undefined && !("cnt$LKEY" in self)) return noBranch;
    return typeof key === "number" && key >= 0 && key < self.cnt$LKEY ? vectorNth(self, key, notFound) : notFound;
  });
  // A map entry's slots (light-cljs-1028): its two items, which its tail
  // holds, at indexes 0 and 1 only (`nth.entries`, `get.entries`), and a
  // vector's `conj`, `assoc` and `peek`, which give what a vector of its
  // items gives. A value of the entries' prototype that is not an entry of
  // two items in a tail, as only reflection makes, takes the operation.
  //@unit MapEntryType.nth effect: MapEntryType when: nth requires: builtinSlot, MapEntryType,
  //@ noBranch, outOfBounds
  MapEntryType.prototype.cljs$core$IIndexed$_nth$arity$2$LKEY = builtinSlot(function (self, index) {
    const tail = self.tail$LKEY;
    if (self.cnt$LKEY !== 2 || tail === undefined) return noBranch;
    return index === 0 || index === 1 ? tail[index] : outOfBounds();
  });
  MapEntryType.prototype.cljs$core$IIndexed$_nth$arity$3$LKEY = builtinSlot(function (self, index, notFound) {
    const tail = self.tail$LKEY;
    if (self.cnt$LKEY !== 2 || tail === undefined) return noBranch;
    return index === 0 || index === 1 ? tail[index] : notFound;
  });
  //@unit MapEntryType.count effect: MapEntryType when: count requires: builtinSlot, MapEntryType,
  //@ noBranch
  MapEntryType.prototype.cljs$core$ICounted$_count$arity$1$LKEY = builtinSlot(function (self) {
    if (self.cnt$LKEY !== 2 || self.tail$LKEY === undefined) return noBranch;
    return 2;
  });
  //@unit MapEntryType.peek effect: MapEntryType when: peek requires: builtinSlot, MapEntryType,
  //@ noBranch
  MapEntryType.prototype.cljs$core$IStack$_peek$arity$1$LKEY = builtinSlot(function (self) {
    const tail = self.tail$LKEY;
    if (self.cnt$LKEY !== 2 || tail === undefined) return noBranch;
    return tail[1];
  });
  //@unit MapEntryType.conjSlot effect: MapEntryType when: conj requires: builtinSlot, conjMeta,
  //@ makeVector, MapEntryType, noBranch, vectorConj
  MapEntryType.prototype.cljs$core$ICollection$_conj$arity$2$LKEY = builtinSlot(function (self, item) {
    if (self.cnt$LKEY !== 2 || self.tail$LKEY === undefined) return noBranch;
    return conjMeta(self, makeVector(null, vectorConj(self, item)));
  });
  //@unit MapEntryType.assocSlot effect: MapEntryType when: assoc requires: builtinSlot, makeVector,
  //@ MapEntryType, noBranch, outOfBounds, preserveMeta, vectorAssoc, vectorConj
  MapEntryType.prototype.cljs$core$IAssociative$_assoc$arity$3$LKEY = builtinSlot(function (self, key, item) {
    if (self.cnt$LKEY !== 2 || self.tail$LKEY === undefined) return noBranch;
    if (typeof key !== "number") throw new Error("Vector key must be a number");
    let store = self;
    if (key >= 0 && key < 2) store = vectorAssoc(store, key, item);
    else if (key === 2) store = vectorConj(store, item);
    else outOfBounds();
    return preserveMeta(self, makeVector(null, store));
  });
  //@unit MapEntryType.lookup effect: MapEntryType when: get requires: builtinSlot, MapEntryType,
  //@ noBranch
  MapEntryType.prototype.cljs$core$ILookup$_lookup$arity$2$LKEY = builtinSlot(function (self, key) {
    const tail = self.tail$LKEY;
    if (self.cnt$LKEY !== 2 || tail === undefined) return noBranch;
    return key === 0 || key === 1 ? tail[key] : null;
  });
  MapEntryType.prototype.cljs$core$ILookup$_lookup$arity$3$LKEY = builtinSlot(function (self, key, notFound) {
    const tail = self.tail$LKEY;
    if (self.cnt$LKEY !== 2 || tail === undefined) return noBranch;
    return key === 0 || key === 1 ? tail[key] : notFound;
  });
  // A transient vector's slots (light-cljs-1028). One that is no longer
  // editable answers as the operations do, through `transientNth` and
  // `editableTransient`: it throws, but the three-argument `nth` gives its
  // item for an index out of bounds. Each slot is complete for a transient
  // vector, as the compile-time runtime's protocol lookup finds and calls it.
  //@unit TransientVectorType.nth effect: TransientVectorType when: nth requires: builtinSlot,
  //@ missing, noBranch, transientNth, TransientVectorType, vectorNth
  TransientVectorType.prototype.cljs$core$IIndexed$_nth$arity$2$LKEY = builtinSlot(function (self, index) {
    const editable = self.editable$LKEY;
    if (editable === true) return vectorNth(self, index, missing);
    return editable === undefined ? noBranch : transientNth(self, index, missing);
  });
  TransientVectorType.prototype.cljs$core$IIndexed$_nth$arity$3$LKEY = builtinSlot(function (self, index, notFound) {
    const editable = self.editable$LKEY;
    if (editable === true) return vectorNth(self, index, notFound);
    return editable === undefined ? noBranch : transientNth(self, index, notFound);
  });
  //@unit TransientVectorType.count effect: TransientVectorType when: count requires: builtinSlot,
  //@ editableTransient, noBranch, TransientVectorType
  TransientVectorType.prototype.cljs$core$ICounted$_count$arity$1$LKEY = builtinSlot(function (self) {
    const editable = self.editable$LKEY;
    if (editable !== true) { if (editable === undefined) return noBranch; editableTransient(self, "count"); }
    return self.cnt$LKEY;
  });
  //@unit TransientVectorType.lookup effect: TransientVectorType when: get requires: builtinSlot,
  //@ editableTransient, noBranch, TransientVectorType, vectorNth
  // A key that is no number is no index; a number is `nth` with the item
  // for a missing key, as `transientLookup` takes it.
  TransientVectorType.prototype.cljs$core$ILookup$_lookup$arity$2$LKEY = builtinSlot(function (self, key) {
    const editable = self.editable$LKEY;
    if (editable !== true) { if (editable === undefined) return noBranch; editableTransient(self, "lookup"); }
    return typeof key === "number" ? vectorNth(self, key, null) : null;
  });
  TransientVectorType.prototype.cljs$core$ILookup$_lookup$arity$3$LKEY = builtinSlot(function (self, key, notFound) {
    const editable = self.editable$LKEY;
    if (editable !== true) { if (editable === undefined) return noBranch; editableTransient(self, "lookup"); }
    return typeof key === "number" ? vectorNth(self, key, notFound) : notFound;
  });

  // Slot-first dispatch (docs/value-layout.md, section 3): an operation
  // given a value of a converted runtime class, which its keyed kind tells,
  // calls the value's keyed slot before any layer of its own; any other
  // value, and a converted value whose slot is gone or holds nil or false
  // (which the older layers' lookups take for no method), takes the
  // operation as before. Each wrapper is the operation's outermost, so it follows the
  // operation's other wrappers here. Program types keep their path until
  // their recognition moves to the keyed kind (light-cljs-1013).
  //@unit nth.slots effect: arrayChunk, MapEntryType, PersistentVectorValue, SubvecValue,
  //@ TransientVectorType when: nth requires: noBranch, nth, slotFirst
  nth = (operation => Object.defineProperty(function (value, index, notFound) {
    if (slotFirst.on && value != null && value.cljs$lang$kind$$LKEY !== undefined && typeof index === "number") {
      const arity = arguments.length;
      if (arity === 2) { const method = value.cljs$core$IIndexed$_nth$arity$2$LKEY; if (method) { const result = method.call(value, value, index); if (result !== noBranch) return result; } }
      else if (arity === 3) { const method = value.cljs$core$IIndexed$_nth$arity$3$LKEY; if (method) { const result = method.call(value, value, index, notFound); if (result !== noBranch) return result; } }
    }
    return operation.apply(null, arguments);
  }, "name", {value: operation.name}))(nth);
  //@unit count.slots effect: arrayChunk, EmptyListValue, IndexedSeqValue, IntegerRangeValue,
  //@ ListValue, MapEntryType, PersistentArrayMapValue, PersistentHashMapValue,
  //@ PersistentVectorValue, SubvecValue, TransientVectorType, treeMapConstructor when: count
  //@ requires: count, noBranch, slotFirst
  count = (operation => Object.defineProperty(function (value) {
    if (slotFirst.on && value != null && value.cljs$lang$kind$$LKEY !== undefined && arguments.length === 1) {
      const method = value.cljs$core$ICounted$_count$arity$1$LKEY;
      if (method) { const result = method.call(value, value); if (result !== noBranch) return result; }
    }
    return operation.apply(null, arguments);
  }, "name", {value: operation.name}))(count);
  //@unit peek.slots effect: MapEntryType, PersistentVectorValue, SubvecValue when: peek requires: noBranch,
  //@ peek, slotFirst
  peek = (operation => Object.defineProperty(function (value) {
    if (slotFirst.on && value != null && value.cljs$lang$kind$$LKEY !== undefined) {
      const method = value.cljs$core$IStack$_peek$arity$1$LKEY;
      if (method) { const result = method.call(value, value); if (result !== noBranch) return result; }
    }
    return operation.apply(null, arguments);
  }, "name", {value: operation.name}))(peek);
  //@unit conj.slots effect: MapEntryType, PersistentArrayMapValue, PersistentHashMapValue,
  //@ PersistentVectorValue, SubvecValue, treeMapConstructor when: conj requires: conj, noBranch,
  //@ slotFirst
  conj = (operation => Object.defineProperty(function (coll, item) {
    if (slotFirst.on && coll != null && coll.cljs$lang$kind$$LKEY !== undefined && arguments.length === 2) {
      const method = coll.cljs$core$ICollection$_conj$arity$2$LKEY;
      if (method) { const result = method.call(coll, coll, item); if (result !== noBranch) return result; }
    }
    return operation.apply(null, arguments);
  }, "name", {value: operation.name}))(conj);
  //@unit assoc.slots effect: MapEntryType, PersistentArrayMapValue, PersistentHashMapValue,
  //@ PersistentVectorValue, SubvecValue, treeMapConstructor when: assoc requires: assoc, noBranch,
  //@ slotFirst
  assoc = (operation => Object.defineProperty(function (value, key, item) {
    if (slotFirst.on && value != null && value.cljs$lang$kind$$LKEY !== undefined && arguments.length === 3) {
      const method = value.cljs$core$IAssociative$_assoc$arity$3$LKEY;
      if (method) { const result = method.call(value, value, key, item); if (result !== noBranch) return result; }
    }
    return operation.apply(null, arguments);
  }, "name", {value: operation.name}))(assoc);
  //@unit get.slots effect: MapEntryType, PersistentArrayMapValue, PersistentHashMapValue,
  //@ PersistentVectorValue, SubvecValue, TransientVectorType, treeMapConstructor when: get
  //@ requires: get, noBranch, slotFirst
  get = (operation => Object.defineProperty(function (value, key, notFound) {
    if (slotFirst.on && value != null && value.cljs$lang$kind$$LKEY !== undefined) {
      const arity = arguments.length;
      if (arity === 2) { const method = value.cljs$core$ILookup$_lookup$arity$2$LKEY; if (method) { const result = method.call(value, value, key); if (result !== noBranch) return result; } }
      else if (arity === 3) { const method = value.cljs$core$ILookup$_lookup$arity$3$LKEY; if (method) { const result = method.call(value, value, key, notFound); if (result !== noBranch) return result; } }
    }
    return operation.apply(null, arguments);
  }, "name", {value: operation.name}))(get);

  //@unit dissoc.slots effect: PersistentArrayMapValue, PersistentHashMapValue, treeMapConstructor
  //@ when: dissoc requires: dissoc, noBranch, slotFirst
  dissoc = (operation => Object.defineProperty(function (value, key) {
    if (slotFirst.on && value != null && value.cljs$lang$kind$$LKEY !== undefined && arguments.length === 2) {
      const method = value.cljs$core$IMap$_dissoc$arity$2$LKEY;
      if (method) { const result = method.call(value, value, key); if (result !== noBranch) return result; }
    }
    return operation.apply(null, arguments);
  }, "name", {value: operation.name}))(dissoc);
  //@unit contains.slots effect: PersistentArrayMapValue, PersistentHashMapValue, treeMapConstructor
  //@ when: contains requires: contains, noBranch, slotFirst
  contains = (operation => Object.defineProperty(function (value, key) {
    if (slotFirst.on && value != null && value.cljs$lang$kind$$LKEY !== undefined && arguments.length === 2) {
      const method = value.cljs$core$IAssociative$_contains_key_QMARK_$arity$2$LKEY;
      if (method) { const result = method.call(value, value, key); if (result !== noBranch) return result; }
    }
    return operation.apply(null, arguments);
  }, "name", {value: operation.name}))(contains);

  // A map's protocol methods are slots of its prototype too
  // (light-cljs-1012), complete for a map of the class layout (`mapOf`):
  // each reads the map's own fields. A map a public constructor built,
  // whose slots can give another count (`constructorSlots`), and a value
  // made by reflection, which has no `entries`, answer `noBranch` where the
  // operation would answer otherwise, and take the operation. The three map
  // types share the methods below, each slot a unit of its type.
  //@unit arrayMapLookup requires: arrayIndex, noBranch
  // `get` of an array map: its keys searched as `arrayIndex` searches
  // them, as `lookup` does.
  function arrayMapLookup(self, key, notFound) {
    const arr = self.arr$LKEY;
    if (arr === undefined) return noBranch;
    const index = arrayIndex(arr, key);
    return index < 0 ? notFound : arr[index + 1];
  }
  //@unit hashMapLookup requires: hash, hashNodeFind, noBranch
  // `get` of a hash map: the entry of its fields' trie, as `trieEntry`
  // finds it: its `nil` entry, or the key's in its `root`.
  function hashMapLookup(self, key, notFound) {
    const cnt = self.cnt$LKEY;
    if (cnt === undefined && !("cnt$LKEY" in self)) return noBranch;
    let entry;
    if (key == null) entry = self.nil$LKEY;
    else { const root = self.root$LKEY; entry = root ? hashNodeFind(root, 0, hash(key), key) : null; }
    return entry ? entry.value : notFound;
  }
  //@unit hashMapCount requires: noBranch
  // `count` of a hash map: its `cnt`, a public constructor's count too
  // (light-cljs-1036).
  function hashMapCount(self) {
    const cnt = self.cnt$LKEY;
    return cnt === undefined && !("cnt$LKEY" in self) ? noBranch : cnt;
  }
  //@unit treeMapLookup requires: noBranch, treeEntryAt, treeVal
  // `get` of a sorted map: the value of the node that holds the key
  // (`treeEntryAt`), as upstream's.
  function treeMapLookup(self, key, notFound) {
    if (self.cnt$LKEY === undefined && !("cnt$LKEY" in self)) return noBranch;
    const node = treeEntryAt(self, key);
    return node === null ? notFound : node.red$LKEY !== undefined ? node.tail$LKEY[1] : treeVal(node);
  }
  //@unit treeMapCount requires: noBranch
  // `count` of a sorted map: its `cnt`, a public constructor's count too
  // (light-cljs-1035). Each map slot is complete for every map of its
  // class, as the compile-time protocol lookup calls it (light-cljs-1028);
  // only a value made by reflection answers `noBranch`.
  function treeMapCount(self) {
    const cnt = self.cnt$LKEY;
    return cnt === undefined && !("cnt$LKEY" in self) ? noBranch : cnt;
  }
  //@unit arrayMapCount requires: noBranch
  // `count` of an array map: its `cnt`, a public constructor's count too.
  function arrayMapCount(self) {
    const cnt = self.cnt$LKEY;
    return self.arr$LKEY === undefined ? noBranch : cnt;
  }
  //@unit mapAssocEntries requires: arrayMapEntries, copyEntries, isArrayMap, kindHooks, maps,
  //@ missing, noBranch, publicCollectionSlots, putCollectionEntry, updatedMap
  // `assoc` of a sorted map, or of an array map that a public constructor
  // built or that a ninth key promotes, as `assoc` builds it: a public
  // constructor's array map of eight or more keys promoted to a hash map
  // (`assocArrayMapSlots`), else a copy of its entries (an array map's of
  // its pairs, `arrayMapEntries`), `putCollectionEntry` and `updatedMap`.
  function mapAssocEntries(self, key, item) {
    const entries = isArrayMap(self) ? arrayMapEntries(self.arr$LKEY) : maps.get(self);
    if (entries === undefined) return noBranch;
    const slots = publicCollectionSlots.get(self);
    if (slots?.arr && slots.cnt >= 8) {
      const promoted = kindHooks.assocArrayMapSlots(self, slots, key, item, []);
      if (promoted !== missing) return promoted;
    }
    const updated = copyEntries(entries);
    putCollectionEntry(self, updated, key, item);
    return updatedMap(self, updated);
  }
  //@unit arrayMapAssoc requires: arrayIndex, ArrayMapCell, mapAssocEntries, noBranch, preserveMeta,
  //@ publicCollectionSlots
  // `assoc` of an array map that stays one (a key it has, or fewer than
  // eight): one copy of its keys and values, the key it has kept, as
  // `mapAssocEntries` would build it; a ninth key promotes it there.
  function arrayMapAssoc(self, key, item) {
    const arr = self.arr$LKEY;
    if (arr === undefined) return noBranch;
    if (publicCollectionSlots.has(self)) return mapAssocEntries(self, key, item);
    const index = arrayIndex(arr, key), n = arr.length;
    if (index < 0 && n >= 16) return mapAssocEntries(self, key, item);
    const copy = arr.slice();
    if (index >= 0) copy[index + 1] = item; else copy.push(key, item);
    return preserveMeta(self, new ArrayMapCell(copy.length >> 1, copy));
  }
  //@unit hashMapAssoc requires: hash, HashMapCell, hashNodeAssoc, noBranch, preserveMeta
  // `assoc` of a hash map: a hash map of its fields with the new entry along
  // one path of its trie, as `putHashEntry` and `updatedMap` would build it;
  // a public constructor's map too, whose count is its `cnt`
  // (light-cljs-1036).
  function hashMapAssoc(self, key, item) {
    const cnt = self.cnt$LKEY;
    if (cnt === undefined && !("cnt$LKEY" in self)) return noBranch;
    let root = self.root$LKEY, nil = self.nil$LKEY, n = cnt;
    if (key == null) { n += !nil; nil = {key: nil ? nil.key : key, value: item, hash: 0}; }
    else {
      const keyHash = hash(key), result = {};
      root = hashNodeAssoc(root, 0, keyHash, {key, value: item, hash: keyHash}, null, result);
      n += !result.old;
    }
    return preserveMeta(self, new HashMapCell(n, root, nil, null));
  }
  //@unit arrayMapDissoc requires: arrayIndex, ArrayMapCell, arrayMapEntries, dissocEntries,
  //@ equalPair, keywordOf, methodKeys, noBranch, preserveMeta, protocolHooks, publicCollectionSlots
  // `dissoc` of one key from an array map, as `dissoc` removes it: the
  // pairs but those whose key equals it, as upstream's copy loop compares
  // each key with `=`, where a positional array keeps room for the removed
  // pair; a public constructor's map, as `dissoc` removes it
  // (`dissocEntries`). A keyword without a program's `IEquiv`, whose `=`
  // runs no program code, takes `arrayIndex`'s search instead of `=` per
  // key (light-cljs-1034).
  function arrayMapDissoc(self, key) {
    const arr = self.arr$LKEY;
    if (arr === undefined) return noBranch;
    if (publicCollectionSlots.has(self)) return dissocEntries(self, arrayMapEntries(arr), [key]);
    const n = arr.length;
    let index = arrayIndex(arr, key), retained;
    if (index < 0) retained = arr.slice();
    else if (n === 2) retained = [];
    else {
      retained = [];
      const builtin = protocolHooks.builtin, own = protocolHooks.keyMethod, k = methodKeys[18];
      if (keywordOf(key) !== undefined && !(builtin && (own ? own(key, k, 2) ?? protocolHooks.externalMethod?.(key, k, 2) : builtin(key, k, 2)))) {
        // The pairs before each equal key, which only a positional array
        // repeats, then the rest.
        let from = 0;
        for (; index >= 0; from = index + 2, index = arrayIndex(arr, key, from)) for (let i = from; i < index; i++) retained.push(arr[i]);
        for (let i = from; i < n; i++) retained.push(arr[i]);
      } else for (let i = 0; i < n; i += 2) if (!equalPair(key, arr[i])) retained.push(arr[i], arr[i + 1]);
      while (retained.length < n - 2) retained.push(undefined, undefined);
    }
    return preserveMeta(self, new ArrayMapCell(retained.length >> 1, retained));
  }
  //@unit hashMapDissoc requires: hash, HashMapCell, hashNodeFind, hashNodeRemove, noBranch,
  //@ preserveMeta
  // `dissoc` of one key from a hash map: a hash map of its fields without
  // the key, as `trieDissoc` removes it; a public constructor's map too,
  // whose count is its `cnt` (light-cljs-1036).
  function hashMapDissoc(self, key) {
    const cnt = self.cnt$LKEY;
    if (cnt === undefined && !("cnt$LKEY" in self)) return noBranch;
    const root = self.root$LKEY, nil = self.nil$LKEY;
    const entry = key == null ? nil : root ? hashNodeFind(root, 0, hash(key), key) : null;
    if (!entry) return preserveMeta(self, new HashMapCell(cnt, root, nil, self.lit$LKEY));
    return preserveMeta(self, key == null ? new HashMapCell(cnt - 1, root, null, null)
      : new HashMapCell(cnt - 1, hashNodeRemove(root, 0, entry.hash, entry, null), nil, null));
  }
  //@unit mapContains requires: get, mapOf, missing, noBranch
  // `contains?` of a map of the class layout: `get` of the key, through the
  // operation's slot-first wrapper, so that an `extend-type` of `ILookup`
  // answers as before.
  function mapContains(self, key) {
    return mapOf(self) === undefined ? noBranch : get(self, key, missing) !== missing;
  }
  //@unit mapConjSlot requires: mapConj, mapOf, noBranch, preserveMeta
  // `conj` of a map of the class layout, as `conj.maps` builds it.
  function mapConjSlot(self, item) {
    return mapOf(self) === undefined ? noBranch : preserveMeta(self, mapConj(self, item));
  }
  //@unit PersistentArrayMapValue.lookup effect: PersistentArrayMapValue when: get requires: arrayMapLookup,
  //@ builtinSlot, PersistentArrayMapValue
  PersistentArrayMapValue.prototype.cljs$core$ILookup$_lookup$arity$2$LKEY = builtinSlot(function (self, key) { return arrayMapLookup(self, key, null); });
  PersistentArrayMapValue.prototype.cljs$core$ILookup$_lookup$arity$3$LKEY = builtinSlot(function (self, key, notFound) { return arrayMapLookup(self, key, notFound); });
  //@unit PersistentHashMapValue.lookup effect: PersistentHashMapValue when: get requires: builtinSlot,
  //@ hashMapLookup, PersistentHashMapValue
  PersistentHashMapValue.prototype.cljs$core$ILookup$_lookup$arity$2$LKEY = builtinSlot(function (self, key) { return hashMapLookup(self, key, null); });
  PersistentHashMapValue.prototype.cljs$core$ILookup$_lookup$arity$3$LKEY = builtinSlot(function (self, key, notFound) { return hashMapLookup(self, key, notFound); });
  //@unit treeMapConstructor.lookupSlot effect: treeMapConstructor when: get requires: builtinSlot,
  //@ treeMapConstructor, treeMapLookup
  treeMapConstructor.prototype.cljs$core$ILookup$_lookup$arity$2$LKEY = builtinSlot(function (self, key) { return treeMapLookup(self, key, null); });
  treeMapConstructor.prototype.cljs$core$ILookup$_lookup$arity$3$LKEY = builtinSlot(function (self, key, notFound) { return treeMapLookup(self, key, notFound); });
  //@unit PersistentArrayMapValue.count effect: PersistentArrayMapValue when: count requires: arrayMapCount,
  //@ builtinSlot, PersistentArrayMapValue
  PersistentArrayMapValue.prototype.cljs$core$ICounted$_count$arity$1$LKEY = builtinSlot(function (self) { return arrayMapCount(self); });
  //@unit PersistentHashMapValue.count effect: PersistentHashMapValue when: count requires: builtinSlot,
  //@ hashMapCount, PersistentHashMapValue
  PersistentHashMapValue.prototype.cljs$core$ICounted$_count$arity$1$LKEY = builtinSlot(function (self) { return hashMapCount(self); });
  //@unit treeMapConstructor.countSlot effect: treeMapConstructor when: count requires: builtinSlot,
  //@ treeMapConstructor, treeMapCount
  treeMapConstructor.prototype.cljs$core$ICounted$_count$arity$1$LKEY = builtinSlot(function (self) { return treeMapCount(self); });
  //@unit PersistentArrayMapValue.assocSlot effect: PersistentArrayMapValue when: assoc requires: arrayMapAssoc,
  //@ builtinSlot, kindHooks, PersistentArrayMapValue
  PersistentArrayMapValue.prototype.cljs$core$IAssociative$_assoc$arity$3$LKEY = builtinSlot(function (self, key, item) { return arrayMapAssoc(self, key, item); });
  // The operation's own path of an array map (`assoc`, light-cljs-1036).
  kindHooks.assocArrayMap = arrayMapAssoc;
  //@unit PersistentHashMapValue.assocSlot effect: PersistentHashMapValue when: assoc requires: builtinSlot,
  //@ hashMapAssoc, kindHooks, PersistentHashMapValue
  PersistentHashMapValue.prototype.cljs$core$IAssociative$_assoc$arity$3$LKEY = builtinSlot(function (self, key, item) { return hashMapAssoc(self, key, item); });
  kindHooks.assocHashMap = hashMapAssoc;
  //@unit treeMapConstructor.assocSlot effect: treeMapConstructor when: assoc requires: builtinSlot,
  //@ kindHooks, treeMapAssoc, treeMapConstructor
  treeMapConstructor.prototype.cljs$core$IAssociative$_assoc$arity$3$LKEY = builtinSlot(function (self, key, item) { return treeMapAssoc(self, key, item); });
  kindHooks.assocTreeMap = treeMapAssoc;
  //@unit PersistentArrayMapValue.dissocSlot effect: PersistentArrayMapValue when: dissoc requires: arrayMapDissoc,
  //@ builtinSlot, kindHooks, PersistentArrayMapValue
  PersistentArrayMapValue.prototype.cljs$core$IMap$_dissoc$arity$2$LKEY = builtinSlot(function (self, key) { return arrayMapDissoc(self, key); });
  // The operation's own path of an array map (`dissoc`, light-cljs-1036).
  kindHooks.dissocArrayMap = arrayMapDissoc;
  //@unit PersistentHashMapValue.dissocSlot effect: PersistentHashMapValue when: dissoc requires: builtinSlot,
  //@ hashMapDissoc, kindHooks, PersistentHashMapValue
  PersistentHashMapValue.prototype.cljs$core$IMap$_dissoc$arity$2$LKEY = builtinSlot(function (self, key) { return hashMapDissoc(self, key); });
  kindHooks.dissocHashMap = hashMapDissoc;
  //@unit treeMapConstructor.dissocSlot effect: treeMapConstructor when: dissoc requires: builtinSlot,
  //@ kindHooks, treeMapConstructor, treeMapDissoc
  treeMapConstructor.prototype.cljs$core$IMap$_dissoc$arity$2$LKEY = builtinSlot(function (self, key) { return treeMapDissoc(self, key); });
  kindHooks.dissocTreeMap = treeMapDissoc;
  //@unit PersistentArrayMapValue.containsSlot effect: PersistentArrayMapValue when: contains
  //@ requires: builtinSlot, mapContains, PersistentArrayMapValue
  PersistentArrayMapValue.prototype.cljs$core$IAssociative$_contains_key_QMARK_$arity$2$LKEY = builtinSlot(function (self, key) { return mapContains(self, key); });
  //@unit PersistentHashMapValue.containsSlot effect: PersistentHashMapValue when: contains
  //@ requires: builtinSlot, mapContains, PersistentHashMapValue
  PersistentHashMapValue.prototype.cljs$core$IAssociative$_contains_key_QMARK_$arity$2$LKEY = builtinSlot(function (self, key) { return mapContains(self, key); });
  //@unit treeMapConstructor.containsSlot effect: treeMapConstructor when: contains requires: builtinSlot,
  //@ mapContains, treeMapConstructor
  treeMapConstructor.prototype.cljs$core$IAssociative$_contains_key_QMARK_$arity$2$LKEY = builtinSlot(function (self, key) { return mapContains(self, key); });
  //@unit PersistentArrayMapValue.conjSlot effect: PersistentArrayMapValue when: conj requires: builtinSlot,
  //@ mapConjSlot, PersistentArrayMapValue
  PersistentArrayMapValue.prototype.cljs$core$ICollection$_conj$arity$2$LKEY = builtinSlot(function (self, item) { return mapConjSlot(self, item); });
  //@unit PersistentHashMapValue.conjSlot effect: PersistentHashMapValue when: conj requires: builtinSlot,
  //@ mapConjSlot, PersistentHashMapValue
  PersistentHashMapValue.prototype.cljs$core$ICollection$_conj$arity$2$LKEY = builtinSlot(function (self, item) { return mapConjSlot(self, item); });
  //@unit treeMapConstructor.conjSlot effect: treeMapConstructor when: conj requires: builtinSlot,
  //@ mapConjSlot, treeMapConstructor
  treeMapConstructor.prototype.cljs$core$ICollection$_conj$arity$2$LKEY = builtinSlot(function (self, item) { return mapConjSlot(self, item); });

  // The seqs over an array or a string (`viewOf`) answer `first`, `rest`,
  // `next` and `seq`, and an indexed seq `count`, from slots of their
  // prototypes, as a vector answers its operations (light-cljs-1010): a step
  // builds one object, of the seq's own class. Each slot is complete for a
  // view of its class and reads only the value's own fields; where today's
  // operation would take a further operation (the `rest` and `seq` that
  // `next` takes of a chunked seq), the slot takes it too, through the
  // operation's wrappers, so that a program's `extend-type` of that
  // operation's protocol answers as before.
  //@unit IndexedSeqValue.first effect: IndexedSeqValue when: first requires: builtinSlot,
  //@ IndexedSeqValue, noBranch
  IndexedSeqValue.prototype.cljs$core$ISeq$_first$arity$1$LKEY = builtinSlot(function (self) {
    const arr = self.arr$LKEY;
    if (arr === undefined) return noBranch;
    const i = self.i$LKEY;
    return i < arr.length ? arr[i] : null;
  });
  //@unit IndexedSeqValue.rest effect: IndexedSeqValue when: rest requires: builtinSlot, emptyList,
  //@ IndexedSeqCell, IndexedSeqValue, noBranch, restSeq
  IndexedSeqValue.prototype.cljs$core$ISeq$_rest$arity$1$LKEY = builtinSlot(function (self) {
    const arr = self.arr$LKEY;
    if (arr === undefined) return noBranch;
    if (self.view$LKEY !== "indexed") return restSeq(self);
    const i = self.i$LKEY + 1;
    return i < arr.length ? new IndexedSeqCell(arr, i, self.indexed$LKEY, "indexed") : emptyList;
  });
  //@unit IndexedSeqValue.next effect: IndexedSeqValue when: next requires: builtinSlot,
  //@ IndexedSeqCell, IndexedSeqValue, noBranch, rest, seq
  // The flat kinds advance directly, as `next` advances them; the seqs of
  // queues and sorted maps take the seq of their rest.
  IndexedSeqValue.prototype.cljs$core$INext$_next$arity$1$LKEY = builtinSlot(function (self) {
    const arr = self.arr$LKEY;
    if (arr === undefined) return noBranch;
    const kind = self.view$LKEY;
    if (kind !== "indexed" && kind !== "set" && kind !== "hash-map" && kind !== "array-map") return seq(rest(self));
    const i = self.i$LKEY + 1;
    return i < arr.length ? new IndexedSeqCell(arr, i, self.indexed$LKEY, kind) : null;
  });
  //@unit IndexedSeqValue.seq effect: IndexedSeqValue when: seq requires: builtinSlot,
  //@ IndexedSeqValue, noBranch
  IndexedSeqValue.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = builtinSlot(function (self) {
    const arr = self.arr$LKEY;
    if (arr === undefined) return noBranch;
    return self.i$LKEY < arr.length ? self : null;
  });
  //@unit IndexedSeqValue.count effect: IndexedSeqValue when: count requires: builtinSlot,
  //@ IndexedSeqValue, noBranch
  IndexedSeqValue.prototype.cljs$core$ICounted$_count$arity$1$LKEY = builtinSlot(function (self) {
    const arr = self.arr$LKEY;
    if (arr === undefined) return noBranch;
    return Math.max(0, arr.length - self.i$LKEY);
  });
  //@unit ChunkedSeqValue.first effect: ChunkedSeqValue when: first requires: builtinSlot,
  //@ ChunkedSeqValue, noBranch
  ChunkedSeqValue.prototype.cljs$core$ISeq$_first$arity$1$LKEY = builtinSlot(function (self) {
    const arr = self.arr$LKEY;
    if (arr === undefined) return noBranch;
    const i = self.i$LKEY;
    return i < arr.length ? arr[i] : null;
  });
  //@unit ChunkedSeqValue.rest effect: ChunkedSeqValue when: rest requires: builtinSlot,
  //@ ChunkedSeqCell, ChunkedSeqValue, chunkedVectorRest, emptyList, noBranch, restSeq
  ChunkedSeqValue.prototype.cljs$core$ISeq$_rest$arity$1$LKEY = builtinSlot(function (self) {
    const arr = self.arr$LKEY;
    if (arr === undefined) return noBranch;
    const vec = self.vec$LKEY, i = self.i$LKEY + 1;
    if (!vec) return restSeq(self);
    if (i > arr.length) return emptyList;
    return i < arr.length ? new ChunkedSeqCell(arr, i, vec, self.base$LKEY) : chunkedVectorRest(self);
  });
  //@unit ChunkedSeqValue.next effect: ChunkedSeqValue when: next requires: builtinSlot,
  //@ ChunkedSeqValue, noBranch, rest, seq
  // The seq of its rest, as `next` takes it.
  ChunkedSeqValue.prototype.cljs$core$INext$_next$arity$1$LKEY = builtinSlot(function (self) {
    if (self.arr$LKEY === undefined) return noBranch;
    return seq(rest(self));
  });
  //@unit ChunkedSeqValue.seq effect: ChunkedSeqValue when: seq requires: builtinSlot,
  //@ ChunkedSeqValue, noBranch
  ChunkedSeqValue.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = builtinSlot(function (self) {
    const arr = self.arr$LKEY;
    if (arr === undefined) return noBranch;
    return self.i$LKEY < arr.length ? self : null;
  });
  //@unit PersistentVectorValue.seq effect: PersistentVectorValue when: seq requires: builtinSlot,
  //@ kindHooks, noBranch, PersistentVectorValue
  PersistentVectorValue.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = builtinSlot(function (self) {
    const store = self;
    if (store.cnt$LKEY === undefined && !("cnt$LKEY" in store)) return noBranch;
    return kindHooks.seqVector(self, store);
  });
  //@unit SubvecValue.seq effect: SubvecValue when: seq requires: builtinSlot, kindHooks, noBranch,
  //@ SubvecValue
  SubvecValue.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = builtinSlot(function (self) {
    if (self.cnt$LKEY === undefined && !("cnt$LKEY" in self)) return noBranch;
    return kindHooks.seqVector(self, self);
  });
  //@unit MapEntryType.seq effect: MapEntryType when: seq requires: builtinSlot, kindHooks,
  //@ MapEntryType, noBranch
  MapEntryType.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = builtinSlot(function (self) {
    if (self.cnt$LKEY !== 2 || self.tail$LKEY === undefined) return noBranch;
    return kindHooks.seqVector(self, self);
  });
  // A lazy seq, a chunked cons and a chunk answer their operations from
  // slots of their prototypes too (light-cljs-1029), so that a step of a
  // lazy pipeline builds one object of its class and calls no older layer.
  // A lazy seq's slots realize it through `seq`, which an `extend-type` of
  // `ISeqable` reaches, then take the operation of what it realized, as the
  // operations do; a chunked cons's `next` is the seq of its rest, and its
  // `first` and `rest` read its chunk through `nth`, `count` and
  // `-drop-first` where protocol implementations exist, through their
  // wrappers.
  //@unit LazySeq.seqSlot effect: LazySeq when: seq requires: builtinSlot, lazyRealized, LazySeq,
  //@ noBranch, seq
  LazySeq.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = builtinSlot(function (self) {
    if (!("fn$LKEY" in self)) return noBranch;
    return seq(lazyRealized(self));
  });
  //@unit LazySeq.firstSlot effect: LazySeq when: first requires: builtinSlot, first, LazySeq,
  //@ noBranch, seq
  LazySeq.prototype.cljs$core$ISeq$_first$arity$1$LKEY = builtinSlot(function (self) {
    if (!("fn$LKEY" in self)) return noBranch;
    seq(self);
    return first(self.s$LKEY);
  });
  //@unit LazySeq.restSlot effect: LazySeq when: rest requires: builtinSlot, LazySeq, noBranch,
  //@ rest, seq
  LazySeq.prototype.cljs$core$ISeq$_rest$arity$1$LKEY = builtinSlot(function (self) {
    if (!("fn$LKEY" in self)) return noBranch;
    seq(self);
    return rest(self.s$LKEY);
  });
  //@unit LazySeq.nextSlot effect: LazySeq when: next requires: builtinSlot, LazySeq, next,
  //@ noBranch, seq
  LazySeq.prototype.cljs$core$INext$_next$arity$1$LKEY = builtinSlot(function (self) {
    if (!("fn$LKEY" in self)) return noBranch;
    seq(self);
    return next(self.s$LKEY);
  });
  //@unit ChunkedCons.first effect: ChunkedCons when: first requires: builtinSlot, ChunkedCons,
  //@ chunkedConsFirst, noBranch
  ChunkedCons.prototype.cljs$core$ISeq$_first$arity$1$LKEY = builtinSlot(function (self) {
    if (!("chunk$LKEY" in self)) return noBranch;
    return chunkedConsFirst(self);
  });
  //@unit ChunkedCons.rest effect: ChunkedCons when: rest requires: builtinSlot, ChunkedCons,
  //@ chunkedConsRest, noBranch
  ChunkedCons.prototype.cljs$core$ISeq$_rest$arity$1$LKEY = builtinSlot(function (self) {
    if (!("chunk$LKEY" in self)) return noBranch;
    return chunkedConsRest(self);
  });
  //@unit ChunkedCons.next effect: ChunkedCons when: next requires: builtinSlot, ChunkedCons,
  //@ noBranch, rest, seq
  ChunkedCons.prototype.cljs$core$INext$_next$arity$1$LKEY = builtinSlot(function (self) {
    if (!("chunk$LKEY" in self)) return noBranch;
    return seq(rest(self));
  });
  //@unit ChunkedCons.seq effect: ChunkedCons when: seq requires: builtinSlot, ChunkedCons, noBranch
  ChunkedCons.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = builtinSlot(function (self) {
    return !("chunk$LKEY" in self) ? noBranch : self;
  });
  //@unit ChunkedCons.chunkedFirst effect: ChunkedCons when: chunkFirst requires: builtinSlot,
  //@ ChunkedCons, noBranch
  ChunkedCons.prototype.cljs$core$IChunkedSeq$_chunked_first$arity$1$LKEY = builtinSlot(function (self) {
    const chunk = self.chunk$LKEY;
    return chunk === undefined ? noBranch : chunk;
  });
  //@unit ChunkedCons.chunkedRest effect: ChunkedCons when: chunkRest requires: builtinSlot,
  //@ ChunkedCons, emptyList, noBranch
  ChunkedCons.prototype.cljs$core$IChunkedSeq$_chunked_rest$arity$1$LKEY = builtinSlot(function (self) {
    return !("chunk$LKEY" in self) ? noBranch : self.more$LKEY ?? emptyList;
  });
  //@unit ChunkedCons.chunkedNext effect: ChunkedCons when: chunkNext requires: builtinSlot,
  //@ ChunkedCons, noBranch
  ChunkedCons.prototype.cljs$core$IChunkedNext$_chunked_next$arity$1$LKEY = builtinSlot(function (self) {
    return !("chunk$LKEY" in self) ? noBranch : self.more$LKEY ?? null;
  });
  //@unit ChunkedSeqValue.chunkedFirst effect: ChunkedSeqValue when: chunkFirst requires: builtinSlot,
  //@ ChunkedSeqValue, noBranch, ownedChunkFirst
  // A vector's chunked seq gives its leaf from its offset as its chunk.
  ChunkedSeqValue.prototype.cljs$core$IChunkedSeq$_chunked_first$arity$1$LKEY = builtinSlot(function (self) {
    return self.arr$LKEY === undefined ? noBranch : ownedChunkFirst(self);
  });
  //@unit ChunkedSeqValue.chunkedRest effect: ChunkedSeqValue when: chunkRest requires: builtinSlot,
  //@ ChunkedSeqValue, noBranch, ownedChunkRest
  ChunkedSeqValue.prototype.cljs$core$IChunkedSeq$_chunked_rest$arity$1$LKEY = builtinSlot(function (self) {
    return self.arr$LKEY === undefined ? noBranch : ownedChunkRest(self);
  });
  //@unit ChunkedSeqValue.chunkedNext effect: ChunkedSeqValue when: chunkNext requires: builtinSlot,
  //@ ChunkedSeqValue, noBranch, ownedChunkRest, seq
  ChunkedSeqValue.prototype.cljs$core$IChunkedNext$_chunked_next$arity$1$LKEY = builtinSlot(function (self) {
    return self.arr$LKEY === undefined ? noBranch : seq(ownedChunkRest(self));
  });
  //@unit arrayChunk.countSlot effect: arrayChunk when: count requires: arrayChunk, builtinSlot,
  //@ noBranch
  arrayChunk.prototype.cljs$core$ICounted$_count$arity$1$LKEY = builtinSlot(function (self) {
    return !("arr$LKEY" in self) ? noBranch : self.end$LKEY - self.off$LKEY;
  });
  //@unit arrayChunk.nthSlot effect: arrayChunk when: nth requires: arrayChunk, builtinSlot,
  //@ chunkNth, missing, noBranch
  arrayChunk.prototype.cljs$core$IIndexed$_nth$arity$2$LKEY = builtinSlot(function (self, index) {
    return !("arr$LKEY" in self) ? noBranch : chunkNth(self, index, missing);
  });
  arrayChunk.prototype.cljs$core$IIndexed$_nth$arity$3$LKEY = builtinSlot(function (self, index, notFound) {
    return !("arr$LKEY" in self) ? noBranch : chunkNth(self, index, notFound);
  });
  //@unit arrayChunk.dropFirstSlot effect: arrayChunk when: chunkDropFirst requires: arrayChunk,
  //@ builtinSlot, dropFirstChunk, noBranch
  arrayChunk.prototype.cljs$core$IChunk$_drop_first$arity$1$LKEY = builtinSlot(function (self) {
    return !("arr$LKEY" in self) ? noBranch : dropFirstChunk(self);
  });
  // A list, a cons, a range and the other seqs of `cellOf` answer `first`,
  // `rest`, `next` and `seq`, and a list and an integer range `count`, from
  // slots of their prototypes (light-cljs-1030), as a lazy seq does: a step
  // builds one object of its class and calls no older layer. A cell's
  // `first` and `rest` are its fields or its prototype's getters; its
  // `next` is the seq of its rest, through the operations' wrappers, so an
  // `extend-type` of `ISeq` answers as before, but for a list's, which is
  // its rest, and a repeat's, which is its next step, as `next` takes them.
  // A value of a cell's prototype without the fields (one made by
  // reflection) answers `noBranch`, and the operation answers it. A sorted
  // map's seq shares its prototype with its views (`viewOf`), so its slots
  // answer both, as the operations do: a core dispatcher calls a slot for
  // either.
  //@unit cellFirstSlot requires: builtinSlot, noBranch
  const cellFirstSlot = builtinSlot(function (self) { return self.cell$LKEY === undefined ? noBranch : self.first$LKEY; });
  //@unit cellRestSlot requires: builtinSlot, emptyList, noBranch
  const cellRestSlot = builtinSlot(function (self) { return self.cell$LKEY === undefined ? noBranch : self.rest$LKEY ?? emptyList; });
  //@unit cellNextSlot requires: builtinSlot, noBranch, rest, seq
  const cellNextSlot = builtinSlot(function (self) { return self.cell$LKEY === undefined ? noBranch : seq(rest(self)); });
  //@unit cellSeqSlot requires: builtinSlot, noBranch
  const cellSeqSlot = builtinSlot(function (self) { return self.cell$LKEY === undefined ? noBranch : self; });
  //@unit Cons.firstSlot effect: Cons when: first requires: builtinSlot, Cons, noBranch
  // A cons's slots of its own, which only conses reach.
  Cons.prototype.cljs$core$ISeq$_first$arity$1$LKEY = builtinSlot(function (self) {
    return self.cell$LKEY === undefined ? noBranch : self.first$LKEY;
  });
  //@unit Cons.restSlot effect: Cons when: rest requires: builtinSlot, Cons, emptyList, noBranch
  Cons.prototype.cljs$core$ISeq$_rest$arity$1$LKEY = builtinSlot(function (self) {
    return self.cell$LKEY === undefined ? noBranch : self.rest$LKEY ?? emptyList;
  });
  //@unit Cons.nextSlot effect: Cons when: next requires: builtinSlot, Cons, noBranch, rest, seq
  Cons.prototype.cljs$core$INext$_next$arity$1$LKEY = builtinSlot(function (self) {
    return self.cell$LKEY === undefined ? noBranch : seq(rest(self));
  });
  //@unit Cons.seqSlot effect: Cons when: seq requires: builtinSlot, Cons, noBranch
  Cons.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = builtinSlot(function (self) {
    return self.cell$LKEY === undefined ? noBranch : self;
  });
  //@unit ListValue.firstSlot effect: ListValue when: first requires: builtinSlot, ListValue,
  //@ noBranch, publicCollectionSlots
  // A list of count 0 is empty unless a public constructor gave it the
  // count (`(List. nil 1 nil 0 nil)`), as the operations read it.
  ListValue.prototype.cljs$core$ISeq$_first$arity$1$LKEY = builtinSlot(function (self) {
    if (self.cell$LKEY === undefined) return noBranch;
    return self.count$LKEY === 0 && !publicCollectionSlots.has(self) ? null : self.first$LKEY;
  });
  //@unit ListValue.restSlot effect: ListValue when: rest requires: builtinSlot, emptyList,
  //@ ListValue, noBranch, publicCollectionSlots
  ListValue.prototype.cljs$core$ISeq$_rest$arity$1$LKEY = builtinSlot(function (self) {
    if (self.cell$LKEY === undefined) return noBranch;
    const count = self.count$LKEY;
    return count === 1 || count === 0 && !publicCollectionSlots.has(self) ? emptyList : self.rest$LKEY;
  });
  //@unit ListValue.nextSlot effect: ListValue when: next requires: builtinSlot, ListValue,
  //@ noBranch, publicCollectionSlots
  ListValue.prototype.cljs$core$INext$_next$arity$1$LKEY = builtinSlot(function (self) {
    if (self.cell$LKEY === undefined) return noBranch;
    const count = self.count$LKEY;
    return count === 1 || count === 0 && !publicCollectionSlots.has(self) ? null : self.rest$LKEY;
  });
  //@unit ListValue.seqSlot effect: ListValue when: seq requires: builtinSlot, ListValue, noBranch,
  //@ publicCollectionSlots
  ListValue.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = builtinSlot(function (self) {
    if (self.cell$LKEY === undefined) return noBranch;
    return self.count$LKEY === 0 && !publicCollectionSlots.has(self) ? null : self;
  });
  //@unit ListValue.countSlot effect: ListValue when: count requires: builtinSlot, ListValue,
  //@ noBranch
  ListValue.prototype.cljs$core$ICounted$_count$arity$1$LKEY = builtinSlot(function (self) {
    return self.cell$LKEY === undefined ? noBranch : self.count$LKEY;
  });
  //@unit EmptyListValue.firstSlot effect: EmptyListValue when: first requires: builtinSlot,
  //@ EmptyListValue, noBranch
  EmptyListValue.prototype.cljs$core$ISeq$_first$arity$1$LKEY = builtinSlot(function (self) {
    return self.cell$LKEY === undefined ? noBranch : null;
  });
  //@unit EmptyListValue.restSlot effect: EmptyListValue when: rest requires: builtinSlot,
  //@ emptyList, EmptyListValue, noBranch
  EmptyListValue.prototype.cljs$core$ISeq$_rest$arity$1$LKEY = builtinSlot(function (self) {
    return self.cell$LKEY === undefined ? noBranch : emptyList;
  });
  //@unit EmptyListValue.nextSlot effect: EmptyListValue when: next requires: builtinSlot,
  //@ EmptyListValue, noBranch
  EmptyListValue.prototype.cljs$core$INext$_next$arity$1$LKEY = builtinSlot(function (self) {
    return self.cell$LKEY === undefined ? noBranch : null;
  });
  //@unit EmptyListValue.seqSlot effect: EmptyListValue when: seq requires: builtinSlot,
  //@ EmptyListValue, noBranch
  EmptyListValue.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = builtinSlot(function (self) {
    return self.cell$LKEY === undefined ? noBranch : null;
  });
  //@unit EmptyListValue.countSlot effect: EmptyListValue when: count requires: builtinSlot,
  //@ EmptyListValue, noBranch
  EmptyListValue.prototype.cljs$core$ICounted$_count$arity$1$LKEY = builtinSlot(function (self) {
    return self.cell$LKEY === undefined ? noBranch : 0;
  });
  //@unit IntegerRangeValue.countSlot effect: IntegerRangeValue when: count requires: builtinSlot,
  //@ IntegerRangeValue, noBranch
  IntegerRangeValue.prototype.cljs$core$ICounted$_count$arity$1$LKEY = builtinSlot(function (self) {
    return self.cell$LKEY === undefined ? noBranch : self.count$LKEY;
  });
  //@unit RepeatValue.nextSlot effect: RepeatValue when: next requires: builtinSlot, noBranch,
  //@ RepeatValue
  // A repeat's next is its next step; one without end is its own.
  RepeatValue.prototype.cljs$core$INext$_next$arity$1$LKEY = builtinSlot(function (self) {
    if (self.cell$LKEY === undefined) return noBranch;
    const remaining = self.remaining$LKEY;
    return self.next$LKEY ?? (remaining === -1 ? self : remaining > 1 ? self.rest$LKEY : null);
  });
  //@unit IntegerRangeValue.firstSlot effect: IntegerRangeValue when: first requires: cellFirstSlot,
  //@ IntegerRangeValue
  IntegerRangeValue.prototype.cljs$core$ISeq$_first$arity$1$LKEY = cellFirstSlot;
  //@unit IntegerRangeValue.restSlot effect: IntegerRangeValue when: rest requires: cellRestSlot,
  //@ IntegerRangeValue
  IntegerRangeValue.prototype.cljs$core$ISeq$_rest$arity$1$LKEY = cellRestSlot;
  //@unit IntegerRangeValue.nextSlot effect: IntegerRangeValue when: next requires: cellNextSlot,
  //@ IntegerRangeValue
  IntegerRangeValue.prototype.cljs$core$INext$_next$arity$1$LKEY = cellNextSlot;
  //@unit IntegerRangeValue.seqSlot effect: IntegerRangeValue when: seq requires: cellSeqSlot,
  //@ IntegerRangeValue
  IntegerRangeValue.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = cellSeqSlot;
  //@unit RangeValue.firstSlot effect: RangeValue when: first requires: cellFirstSlot, RangeValue
  RangeValue.prototype.cljs$core$ISeq$_first$arity$1$LKEY = cellFirstSlot;
  //@unit RangeValue.restSlot effect: RangeValue when: rest requires: cellRestSlot, RangeValue
  RangeValue.prototype.cljs$core$ISeq$_rest$arity$1$LKEY = cellRestSlot;
  //@unit RangeValue.nextSlot effect: RangeValue when: next requires: cellNextSlot, RangeValue
  RangeValue.prototype.cljs$core$INext$_next$arity$1$LKEY = cellNextSlot;
  //@unit RangeValue.seqSlot effect: RangeValue when: seq requires: cellSeqSlot, RangeValue
  RangeValue.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = cellSeqSlot;
  //@unit IterateValue.firstSlot effect: IterateValue when: first requires: cellFirstSlot,
  //@ IterateValue
  IterateValue.prototype.cljs$core$ISeq$_first$arity$1$LKEY = cellFirstSlot;
  //@unit IterateValue.restSlot effect: IterateValue when: rest requires: cellRestSlot, IterateValue
  IterateValue.prototype.cljs$core$ISeq$_rest$arity$1$LKEY = cellRestSlot;
  //@unit IterateValue.nextSlot effect: IterateValue when: next requires: cellNextSlot, IterateValue
  IterateValue.prototype.cljs$core$INext$_next$arity$1$LKEY = cellNextSlot;
  //@unit IterateValue.seqSlot effect: IterateValue when: seq requires: cellSeqSlot, IterateValue
  IterateValue.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = cellSeqSlot;
  //@unit CycleValue.firstSlot effect: CycleValue when: first requires: cellFirstSlot, CycleValue
  CycleValue.prototype.cljs$core$ISeq$_first$arity$1$LKEY = cellFirstSlot;
  //@unit CycleValue.restSlot effect: CycleValue when: rest requires: cellRestSlot, CycleValue
  CycleValue.prototype.cljs$core$ISeq$_rest$arity$1$LKEY = cellRestSlot;
  //@unit CycleValue.nextSlot effect: CycleValue when: next requires: cellNextSlot, CycleValue
  CycleValue.prototype.cljs$core$INext$_next$arity$1$LKEY = cellNextSlot;
  //@unit CycleValue.seqSlot effect: CycleValue when: seq requires: cellSeqSlot, CycleValue
  CycleValue.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = cellSeqSlot;
  //@unit RepeatValue.firstSlot effect: RepeatValue when: first requires: cellFirstSlot, RepeatValue
  RepeatValue.prototype.cljs$core$ISeq$_first$arity$1$LKEY = cellFirstSlot;
  //@unit RepeatValue.restSlot effect: RepeatValue when: rest requires: cellRestSlot, RepeatValue
  RepeatValue.prototype.cljs$core$ISeq$_rest$arity$1$LKEY = cellRestSlot;
  //@unit RepeatValue.seqSlot effect: RepeatValue when: seq requires: cellSeqSlot, RepeatValue
  RepeatValue.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = cellSeqSlot;
  //@unit KeySeqCell.firstSlot effect: KeySeqCell when: first requires: cellFirstSlot, KeySeqCell
  KeySeqCell.prototype.cljs$core$ISeq$_first$arity$1$LKEY = cellFirstSlot;
  //@unit KeySeqCell.restSlot effect: KeySeqCell when: rest requires: cellRestSlot, KeySeqCell
  KeySeqCell.prototype.cljs$core$ISeq$_rest$arity$1$LKEY = cellRestSlot;
  //@unit KeySeqCell.nextSlot effect: KeySeqCell when: next requires: cellNextSlot, KeySeqCell
  KeySeqCell.prototype.cljs$core$INext$_next$arity$1$LKEY = cellNextSlot;
  //@unit KeySeqCell.seqSlot effect: KeySeqCell when: seq requires: cellSeqSlot, KeySeqCell
  KeySeqCell.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = cellSeqSlot;
  //@unit ReSeqCell.firstSlot effect: ReSeqCell when: first requires: cellFirstSlot, ReSeqCell
  ReSeqCell.prototype.cljs$core$ISeq$_first$arity$1$LKEY = cellFirstSlot;
  //@unit ReSeqCell.restSlot effect: ReSeqCell when: rest requires: cellRestSlot, ReSeqCell
  ReSeqCell.prototype.cljs$core$ISeq$_rest$arity$1$LKEY = cellRestSlot;
  //@unit ReSeqCell.nextSlot effect: ReSeqCell when: next requires: cellNextSlot, ReSeqCell
  ReSeqCell.prototype.cljs$core$INext$_next$arity$1$LKEY = cellNextSlot;
  //@unit ReSeqCell.seqSlot effect: ReSeqCell when: seq requires: cellSeqSlot, ReSeqCell
  ReSeqCell.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = cellSeqSlot;
  //@unit treeMapSeqConstructor.firstSlot effect: treeMapSeqConstructor when: first requires: builtinSlot,
  //@ noBranch, treeMapSeqConstructor
  // A sorted map's seq is a cell of its stack of nodes (light-cljs-1035),
  // whatever builds it; a value made by reflection, with no `cell` field,
  // takes the operation.
  treeMapSeqConstructor.prototype.cljs$core$ISeq$_first$arity$1$LKEY = builtinSlot(function (self) {
    return self.cell$LKEY === undefined ? noBranch : self.first$LKEY;
  });
  //@unit treeMapSeqConstructor.restSlot effect: treeMapSeqConstructor when: rest requires: builtinSlot,
  //@ emptyList, noBranch, treeMapSeqConstructor
  treeMapSeqConstructor.prototype.cljs$core$ISeq$_rest$arity$1$LKEY = builtinSlot(function (self) {
    return self.cell$LKEY === undefined ? noBranch : self.rest$LKEY ?? emptyList;
  });
  //@unit treeMapSeqConstructor.nextSlot effect: treeMapSeqConstructor when: next requires: builtinSlot,
  //@ noBranch, rest, seq, treeMapSeqConstructor
  // The seq of its rest, through the wrappers.
  treeMapSeqConstructor.prototype.cljs$core$INext$_next$arity$1$LKEY = builtinSlot(function (self) {
    return self.cell$LKEY === undefined ? noBranch : seq(rest(self));
  });
  //@unit treeMapSeqConstructor.seqSlot effect: treeMapSeqConstructor when: seq requires: builtinSlot,
  //@ noBranch, treeMapSeqConstructor
  treeMapSeqConstructor.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = builtinSlot(function (self) {
    return self.cell$LKEY === undefined ? noBranch : self;
  });
  //@unit first.slots effect: ChunkedCons, ChunkedSeqValue, IndexedSeqValue, LazySeq, Cons,
  //@ CycleValue, EmptyListValue, IntegerRangeValue, IterateValue, KeySeqCell, ListValue,
  //@ RangeValue, RepeatValue, ReSeqCell, treeMapSeqConstructor when: first requires: first,
  //@ noBranch, slotFirst
  first = (operation => Object.defineProperty(function (value) {
    if (slotFirst.on && value != null && value.cljs$lang$kind$$LKEY !== undefined && arguments.length === 1) {
      const method = value.cljs$core$ISeq$_first$arity$1$LKEY;
      if (method) { const result = method.call(value, value); if (result !== noBranch) return result; }
    }
    return operation.apply(null, arguments);
  }, "name", {value: operation.name}))(first);
  //@unit rest.slots effect: ChunkedCons, ChunkedSeqValue, IndexedSeqValue, LazySeq, Cons,
  //@ CycleValue, EmptyListValue, IntegerRangeValue, IterateValue, KeySeqCell, ListValue,
  //@ RangeValue, RepeatValue, ReSeqCell, treeMapSeqConstructor when: rest requires: noBranch, rest,
  //@ slotFirst
  rest = (operation => Object.defineProperty(function (value) {
    if (slotFirst.on && value != null && value.cljs$lang$kind$$LKEY !== undefined && arguments.length === 1) {
      const method = value.cljs$core$ISeq$_rest$arity$1$LKEY;
      if (method) { const result = method.call(value, value); if (result !== noBranch) return result; }
    }
    return operation.apply(null, arguments);
  }, "name", {value: operation.name}))(rest);
  //@unit next.slots effect: ChunkedCons, ChunkedSeqValue, IndexedSeqValue, LazySeq, Cons,
  //@ CycleValue, EmptyListValue, IntegerRangeValue, IterateValue, KeySeqCell, ListValue,
  //@ RangeValue, RepeatValue, ReSeqCell, treeMapSeqConstructor when: next requires: next, noBranch,
  //@ slotFirst
  next = (operation => Object.defineProperty(function (value) {
    if (slotFirst.on && value != null && value.cljs$lang$kind$$LKEY !== undefined && arguments.length === 1) {
      const method = value.cljs$core$INext$_next$arity$1$LKEY;
      if (method) { const result = method.call(value, value); if (result !== noBranch) return result; }
    }
    return operation.apply(null, arguments);
  }, "name", {value: operation.name}))(next);
  //@unit seq.slots effect: ChunkedCons, ChunkedSeqValue, IndexedSeqValue, LazySeq, MapEntryType,
  //@ PersistentVectorValue, SubvecValue, Cons, CycleValue, EmptyListValue, IntegerRangeValue,
  //@ IterateValue, KeySeqCell, ListValue, RangeValue, RepeatValue, ReSeqCell, treeMapSeqConstructor
  //@ when: seq requires: emptyList, noBranch, seq, slotFirst
  // The empty list takes the operation, as before it had slots: every walk
  // ends with its `seq`, and its slot made the read and call each step takes
  // megamorphic (light-cljs-1033, docs/value-layout.md). The empty list and
  // `noBranch` are parameters, read with no check for an uninitialized
  // `const`, so the wrapper keeps under the 100 bytes of bytecode that V8's
  // Maglev tier inlines (94).
  seq = ((operation, empty, none) => Object.defineProperty(function (value) {
    if (slotFirst.on && value != null && value !== empty && value.cljs$lang$kind$$LKEY !== undefined && arguments.length === 1) {
      const method = value.cljs$core$ISeqable$_seq$arity$1$LKEY;
      if (method) { const result = method.call(value, value); if (result !== none) return result; }
    }
    return operation.apply(null, arguments);
  }, "name", {value: operation.name}))(seq, emptyList, noBranch);
  //@unit contains requires: get, methodKeys, missing, protocolHooks, vectorStore
  function contains(value, key) {
    const method = protocolHooks.publicMethod?.(value, methodKeys[2], 2);
    if (method) return method.call(value, value, key);
    const store = vectorStore(value);
    if (store) return Number.isInteger(key) && key >= 0 && key < store.cnt$LKEY;
    return get(value, key, missing) !== missing;
  }
  //@unit contains.own effect: installMethods when: contains requires: contains, demandKey, keySlot,
  //@ methodKeys, protocolHooks
  // As `get.own`, but for a runtime class's value, whose mask bit declares
  // what it implements natively and which has no `-contains-key?` slot
  // (light-cljs-1028).
  contains = (operation => Object.defineProperty(function (value, key) {
    const resolved = methodKeys[2];
    if (value != null && (value[resolved.mask] & resolved.bit) !== 0 && value.cljs$lang$kind$$LKEY === undefined) {
      demandKey(resolved, 2, value);
      const slot = value[keySlot(resolved, 2)], own = slot ? slot : protocolHooks.declaredMethod(value, resolved, 2, slot);
      if (own) return own.call(value, value, key);
    }
    return operation(value, key);
  }, "name", {value: operation.name}))(contains);
  //@unit contains.host effect: contains, extendType, extendInstance when: setPrototype requires: contains,
  //@ hostHooks
  hostHooks.has = contains;
  //@unit protocolHooks.count effect: installMethods when: makeMap.equiv, makeSet.equiv requires: contains,
  //@ count, protocolHooks
  // A map or set compares with a value of a program type through the generic
  // operations.
  protocolHooks.count = count;
  protocolHooks.has = contains;

  //@unit every requires: first, next, seq, truth
  function every(predicate, value) {
    for (let current = seq(value); current !== null; current = next(current)) if (!truth(predicate.call(null, first(current)))) return false;
    return true;
  }
  //@unit find requires: arrayIndex, collectionEntryIndex, contains, get, hash, hashNodeFind,
  //@ isArrayMap, isAssociative, isHashMap, isTreeMap, kindHooks, mapEntry, maps, methodKeys,
  //@ protocolHooks, recordTypes
  function find(value, key) {
    const method = protocolHooks.publicMethod?.(value, methodKeys[19], 2);
    if (method) return method.call(value, value, key);
    // An array map's pairs and a hash map's trie are searched once
    // (light-cljs-1036), as the entries below.
    if (isArrayMap(value)) {
      const arr = value.arr$LKEY, index = arrayIndex(arr, key);
      return index < 0 ? null : mapEntry(arr[index], arr[index + 1]);
    }
    if (isHashMap(value)) {
      const root = value.root$LKEY, entry = key == null ? value.nil$LKEY : root ? hashNodeFind(root, 0, hash(key), key) : null;
      return entry ? mapEntry(entry.key, entry.value) : null;
    }
    // A sorted map's node that holds the key, itself a map entry, as
    // upstream's (light-cljs-1035).
    if (isTreeMap(value)) return kindHooks.treeEntryAt(value, key);
    const entries = !recordTypes.has(value) && maps.get(value);
    if (entries) {
      // IFind returns the retained entry from one search. Repeating contains?
      // or get would repeat observable IHash/IEquiv calls on the query key.
      if (!Array.isArray(entries)) {
        const entry = kindHooks.trieEntry(entries, key);
        return entry ? mapEntry(entry.key, entry.value) : null;
      }
      const index = collectionEntryIndex(value, entries, key);
      return index < 0 ? null : mapEntry(entries[index].key, entries[index].value);
    }
    if (value == null || !isAssociative(value) || !contains(value, key)) return null;
    return mapEntry(key, get(value, key));
  }

  //@unit getIn requires: first, get, invalidArity, missing, next, seq
  function getIn(value, keys, notFound) {
    if (arguments.length !== 2 && arguments.length !== 3) return invalidArity(arguments.length);
    const fallback = arguments.length === 3;
    for (let path = seq(keys); path !== null; path = next(path)) {
      value = fallback ? get(value, first(path), missing) : get(value, first(path));
      if (value === missing) return notFound;
    }
    return value;
  }
  //@unit assocIn requires: assoc, first, get, next, seq
  function assocIn(value, path, item) {
    const current = seq(path), key = first(current), more = next(current);
    return assoc(value, key, more == null ? item : assocIn(get(value, key), more, item));
  }
  //@unit updateIn requires: apply, fnRest, updatePath
  function updateIn(value, path, fn, ...args) {
    const variadic = arguments.length < 3 || arguments.length > 6;
    return updatePath(value, path, old => variadic
      ? apply(fn, old, args[0], args[1], args[2], fnRest(args, 3))
      : fn.call(null, old, ...args));
  }
  //@unit updatePath requires: assoc, first, get, next, seq
  function updatePath(value, path, updateLeaf) {
    const current = seq(path), key = first(current), more = next(current);
    const old = get(value, key);
    return assoc(value, key, more == null ? updateLeaf(old) : updatePath(old, more, updateLeaf));
  }
  //@unit selectKeys requires: assoc, collectionWithMeta, first, get, makeMap, meta, missing, next,
  //@ PersistentArrayMapValue, seq
  function selectKeys(value, keyseq) {
    let result = makeMap(PersistentArrayMapValue, [], "array-map");
    for (let current = seq(keyseq); current != null; current = next(current)) {
      const key = first(current), item = get(value, key, missing);
      if (item !== missing) result = assoc(result, key, item);
    }
    return collectionWithMeta(result, meta(value));
  }
  //@unit mergeMaps requires: assoc, conj, contains, first, get, keyProtocol, makeMap, missing,
  //@ next, PersistentArrayMapValue, reduce, seq, truth, valProtocol
  function mergeMaps(values, combine) {
    // `some identity` precedes reduction, including its sequence effects.
    let present = false;
    for (let current = seq(values); current != null; current = next(current)) {
      if (truth(first(current))) { present = true; break; }
    }
    if (!present) return null;
    return reduce((left, right) => {
      const target = truth(left) ? left : makeMap(PersistentArrayMapValue, [], "array-map");
      if (combine === missing) return conj(target, right);
      return reduce((result, entry) => {
        const key = keyProtocol(entry), item = valProtocol(entry);
        return assoc(result, key, truth(contains(result, key)) ? combine.call(null, get(result, key), item) : item);
      }, target, seq(right));
    }, values);
  }
  //@unit someVals requires: assoc, reduceKv
  function someVals(value) {
    return reduceKv((result, key, item) => item == null ? result : assoc(result, key, item), null, value);
  }
  //@unit requiredGet requires: displayStr, get, missing
  function requiredGet(value, key) {
    const result = get(value, key, missing);
    if (result === missing) throw new Error("Missing required key: " + displayStr(key));
    return result;
  }
  //@unit keyExtreme requires: first, next, seq
  function keyExtreme(less, key, x, y, more) {
    let result = (less ? key.call(null, x) < key.call(null, y) : key.call(null, x) > key.call(null, y)) ? x : y;
    for (let s = seq(more); s !== null; s = next(s)) {
      const item = first(s);
      result = (less ? key.call(null, result) < key.call(null, item) : key.call(null, result) > key.call(null, item)) ? result : item;
    }
    return result;
  }
  //@unit minKey requires: fnRest, keyExtreme
  function minKey(key, x, y, ...more) { return arguments.length === 2 ? x : keyExtreme(true, key, x, y, fnRest(more, 0)); }
  //@unit maxKey requires: fnRest, keyExtreme
  function maxKey(key, x, y, ...more) { return arguments.length === 2 ? x : keyExtreme(false, key, x, y, fnRest(more, 0)); }
  //@unit reductions requires: cons, first, invalidArity, lazySequence, list, reducedValues, rest,
  //@ seq
  function reductions(fn, initial, coll) {
    if (arguments.length === 2) return lazySequence(() => {
      const s = seq(initial);
      return s === null ? list(fn.call(null)) : reductions(fn, first(s), rest(s));
    });
    if (arguments.length !== 3) return invalidArity(arguments.length);
    if (reducedValues.has(initial)) return list(reducedValues.get(initial));
    return cons(initial, lazySequence(() => {
      const s = seq(coll);
      return s === null ? null : reductions(fn, fn.call(null, initial, first(s)), rest(s));
    }));
  }
  //@unit replace requires: assoc, collectionArity, count, find, isVector, mapSeq, nth
  function replace(substitutions, coll) {
    const replaceOne = value => { const entry = find(substitutions, value); return entry == null ? value : nth(entry, 1); };
    if (arguments.length === 1) return mapSeq(replaceOne);
    collectionArity("replace", arguments.length);
    if (isVector(coll)) {
      let result = coll;
      for (let i = 0, size = count(coll); i < size; i++) {
        const entry = find(substitutions, nth(result, i));
        if (entry !== null) result = assoc(result, i, nth(entry, 1));
      }
      return result;
    }
    return mapSeq(replaceOne, coll);
  }
  //@unit distinct requires: conj, cons, contains, first, invalidArity, lazySequence, makeSet,
  //@ PersistentHashSetValue, reducingFunction, rest, seq, transform
  function distinct(coll) {
    if (arguments.length === 0) return transform(rf => {
      let seen = makeSet(PersistentHashSetValue, [], "array-map");
      return reducingFunction(rf, (result, input) => {
        if (contains(seen, input)) return result;
        seen = conj(seen, input);
        return rf.call(null, result, input);
      });
    });
    if (arguments.length !== 1) return invalidArity(arguments.length);
    const step = (values, seen) => lazySequence(() => {
      for (let s = seq(values); s !== null; s = seq(rest(s))) {
        const value = first(s);
        if (!contains(seen, value)) return cons(value, step(rest(s), conj(seen, value)));
      }
      return null;
    });
    return step(coll, makeSet(PersistentHashSetValue, [], "array-map"));
  }
  //@unit distinctMore requires: conj, contains, equalPair, first, hashSet, next
  function distinctMore(x, y, more) {
    if (equalPair(x, y)) return false;
    let seen = hashSet(x, y), s = more;
    for (;;) {
      const value = first(s), following = next(s);
      if (s == null) return true;
      if (contains(seen, value)) return false;
      seen = conj(seen, value); s = following;
    }
  }
  //@unit isDistinct requires: distinctMore, fnRest
  function isDistinct(x, y, ...more) {
    return arguments.length === 1 ? true : distinctMore(x, y, fnRest(more, 0));
  }
  //@unit fnComparator requires: compareKeys, truth
  function fnComparator(fn) {
    if (fn === compareKeys) return fn;
    return (x, y) => {
      const result = fn.call(null, x, y);
      return typeof result === "number" ? result : truth(result) ? -1 : truth(fn.call(null, y, x)) ? 1 : 0;
    };
  }
  //@unit sort requires: collectionWithMeta, compareKeys, emptyList, fnComparator, invalidArity,
  //@ meta, seq, toArray
  function sort(fn, coll) {
    if (arguments.length === 1) { coll = fn; fn = compareKeys; }
    else if (arguments.length !== 2) return invalidArity(arguments.length);
    if (seq(coll) === null) return emptyList;
    const values = toArray(coll);
    values.sort(fnComparator(fn));
    return collectionWithMeta(seq(values), meta(coll));
  }
  //@unit sortBy requires: compareKeys, fnComparator, invalidArity, sort
  function sortBy(key, fn, coll) {
    if (arguments.length === 2) { coll = fn; fn = compareKeys; }
    else if (arguments.length !== 3) return invalidArity(arguments.length);
    return sort((x, y) => fnComparator(fn)(key.call(null, x), key.call(null, y)), coll);
  }
  //@unit comparator requires: truth
  function comparator(predicate) {
    return (x, y) => truth(predicate.call(null, x, y)) ? -1 : truth(predicate.call(null, y, x)) ? 1 : 0;
  }
  //@unit isJsSymbol
  // Public core utilities follow the pinned core.cljs Number/UTF-16 contracts.
  function isJsSymbol(value) {
    return typeof value === "symbol" || (typeof Symbol !== "undefined" && value instanceof Symbol);
  }
  //@unit specialSymbols
  const specialSymbols = new Set("if def fn* do let* loop* letfn* throw try catch finally recur new set! ns deftype* defrecord* . js* & quote case* var ns*".split(" "));
  //@unit isSpecialSymbol requires: specialSymbols, symbolOf
  function isSpecialSymbol(value) {
    const data = symbolOf(value);
    return !!data && data.ns$LKEY === null && specialSymbols.has(data.name$LKEY);
  }
  //@unit parsingString
  function parsingString(value) {
    if (typeof value !== "string") throw new Error("Expected string, got: " + (value == null ? "nil" : Array.isArray(value) ? "array" : typeof value));
    return value;
  }
  //@unit parseLong requires: parsingString
  function parseLong(value) {
    const s = parsingString(value);
    if (/^[+-]?\d+$/.exec(s)?.[0] !== s) return null;
    const n = parseInt(s, 10);
    return n <= Number.MAX_SAFE_INTEGER && n >= Number.MIN_SAFE_INTEGER ? n : null;
  }
  //@unit parseDouble requires: parsingString
  function parseDouble(value) {
    const s = parsingString(value);
    if (/^[\x00-\x20]*[+-]?NaN[\x00-\x20]*$/.test(s)) return NaN;
    return /^[\x00-\x20]*[+-]?(Infinity|((\d+\.?\d*|\.\d+)([eE][+-]?\d+)?)[dDfF]?)[\x00-\x20]*$/.test(s) ? parseFloat(s) : null;
  }
  //@unit parseUuid requires: parsingString, uuid
  function parseUuid(value) {
    const s = parsingString(value);
    return s.length === 36 && /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(s) ? uuid(s) : null;
  }
  //@unit parseBoolean requires: parsingString
  function parseBoolean(value) {
    const s = parsingString(value);
    return s === "true" ? true : s === "false" ? false : null;
  }
  //@unit groupBy requires: assoc, conj, get, mapFromArray, reduce, vector
  function groupBy(fn, coll) {
    return reduce((result, value) => {
      const key = fn.call(null, value);
      return assoc(result, key, conj(get(result, key, vector()), value));
    }, mapFromArray([]), coll);
  }
  //@unit frequencies requires: assoc, get, mapFromArray, reduce
  function frequencies(coll) {
    return reduce((result, value) => assoc(result, value, get(result, value, 0) + 1), mapFromArray([]), coll);
  }
  //@unit updateVals requires: assoc, collectionWithMeta, mapFromArray, mapKinds, meta, reduceKv
  function updateVals(coll, fn) {
    const kind = mapKinds.get(coll);
    const initial = kind === "array-map" || kind === "hash-map" ? coll : mapFromArray([]);
    return collectionWithMeta(reduceKv((result, key, value) => assoc(result, key, fn.call(null, value)), initial, coll), meta(coll));
  }
  //@unit updateKeys requires: assoc, collectionWithMeta, mapFromArray, meta, reduceKv
  function updateKeys(coll, fn) {
    return collectionWithMeta(reduceKv((result, key, value) => assoc(result, fn.call(null, key), value), mapFromArray([]), coll), meta(coll));
  }
  //@unit merge requires: fnRest, mergeMaps, missing
  function merge(...values) { return mergeMaps(fnRest(values, 0), missing); }
  //@unit mergeWith requires: fnRest, mergeMaps
  function mergeWith(fn, ...values) { return mergeMaps(fnRest(values, 0), fn); }
  //@unit zipmap requires: assoc, first, makeMap, next, PersistentArrayMapValue, seq
  function zipmap(keys, values) {
    let result = makeMap(PersistentArrayMapValue, [], "array-map");
    let ks = seq(keys), vs = seq(values);
    while (ks != null && vs != null) {
      result = assoc(result, first(ks), first(vs));
      ks = next(ks);
      vs = next(vs);
    }
    return result;
  }
  //@unit set requires: first, isSet, makeSet, next, PersistentHashSetValue, putMember, seq,
  //@ withMeta
  function set(value) {
    if (isSet(value)) return withMeta(value, null);
    let members = [];
    const edit = {};
    for (let current = seq(value); current != null; current = next(current)) members = putMember(members, first(current), edit);
    return makeSet(PersistentHashSetValue, members, Array.isArray(members) ? "array-map" : "hash-map");
  }
  //@unit objectEquiv requires: comparedSequential, equalPair, maps, methodKeys, namedOf,
  //@ protocolHooks, sets, structuralEqualPair
  // Object.equiv calls the receiver's IEquiv implementation directly, even
  // when an extension rejects an identical value. Language = keeps its own
  // identity shortcut. Saved Object methods dispatch on the borrowed receiver.
  function objectEquiv(value, other) {
    const equiv = protocolHooks.builtin?.(value, methodKeys[18], 2);
    if (equiv) return equiv.call(value, value, other);
    if (maps.has(value)) return structuralEqualPair(value, other);
    // Sequence/collection protocols compare contents even for the same receiver
    // (for example, a list containing NaN is not equivalent to itself). Cycle
    // and Iterate retain their default identity equality, and so does a
    // keyword or symbol whose public constructor was given a NaN `fqn`.
    return sets.has(value) || namedOf(value) !== undefined || comparedSequential(value)
      ? structuralEqualPair(value, other) : equalPair(value, other);
  }
  //@unit equalPair requires: methodKeys, protocolHooks, structuralEqualPair
  function equalPair(left, right) {
    if (left == null) return right == null;
    if (left === right) return true;
    const equiv = protocolHooks.builtin?.(left, methodKeys[18], 2);
    if (equiv) return equiv.call(left, left, right);
    return structuralEqualPair(left, right);
  }
  //@unit structuralEqualPair requires: comparedSequential, keywordOf, kindHooks, maps, recordTypes,
  //@ sets
  function structuralEqualPair(left, right) {
    if (left instanceof Date) return right instanceof Date && left.valueOf() === right.valueOf();
    // Keywords compare by kind and `fqn` (docs/value-layout.md, section 3).
    const leftKeyword = keywordOf(left);
    if (leftKeyword) {
      const rightKeyword = keywordOf(right);
      return !!rightKeyword && leftKeyword.fqn$LKEY === rightKeyword.fqn$LKEY;
    }
    if (recordTypes.get(left) !== recordTypes.get(right)) return false;
    if (maps.has(left)) return kindHooks.equivMap(left, right);
    if (sets.has(left)) return kindHooks.equivSet(left, right);
    if (comparedSequential(left)) return (kindHooks.equivSequential ?? kindHooks.equivVectors)(left, right);
    // UUIDs and Vars, whose builders install their comparison; primitive
    // NaN stays unequal, and foreign objects use identity only.
    return kindHooks.equivNamed?.(left, right) ?? false;
  }
  //@unit structuralEqualPair.symbols effect: SymbolValue, symbol when: structuralEqualPair
  //@ requires: structuralEqualPair, symbolOf
  // Symbols compare their kind and `str`, kept only where a program builds
  // them (light-cljs-247). No other kind's branch takes a symbol, so it may
  // come first.
  structuralEqualPair = (operation => (left, right) => {
    const leftSymbol = symbolOf(left);
    if (!leftSymbol) return operation(left, right);
    const rightSymbol = symbolOf(right);
    return !!rightSymbol && leftSymbol.str$LKEY === rightSymbol.str$LKEY;
  })(structuralEqualPair);
  //@unit uuidValue.equiv effect: uuidValue, makeVar when: structuralEqualPair requires: equalPair,
  //@ kindHooks, uuids, vars
  // A UUID or a Var, or undefined for another value.
  kindHooks.equivNamed = (left, right) => uuids.has(left) ? uuids.has(right) && uuids.get(left) === uuids.get(right)
    : vars.has(left) ? vars.has(right) && equalPair(vars.get(left).symbol, vars.get(right).symbol) : undefined;
  //@unit makeMap.equiv effect: ArrayMapCell, makeMap, treeMapConstructor when: structuralEqualPair
  //@ requires: equalPair, get, kindHooks, mapCount, mapKinds, mapPairs, maps, methodKeys, missing,
  //@ protocolHooks
  // A record is a map too, but compares through its `IEquiv` method
  // (`recordMethods.equiv`), which every reader of this hook keeps.
  kindHooks.equivMap = (left, right) => {
    if (!(right != null && (protocolHooks.satisfies?.(methodKeys[33], right) ?? maps.has(right))) || (protocolHooks.satisfies?.(methodKeys[42], right) ?? false)) return false;
    if (protocolHooks.count ? protocolHooks.count(left) !== protocolHooks.count(right) : mapCount(left) !== mapCount(right)) return false;
    // Hash/tree maps use equiv-map's right-first value dispatch. Array
    // maps retain their distinct left-first IEquiv implementation.
    const pairs = mapPairs(left), leftFirst = mapKinds.get(left) === "array-map";
    for (let i = 0; i < pairs.length; i += 2) {
      const found = get(right, pairs[i], missing);
      if (found === missing || !(leftFirst ? equalPair(pairs[i + 1], found) : equalPair(found, pairs[i + 1]))) return false;
    }
    return true;
  };
  //@unit makeSet.equiv effect: makeSet, treeSetConstructor when: structuralEqualPair requires: kindHooks,
  //@ lookup, methodKeys, missing, protocolHooks, setCount, sets, setTraversal
  // Each member of `left`, in the order `seq` walks them, as upstream's
  // `reduce-kv` over its map, is looked up in the other set: O(n) lookups.
  kindHooks.equivSet = (left, right) => {
    if (!(right != null && (protocolHooks.satisfies?.(methodKeys[55], right) ?? sets.has(right))) || sets.get(left).length !== (protocolHooks.count ? protocolHooks.count(right) : setCount(right))) return false;
    try { return setTraversal(left).every(entry => protocolHooks.has ? protocolHooks.has(right, entry.key) : lookup(right, entry.key, missing) !== missing); }
    catch (error) { if (error instanceof Error) return false; throw error; }
  };
  //@unit setCount requires: kindHooks, publicCollectionSlots, sets
  // `count` of a set without protocol implementations.
  function setCount(value) {
    const slots = publicCollectionSlots.get(value);
    return slots ? kindHooks.countPublicSet(slots) : sets.get(value).length;
  }
  //@unit publicHashSet.count effect: publicHashSet, treeSetConstructor when: setCount requires: count,
  //@ kindHooks
  // A set its public constructor built counts the map it received.
  kindHooks.countPublicSet = slots => count("tree-map" in slots ? slots["tree-map"] : slots["hash-map"]);
  //@unit mapEntry.equiv effect: mapEntry when: structuralEqualPair unless: makeVector requires: equalPair,
  //@ kindHooks, vectorStore
  // `makeVector.equiv`, which follows, where map entries, of two items, are
  // the only values with items.
  kindHooks.equivVectors = (left, right) => {
    const a = vectorStore(left).tail$LKEY, b = vectorStore(right)?.tail$LKEY;
    return !!b && equalPair(a[0], b[0]) && equalPair(a[1], b[1]);
  };
  //@unit makeVector.equiv effect: makeVector when: structuralEqualPair requires: equalPair,
  //@ holdsVector, kindHooks, vectorCount, vectorStorage
  // Where no other sequential value and no protocol implementation exists.
  kindHooks.equivVectors = (left, right) => {
    if (!holdsVector(right) || vectorCount(left) !== vectorCount(right)) return false;
    const a = vectorStorage(left), b = vectorStorage(right);
    let index = 0, more = a !== null, other = b !== null;
    while (more) {
      if (!other || !equalPair(a[index], b[index])) return false;
      more = ++index < a.length; other = index < b.length;
    }
    return !other;
  };
  //@unit cons.equiv effect: emptyList, Cons, ListValue, EmptyListValue, RepeatValue, cons,
  //@ listFromArray, compilerFormBridge, mapProjection, cycleNode, treeMapSeqConstructor, reSeq,
  //@ iterateNode, ChunkedCons, rangeNode, view, ChunkedSeqValue, IndexedSeqValue, queueFromArray,
  //@ LazySeq, Eduction, ISequential, extendType, extendInstance when: structuralEqualPair requires: count,
  //@ countedValue, equalPair, first, kindHooks, methodKeys, next, protocolHooks, seq, sequential
  // Sequential values: vectors, cells, views, queues and lazy seqs, and the
  // values of a program type that implements `ISequential`, which keep the
  // protocol's unit (`PROTOCOL_ROOTS` in src/runtime_slice.rs), as
  // `extend-type` and `specify!` can give any value an implementation.
  kindHooks.equivSequential = (left, right) => {
    if (!(protocolHooks.satisfies?.(methodKeys[53], right) ?? sequential(right))) return false;
    if ((protocolHooks.satisfies?.(methodKeys[11], left) ?? countedValue(left)) && (protocolHooks.satisfies?.(methodKeys[11], right) ?? countedValue(right)) && count(left) !== count(right)) return false;
    let a = seq(left), b = seq(right);
    while (a !== null) {
      if (b === null || !equalPair(first(a), first(b))) return false;
      a = next(a); b = next(b);
    }
    return b === null;
  };

  //@unit equal requires: equalPair
  function equal(...values) {
    for (let i = 1; i < values.length; i++) {
      if (!equalPair(values[i - 1], values[i])) return false;
    }
    return true;
  }

  //@unit sequential requires: holdsVector, isCell, isChunkedCons, isLazy, isView, queues
  function sequential(value) {
    return holdsVector(value) || isCell(value) || isChunkedCons(value) || isView(value) || queues.has(value) || isLazy(value);
  }
  //@unit comparedSequential requires: holdsVector, kindHooks
  // The sequential values that equality and hashing compare by their items:
  // all but cycle and iterate seqs, which keep identity. Values other than
  // vectors only where the program builds them (`cons.compared`).
  function comparedSequential(value) {
    return holdsVector(value) || !!kindHooks.comparedSeq?.(value);
  }
  //@unit cons.compared effect: emptyList, Cons, ListValue, EmptyListValue, RepeatValue, cons,
  //@ listFromArray, compilerFormBridge, mapProjection, cycleNode, treeMapSeqConstructor, reSeq,
  //@ iterateNode, ChunkedCons, rangeNode, view, ChunkedSeqValue, IndexedSeqValue, queueFromArray,
  //@ LazySeq when: comparedSequential requires: cellOf, isCell, isChunkedCons, isLazy, isView,
  //@ kindHooks, queues
  kindHooks.comparedSeq = value => isCell(value) ? !["cycle", "iterate"].includes(cellOf(value).cell$LKEY)
    : isChunkedCons(value) || isView(value) || queues.has(value) || isLazy(value);

  //@unit identityHashes
  const identityHashes = new WeakMap();
  //@unit identityCounter
  let identityCounter = 0;
  //@unit stringHash
  function stringHash(value) {
    if (value == null) return 0;
    let result = 0;
    for (let i = 0; i < value.length; i++) result = Math.imul(result, 31) + value.charCodeAt(i);
    return result;
  }

  //@unit rotateLeft
  // ClojureScript's public Murmur3 collection/name hashes. Number hashes
  // follow the JavaScript Number profile, including signed zero and nonfinite values.
  function rotateLeft(value, distance) { return (value << distance) | (value >>> (32 - distance)); }
  //@unit mixK1 requires: rotateLeft
  function mixK1(value) { return Math.imul(rotateLeft(Math.imul(value, 0xcc9e2d51), 15), 0x1b873593); }
  //@unit mixH1 requires: rotateLeft
  function mixH1(hash, value) { return (Math.imul(rotateLeft(hash ^ value, 13), 5) + 0xe6546b64) | 0; }
  //@unit fmix
  function fmix(hash, length) {
    hash ^= length;
    hash = Math.imul(hash ^ (hash >>> 16), 0x85ebca6b);
    hash = Math.imul(hash ^ (hash >>> 13), 0xc2b2ae35);
    return hash ^ (hash >>> 16);
  }
  //@unit hashInt requires: fmix, mixH1, mixK1
  function hashInt(value) { return value === 0 ? 0 : fmix(mixH1(0, mixK1(value)), 4); }
  //@unit hashChars requires: fmix, mixH1, mixK1
  function hashChars(value) {
    let h = 0, i = 1;
    for (; i < value.length; i += 2) h = mixH1(h, mixK1(value.charCodeAt(i - 1) | (value.charCodeAt(i) << 16)));
    if (value.length & 1) h ^= mixK1(value.charCodeAt(value.length - 1));
    return fmix(h, value.length * 2);
  }
  //@unit hashNamed requires: hashChars, stringHash
  // The hash of a keyword's or symbol's fields (`namedOf`).
  function hashNamed(value) {
    const seed = hashChars(value.name$LKEY);
    return seed ^ (stringHash(value.ns$LKEY) + 0x9e3779b9 + (seed << 6) + (seed >> 2));
  }
  //@unit hashDouble
  function hashDouble(value) {
    const buffer = new ArrayBuffer(8), view = new DataView(buffer);
    // Upstream reads native little-endian Float64 bytes as big-endian int32s.
    view.setFloat64(0, value, true);
    return view.getInt32(0) ^ view.getInt32(4);
  }
  //@unit mixCollectionHash requires: fmix, mixH1, mixK1
  function mixCollectionHash(basis, count) { return fmix(mixH1(0, mixK1(basis)), count); }
  //@unit hashOrderedColl requires: first, hash, mixCollectionHash, next, seq
  function hashOrderedColl(value) {
    let result = 1, n = 0;
    for (let current = seq(value); current !== null; current = next(current), n++)
      result = (Math.imul(result, 31) + hash(first(current))) | 0;
    return mixCollectionHash(result, n);
  }
  //@unit hashUnorderedColl requires: first, hash, mixCollectionHash, next, seq
  function hashUnorderedColl(value) {
    let result = 0, n = 0;
    for (let current = seq(value); current !== null; current = next(current), n++)
      result = (result + hash(first(current))) | 0;
    return mixCollectionHash(result, n);
  }
  //@unit hash requires: comparedSequential, constructorHashes, hashDouble, hashInt, hashNamed,
  //@ identityCounter, identityHashes, keywordOf, kindHooks, maps, mapTraversal, methodKeys,
  //@ mixCollectionHash, ownMeta, protocolHooks, sets, setTraversal, stringHash, symbolOf, uuids,
  //@ vars
  // Weak identity bookkeeping never reads or mutates foreign object fields.
  function hash(value) {
    if (value == null) return 0;
    const method = protocolHooks.implements?.(methodKeys[23], value) ? protocolHooks.builtin?.(value, methodKeys[22], 1) : null;
    if (method) return method.call(value, value) | 0;
    if (typeof value === "boolean") return value ? 1231 : 1237;
    if (typeof value === "number") {
      if (!Number.isFinite(value)) return value === Infinity ? 2146435072 : value === -Infinity ? -1048576 : 2146959360;
      return Number.isSafeInteger(value) ? (Math.floor(value) % 2147483647) | 0 : hashDouble(value);
    }
    if (typeof value === "string") return hashInt(stringHash(value));
    if (value instanceof Date) return value.valueOf() | 0;
    if (uuids.has(value)) {
      if (value.hash$LKEY == null) value.hash$LKEY = hash(value.uuid$LKEY);
      return value.hash$LKEY;
    }
    if (vars.has(value)) return hash(vars.get(value).symbol);
    // A keyword or a symbol caches its hash in its own field, which a
    // keyword literal at `Advanced` holds from its construction.
    const kw = keywordOf(value), named = kw ?? symbolOf(value);
    if (named !== undefined) {
      const known = named.hash$LKEY;
      return known != null ? known | 0 : (named.hash$LKEY = (hashNamed(named) + (kw !== undefined ? 0x9e3779b9 : 0)) | 0);
    }
    // A vector, a subvector or a map entry caches its hash in its own field.
    if (ownMeta(value)) { const known = value.hash$LKEY; if (known != null) return known | 0; }
    const cached = constructorHashes.get(value);
    if (cached != null) return cached | 0;
    // An array map's and a hash map's fields, of one read of the keyed kind.
    const mapKind = value.cljs$lang$kind$$LKEY;
    if (mapKind === 28 && value.arr$LKEY !== undefined) {
      // An array map's keys and values, flat (`mapPairs`).
      const arr = value.arr$LKEY;
      let result = 0;
      for (let i = 0; i < arr.length; i += 2) result = (result + mixCollectionHash((Math.imul(31, 31 + hash(arr[i])) + hash(arr[i + 1])) | 0, 2)) | 0;
      const resultHash = mixCollectionHash(result, (arr.length + 1) >> 1);
      constructorHashes.set(value, resultHash);
      return resultHash;
    }
    // A hash map's entries are its trie's leaves (light-cljs-1036), a sorted
    // map's its tree's nodes (light-cljs-1035).
    const entries = (mapKind === 29 || mapKind === 30) && (value.cnt$LKEY !== undefined || "cnt$LKEY" in value) ? mapTraversal(value) : maps.get(value) && mapTraversal(value);
    if (entries) {
      let result = 0;
      for (const entry of entries) result = (result + mixCollectionHash((Math.imul(31, 31 + hash(entry.key)) + hash(entry.value)) | 0, 2)) | 0;
      const resultHash = mixCollectionHash(result, entries.length);
      constructorHashes.set(value, resultHash);
      return resultHash;
    }
    const members = sets.get(value);
    if (members) {
      let result = 0;
      for (const entry of setTraversal(value)) result = (result + entry.hash) | 0;
      const resultHash = mixCollectionHash(result, members.length);
      constructorHashes.set(value, resultHash);
      return resultHash;
    }
    if (comparedSequential(value)) return kindHooks.hashSequential(value);
    const nativeHash = protocolHooks.builtin?.(value, methodKeys[22], 1);
    if (nativeHash) return nativeHash.call(value, value) | 0;
    if (typeof value === "symbol" || typeof value === "bigint") return hash(String(value));
    if (!identityHashes.has(value)) identityHashes.set(value, ++identityCounter);
    return identityHashes.get(value);
  }

  //@unit makeVector.hash effect: makeVector, mapEntry, emptyList, Cons, ListValue, EmptyListValue,
  //@ RepeatValue, cons, listFromArray, compilerFormBridge, mapProjection, cycleNode,
  //@ treeMapSeqConstructor, reSeq, iterateNode, ChunkedCons, rangeNode, view, ChunkedSeqValue,
  //@ IndexedSeqValue, queueFromArray, LazySeq when: hash requires: cellOf, chunkedConsOf,
  //@ constructorHashes, hashOrderedColl, holdsVector, kindHooks, lazyOf, ownMeta,
  //@ publicCollectionSlots, queues, viewOf
  // Sequential values (see `makeVector.equiv`).
  kindHooks.hashSequential = value => {
    if (holdsVector(value) || queues.has(value) || ["cons", "list", "repeat"].includes(cellOf(value)?.cell$LKEY) || publicCollectionSlots.has(value) || viewOf(value)?.view$LKEY === "chunked") {
      const result = hashOrderedColl(value);
      if (ownMeta(value)) value.hash$LKEY = result;
      else constructorHashes.set(value, result);
      return result;
    }
    const cached = lazyOf(value) ?? chunkedConsOf(value);
    if (cached?.hash$LKEY != null) return cached.hash$LKEY;
    const result = hashOrderedColl(value);
    if (cached) cached.hash$LKEY = result;
    return result;
  };
  //@unit entryHashing
  // The hash of a key for map and set entries that compute it on demand. Only
  // code that hashes, and so keeps `hash`, reads an entry's hash.
  const entryHashing = {};
  //@unit hash.entries effect: hash requires: entryHashing, hash
  entryHashing.hash = hash;
  //@unit hashed requires: constructorHashes
  // A constant with the hash the compiler computed, in its own field where
  // its class has one, which a fresh constant holds as null (`ownMeta`
  // tells the classes `hash` reads it of).
  function hashed(value, cached) { if (value.hash$LKEY === null) value.hash$LKEY = cached; else constructorHashes.set(value, cached); return value; }

  //@unit compilerFormBridge compile-time requires: atoms, cellOf, chunkedConsFirst, chunkedConsOf,
  //@ chunkedConsRest, EmptyListCell, generatedNames, holdsVector, ifnValues, isCell, isChunkedCons,
  //@ isLazy, isView, jsObject, keyword, lazyOf, lazyStep, listFromArray, mapFromArray, maps,
  //@ mapTraversal, metadataValues, name, namedOf, ownMeta, queueFromArray, queues, recordTypes,
  //@ reifyValues, rePattern, setFromArray, setMeta, sets, setTraversal, stamped, symbol, symbolOf,
  //@ toArray, typeFields, uuid, uuids, vars, vectorFromArray, vectorItems, vectorNth, vectorStore,
  //@ viewAt, viewOf, volatiles
  // Versioned in-process compiler bridge. Descriptors are private arrays,
  // never printed text or JSON. Only brands above authorize language values.
  function compilerFormBridge() {
    const borrowed = new WeakMap(), literals = new WeakMap(), environments = new Map(), macroInputs = new WeakMap(), frames = new WeakMap();
    let primitives = new Map();
    const negativeZero = Object.freeze({});
    const primitiveKey = v => typeof v === "number" && Object.is(v, -0) ? negativeZero : v;
    const dateTime = Function.prototype.call.bind(Date.prototype.getTime);
    const regexSource = Function.prototype.call.bind(Object.getOwnPropertyDescriptor(RegExp.prototype, "source").get);
    const regexFlags = [["hasIndices","d"],["global","g"],["ignoreCase","i"],["multiline","m"],["dotAll","s"],["unicode","u"],["sticky","y"]]
      .map(([property, letter]) => [Object.getOwnPropertyDescriptor(RegExp.prototype, property)?.get, letter]);
    let terminal = null;
    const fail = (category, path) => {
      const message = "Unsupported macro result " + category + " at " + path;
      if (category.includes("limit")) terminal ??= message;
      throw new Error(message);
    };
    function decode(root, snapshotLimits = null, macroInput = false) {
      // Bootstrap syntax quotations can be deep. Traverse descriptors explicitly
      // so the documented form depth budget does not consume the engine stack.
      const values = new Map(), pending = [root, false];
      while (pending.length) {
        const ready = pending.pop(), d = pending.pop();
        if (ready) {
          const value = decodeNode(d, values, false, macroInput); values.set(d, value);
          if (d[5] != null) environments.set(d[5], value);
          continue;
        }
        pending.push(d, true);
        const kind = d[0], metadata = d[2], a = d[3];
        if (metadata != null) pending.push(metadata, false);
        switch (kind) {
          case "list": case "vector": case "map": case "set":
            for (let i = a.length - 1; i >= 0; --i) pending.push(a[i], false);
            break;
          case "regex": case "instant": case "uuid": case "queue": case "js-array": case "js-object":
            pending.push(a, false);
            break;
        }
      }
      const value = values.get(root);
      if (macroInput) { macroInputs.set(value, {root, values}); frames.set(value, primitives); primitives = new Map(); }
      if (snapshotLimits === null) return value;
      // Only final replacements matter when a large import recycles a slot.
      // Discarded map/set entries and literal payloads may have no snapshot;
      // never publish them with uninitialized canonical provenance.
      const updates = new Map(root[6]), unseen = new Set();
      for (const d of updates.values()) unseen.add(values.get(d));
      // Snapshot only fresh components. Cached immutable components already
      // have canonical provenance; walking them again would undo the cache.
      const inputs = root[0] === "map" && root[1] == null && root[2] == null
        ? root[3].filter((d, i) => i % 2 === 1 && d[0] !== "environment") : [root];
      const snapshots = inputs.map(d => encode(values.get(d), ...snapshotLimits, unseen));
      const published = unseen.size === 0;
      // References name the previous completed import. Commit only after
      // resolving every descriptor and validating the normal snapshots.
      if (published) for (const [slot, d] of updates) environments.set(slot, values.get(d));
      return [value, snapshots, published];
    }
    function decodeNode(d, values, reuse = false, input = false) {
      const decode = child => values.get(child);
      const kind = d[0], id = d[1], metadata = d[2], a = d[3], b = d[4];
      let value;
      if (reuse) value = values.get(d);
      else switch (kind) {
        case "environment": return environments.get(a);
        case "nil": value = null; break;
        case "bool": case "number": value = a; break;
        case "string": case "character": value = a; break;
        case "symbol": case "keyword": value = (kind === "symbol" ? symbol : keyword)(a == null ? null : a, b); if (kind === "symbol" && a == null) generatedNames.add(b); break;
        case "list": value = listFromArray(a.map(decode));
          if (a.length === 0) value = new EmptyListCell();
          break;
        case "vector": value = vectorFromArray(a.map(decode)); break;
        case "map": value = mapFromArray(a.map(decode)); break;
        case "set": value = setFromArray(a.map(decode)); break;
        case "regex": value = rePattern(decode(a)); break;
        case "instant": value = new Date(decode(a)); break;
        case "uuid": value = uuid(decode(a)); break;
        case "queue": value = queueFromArray(vectorItems(decode(a))); break;
        case "js-array": value = vectorItems(decode(a)); literals.set(value, "js-array"); break;
        case "js-object": {
          const payload = decode(a), pairs = [];
          for (const e of mapTraversal(payload)) pairs.push(typeof e.key === "string" ? e.key : name(e.key), e.value);
          value = jsObject(pairs); literals.set(value, "js-object"); break;
        }
        default: throw new Error("Unknown compiler form descriptor");
      }
      if (metadata != null) {
        const m = decode(metadata);
        // Compiler metadata uses the canonical private store, never user hooks.
        if (value == null || (typeof value !== "object" && typeof value !== "function")) throw new Error("Invalid compiler metadata target");
        setMeta(value, m);
      }
      if (id != null && id !== 0) {
        // Scalars and interned keywords: the latest provenance in this call's table.
        if (value != null && typeof value === "object" && kind !== "keyword") borrowed.set(value, id);
        else if (!input) primitives.set(primitiveKey(value), id);
      }
      return value;
    }
    function macroArguments(form, nodes, depth) {
      const input = macroInputs.get(form);
      if (!input || input.root[0] !== "list") throw new Error("Missing compiler macro input");
      const args = [], snapshots = [];
      primitives.clear();
      try {
        for (const descriptor of input.root[3].slice(1)) {
          // Restore the same per-argument metadata/provenance order as a new
          // import, without reconstructing the objects already in &form.
          const pending = [descriptor, false];
          while (pending.length) {
            const ready = pending.pop(), d = pending.pop();
            if (ready) { decodeNode(d, input.values, true); continue; }
            pending.push(d, true);
            if (d[2] != null) pending.push(d[2], false);
            switch (d[0]) {
              case "list": case "vector": case "map": case "set":
                for (let i = d[3].length - 1; i >= 0; --i) pending.push(d[3][i], false);
                break;
              case "regex": case "instant": case "uuid": case "queue": case "js-array": case "js-object":
                pending.push(d[3], false); break;
            }
          }
          const value = input.values.get(descriptor);
          args.push(value); snapshots.push(encode(value, nodes, depth));
        }
        return [args, snapshots];
      } finally { macroInputs.delete(form); }
    }

    function encode(value, limit, depthLimit, unseen = null) {
      let remaining = limit;
      const active = new WeakSet();
      function visit(v, path, depth) {
        if (--remaining < 0) fail("node limit", path);
        if (depth > depthLimit) fail("depth limit", path);
        const kind = typeof v;
        const object = v != null && (kind === "object" || kind === "function");
        if (object && active.has(v)) fail("cyclic data", path);
        if (object) active.add(v);
        try {
          let tag, a, b;
          if (v === null) tag = "nil";
          else if (kind === "boolean") { tag = "bool"; a = v; }
          else if (kind === "number") { tag = "number"; a = v; }
          else if (kind === "string") { tag = "string"; a = v; }
          else if (kind !== "object") fail(kind, path);
          else if (vars.has(v)) fail("Var", path);
          else if (stamped(v) && atoms.has(v)) fail("atom", path);
          else if (stamped(v) && volatiles.has(v)) fail("volatile", path);
          else if (typeFields.has(v) || recordTypes.has(v) || reifyValues.has(v) || ifnValues.has(v)) fail("protocol/type instance", path);
          else if (stamped(v) && namedOf(v) !== undefined) {
            const n = namedOf(v); tag = symbolOf(v) !== undefined ? "symbol" : "keyword";
            a = n.ns$LKEY == null ? null : n.ns$LKEY; b = n.name$LKEY;
          } else if (stamped(v) && holdsVector(v)) {
            // Each item is read just before it is visited, within the limits.
            tag = "vector"; a = []; const store = vectorStore(v);
            for (let i = 0; i < store.cnt$LKEY; i++) a.push(visit(vectorNth(store, i, null), path + "[" + i + "]", depth + 1));
          }
          else if (stamped(v) && maps.has(v)) { tag = "map"; a = []; let i = 0;
            for (const e of mapTraversal(v)) { a.push(visit(e.key, path + ".key[" + i + "]", depth + 1), visit(e.value, path + ".value[" + i++ + "]", depth + 1)); }
          } else if (sets.store.has(v) || stamped(v) && sets.has(v)) { tag = "set"; a = setTraversal(v).map((e, i) => visit(e.key, path + "[" + i + "]", depth + 1)); }
          else if (stamped(v) && queues.has(v)) { tag = "queue"; a = visit(vectorFromArray(toArray(v)), path + ".payload", depth + 1); }
          else if (stamped(v) && uuids.has(v)) { tag = "uuid"; a = visit(uuids.get(v), path + ".payload", depth + 1); }
          else if (literals.has(v)) {
            tag = literals.get(v);
            if (--remaining < 0) fail("node limit", path + ".payload");
            if (depth + 1 > depthLimit) fail("depth limit", path + ".payload");
            const data = Object.getOwnPropertyDescriptors(v), items = [];
            if (tag === "js-array") {
              const length = data.length.value;
              if (length > remaining) fail("node limit", path);
              for (let i = 0; i < length; i++) {
                const d = data[i]; if (!d || !("value" in d)) fail("accessor/hole in #js array", path + "[" + i + "]");
                items.push(visit(d.value, path + "[" + i + "]", depth + 2));
              }
            } else {
              for (const key of Reflect.ownKeys(data)) {
                if (typeof key !== "string") fail("symbol key in #js object", path);
                const d = data[key]; if (!("value" in d)) fail("accessor in #js object", path);
                items.push(visit(key, path + ".key", depth + 2), visit(d.value, path + "." + key, depth + 2));
              }
            }
            a = [tag === "js-array" ? "vector" : "map", null, null, items];
          } else if (stamped(v) && (isCell(v) || isView(v) || isChunkedCons(v) || isLazy(v))) {
            tag = "list"; a = [];
            const seen = new WeakSet(); let current = v;
            while (true) {
              if (--remaining < 0) fail("sequence limit", path);
              if (current == null) break;
              if (!(stamped(current) && (isCell(current) || isView(current) || isChunkedCons(current) || isLazy(current)))) fail("noncanonical sequence tail", path);
              if (seen.has(current)) fail("cyclic sequence", path); seen.add(current);
              // Lazy realization is intentionally executable but shares limits.
              // Work only with private payloads. User protocol objects are not
              // expansion trees; realization cannot call their getters to diagnose.
              try {
                while (stamped(current) && isLazy(current)) {
                  if (--remaining < 0) fail("sequence limit", path);
                  current = lazyStep(lazyOf(current));
                }
              } catch (_) { fail("lazy realization failure", path); }
              if (current == null) break;
              // A vector, or a chunked seq over one, ends the sequence: a chunked
              // seq's node from its offset, as its `first` and `rest` read it,
              // then the vector's items after that node, each read within the
              // limit.
              const slots = stamped(current) ? viewOf(current) : undefined, chunked = slots?.vec$LKEY || undefined;
              const store = stamped(chunked ?? current) ? vectorStore(chunked ?? current) : undefined;
              if (store) {
                let first = true;
                if (chunked) for (let j = slots.i$LKEY; j < slots.arr$LKEY.length; j++, first = false) {
                  if (!first && --remaining < 0) fail("sequence limit", path);
                  a.push(visit(slots.arr$LKEY[j], path + "[" + a.length + "]", depth + 1));
                }
                for (let i = chunked ? slots.base$LKEY + slots.arr$LKEY.length : 0; i < store.cnt$LKEY; i++, first = false) {
                  if (!first && --remaining < 0) fail("sequence limit", path);
                  a.push(visit(vectorNth(store, i, null), path + "[" + a.length + "]", depth + 1));
                }
                break;
              }
              if (stamped(current) && isCell(current)) {
                const cell = cellOf(current);
                if (cell.cell$LKEY === "list" && cell.count$LKEY === 0) break;
                let head, tail;
                try { head = cell.first$LKEY; tail = cell.rest$LKEY; } catch (_) { fail("sequence realization failure", path); }
                a.push(visit(head, path + "[" + a.length + "]", depth + 1));
                current = tail;
              } else if (stamped(current) && isChunkedCons(current)) {
                const chunked = chunkedConsOf(current);
                let head, tail;
                try { head = chunkedConsFirst(chunked); tail = chunkedConsRest(chunked); } catch (_) { fail("sequence realization failure", path); }
                a.push(visit(head, path + "[" + a.length + "]", depth + 1));
                current = tail;
              } else if (stamped(current) && isView(current)) {
                const slice = viewOf(current);
                if (slice.i$LKEY >= slice.arr$LKEY.length) break;
                a.push(visit(slice.arr$LKEY[slice.i$LKEY], path + "[" + a.length + "]", depth + 1));
                current = slice.i$LKEY + 1 < slice.arr$LKEY.length ? viewAt(slice, slice.i$LKEY + 1) : null;
              } else fail("noncanonical sequence tail", path);
            }
          } else {
            // Intrinsic brand checks avoid getters, custom toString and instanceof.
            try { const ms = dateTime(v); tag = "instant"; if (!Number.isFinite(ms)) fail("invalid instant", path); a = visit(ms, path + ".payload", depth + 1); } catch (e) { if (tag) throw e; }
            if (!tag) {
              let pattern;
              try { pattern = regexSource(v); } catch (_) {}
              if (pattern != null) {
                let flags = "";
                for (const [g, letter] of regexFlags) {
                  if (g && g.call(v)) flags += letter;
                }
                tag = "regex"; a = visit(flags ? "(?" + flags + ")" + pattern : pattern, path + ".payload", depth + 1);
              }
            }
            if (!tag) fail("arbitrary object (including promise)", path);
          }
          // A lazy seq, chunked cons or repeat has metadata, nil where none is stored.
          // A vector, a subvector or a map entry holds its own (`ownMeta`).
          const metadata = object && stamped(v) && ownMeta(v) ? v.meta$LKEY === null ? null : visit(v.meta$LKEY, path + ".meta", depth + 1)
            : object && (metadataValues.has(v) || (stamped(v) && (isLazy(v) || isChunkedCons(v) || cellOf(v)?.cell$LKEY === "repeat")))
            ? visit(metadataValues.has(v) ? metadataValues.get(v) : null, path + ".meta", depth + 1) : null;
          if (object) unseen?.delete(v);
          return [tag, (object && tag !== "keyword" ? borrowed.get(v) : primitives.get(primitiveKey(v))) ?? null, metadata, a, b];
        } finally { if (object) active.delete(v); }
      }
      return visit(value, "$", 0);
    }
    return Object.freeze({version: 5, decode, encode, macroArguments, releaseMacroInput: value => { macroInputs.delete(value); primitives = frames.get(value) ?? primitives; },
      check: () => { if (terminal != null) throw new Error(terminal); },
      // Explicit descriptors authorize host literal data; ordinary objects remain invalid.
      jsArray: items => { const v = Array.from(items); literals.set(v, "js-array"); return v; },
      jsObject: pairs => { const v = jsObject(pairs); literals.set(v, "js-object"); return v; },
      withMeta: (value, metadata) => {
        if (!literals.has(value)) throw new Error("Invalid compiler literal metadata target");
        setMeta(value, metadata); return value;
      }});
  }

  //@unit inspect requires: atoms, cellOf, first, holdsVector, isCell, isChunkedCons, isReduced,
  //@ isView, keywordOf, mapTraversal, next, nth, recordEntries, recordTypes, seq, sets,
  //@ setTraversal, symbolOf, uuids, vars, vectorItems, volatiles
  // A value's shape for the reader (EDN data included) and the observation
  // harness, which alone registers inspect. Exposed storage remains frozen.
  // At compile time a vector is told by its stamp before its keyed kind
  // (`/*@stamped*/`, build.rs), so a host value's getters never run.
  function inspect(value) {
    if (/*@stamped*/isReduced(value) || /*@stamped*/uuids.has(value) || vars.has(value) || /*@stamped*/atoms.has(value) || /*@stamped*/volatiles.has(value)) return Object.freeze({kind: "opaque"});
    const kw = /*@stamped*/keywordOf(value), named = kw || /*@stamped*/symbolOf(value);
    if (named) return Object.freeze({kind: kw ? "keyword" : "symbol",
      namespace: named.ns$LKEY, name: named.name$LKEY, spelling: kw ? named.fqn$LKEY : named.str$LKEY});
    if (/*@stamped*/holdsVector(value)) return Object.freeze({kind: "vector", items: Object.freeze(vectorItems(value))});
    const entries = recordTypes.has(value)
      ? recordEntries(value, recordTypes.get(value)).map(pair => ({key: nth(pair, 0), value: nth(pair, 1)}))
      : /*@stamped*/mapTraversal(value);
    if (entries) return Object.freeze({kind: "map", entries: Object.freeze(entries.map(
      entry => Object.freeze([entry.key, entry.value]))), count: entries.length});
    // A sorted set by its store's brand, another set by its stamp first.
    const members = sets.store.has(value) || /*@stamped*/sets.has(value) ? sets.get(value) : undefined;
    if (members) return Object.freeze({kind: "set",
      items: Object.freeze(setTraversal(value).map(entry => entry.key)), count: members.length});
    if (/*@stamped*/isCell(value) || /*@stamped*/isChunkedCons(value) || /*@stamped*/isView(value)) {
      const items = [];
      for (let current = seq(value); current !== null; current = next(current)) items.push(first(current));
      return Object.freeze({kind: cellOf(value)?.cell$LKEY === "list" ? "list" : "seq", items: Object.freeze(items)});
    }
    return null;
  }

  //@unit queueFromArray requires: QueueCell
  function queueFromArray(items) { return new QueueCell(items.slice()); }

  //@unit jsObject
  function jsObject(pairs) {
    const result = {};
    for (let i = 0; i < pairs.length; i += 2) {
      // #js objects have object-literal __proto__ semantics.
      if (pairs[i] === "__proto__") {
        const prototype = pairs[i + 1];
        if (prototype === null || typeof prototype === "object" || typeof prototype === "function") Object.setPrototypeOf(result, prototype);
      } else Object.defineProperty(result, pairs[i], {value: pairs[i + 1], writable: true, enumerable: true, configurable: true});
    }
    return result;
  }

  //@unit IChunk
  const IChunk = function () {};
  //@unit IChunkedNext
  const IChunkedNext = function () {};
  //@unit IDrop
  const IDrop = function () {};
  //@unit IMapEntry
  const IMapEntry = function () {};
  //@unit IFind
  const IFind = function () {};
  //@unit ISorted
  const ISorted = function () {};
  //@unit IChunkedSeq
  const IChunkedSeq = function () {};
  //@unit IEncodeJS
  const IEncodeJS = function () {};
  //@unit IEncodeClojure
  const IEncodeClojure = function () {};
  //@unit IRecord
  const IRecord = function () {};
  //@unit ICloneable
  const ICloneable = function () {};
  //@unit IPrintWithWriter
  const IPrintWithWriter = function () {};
  //@unit IWriter
  const IWriter = function () {};
  //@unit INamed
  const INamed = function () {};
  //@unit Inst
  const Inst = function () {};
  //@unit IUUID
  const IUUID = function () {};
  //@unit ASeq
  const ASeq = function () {};
  //@unit IDerefWithTimeout
  const IDerefWithTimeout = function () {};
  //@unit IMeta
  const IMeta = function () {};
  //@unit IWithMeta
  const IWithMeta = function () {};
  //@unit IHash
  const IHash = function () {};
  //@unit IEquiv
  const IEquiv = function () {};
  //@unit IPending
  const IPending = function () {};
  //@unit IDeref
  const IDeref = function () {};
  //@unit IAtom
  const IAtom = function () {};
  //@unit IReset
  const IReset = function () {};
  //@unit ISwap
  const ISwap = function () {};
  //@unit IVolatile
  const IVolatile = function () {};
  //@unit IWatchable
  const IWatchable = function () {};
  //@unit Fn
  const Fn = function () {};
  //@unit IList
  const IList = function () {};
  //@unit IFn
  const IFn = function () {};
  //@unit ISeqable
  const ISeqable = function () {};
  //@unit ISeq
  const ISeq = function () {};
  //@unit INext
  const INext = function () {};
  //@unit ICounted
  const ICounted = function () {};
  //@unit IIndexed
  const IIndexed = function () {};
  //@unit ILookup
  const ILookup = function () {};
  //@unit ISequential
  const ISequential = function () {};
  //@unit IAssociative
  const IAssociative = function () {};
  //@unit IReduce
  const IReduce = function () {};
  //@unit IKVReduce
  const IKVReduce = function () {};
  //@unit IVector
  const IVector = function () {};
  //@unit IMap
  const IMap = function () {};
  //@unit ICollection
  const ICollection = function () {};
  //@unit IEmptyableCollection
  const IEmptyableCollection = function () {};
  //@unit ISet
  const ISet = function () {};
  //@unit IStack
  const IStack = function () {};
  //@unit IReversible
  const IReversible = function () {};
  //@unit IComparable
  const IComparable = function () {};
  //@unit IIterable
  const IIterable = function () {};
  //@unit IMultiFn
  const IMultiFn = function () {};
  //@unit IEditableCollection
  const IEditableCollection = function () {};
  //@unit ITransientCollection
  const ITransientCollection = function () {};
  //@unit ITransientAssociative
  const ITransientAssociative = function () {};
  //@unit ITransientMap
  const ITransientMap = function () {};
  //@unit ITransientVector
  const ITransientVector = function () {};
  //@unit ITransientSet
  const ITransientSet = function () {};
  //@unit coreProtocolNames requires: ASeq, Fn, IAssociative, IAtom, IChunk, IChunkedNext,
  //@ IChunkedSeq, ICloneable, ICollection, IComparable, ICounted, IDeref, IDerefWithTimeout, IDrop,
  //@ IEditableCollection, IEmptyableCollection, IEncodeClojure, IEncodeJS, IEquiv, IFind, IFn,
  //@ IHash, IIndexed, IIterable, IKVReduce, IList, ILookup, IMap, IMapEntry, IMeta, IMultiFn,
  //@ INamed, INext, Inst, IPending, IPrintWithWriter, IRecord, IReduce, IReset, IReversible, ISeq,
  //@ ISeqable, ISequential, ISet, ISorted, IStack, ISwap, ITransientAssociative,
  //@ ITransientCollection, ITransientMap, ITransientSet, ITransientVector, IUUID, IVector,
  //@ IVolatile, IWatchable, IWithMeta, IWriter
  const coreProtocolNames = new Map([[Inst, "Inst"], [IUUID, "IUUID"], [ASeq, "ASeq"], [IDerefWithTimeout, "IDerefWithTimeout"], [INamed, "INamed"], [IMultiFn, "IMultiFn"], [IEditableCollection, "IEditableCollection"], [ITransientCollection, "ITransientCollection"], [ITransientAssociative, "ITransientAssociative"], [ITransientMap, "ITransientMap"], [ITransientVector, "ITransientVector"], [ITransientSet, "ITransientSet"], [IEncodeJS, "IEncodeJS"], [IEncodeClojure, "IEncodeClojure"], [IChunk, "IChunk"], [IChunkedNext, "IChunkedNext"], [IDrop, "IDrop"], [IPending, "IPending"], [Fn, "Fn"], [IList, "IList"], [IMapEntry, "IMapEntry"], [IFind, "IFind"], [ISorted, "ISorted"], [IChunkedSeq, "IChunkedSeq"], [IRecord, "IRecord"], [ICloneable, "ICloneable"], [IPrintWithWriter, "IPrintWithWriter"], [IWriter, "IWriter"], [IReduce, "IReduce"], [IKVReduce, "IKVReduce"], [IVector, "IVector"], [IMap, "IMap"], [ICollection, "ICollection"], [IEmptyableCollection, "IEmptyableCollection"], [ISet, "ISet"], [IStack, "IStack"], [IReversible, "IReversible"], [IComparable, "IComparable"], [IIterable, "IIterable"], [IHash, "IHash"], [IEquiv, "IEquiv"], [IFn, "IFn"], [ISeqable, "ISeqable"], [ISeq, "ISeq"], [INext, "INext"], [ICounted, "ICounted"], [IIndexed, "IIndexed"], [ILookup, "ILookup"], [ISequential, "ISequential"], [IAssociative, "IAssociative"], [IMeta, "IMeta"], [IWithMeta, "IWithMeta"], [IDeref, "IDeref"], [IAtom, "IAtom"], [IReset, "IReset"], [ISwap, "ISwap"], [IVolatile, "IVolatile"], [IWatchable, "IWatchable"]]);
  //@unit coreProtocolsByName requires: coreProtocolNames
  const coreProtocolsByName = new Map([...coreProtocolNames].map(([protocol, name]) => [name, protocol]));
  //@unit IMeta.defaults effect: IMeta requires: IMeta
  IMeta.function = true;
  //@unit Fn.defaults effect: Fn requires: Fn
  Fn.function = true;
  //@unit ICounted.defaults effect: ICounted requires: ICounted
  ICounted.null = true;
  //@unit IHash.defaults effect: IHash requires: IHash
  IHash._ = true;
  //@unit IEquiv.defaults effect: IEquiv requires: IEquiv
  IEquiv._ = true;
  //@unit meta requires: atoms, deferredVarMetadata, kindHooks, metadataValues, methodKeys, ownMeta,
  //@ protocolHooks
  // Metadata belongs to values, never structural equality or hash payloads.
  function meta(value) {
    const method = value == null ? null : protocolHooks.publicMethod?.(value, methodKeys[35], 1);
    if (method) return method.call(value, value);
    if (ownMeta(value)) return value.meta$LKEY;
    const deferred = deferredVarMetadata.get(value);
    if (deferred) kindHooks.metaDeferred(value, deferred);
    return atoms.has(value) ? value.meta : metadataValues.has(value) ? metadataValues.get(value) : null;
  }
  //@unit definitionVar.meta effect: definitionVar when: meta requires: assoc, deferredVarMetadata,
  //@ keyword, kindHooks, metadataValues
  // A Var's deferred :test metadata; only `definitionVar` defers it.
  kindHooks.metaDeferred = (value, deferred) => {
    const metadata = assoc(metadataValues.get(value), keyword(null, "test"), deferred());
    metadataValues.set(value, metadata);
    deferredVarMetadata.delete(value);
  };
  //@unit empty requires: cellOf, collectionWithMeta, comparatorOf, emptyList, holdsVector,
  //@ isArrayMap, isCell, isChunkedCons, isEntryCell, isLazy, isTreeMap, isView, makeMap, makeSet,
  //@ mapKinds, maps, mapTypes, meta, methodKeys, preserveMeta, protocolHooks,
  //@ publicCollectionSlots, queueFromArray, queues, recordTypes, setComparators, setKinds, sets,
  //@ setType, vector, viewOf
  function empty(value) {
    const method = value == null ? null : protocolHooks.publicMethod?.(value, methodKeys[15], 1);
    if (method) return method.call(value, value);
    if (value == null) return null;
    if (recordTypes.has(value) || isEntryCell(value)) return null;
    if (holdsVector(value)) return preserveMeta(value, vector());
    if (maps.has(value)) return preserveMeta(value, makeMap(mapTypes[mapKinds.get(value)], isArrayMap(value) || isTreeMap(value) || Array.isArray(maps.get(value)) ? [] : {root: null, nil: null, length: 0}, mapKinds.get(value), comparatorOf(value)));
    if (sets.has(value)) return preserveMeta(value, makeSet(setType(setKinds.get(value)), Array.isArray(sets.get(value)) ? [] : {root: null, nil: null, length: 0}, setKinds.get(value), setComparators.get(value)));
    if (queues.has(value)) return preserveMeta(value, queueFromArray([]));
    if (cellOf(value)?.cell$LKEY === "list" && cellOf(value).count$LKEY === 0 && !publicCollectionSlots.has(value)) return value;
    if (isCell(value) || isChunkedCons(value) || isView(value) || isLazy(value))
      return cellOf(value)?.cell$LKEY === "list" || viewOf(value)?.view$LKEY === "queue" || isLazy(value) ? collectionWithMeta(emptyList, meta(value)) : emptyList;
    return null;
  }
  //@unit peek requires: cellOf, first, holdsVector, methodKeys, protocolHooks,
  //@ publicCollectionSlots, queues, vectorNth, vectorStore
  function peek(value) {
    const method = value == null ? null : protocolHooks.builtin?.(value, methodKeys[57], 1);
    if (method) return method.call(value, value);
    if (value == null) return null;
    if (holdsVector(value)) { const store = vectorStore(value); return store.cnt$LKEY ? vectorNth(store, store.cnt$LKEY - 1, null) : null; }
    if (queues.has(value) && publicCollectionSlots.has(value)) return first(publicCollectionSlots.get(value).front);
    if (queues.has(value) || cellOf(value)?.cell$LKEY === "list") return first(value);
    throw new Error("No protocol method IStack.-peek defined for this value");
  }
  //@unit pop requires: cellOf, count, holdsVector, makeVector, meta, methodKeys, next,
  //@ PersistentQueueValue, preserveMeta, protocolHooks, publicCollectionSlots, queueFromArray,
  //@ queues, rest, seq, subvecSources, SubvecValue, vector, vectorPop, vectorStore
  function pop(value) {
    const method = value == null ? null : protocolHooks.builtin?.(value, methodKeys[58], 1);
    if (method) return method.call(value, value);
    if (value == null) return null;
    if (holdsVector(value)) {
      const store = vectorStore(value);
      if (!store.cnt$LKEY) throw new Error("Can't pop empty vector");
      const slots = publicCollectionSlots.get(value);
      if (slots && subvecSources.has(value)) return SubvecValue(meta(value), subvecSources.get(value), slots.start, slots.end - 1, null);
      return preserveMeta(value, makeVector(null, vectorPop(store)));
    }
    if (queues.has(value)) {
      const q = publicCollectionSlots.get(value);
      if (q) { if (q.front == null) return value; const front = next(q.front);
        return PersistentQueueValue(meta(value), q.count - 1, front ?? seq(q.rear), front == null ? vector() : q.rear, null); }
      return preserveMeta(value, queueFromArray(queues.get(value).slice(1)));
    }
    if (cellOf(value)?.cell$LKEY === "list") {
      if (!count(value)) throw new Error("Can't pop empty list");
      return rest(value);
    }
    throw new Error("No protocol method IStack.-pop defined for this value");
  }
  //@unit rseq requires: comparatorOf, holdsVector, IndexedSeqValue, isTreeMap, kindHooks,
  //@ methodKeys, protocolHooks, publicCollectionSlots, RSeqType, toArray, vectorItems, view, viewOf
  function rseq(value) {
    const method = protocolHooks.builtin?.(value, methodKeys[47], 1);
    if (method) return method.call(value, value);
    // A sorted map's walks its tree from its last node (light-cljs-1035).
    if (isTreeMap(value)) return kindHooks.seqTree(value, false);
    if (!holdsVector(value) && viewOf(value)?.view$LKEY !== "indexed" && comparatorOf(value) === undefined)
      throw new Error("No protocol method IReversible.-rseq defined for this value");
    // A vector's items copy leaf by leaf; one a public constructor built, or
    // whose seq a protocol implementation replaces, walks its seq, as before.
    const items = Object.freeze((holdsVector(value) && !publicCollectionSlots.has(value)
      && !protocolHooks.publicMethod?.(value, methodKeys[52], 1, true) ? vectorItems(value) : toArray(value)).reverse());
    return items.length ? view(comparatorOf(value) !== undefined ? IndexedSeqValue : RSeqType, items, 0, false, "reverse") : null;
  }
  //@unit mapProjection requires: KeySeqCell, seq
  function mapProjection(value, index, alreadySeq = false) {
    const entries = alreadySeq ? value : seq(value);
    if (entries === null) return null;
    return new KeySeqCell(index === 0 ? "keys" : "vals", entries);
  }
  //@unit KeySeqCell.type effect: KeySeqCell requires: emptyList, entryKey, entryVal, first,
  //@ KeySeqCell, mapProjection, methodKeys, next, protocolHooks, sequencePrototype
  // A key or value seq's prototype, of no public type: its `first` is the
  // key or value of its first entry, its `rest` the projection of the
  // entries' next, each read when a step asks.
  KeySeqCell.prototype = Object.create(sequencePrototype);
  KeySeqCell.prototype.cljs$lang$kind$$LKEY = 26;
  Object.defineProperties(KeySeqCell.prototype, {
    first$LKEY: {configurable: true, get() {
      const entry = (protocolHooks.firstProtocol ?? first)(this.mseq$LKEY);
      return this.cell$LKEY === "keys" ? (protocolHooks.keyProtocol ?? entryKey)(entry) : (protocolHooks.valProtocol ?? entryVal)(entry);
    }},
    rest$LKEY: {configurable: true, get() {
      const entries = this.mseq$LKEY;
      const following = protocolHooks.satisfies?.(methodKeys[39], entries) ? protocolHooks.nextProtocol(entries) : next(entries);
      return following == null ? emptyList : mapProjection(following, this.cell$LKEY === "keys" ? 0 : 1, true);
    }}
  });
  //@unit keys requires: mapProjection
  function keys(value) { return mapProjection(value, 0); }
  //@unit vals requires: mapProjection
  function vals(value) { return mapProjection(value, 1); }
  //@unit mapProjection.host effect: mapProjection, extendType, extendInstance, treeMapConstructor
  //@ when: mapPrototype requires: es6Iterator, hostHooks, keys, vals
  hostHooks.keys = value => es6Iterator(keys(value));
  hostHooks.values = value => es6Iterator(vals(value));
  //@unit repeat requires: emptyList, invalidArity, RepeatValue
  function repeat(n, item) {
    if (arguments.length === 1) { item = n; n = -1; }
    if (arguments.length < 1 || arguments.length > 2) return invalidArity(arguments.length);
    if (arguments.length === 2 && !(n > 0)) return emptyList;
    return RepeatValue(null, n, item, null, null);
  }
  //@unit cycleNode requires: CycleCell
  function cycleNode(all, previous, current, following = null) {
    return new CycleCell(all, previous, current, following);
  }
  //@unit cycleCurrent requires: next, truth
  // A cycle's current seq (`CycleCell`): the one it was given, else the
  // next of its previous one, or its whole seq again, kept once computed.
  function cycleCurrent(cell) {
    const current = cell.current$LKEY;
    return truth(current) ? current : (cell.current$LKEY = next(cell.prev$LKEY) ?? cell.all$LKEY);
  }
  //@unit cycle requires: cycleNode, emptyList, seq
  function cycle(collection) {
    const start = seq(collection);
    return start === null ? emptyList : cycleNode(start, null, start);
  }
  //@unit compareKeys requires: displayStr, holdsVector, keywordOf, methodKeys, namedOf,
  //@ protocolHooks, uuids, vectorItems
  function compareKeys(a, b) {
    if (a === b) return 0;
    if (a == null) return -1;
    if (b == null) return 1;
    const cannotCompare = () => { throw new Error("Cannot compare " + displayStr(a) + " to " + displayStr(b)); };
    if (typeof a === "number") return typeof b === "number" ? (a < b ? -1 : a > b ? 1 : 0) : cannotCompare();
    const method = protocolHooks.builtin?.(a, methodKeys[9], 2);
    if (method) return method.call(a, a, b);
    if (uuids.has(a)) return uuids.has(b) ? compareKeys(uuids.get(a), uuids.get(b)) : cannotCompare();
    if (a instanceof Date) return b instanceof Date ? compareKeys(a.valueOf(), b.valueOf()) : cannotCompare();
    const left = namedOf(a), right = namedOf(b);
    if (left || right) {
      if (!left || !right || (keywordOf(a) === undefined) !== (keywordOf(b) === undefined)) throw new Error("Cannot compare different types");
      return compareKeys(left.ns$LKEY, right.ns$LKEY) || compareKeys(left.name$LKEY, right.name$LKEY);
    }
    if (holdsVector(a) && holdsVector(b)) {
      const xs = vectorItems(a), ys = vectorItems(b);
      if (xs.length !== ys.length) return xs.length < ys.length ? -1 : 1;
      for (let i = 0; i < xs.length; i++) { const order = compareKeys(xs[i], ys[i]); if (order) return order; }
      return 0;
    }
    if (!(Array.isArray(a) && Array.isArray(b)) && (typeof a !== typeof b || !["string", "boolean"].includes(typeof a))) return cannotCompare();
    return a < b ? -1 : a > b ? 1 : 0;
  }
  //@unit sequentialIndex requires: count, indexIn, nth
  function sequentialIndex(coll, value, start, reverse) {
    return indexIn(count(coll), i => nth(coll, i), value, start, reverse);
  }
  //@unit indexIn requires: equalPair
  function indexIn(length, at, value, start, reverse) {
    if (reverse) {
      if (length === 0) return -1;
      for (let i = start > 0 ? Math.min(length - 1, start) : start < 0 ? length + start : start; i >= 0; i--)
        if (equalPair(at(i), value)) return i;
    } else {
      for (let i = start < 0 ? Math.max(0, length + start) : start; i < length; i++)
        if (equalPair(at(i), value)) return i;
    }
    return -1;
  }
  //@unit sequentialIndex.host effect: sequentialIndex, extendType, extendInstance when: entryPrototype,
  //@ vectorIndexOf requires: hostHooks, sequentialIndex
  hostHooks.index = sequentialIndex;
  //@unit treeSetMaps
  // The sorted map a sorted set's `tree-map` field gives, built once.
  const treeSetMaps = new WeakMap();
  //@unit treeKey requires: nth
  // A sorted map's tree is its nodes (`TreeNodeCell`), or what a public
  // constructor was given, which its readers read as upstream's do, by the
  // names `key`, `val`, `left` and `right` (a node of another kind of map
  // entry by its items); an update first rebuilds such a tree of nodes
  // (`ownTree`). A node of the runtime's has a boolean `red`.
  const treeKey = node => node.red$LKEY !== undefined ? node.tail$LKEY[0] : "key" in node ? node.key : nth(node, 0);
  const treeVal = node => node.red$LKEY !== undefined ? node.tail$LKEY[1] : "val" in node ? node.val : nth(node, 1);
  const treeLeft = node => node.red$LKEY !== undefined ? node.left$LKEY : node.left;
  const treeRight = node => node.red$LKEY !== undefined ? node.right$LKEY : node.right;
  //@unit treeOfPairs requires: TreeNodeCell
  // The tree of `pairs`, keys and values flat in their comparator's order,
  // each key once: balanced, each node at the deepest level red (where a
  // tree is not perfect it has nodes there and above alone, so every path
  // has the same number of black nodes).
  function treeOfPairs(pairs) {
    const n = pairs.length >> 1;
    let levels = 0;
    while ((2 ** levels) - 1 < n) levels++;
    const build = (lo, hi, depth) => {
      if (lo >= hi) return null;
      const mid = (lo + hi) >>> 1;
      return new TreeNodeCell([pairs[2 * mid], pairs[2 * mid + 1]], build(lo, mid, depth + 1), build(mid + 1, hi, depth + 1), depth > 0 && depth === levels - 1);
    };
    return build(0, n, 0);
  }
  //@unit treeOfEntries requires: treeOfPairs
  // The tree of `entries` (`{key, value}`) in their comparator's order, for
  // `makeMap`.
  function treeOfEntries(entries) {
    const pairs = [];
    for (let i = 0; i < entries.length; i++) pairs.push(entries[i].key, entries[i].value);
    return treeOfPairs(pairs);
  }
  //@unit treePairs requires: treeKey, treeLeft, treeRight, treeVal
  // A tree's keys and values, flat, in order (`mapPairs`).
  function treePairs(tree, pairs) {
    if (tree == null) return pairs;
    treePairs(treeLeft(tree), pairs);
    pairs.push(treeKey(tree), treeVal(tree));
    return treePairs(treeRight(tree), pairs);
  }
  //@unit ownTree requires: treeOfPairs, treePairs
  // A sorted map's tree of the runtime's nodes, which its updates walk: its
  // own, or one rebuilt of the tree a public constructor gave it.
  function ownTree(map) {
    const tree = map.tree$LKEY;
    return tree == null ? null : tree.red$LKEY !== undefined ? tree : treeOfPairs(treePairs(tree, []));
  }
  //@unit treeBlacken requires: TreeNodeCell
  // Upstream's node methods and balancing functions of a red-black tree,
  // over a node's `tail` (its key and value), which a rebuilt node shares.
  const treeBlacken = node => node.red$LKEY ? new TreeNodeCell(node.tail$LKEY, node.left$LKEY, node.right$LKEY, false) : node;
  function treeRedden(node) {
    if (node.red$LKEY) throw new Error("red-black tree invariant violation");
    return new TreeNodeCell(node.tail$LKEY, node.left$LKEY, node.right$LKEY, true);
  }
  // Upstream's `balance-left`: a black node of `tail` over `ins` and `right`,
  // rotated where `ins` is red with a red child.
  function treeBalanceLeft(tail, ins, right) {
    if (ins !== null && ins.red$LKEY) {
      const left = ins.left$LKEY, inner = ins.right$LKEY;
      if (left !== null && left.red$LKEY) return new TreeNodeCell(ins.tail$LKEY, treeBlacken(left), new TreeNodeCell(tail, inner, right, false), true);
      if (inner !== null && inner.red$LKEY) return new TreeNodeCell(inner.tail$LKEY, new TreeNodeCell(ins.tail$LKEY, left, inner.left$LKEY, false), new TreeNodeCell(tail, inner.right$LKEY, right, false), true);
    }
    return new TreeNodeCell(tail, ins, right, false);
  }
  // Upstream's `balance-right`, its mirror.
  function treeBalanceRight(tail, left, ins) {
    if (ins !== null && ins.red$LKEY) {
      const inner = ins.left$LKEY, right = ins.right$LKEY;
      if (right !== null && right.red$LKEY) return new TreeNodeCell(ins.tail$LKEY, new TreeNodeCell(tail, left, inner, false), treeBlacken(right), true);
      if (inner !== null && inner.red$LKEY) return new TreeNodeCell(inner.tail$LKEY, new TreeNodeCell(tail, left, inner.left$LKEY, false), new TreeNodeCell(ins.tail$LKEY, inner.right$LKEY, right, false), true);
    }
    return new TreeNodeCell(tail, left, ins, false);
  }
  //@unit treePut requires: equalPair, treeBalanceLeft, treeBalanceRight, TreeNodeCell
  // `assoc` of `key` and `val` in a tree of the runtime's nodes, upstream's
  // `tree-map-add` and `tree-map-replace` in one walk, calling the
  // comparator as upstream's, the key first: the new tree, `tree` itself
  // where it holds the key with an equal value (upstream's `=` of the value
  // given and the one there); `state.added` where it adds the key. A
  // replaced value keeps the key there.
  function treePut(comp, tree, key, val, state) {
    if (tree === null) { state.added = true; return new TreeNodeCell([key, val], null, null, true); }
    const tail = tree.tail$LKEY, order = comp.call(null, key, tail[0]);
    if (order === 0) return equalPair(val, tail[1]) ? tree : new TreeNodeCell([tail[0], val], tree.left$LKEY, tree.right$LKEY, tree.red$LKEY);
    if (order < 0) {
      const left = tree.left$LKEY, ins = treePut(comp, left, key, val, state);
      return ins === left ? tree : !state.added || tree.red$LKEY ? new TreeNodeCell(tail, ins, tree.right$LKEY, tree.red$LKEY) : treeBalanceLeft(tail, ins, tree.right$LKEY);
    }
    const right = tree.right$LKEY, ins = treePut(comp, right, key, val, state);
    return ins === right ? tree : !state.added || tree.red$LKEY ? new TreeNodeCell(tail, tree.left$LKEY, ins, tree.red$LKEY) : treeBalanceRight(tail, tree.left$LKEY, ins);
  }
  //@unit treeRemove requires: treeBalanceLeft, treeBalanceRight, treeBlacken, TreeNodeCell,
  //@ treeRedden
  // Upstream's `tree-map-remove`: the tree without `key`, or null; the
  // removed node in `found[0]`.
  function treeRemove(comp, tree, key, found) {
    if (tree === null) return null;
    const tail = tree.tail$LKEY, order = comp.call(null, key, tail[0]);
    if (order === 0) { found[0] = tree; return treeAppend(tree.left$LKEY, tree.right$LKEY); }
    if (order < 0) {
      const left = tree.left$LKEY, del = treeRemove(comp, left, key, found);
      if (del === null && found[0] === null) return null;
      return left !== null && !left.red$LKEY ? treeBalanceLeftDel(tail, del, tree.right$LKEY) : new TreeNodeCell(tail, del, tree.right$LKEY, true);
    }
    const right = tree.right$LKEY, del = treeRemove(comp, right, key, found);
    if (del === null && found[0] === null) return null;
    return right !== null && !right.red$LKEY ? treeBalanceRightDel(tail, tree.left$LKEY, del) : new TreeNodeCell(tail, tree.left$LKEY, del, true);
  }
  // Upstream's `balance-left-del` and `balance-right-del`.
  function treeBalanceLeftDel(tail, del, right) {
    if (del !== null && del.red$LKEY) return new TreeNodeCell(tail, treeBlacken(del), right, true);
    if (right !== null && !right.red$LKEY) return treeBalanceRight(tail, del, treeRedden(right));
    const inner = right?.left$LKEY;
    if (inner != null && !inner.red$LKEY)
      return new TreeNodeCell(inner.tail$LKEY, new TreeNodeCell(tail, del, inner.left$LKEY, false), treeBalanceRight(right.tail$LKEY, inner.right$LKEY, treeRedden(right.right$LKEY)), true);
    throw new Error("red-black tree invariant violation");
  }
  function treeBalanceRightDel(tail, left, del) {
    if (del !== null && del.red$LKEY) return new TreeNodeCell(tail, left, treeBlacken(del), true);
    if (left !== null && !left.red$LKEY) return treeBalanceLeft(tail, treeRedden(left), del);
    const inner = left?.right$LKEY;
    if (inner != null && !inner.red$LKEY)
      return new TreeNodeCell(inner.tail$LKEY, treeBalanceLeft(left.tail$LKEY, treeRedden(left.left$LKEY), inner.left$LKEY), new TreeNodeCell(tail, inner.right$LKEY, del, false), true);
    throw new Error("red-black tree invariant violation");
  }
  // Upstream's `tree-map-append`: the tree of `left`'s and `right`'s nodes,
  // where a removed node held them.
  function treeAppend(left, right) {
    if (left === null) return right;
    if (right === null) return left;
    if (left.red$LKEY) {
      if (!right.red$LKEY) return new TreeNodeCell(left.tail$LKEY, left.left$LKEY, treeAppend(left.right$LKEY, right), true);
      const app = treeAppend(left.right$LKEY, right.left$LKEY);
      return app !== null && app.red$LKEY
        ? new TreeNodeCell(app.tail$LKEY, new TreeNodeCell(left.tail$LKEY, left.left$LKEY, app.left$LKEY, true), new TreeNodeCell(right.tail$LKEY, app.right$LKEY, right.right$LKEY, true), true)
        : new TreeNodeCell(left.tail$LKEY, left.left$LKEY, new TreeNodeCell(right.tail$LKEY, app, right.right$LKEY, true), true);
    }
    if (right.red$LKEY) return new TreeNodeCell(right.tail$LKEY, treeAppend(left, right.left$LKEY), right.right$LKEY, true);
    const app = treeAppend(left.right$LKEY, right.left$LKEY);
    return app !== null && app.red$LKEY
      ? new TreeNodeCell(app.tail$LKEY, new TreeNodeCell(left.tail$LKEY, left.left$LKEY, app.left$LKEY, false), new TreeNodeCell(right.tail$LKEY, app.right$LKEY, right.right$LKEY, false), true)
      : treeBalanceLeftDel(left.tail$LKEY, left.left$LKEY, new TreeNodeCell(right.tail$LKEY, app, right.right$LKEY, false));
  }
  //@unit treeMapAssoc requires: noBranch, ownTree, preserveMeta, treeBlacken, TreeMapCell, treePut
  // `assoc` of a sorted map, upstream's: the map itself where it holds the
  // key with an equal value, else a map of the new tree, one more in its
  // count (a public constructor's too) where the key is new.
  function treeMapAssoc(self, key, item) {
    const cnt = self.cnt$LKEY;
    if (cnt === undefined && !("cnt$LKEY" in self)) return noBranch;
    const tree = ownTree(self), comp = self.comp$LKEY, state = {added: false}, result = treePut(comp, tree, key, item, state);
    if (result === tree) return self;
    return preserveMeta(self, state.added ? new TreeMapCell(comp, treeBlacken(result), cnt + 1) : new TreeMapCell(comp, result, cnt));
  }
  //@unit treeMapDissoc requires: noBranch, ownTree, preserveMeta, treeBlacken, TreeMapCell,
  //@ treeRemove
  // `dissoc` of a sorted map, upstream's: the map itself without the key.
  function treeMapDissoc(self, key) {
    const cnt = self.cnt$LKEY;
    if (cnt === undefined && !("cnt$LKEY" in self)) return noBranch;
    const found = [null], comp = self.comp$LKEY, result = treeRemove(comp, ownTree(self), key, found);
    if (found[0] === null) return self;
    return preserveMeta(self, result === null ? new TreeMapCell(comp, null, 0) : new TreeMapCell(comp, treeBlacken(result), cnt - 1));
  }
  //@unit treeEntryAt requires: treeKey, treeLeft, treeRight
  // The node of a sorted map that holds `key`, or null, upstream's
  // `entry-at`: a walk of its tree that calls the comparator once a level.
  function treeEntryAt(map, key) {
    const comp = map.comp$LKEY;
    let node = map.tree$LKEY;
    if (node != null && node.red$LKEY === undefined) {
      while (node != null) {
        const order = comp.call(null, key, treeKey(node));
        if (order === 0) return node;
        node = order < 0 ? treeLeft(node) : treeRight(node);
      }
      return null;
    }
    while (node != null) {
      const order = comp.call(null, key, node.tail$LKEY[0]);
      if (order === 0) return node;
      node = order < 0 ? node.left$LKEY : node.right$LKEY;
    }
    return null;
  }
  //@unit treeKvReduce requires: isReduced, treeKey, treeLeft, treeRight, treeVal
  // `reduce-kv` of a tree, upstream's `tree-map-kv-reduce`: in order, until
  // `f` gives a reduced value, which it gives back.
  function treeKvReduce(node, f, init) {
    const left = treeLeft(node);
    if (left != null) { init = treeKvReduce(left, f, init); if (isReduced(init)) return init; }
    init = f.call(null, init, treeKey(node), treeVal(node));
    if (isReduced(init)) return init;
    const right = treeRight(node);
    return right != null ? treeKvReduce(right, f, init) : init;
  }
  //@unit deferredEntries
  // A public positional constructor stores its tree/backing-map immediately,
  // without traversing nodes, hashing keys or calling the comparator. Only
  // operations that consume entries demand this private array facade.
  function deferredEntries(realize) {
    let entries;
    const demand = () => entries ?? (entries = Object.freeze(realize()));
    return new Proxy([], {
      get(_target, property) { return Reflect.get(demand(), property); },
      has(_target, property) { return Reflect.has(demand(), property); }
    });
  }
  //@unit treeMapConstructor requires: constructorHashes, metadataValues, TreeMapCell
  // The public constructor: a sorted map of the comparator, tree and count
  // it is given, as they are (light-cljs-1035), which its operations read
  // as any sorted map's; it reads no node until one of them does.
  function treeMapConstructor(comparator, tree, suppliedCount, metadata, cachedHash) {
    const value = new TreeMapCell(comparator, tree, suppliedCount);
    metadataValues.set(value, metadata);
    if (cachedHash !== null) constructorHashes.set(value, cachedHash);
    return value;
  }
  //@unit treeSetConstructor requires: comparatorOf, compareKeys, constructorSlots, deferredEntries,
  //@ makeSet, mapTraversal, sets
  function treeSetConstructor(metadata, treeMap, cachedHash) {
    const comparator = comparatorOf(treeMap);
    const value = makeSet(treeSetConstructor, [], "sorted-set", comparator === undefined ? compareKeys : comparator);
    sets.reset(value, deferredEntries(() => treeMap == null ? [] : mapTraversal(treeMap).map(entry => Object.freeze({key: entry.key, get hash() { return entry.hash; }}))));
    return constructorSlots(value, {"tree-map": treeMap}, metadata, cachedHash);
  }
  //@unit treeMapSeqConstructor requires: conj, constructorSlots, emptyList, first, ListCell, next,
  //@ protocolHooks, treeLeft, TreeMapStackCell, treeRight, truth
  // The public constructor; and the seqs a sorted map builds (light-cljs-1035),
  // of its own, which build their cells and their stacks' lists here.
  function treeMapSeqConstructor(metadata, stack, ascending, suppliedCount, cachedHash) {
    return constructorSlots(new TreeMapStackCell(stack, ascending, suppliedCount), {stack, "ascending?": ascending, cnt: suppliedCount}, metadata, cachedHash);
  }
  // Whether a sorted map seq's stack is a list of one node or more, whose
  // fields give its first and next as the list's slots do, the stacks that
  // `treeStackPush` builds among them; where a program can give a list
  // protocol implementations (`extend-type`, `specify!`), every stack takes
  // `peek`, `first`, `next` and `conj`, as upstream's.
  const isListCell = stack => !protocolHooks.extend && stack != null && stack.cljs$lang$kind$$LKEY === 12 && stack.count$LKEY >= 1;
  // Upstream's `(conj stack node)` of a sorted map seq's stack: a list cell
  // onto nil or a list, else `conj`.
  const treeStackPush = (stack, node) => stack == null ? new ListCell(node, emptyList, 1) : isListCell(stack) ? new ListCell(node, stack, stack.count$LKEY + 1) : conj(stack, node);
  // Upstream's `tree-map-seq-push`: `stack` with `tree` and its leftmost
  // (rightmost, descending) path pushed.
  function treeMapPush(tree, stack, ascending) {
    const forward = truth(ascending);
    while (tree != null) { stack = treeStackPush(stack, tree); tree = forward ? treeLeft(tree) : treeRight(tree); }
    return stack;
  }
  // A sorted map's seq, `rseq` where not `ascending`: upstream's
  // PersistentTreeMapSeq of its tree's stack and its count, or nil; or a
  // seq of `stack` of an unknown count (`sortedSeqFrom`).
  function treeMapSeq(map, ascending) {
    const cnt = map.cnt$LKEY;
    return cnt > 0 ? new TreeMapStackCell(treeMapPush(map.tree$LKEY, null, ascending), ascending, cnt) : null;
  }
  const treeStackSeq = (stack, ascending) => new TreeMapStackCell(stack, ascending, -1);
  // The seq past a sorted map seq's first node, or null, upstream's `-next`:
  // the next subtree of that node pushed onto the rest of its stack.
  function treeSeqNext(seq) {
    const stack = seq.stack$LKEY, ascending = seq.ascending$LKEY;
    let node, below;
    if (isListCell(stack)) { node = stack.first$LKEY; below = stack.count$LKEY === 1 ? null : stack.rest$LKEY; }
    else { node = first(stack); below = next(stack); }
    const pending = treeMapPush(node == null ? null : truth(ascending) ? treeRight(node) : treeLeft(node), below, ascending);
    return pending == null ? null : new TreeMapStackCell(pending, ascending, seq.cnt$LKEY - 1);
  }
  //@unit sortedPosition
  function sortedPosition(entries, key, comparator) {
    let lo = 0, hi = entries.length;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      if (comparator.call(null, entries[mid].key, key) < 0) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }
  //@unit collectionEntryIndex requires: comparatorOf, entryIndex, kindHooks, mapKinds, setKinds
  function collectionEntryIndex(value, entries, key) {
    const comparator = comparatorOf(value);
    if (comparator) return kindHooks.sortedEntryIndex(entries, key, comparator);
    const kind = mapKinds.get(value) ?? setKinds.get(value);
    return kind === "array-map" ? entryIndex(entries, key) : kindHooks.hashEntryIndex(entries, key, kind);
  }
  //@unit PersistentHashMapValue.entryIndex effect: PersistentHashMapValue, recordMethods,
  //@ publicHashSet, putMember when: collectionEntryIndex requires: hashEntryIndex, kindHooks
  // Maps and sets of a kind other than array-map or sorted: hash maps, hash
  // sets (public hash sets keep their map's kind) and records. Hash-map nil
  // slots are separate from the trie, and a hash map without other keys
  // never hashes a query (`hashEntryIndex` finds the nil key).
  kindHooks.hashEntryIndex = (entries, key, kind) => kind === "hash-map" && key != null && !entries.some(entry => entry.key != null) ? -1 : hashEntryIndex(entries, key);
  //@unit treeMapConstructor.entryIndex effect: treeMapConstructor, treeSetConstructor when: collectionEntryIndex
  //@ requires: kindHooks, sortedPosition
  // Sorted maps and sets, which only their constructors build.
  kindHooks.sortedEntryIndex = (entries, key, comparator) => {
    const i = sortedPosition(entries, key, comparator);
    return i < entries.length && comparator.call(null, entries[i].key, key) === 0 ? i : -1;
  };
  //@unit putSortedEntry requires: entryHashing, equalPair, sortedPosition
  function putSortedEntry(entries, key, value, comparator, member = false) {
    const i = sortedPosition(entries, key, comparator);
    const exists = i < entries.length && comparator.call(null, entries[i].key, key) === 0;
    if (exists && (member || equalPair(entries[i].value, value))) return;
    const retained = exists ? entries[i].key : key;
    const entry = Object.freeze({key: retained, value, get hash() { return entryHashing.hash(retained); }});
    if (exists) entries[i] = entry;
    else entries.splice(i, 0, entry);
  }
  //@unit putCollectionEntry requires: comparatorOf, entryIndex, hashRoots, mapKinds,
  //@ promoteEntries, putEntry, putSortedEntry
  function putCollectionEntry(coll, entries, key, value) {
    const comparator = comparatorOf(coll);
    if (comparator) putSortedEntry(entries, key, value, comparator);
    else {
      const kind = mapKinds.get(coll);
      // A fresh nil map starts as an array map; the ninth distinct key promotes
      // it. Check existing keys before promotion, as canonical assoc does.
      const array = (coll == null || kind === "array-map") &&
        (entries.length < 8 || entryIndex(entries, key) >= 0);
      if (!array && kind === "array-map") {
        const promoted = promoteEntries(entries, {});
        entries.splice(0, entries.length, ...promoted);
        hashRoots.set(entries, hashRoots.get(promoted));
      }
      putEntry(entries, key, value, array ? "array-map" : undefined);
    }
  }
  //@unit buildSortedMap requires: treeBlacken, TreeMapCell, treePut
  // A sorted map of `pairs`, each `assoc`ed in turn, as upstream's
  // `sorted-map` builds it.
  function buildSortedMap(comparator, pairs) {
    let tree = null, cnt = 0;
    for (let i = 0; i < pairs.length; i += 2) {
      const state = {added: false};
      tree = treePut(comparator, tree, pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null, state);
      if (state.added) { tree = treeBlacken(tree); cnt++; }
    }
    return new TreeMapCell(comparator, tree, cnt);
  }
  //@unit sortedMap requires: buildSortedMap, compareKeys
  function sortedMap(...pairs) { return buildSortedMap(compareKeys, pairs); }
  //@unit sortedMapBy requires: buildSortedMap, fnComparator
  function sortedMapBy(comparator, ...pairs) {
    return buildSortedMap(fnComparator(comparator), pairs);
  }
  //@unit buildSortedSet requires: makeSet, putSortedEntry, treeSetConstructor
  function buildSortedSet(comparator, items) {
    const entries = [];
    for (const item of items) putSortedEntry(entries, item, undefined, comparator, true);
    return makeSet(treeSetConstructor, entries, "sorted-set", comparator);
  }
  //@unit sortedSet requires: buildSortedSet, compareKeys
  function sortedSet(...items) { return buildSortedSet(compareKeys, items); }
  //@unit sortedSetBy requires: buildSortedSet, fnComparator
  function sortedSetBy(comparator, ...items) {
    return buildSortedSet(fnComparator(comparator), items);
  }
  //@unit sortedSeqFrom requires: comparatorOf, identity, isTreeMap, mapSeq, sets, sortedPosition,
  //@ treeKey, treeLeft, treeRight, treeStackPush, treeStackSeq, truth
  function sortedSeqFrom(value, key, ascending) {
    // A sorted map's, upstream's: the stack of the path to `key`, of the
    // nodes at or past it in the seq's order, of an unknown count.
    if (isTreeMap(value)) {
      if (!(value.cnt$LKEY > 0)) return null;
      const comp = value.comp$LKEY, forward = truth(ascending);
      let stack = null;
      for (let node = value.tree$LKEY; node != null;) {
        const order = comp.call(null, key, treeKey(node));
        if (order === 0) return treeStackSeq(treeStackPush(stack, node), ascending);
        if (forward ? order < 0 : order > 0) { stack = treeStackPush(stack, node); node = forward ? treeLeft(node) : treeRight(node); }
        else node = forward ? treeRight(node) : treeLeft(node);
      }
      return stack == null ? null : treeStackSeq(stack, ascending);
    }
    const entries = sets.get(value), comparator = comparatorOf(value);
    // A sorted set's members, an array until sets are classes
    // (light-cljs-1013).
    let i = sortedPosition(entries, key, comparator);
    if (!truth(ascending)) {
      if (i === entries.length || comparator.call(null, entries[i].key, key) !== 0) i--;
      if (i < 0) return mapSeq(identity, null);
    } else if (i === entries.length) return mapSeq(identity, null);
    return mapSeq(entry => entry.key, truth(ascending) ? entries.slice(i) : entries.slice(0, i + 1).reverse());
  }
  //@unit mkBoundFn requires: comparatorProtocol, entryKeyProtocol, invalidArity, truth
  function mkBoundFn(sc, test, key) {
    if (arguments.length !== 3) return invalidArity(arguments.length);
    return entry => truth(test.call(null, comparatorProtocol(sc).call(null, entryKeyProtocol(sc, entry), key), 0));
  }
  //@unit subseq requires: first, greater, greaterEqual, invalidArity, mkBoundFn, next,
  //@ sortedSeqFromProtocol, sortedSeqProtocol, takeWhile
  function subseq(sc, test, key, endTest, endKey) {
    if (arguments.length !== 3 && arguments.length !== 5) return invalidArity(arguments.length);
    const include = mkBoundFn(sc, test, key);
    if (arguments.length === 3 && test !== greater && test !== greaterEqual)
      return takeWhile(include, sortedSeqProtocol(sc, true));
    let s = sortedSeqFromProtocol(sc, key, true);
    if (s == null) return null;
    if (!include(first(s))) s = next(s);
    return arguments.length === 3 ? s : takeWhile(mkBoundFn(sc, endTest, endKey), s);
  }
  //@unit rsubseq requires: first, invalidArity, less, lessEqual, mkBoundFn, next,
  //@ sortedSeqFromProtocol, sortedSeqProtocol, takeWhile
  function rsubseq(sc, test, key, endTest, endKey) {
    if (arguments.length !== 3 && arguments.length !== 5) return invalidArity(arguments.length);
    if (arguments.length === 3 && test !== less && test !== lessEqual)
      return takeWhile(mkBoundFn(sc, test, key), sortedSeqProtocol(sc, false));
    const include = arguments.length === 3 ? mkBoundFn(sc, test, key) : mkBoundFn(sc, endTest, endKey);
    let s = sortedSeqFromProtocol(sc, arguments.length === 3 ? key : endKey, false);
    if (s == null) return null;
    if (!include(first(s))) s = next(s);
    return arguments.length === 3 ? s : takeWhile(mkBoundFn(sc, test, key), s);
  }
  //@unit metaSupported requires: metaValue, methodKeys, protocolHooks
  function metaSupported(value) {
    return typeof value === "function" || !!protocolHooks.builtin?.(value, methodKeys[35], 1) || metaValue(value);
  }
  //@unit withMetaSupported requires: methodKeys, protocolHooks, withMetaValue
  function withMetaSupported(value) {
    return !!protocolHooks.builtin?.(value, methodKeys[66], 2) || withMetaValue(value);
  }
  //@unit declarationJournal
  // The open declarationTransaction's undo journal; applications open none.
  let declarationJournal = null;
  //@unit demandProtocol
  // No-ops until setDeclarationDemand installs a compiler session's demand.
  let demandProtocol = () => {}, demandMethod = () => {}, demandMulti = () => {};
  //@unit setDeclarationDemand compile-time requires: demandMethod, demandMulti, demandProtocol,
  //@ print, slotFirst, typeNames
  function setDeclarationDemand(demand) {
    // A session that demands declarations reads no slot before its demand
    // (`slotFirst`).
    slotFirst.off();
    const methodProtocols = new Map();
    demandProtocol = (key, value) => { if (demand) demand(key, value, typeNames.get(value?.constructor)); };
    demandMethod = (key, arity, value) => {
      if (arity === 0) return demandProtocol(key, value);
      let protocol = methodProtocols.get(key);
      if (protocol === undefined) {
        protocol = key.slice(0, key.lastIndexOf("/"));
        methodProtocols.set(key, protocol);
      }
      return demandProtocol(protocol, value);
    };
    demandMulti = mf => { if (demand) demand("multifn:" + print(mf.name, false), mf); };
  }
  //@unit declarationReceiver compile-time requires: demandProtocol
  function declarationReceiver(value) { demandProtocol("Object", value); return value; }
  //@unit patchDeclaration mutator
  // An open declarationTransaction replaces both with journaling versions.
  let patchDeclaration = (value, key, descriptor) => { Object.defineProperty(value, key, descriptor); };
  //@unit setDeclaration mutator
  let setDeclaration = (store, key, value) => { store.set(key, value); };
  //@unit declarationTransaction compile-time requires: declarationJournal, patchDeclaration,
  //@ setDeclaration
  function declarationTransaction(run, capture = false) {
    const undo = [], saved = [declarationJournal, patchDeclaration, setDeclaration];
    declarationJournal = undo;
    patchDeclaration = (value, key, descriptor) => {
      const old = Object.getOwnPropertyDescriptor(value, key);
      undo.push({undo: () => old ? Object.defineProperty(value, key, old) : Reflect.deleteProperty(value, key), redo: () => Object.defineProperty(value, key, descriptor), snapshot: () => { const saved = Object.getOwnPropertyDescriptor(value, key); return () => saved ? Object.defineProperty(value, key, saved) : Reflect.deleteProperty(value, key); }});
      Object.defineProperty(value, key, descriptor);
    };
    setDeclaration = (store, key, value) => {
      const present = store.has(key), previous = store.get(key);
      undo.push({undo: () => present ? store.set(key, previous) : store.delete(key), redo: () => store.set(key, value), snapshot: () => { const saved = store.get(key), has = store.has(key); return () => has ? store.set(key, saved) : store.delete(key); }});
      store.set(key, value);
    };
    try { const value = run(); return capture ? {value, patches: undo} : value; }
    catch (error) {
      for (let i = undo.length - 1; i >= 0; i--) undo[i].undo();
      throw error;
    } finally { [declarationJournal, patchDeclaration, setDeclaration] = saved; }
  }
  //@unit applyDeclarationPatches compile-time
  function applyDeclarationPatches(patches, forward) {
    if (forward) for (const patch of patches) patch.redo();
    else for (let i = patches.length - 1; i >= 0; i--) patches[i].undo();
  }
  //@unit snapshotDeclarationPatches compile-time
  function snapshotDeclarationPatches(patches) { return patches.map(patch => patch.snapshot()); }
  //@unit restoreDeclarationPatches compile-time
  function restoreDeclarationPatches(saved) { for (let i = saved.length - 1; i >= 0; i--) saved[i](); }
  //@unit protocolPredicate requires: demandProtocol, directMethod, nativeType
  function protocolPredicate(protocol, key, value, direct) {
    demandProtocol(key, value);
    if (directMethod(value, key, 0)) return true;
    if (direct) return false;
    return !!(protocol[nativeType(value)] || protocol._);
  }
  //@unit implementsProtocol requires: coreProtocolImplementation, directMethod, protocolNames
  function implementsProtocol(protocol, value) {
    const key = protocolNames.get(protocol);
    if (key) return !!directMethod(value, key, 0);
    return coreProtocolImplementation(protocol, value);
  }
  //@unit satisfies requires: coreProtocolNames, demandProtocol, implementsProtocol, nativeMethods,
  //@ nativeType, ownedProtocolValue, protocolMasks, protocolNames
  function satisfies(protocol, value) {
    demandProtocol(protocolNames.get(protocol) ?? coreProtocolNames.get(protocol), value);
    if (implementsProtocol(protocol, value)) return true;
    const key = protocolNames.get(protocol);
    if (key) return !!(protocol[nativeType(value)] || protocol._);
    if (ownedProtocolValue(value)) return false;
    const coreName = coreProtocolNames.get(protocol);
    const mask = protocolMasks.get(coreName);
    if (mask && value?.[mask[0]]) return false;
    return coreName ? !!(protocol[nativeType(value)] || protocol._ || nativeMethods.get(coreName)?.get(nativeType(value)) || nativeMethods.get(coreName)?.get("_")) : false;
  }
  //@unit countedValue requires: cellOf, chunkBufferOf, chunkOf, holdsVector, isChunkedCons, isLazy,
  //@ isTransient, isView, maps, queues, sets, viewOf
  // A core protocol's native test, without the protocol objects (one per
  // protocol, here ICounted; see `builtinCoreImplementation`).
  function countedValue(value) {
    // A value is in one payload store: a cell or a lazy seq is in no other,
    // and a chunked cons, which `count` walks one by one, is no other value.
    const cell = cellOf(value);
    if (cell !== undefined) return cell.cell$LKEY === "tree-seq" || cell.integer$LKEY === true || cell.cell$LKEY === "list";
    return !isChunkedCons(value) && !isLazy(value) && (isTransient(value) || chunkBufferOf(value) !== undefined || chunkOf(value) !== undefined || holdsVector(value) || maps.has(value) || sets.has(value) || queues.has(value) || (isView(value) && ["indexed", "array-map", "sorted-map", "chunked"].includes(viewOf(value).view$LKEY)));
  }
  //@unit seqValue requires: chunkOf, isCell, isChunkedCons, isLazy, isView, iteratorSeqs, queues
  function seqValue(value) {
    return isCell(value) || isChunkedCons(value) || isLazy(value) || isView(value) || (chunkOf(value)?.step$LKEY ?? null) !== null || queues.has(value) || iteratorSeqs.has(value);
  }
  //@unit nextValue requires: isCell, isChunkedCons, isLazy, isView, queues
  function nextValue(value) {
    return isCell(value) || isChunkedCons(value) || isView(value) || queues.has(value) || isLazy(value);
  }
  //@unit storesPredicate requires: canHoldField, payloads
  // Brand inventories are fixed. Build one private inventory per predicate,
  // without allocating an array and invoking a callback for each probe.
  // The private-field stores answer together, with one read of the value's
  // kind (`payloads`, light-cljs-990); only the stores that are not exact,
  // the WeakMaps and the wider tests of maps and sets (`stores`), are probed
  // one by one.
  // No store holds a value that cannot hold a private field.
  function storesPredicate(...inventory) {
    // Own metadata only: a host may have given `Object.prototype` some. By
    // descriptor, as the Pants engine has no `Object.hasOwn`.
    const kinds = inventory.reduce((bits, store) => bits | Object.getOwnPropertyDescriptor(store, "kindBits")?.value, 0),
      stores = inventory.filter(store => Object.getOwnPropertyDescriptor(store, "exact")?.value !== true);
    return value => {
      if (!canHoldField(value)) return false;
      if (kinds >> payloads.kind(value) & 1) return true;
      for (let i = 0; i < stores.length; i++) if (stores[i].has(value)) return true;
      return false;
    };
  }
  //@unit seqableValue requires: chunkedConsCells, iteratorSeqs, lazyCells, maps, queues, seqCells,
  //@ sets, storesPredicate, vectorCells, viewCells
  const seqableValue = storesPredicate(vectorCells, maps, sets, seqCells, chunkedConsCells, viewCells, queues, lazyCells, iteratorSeqs);
  //@unit indexedValue requires: cellOf, chunkOf, holdsVector, isTransientCell, transientState,
  //@ viewOf
  function indexedValue(value) {
    // Of the transients, a transient vector (`isTransientCell`).
    if (transientState(value)) return isTransientCell(value);
    return chunkOf(value) !== undefined || cellOf(value)?.integer$LKEY === true || holdsVector(value) || viewOf(value)?.indexed$LKEY === true;
  }
  //@unit cloneablePayload requires: cellOf, holdsVector, isEntryCell, maps, queues, recordTypes,
  //@ sets, viewOf
  function cloneablePayload(value) { return (holdsVector(value) && !isEntryCell(value)) || (maps.has(value) && !recordTypes.has(value)) || sets.has(value) || queues.has(value) || ["list", "cons", "range"].includes(cellOf(value)?.cell$LKEY) || ["indexed", "reverse"].includes(viewOf(value)?.view$LKEY); }
  //@unit lookupValue requires: holdsVector, isTransient, maps, sets
  const lookupValue = value => isTransient(value) || holdsVector(value) || maps.has(value) || sets.has(value);
  //@unit associativeValue requires: holdsVector, maps
  const associativeValue = value => holdsVector(value) || maps.has(value);
  //@unit collectionValue requires: chunkedConsCells, lazyCells, maps, queues, seqCells, sets,
  //@ storesPredicate, vectorCells, viewCells
  const collectionValue = storesPredicate(vectorCells, maps, sets, seqCells, chunkedConsCells, viewCells, queues, lazyCells);
  //@unit emptyableValue requires: collectionValue, recordTypes
  const emptyableValue = value => !recordTypes.has(value) && collectionValue(value);
  //@unit editableValue requires: comparatorOf, holdsVector, isEntryCell, maps, recordTypes, sets,
  //@ subvecSources
  const editableValue = value => holdsVector(value) && !isEntryCell(value) && !subvecSources.has(value) || maps.has(value) && !recordTypes.has(value) && comparatorOf(value) === undefined || sets.has(value) && comparatorOf(value) === undefined;
  //@unit reduceValue requires: chunkOf, holdsVector, isCell, isLazy, isView, maps, recordTypes,
  //@ viewOf
  const reduceValue = value => (chunkOf(value)?.step$LKEY === null || holdsVector(value) || (maps.has(value) && !recordTypes.has(value)) || isCell(value) || isLazy(value) || (isView(value) && viewOf(value).view$LKEY !== "queue"));
  //@unit kvReduceValue requires: holdsVector, isEntryCell, maps
  const kvReduceValue = value => holdsVector(value) && !isEntryCell(value) || maps.has(value);
  //@unit iterableValue requires: cellOf, collectionValue, isChunkedCons
  const iterableValue = value => !["cycle", "iterate", "repeat"].includes(cellOf(value)?.cell$LKEY) && !isChunkedCons(value) && collectionValue(value);
  //@unit stackValue requires: cellOf, holdsVector, isCell, queues
  const stackValue = value => !["cycle", "iterate", "range", "tree-seq", "repeat"].includes(cellOf(value)?.cell$LKEY) && (holdsVector(value) || isCell(value) || queues.has(value));
  //@unit chunkedSeqValue requires: cellOf, isChunkedCons, viewOf
  const chunkedSeqValue = value => cellOf(value)?.cell$LKEY === "range" || isChunkedCons(value) || viewOf(value)?.view$LKEY === "chunked";
  //@unit chunkValue requires: chunkOf
  const chunkValue = value => chunkOf(value) !== undefined;
  //@unit namedValue requires: multiFns, namedOf
  const namedValue = value => namedOf(value) !== undefined || multiFns.has(value);
  //@unit ifnValue requires: holdsVector, ifnMethods, isIdent, isTransient, maps, multiFns,
  //@ recordTypes, sets, vars
  const ifnValue = value => multiFns.has(value) || isTransient(value) || !!ifnMethods(value) || isIdent(value) || holdsVector(value) || (maps.has(value) && !recordTypes.has(value)) || sets.has(value) || vars.has(value);
  //@unit fnValue requires: metaFunctions, vars
  const fnValue = value => metaFunctions.has(value) || vars.has(value);
  //@unit multiFnValue requires: multiFns
  const multiFnValue = value => multiFns.has(value);
  //@unit transientValue requires: isTransient
  const transientValue = value => isTransient(value);
  //@unit transientAssociativeValue requires: isTransient, transientState
  const transientAssociativeValue = value => isTransient(value) && transientState(value).kind !== "set";
  //@unit transientVectorValue requires: isTransientCell
  const transientVectorValue = isTransientCell;
  //@unit transientMapValue requires: isMapTransient
  const transientMapValue = value => isMapTransient(value);
  //@unit transientSetValue requires: transientState
  const transientSetValue = value => transientState(value)?.kind === "set";
  //@unit mapEntryValue requires: isEntryCell
  const mapEntryValue = value => isEntryCell(value);
  //@unit sortedValue requires: comparatorOf
  const sortedValue = value => comparatorOf(value) !== undefined;
  //@unit findValue requires: holdsVector, maps, recordTypes
  const findValue = value => holdsVector(value) || (maps.has(value) && !recordTypes.has(value));
  //@unit reversibleValue requires: comparatorOf, holdsVector, viewOf
  const reversibleValue = value => holdsVector(value) || viewOf(value)?.view$LKEY === "indexed" || comparatorOf(value) !== undefined;
  //@unit dropValue requires: cellOf, holdsVector, isLazy, mapKinds, viewOf
  const dropValue = value => {
    // As `countedValue`, a cell or a lazy seq is in no other store.
    const cell = cellOf(value);
    if (cell !== undefined) return cell.integer$LKEY === true || cell.cell$LKEY === "repeat";
    return !isLazy(value) && (mapKinds.get(value) === "array-map" || holdsVector(value) || ["indexed", "chunked", "array-map"].includes(viewOf(value)?.view$LKEY));
  };
  //@unit metaValue requires: atoms, chunkedConsCells, lazyCells, maps, metaFunctions, queues,
  //@ reifyValues, seqCells, sets, storesPredicate, symbolCells, vars, vectorCells, viewCells
  const metaValue = storesPredicate(symbolCells, vectorCells, maps, sets, seqCells, chunkedConsCells, viewCells, queues, lazyCells, vars, atoms, metaFunctions, reifyValues);
  //@unit withMetaValue requires: chunkedConsCells, lazyCells, maps, metaFunctions, queues,
  //@ reifyValues, seqCells, sets, storesPredicate, symbolCells, vars, vectorCells, viewCells
  const withMetaValue = storesPredicate(symbolCells, vectorCells, maps, sets, seqCells, chunkedConsCells, viewCells, queues, lazyCells, vars, metaFunctions, reifyValues);
  //@unit instValue
  const instValue = value => value instanceof Date;
  //@unit uuidImplementation requires: uuids
  const uuidImplementation = value => uuids.has(value);
  //@unit aseqValue requires: cellOf, isChunkedCons, publicCollectionSlots, viewOf
  function aseqValue(value) {
    const cell = cellOf(value), kind = viewOf(value)?.view$LKEY;
    return publicCollectionSlots.has(value) && cell?.cell$LKEY === "list" || kind === "chunked" || cell?.cell$LKEY === "cons" || (cell?.cell$LKEY === "list" && cell.count$LKEY > 0) || isChunkedCons(value) || kind === "indexed" || kind === "hash-map";
  }
  //@unit listValue requires: cellOf
  const listValue = value => ["list", "cons"].includes(cellOf(value)?.cell$LKEY);
  //@unit printableValue requires: isTransient, ownedProtocolValue
  const printableValue = value => !isTransient(value) && ownedProtocolValue(value);
  //@unit hashableValue requires: cellOf, printableValue
  const hashableValue = value => !["cycle", "iterate"].includes(cellOf(value)?.cell$LKEY) && printableValue(value);
  //@unit vectorValue requires: holdsVector
  const vectorValue = value => holdsVector(value);
  //@unit mapValue requires: maps
  const mapValue = value => maps.has(value);
  //@unit setValue requires: sets
  const setValue = value => sets.has(value);
  //@unit comparableValue requires: holdsVector, namedOf, uuids
  const comparableValue = value => holdsVector(value) || uuids.has(value) || namedOf(value) !== undefined || value instanceof Date;
  //@unit atomValue requires: atoms, directMethod, isTransient
  const atomValue = value => atoms.has(value) || !isTransient(value) && !!directMethod(value, "IAtom", 0);
  //@unit pendingValue requires: cellOf, delays, isLazy
  const pendingValue = value => ["repeat", "cycle", "iterate"].includes(cellOf(value)?.cell$LKEY) || delays.has(value) || isLazy(value);
  //@unit derefValue requires: atoms, delays, reducedValues, vars, volatiles
  const derefValue = value => delays.has(value) || vars.has(value) || atoms.has(value) || volatiles.has(value) || reducedValues.has(value);
  //@unit volatileValue requires: volatiles
  const volatileValue = value => volatiles.has(value);
  //@unit watchableValue requires: atoms
  const watchableValue = value => atoms.has(value);
  //@unit writerValue requires: stringBufferWriters
  const writerValue = value => stringBufferWriters.has(value);
  //@unit coreProtocolImplementation requires: builtinCoreImplementation, coreProtocolNames,
  //@ directMethod, protocolMasks
  function coreProtocolImplementation(protocol, value) {
    const name = coreProtocolNames.get(protocol);
    const mask = protocolMasks.get(name);
    if (mask && (value?.[mask[0]] & mask[1])) return true;
    if (name && directMethod(value, name, 0)) return true;
    return builtinCoreImplementation(protocol, value);
  }
  //@unit builtinCoreImplementation requires: ASeq, aseqValue, associativeValue, atomValue,
  //@ chunkedSeqValue, chunkValue, cloneablePayload, collectionValue, comparableValue, countedValue,
  //@ derefValue, dropValue, editableValue, emptyableValue, findValue, Fn, fnValue, hashableValue,
  //@ IAssociative, IAtom, IChunk, IChunkedNext, IChunkedSeq, ICloneable, ICollection, IComparable,
  //@ ICounted, IDeref, IDrop, IEditableCollection, IEmptyableCollection, IEquiv, IFind, IFn,
  //@ ifnValue, IHash, IIndexed, IIterable, IKVReduce, IList, ILookup, IMap, IMapEntry, IMeta,
  //@ IMultiFn, INamed, indexedValue, INext, Inst, instValue, IPending, IPrintWithWriter, IReduce,
  //@ IReversible, ISeq, ISeqable, ISequential, ISet, ISorted, IStack, iterableValue,
  //@ ITransientAssociative, ITransientCollection, ITransientMap, ITransientSet, ITransientVector,
  //@ IUUID, IVector, IVolatile, IWatchable, IWithMeta, IWriter, kvReduceValue, listValue,
  //@ lookupValue, mapEntryValue, mapValue, metaValue, multiFnValue, namedValue, nextValue,
  //@ pendingValue, printableValue, reduceValue, reversibleValue, seqableValue, sequential,
  //@ seqValue, setValue, sortedValue, stackValue, transientAssociativeValue, transientMapValue,
  //@ transientSetValue, transientValue, transientVectorValue, uuidImplementation, vectorValue,
  //@ volatileValue, watchableValue, withMetaValue, writerValue
  function builtinCoreImplementation(protocol, value) { return !!nativeImplementations.get(protocol)?.(value); }
  const nativeImplementations = new Map([[Inst, instValue], [IUUID, uuidImplementation], [ASeq, aseqValue], [INamed, namedValue], [IMultiFn, multiFnValue], [IFn, ifnValue], [IEditableCollection, editableValue], [ITransientCollection, transientValue], [ITransientAssociative, transientAssociativeValue], [ITransientVector, transientVectorValue], [ITransientMap, transientMapValue], [ITransientSet, transientSetValue], [Fn, fnValue], [IList, listValue], [IChunk, chunkValue], [IChunkedNext, chunkedSeqValue], [IDrop, dropValue], [IReduce, reduceValue], [IKVReduce, kvReduceValue], [IPrintWithWriter, printableValue], [IHash, hashableValue], [IEquiv, hashableValue], [IMapEntry, mapEntryValue], [IFind, findValue], [ISorted, sortedValue], [IChunkedSeq, chunkedSeqValue], [ICloneable, cloneablePayload], [IVector, vectorValue], [IMap, mapValue], [ISet, setValue], [ICollection, collectionValue], [IEmptyableCollection, emptyableValue], [IStack, stackValue], [IReversible, reversibleValue], [IIterable, iterableValue], [IComparable, comparableValue], [ISeqable, seqableValue], [ISeq, seqValue], [INext, nextValue], [ISequential, sequential], [ICounted, countedValue], [IIndexed, indexedValue], [ILookup, lookupValue], [IAssociative, associativeValue], [IMeta, metaValue], [IWithMeta, withMetaValue], [IAtom, atomValue], [IPending, pendingValue], [IDeref, derefValue], [IVolatile, volatileValue], [IWatchable, watchableValue], [IWriter, writerValue]]);
  //@unit metaProtocol requires: meta, metaSupported
  function metaProtocol(value) {
    if (!metaSupported(value)) throw new Error("No protocol method IMeta.-meta defined for this value");
    return meta(value);
  }
  //@unit withMetaProtocol requires: collectionWithMeta, kindHooks, protocolHooks, withMetaSupported
  function withMetaProtocol(value, metadata) {
    if (!withMetaSupported(value)) throw new Error("No protocol method IWithMeta.-with-meta defined for this value");
    // A function here has metadata, or a protocol implementation.
    return typeof value === "function" ? (protocolHooks.fnWithMeta ?? kindHooks.withMetaFn)(value, metadata) : collectionWithMeta(value, metadata);
  }
  //@unit preserveMeta requires: meta, metadataValues, ownMeta, setMeta
  // `result` with the metadata of `original` where it has some: one in its
  // own field has some unless it is nil (`ownMeta`).
  function preserveMeta(original, result) {
    const metadata = meta(original);
    if (metadata != null || (ownMeta(original) ? metadata !== null : metadataValues.has(original))) setMeta(result, metadata);
    return result;
  }
  //@unit setMeta requires: metadataValues, ownMeta
  // Gives a value the metadata `metadata`: in its own field (`ownMeta`), else
  // in `metadataValues`.
  function setMeta(value, metadata) {
    if (ownMeta(value)) value.meta$LKEY = metadata;
    else metadataValues.set(value, metadata);
  }
  //@unit varyMeta requires: apply, meta, withMeta
  function varyMeta(value, fn, ...args) { return withMeta(value, apply(fn, meta(value), args)); }
  //@unit resetMeta
  // CLJS reference metadata operations assign .meta; Var uses a separate
  // immutable _meta, so reset-meta! does not change what (meta #'x) observes.
  function resetMeta(value, metadata) { value.meta = metadata; return metadata; }
  //@unit alterMeta requires: apply, resetMeta
  function alterMeta(value, fn, ...args) { return resetMeta(value, apply(fn, value.meta, args)); }
  //@unit Atom requires: objectEquiv
  class Atom {
    equiv(other) { return objectEquiv(this, other); }
    constructor(state, meta, validator, watches) {
      this.state = state; this.meta = meta; this.validator = validator; this.watches = watches;
      /*@brand*/
    }
  }
  //@unit Atom.kind effect: Atom requires: Atom
  // An atom is a class of the class layout, kind 38 (`atoms`, light-cljs-1013).
  Atom.prototype.cljs$lang$kind$$LKEY = 38;
  //@unit Volatile
  class Volatile {
    constructor(state) { this.state = state; /*@brand*/ }
  }
  //@unit Volatile.kind effect: Volatile requires: Volatile
  // A volatile is a class of the class layout, kind 39 (`volatiles`).
  Volatile.prototype.cljs$lang$kind$$LKEY = 39;
  //@unit atomFactory requires: Atom
  function atomFactory(state, meta, validator, watches) { return new Atom(state, meta, validator, watches); }
  //@unit atom requires: Atom, get, keyword, listFromArray, seqToMap
  function atom(state, ...options) {
    const opts = seqToMap(listFromArray(options));
    // Construction does not validate the initial state in ClojureScript.
    return new Atom(state, get(opts, keyword(null, "meta")), get(opts, keyword(null, "validator")), null);
  }
  //@unit globalHierarchy
  // Hierarchies are ordinary immutable language maps. The global reference,
  // multimethods and caches belong to this runtime factory (one phase/session).
  let globalHierarchy = null;
  //@unit multiFns
  // The multimethods: instances of `MultiFn`, whose prototype's keyed kind
  // is 41 (light-cljs-1013), with their own `dispatch_fn` field.
  const multiFns = {has: value => value != null && value.cljs$lang$kind$$LKEY === 41 && "dispatch_fn" in value};
  //@unit makeHierarchy requires: arrayMapFromArray, invalidArity, keyword
  function makeHierarchy() {
    if (arguments.length) return invalidArity(arguments.length);
    return arrayMapFromArray([keyword(null, "parents"), arrayMapFromArray([]), keyword(null, "ancestors"), arrayMapFromArray([]), keyword(null, "descendants"), arrayMapFromArray([])]);
  }
  //@unit getGlobalHierarchy requires: Atom, globalHierarchy, invalidArity, makeHierarchy,
  //@ protocolHooks
  function getGlobalHierarchy() {
    if (arguments.length) return invalidArity(arguments.length);
    // `(atom h)`, whose option lookups only protocol implementations can see.
    return globalHierarchy ??= protocolHooks.atom ? protocolHooks.atom(makeHierarchy()) : new Atom(makeHierarchy(), null, null, null);
  }
  //@unit hierarchyPart requires: get, keyword
  function hierarchyPart(h, part) { return get(h, keyword(null, part)); }
  //@unit bases requires: invalidArity, listFromArray
  function bases(value) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    if (value == null || value === false) return null;
    const base = Object.getPrototypeOf(value);
    return base == null ? null : listFromArray([base]);
  }
  //@unit supers requires: bases, first, invalidArity, setFromArray
  function supers(value) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    const result = [];
    for (let pending = bases(value); pending != null; pending = bases(first(pending))) result.push(first(pending));
    return result.length ? setFromArray(result) : null;
  }
  //@unit isa requires: deref, getGlobalHierarchy, invalidArity, isaIn
  function isa(h, child, parent) {
    if (arguments.length === 2) return isaIn(deref(getGlobalHierarchy()), h, child);
    if (arguments.length !== 3) return invalidArity(arguments.length);
    return isaIn(h, child, parent);
  }
  //@unit isaIn requires: contains, equalPair, get, hierarchyPart, isaItems, protocolHooks
  // `(isa? h child parent)`, which multimethods call without `deref`.
  function isaIn(h, child, parent) {
    if (equalPair(child, parent)) return true;
    if (typeof parent === "function" && typeof child === "function" && child instanceof parent) return true;
    const ancestors = hierarchyPart(h, "ancestors");
    if (contains(get(ancestors, child), parent)) return true;
    // Any of `(supers child)`, whose set only protocol implementations can
    // see being built.
    if (typeof child === "function") {
      let bases = protocolHooks.supers?.(child);
      if (!bases) { bases = []; for (let base = Object.getPrototypeOf(child); base != null; base = Object.getPrototypeOf(base)) bases.push(base); }
      for (const base of bases) if (contains(get(ancestors, base), parent)) return true;
    }
    const cv = isaItems(child), pv = isaItems(parent);
    return !!(cv && pv && cv.length === pv.length && cv.every((v, i) => isaIn(h, v, pv[i])));
  }
  //@unit isaPlain requires: equalPair, isaItems
  // `isaIn` of a hierarchy without ancestors, which multimethods have where
  // their tables are arrays (`multi_tables` in src/rta.rs): nothing derives.
  function isaPlain(child, parent) {
    if (equalPair(child, parent)) return true;
    if (typeof parent === "function" && typeof child === "function" && child instanceof parent) return true;
    const cv = isaItems(child), pv = isaItems(parent);
    return !!(cv && pv && cv.length === pv.length && cv.every((v, i) => isaPlain(v, pv[i])));
  }
  //@unit leanCached requires: isaItems
  // Whether `isaPlain`'s answers for `dispatch` can change while the methods
  // do not, or differ from the cache's first entries, the methods: a
  // function's prototypes can change and make it isa more or fewer keys, also
  // as an item of a vector. Any other value isa only the keys equal to it.
  const leanCached = value => typeof value === "function" || !!isaItems(value)?.some(leanCached);
  //@unit isaItems requires: holdsVector, kindHooks, vectorStore
  // The items of a vector that `isa?` compares, or null. Where no vector can
  // be built map entries, which hold their two in the tail of their store,
  // are the only values with items (`makeVector.isa`).
  const isaItems = value => !holdsVector(value) ? null : kindHooks.isaItems ? kindHooks.isaItems(value) : vectorStore(value).tail$LKEY;
  //@unit makeVector.isa effect: makeVector when: isaItems requires: kindHooks, vectorItems
  kindHooks.isaItems = vectorItems;
  //@unit hierarchyRel requires: bases, count, deref, get, getGlobalHierarchy, hierarchyPart,
  //@ invalidArity, setFromArray, supers, toArray
  function hierarchyRel(part, h, tag, arity) {
    if (arity === 1) { tag = h; h = deref(getGlobalHierarchy()); }
    else if (arity !== 2) return invalidArity(arity);
    let result = get(hierarchyPart(h, part), tag);
    if (typeof tag === "function") {
      if (part === "descendants") throw new Error("Can't get descendants of constructors");
      const prototypes = toArray(part === "parents" ? bases(tag) : supers(tag));
      const values = toArray(result).concat(prototypes);
      if (part === "ancestors") for (const base of prototypes) values.push(...toArray(get(hierarchyPart(h, part), base)));
      result = setFromArray(values);
    }
    return count(result) ? result : null;
  }
  //@unit parents requires: hierarchyRel
  function parents(h, tag) { return hierarchyRel("parents", h, tag, arguments.length); }
  //@unit ancestors requires: hierarchyRel
  function ancestors(h, tag) { return hierarchyRel("ancestors", h, tag, arguments.length); }
  //@unit descendants requires: hierarchyRel
  function descendants(h, tag) { return hierarchyRel("descendants", h, tag, arguments.length); }
  //@unit derive requires: assoc, conj, contains, deref, equalPair, get, getGlobalHierarchy,
  //@ hierarchyPart, invalidArity, isIdent, keyword, mapFromArray, namespace, printHooks, reset,
  //@ setFromArray, toArray
  function derive(h, tag, parent) {
    if (arguments.length === 2) {
      parent = tag; tag = h;
      if (!isIdent(parent) || !namespace(parent) || !(typeof tag === "function" || isIdent(tag) && namespace(tag))) throw new Error("Assert failed: qualified derive tags");
      const ref = getGlobalHierarchy(); reset(ref, derive(deref(ref), tag, parent)); return null;
    }
    if (arguments.length !== 3) return invalidArity(arguments.length);
    if (equalPair(tag, parent) || !(typeof tag === "function" || isIdent(tag)) || !isIdent(parent)) throw new Error("Assert failed: derive tags");
    const p = hierarchyPart(h, "parents"), a = hierarchyPart(h, "ancestors"), d = hierarchyPart(h, "descendants");
    if (contains(get(p, tag), parent)) return h;
    if (contains(get(a, tag), parent)) throw new Error(printHooks.text(tag) + "already has" + printHooks.text(parent) + "as ancestor");
    if (contains(get(a, parent), tag)) throw new Error("Cyclic derivation:" + printHooks.text(parent) + "has" + printHooks.text(tag) + "as ancestor");
    function closure(table, source, sources, target, targets) {
      let result = table;
      for (const key of [source, ...toArray(get(sources, source))]) result = assoc(result, key, setFromArray([...toArray(get(targets, key)), target, ...toArray(get(targets, target))]));
      return result;
    }
    return mapFromArray([keyword(null, "parents"), assoc(p, tag, conj(get(p, tag, setFromArray([])), parent)), keyword(null, "ancestors"), closure(a, tag, d, parent, a), keyword(null, "descendants"), closure(d, parent, a, tag, d)]);
  }
  //@unit underive requires: assoc, contains, count, deref, derive, disj, dissoc, get,
  //@ getGlobalHierarchy, hierarchyPart, invalidArity, makeHierarchy, nth, reset, toArray
  function underive(h, tag, parent) {
    if (arguments.length === 2) { const ref = getGlobalHierarchy(); reset(ref, underive(deref(ref), h, tag)); return null; }
    if (arguments.length !== 3) return invalidArity(arguments.length);
    const p = hierarchyPart(h, "parents");
    if (!contains(get(p, tag), parent)) return h;
    const remaining = disj(get(p, tag), parent), updated = count(remaining) ? assoc(p, tag, remaining) : dissoc(p, tag);
    let result = makeHierarchy();
    for (const entry of toArray(updated)) for (const ancestor of toArray(nth(entry, 1))) result = derive(result, nth(entry, 0), ancestor);
    return result;
  }
  //@unit MultiFn requires: getMethod, multiHooks, printHooks, truth
  class MultiFn {
    constructor(name, dispatchFn, defaultDispatchVal, hierarchy, methodTable, preferTable, methodCache, cachedHierarchy) {
      this.name = name; this.dispatch_fn = dispatchFn; this.default_dispatch_val = defaultDispatchVal;
      this.hierarchy = hierarchy; this.method_table = methodTable; this.prefer_table = preferTable;
      this.method_cache = methodCache; this.cached_hierarchy = cachedHierarchy;
      /*@brand*/
    }
    call(_self, ...args) {
      // `-get-method`, which only multimethods implement where their tables
      // are arrays (`multi_tables` in src/rta.rs).
      const dispatch = this.dispatch_fn.call(null, ...args), target = (multiHooks.getMethodProtocol ?? getMethod)(this, dispatch);
      if (!truth(target)) throw new Error("No method in multimethod '" + printHooks.text(this.name) + "' for dispatch value: " + printHooks.text(dispatch));
      return target.call(null, ...args);
    }
    apply(_self, args) { return this.call(null, ...Array.from(args)); }
  }
  //@unit MultiFn.kind effect: MultiFn requires: MultiFn
  // A multimethod is a class of the class layout, kind 41 (`multiFns`,
  // light-cljs-1013).
  MultiFn.prototype.cljs$lang$kind$$LKEY = 41;
  //@unit multiState requires: atoms, declarationJournal, reset
  function multiState(ref, value) {
    // Journal compiler declaration table/cache writes for failed bundles and
    // source-frontier rewinds; ordinary runtime mutations retain source order.
    const transaction = declarationJournal;
    if (transaction && atoms.has(ref)) {
      const previous = ref.state;
      transaction.push({undo: () => { ref.state = previous; }, redo: () => { ref.state = value; }, snapshot: () => { const saved = ref.state; return () => { ref.state = saved; }; }});
    }
    return reset(ref, value);
  }
  //@unit resetMultiCache requires: deref, multiState
  function resetMultiCache(mf) {
    multiState(mf.method_cache, deref(mf.method_table));
    return multiState(mf.cached_hierarchy, deref(mf.hierarchy));
  }
  //@unit resetMulti requires: demandMulti, mapFromArray, multiState
  function resetMulti(mf) {
    demandMulti(mf);
    multiState(mf.method_table, mapFromArray([])); multiState(mf.method_cache, mapFromArray([]));
    multiState(mf.prefer_table, mapFromArray([])); multiState(mf.cached_hierarchy, null); return mf;
  }
  //@unit multiHooks
  // Multimethods keep their tables in their atoms where `MultiFn.tables` is
  // kept; elsewhere (only at `Advanced`) a multimethod's method table and
  // cache are arrays of the entries their array maps would hold
  // (`leanTables`), which the same steps read and write.
  const multiHooks = {};
  //@unit leanTables requires: entryIndex
  const leanTables = new WeakMap();
  // The value of `key` in `entries`, as `get` of an array map gives it.
  const leanGet = (entries, key) => entries[entryIndex(entries, key)]?.value;
  // As `assoc` of an array map: a replaced entry keeps its first key.
  function leanPut(entries, key, value) {
    const index = entryIndex(entries, key);
    if (index < 0) entries.push({key, value}); else entries[index] = {key: entries[index].key, value};
  }
  //@unit multiTables
  // Kept where an `Advanced` program could see the tables otherwise, fill
  // one past eight methods or give a hierarchy ancestors (`multi_tables` in
  // src/rta.rs).
  const multiTables = null;
  //@unit leanAdd requires: leanPut, leanTables
  // `-add-method` of a multimethod whose tables are arrays.
  function leanAdd(mf, dispatch, method) {
    const methods = leanTables.get(mf)?.methods ?? [];
    // `resetMultiCache`: the cache starts as the method table; answers for
    // functions, equal only to themselves, are cached by identity (`fns`).
    leanPut(methods, dispatch, method); leanTables.set(mf, {methods, cache: methods.slice(), fns: new Map()}); return mf;
  }
  //@unit addMethod requires: leanAdd, multiHooks
  function addMethod(mf, dispatch, method) { return (multiHooks.addMethod ?? leanAdd)(mf, dispatch, method); }
  //@unit addMethodDirect requires: leanAdd, multiFns
  // `-add-method` where only multimethods implement `IMultiFn`, which
  // `Advanced` output calls there (`lean_multi` in src/rta.rs).
  function addMethodDirect(mf, dispatch, method) {
    if (multiFns.has(mf)) return leanAdd(mf, dispatch, method);
    throw new Error("No protocol method IMultiFn/-add-method defined for this value");
  }
  //@unit removeMethod requires: demandMulti, deref, dissoc, multiState, resetMultiCache
  function removeMethod(mf, dispatch) {
    demandMulti(mf); multiState(mf.method_table, dissoc(deref(mf.method_table), dispatch)); resetMultiCache(mf); return mf;
  }
  //@unit multiPrefers requires: deref, get, truth
  function multiPrefers(mf, x, y) {
    // The pinned CLJS prefers* loops do not return recursive matches: only the
    // direct preference is effective, unlike Clojure's inherited preference.
    return truth(get(get(deref(mf.prefer_table), x), y));
  }
  //@unit preferMethod requires: assoc, conj, demandMulti, deref, get, multiPrefers, multiState,
  //@ printHooks, resetMultiCache, setFromArray
  function preferMethod(mf, x, y) {
    demandMulti(mf);
    if (multiPrefers(mf, y, x)) throw new Error("Preference conflict in multimethod '" + printHooks.text(mf.name) + "': " + printHooks.text(y) + " is already preferred to " + printHooks.text(x));
    const table = deref(mf.prefer_table);
    multiState(mf.prefer_table, assoc(table, x, conj(get(table, x, setFromArray([])), y)));
    return resetMultiCache(mf);
  }
  //@unit bestMethod requires: printHooks
  // The entry of a method table, in table order, whose key (`keyOf`)
  // `dispatch` isa and that dominates every other one.
  function bestMethod(mf, dispatch, entries, keyOf, isa, dominates) {
    let best = null;
    for (const entry of entries) {
      const key = keyOf(entry);
      if (!isa(dispatch, key)) continue;
      const candidate = best === null || dominates(key, keyOf(best)) ? entry : best;
      if (!dominates(keyOf(candidate), key)) throw new Error("Multiple methods in multimethod '" + printHooks.text(mf.name) + "' match dispatch value: " + printHooks.text(dispatch) + " -> " + printHooks.text(key) + " and " + printHooks.text(keyOf(candidate)) + ", and neither is preferred");
      best = candidate;
    }
    return best;
  }
  //@unit getMethod requires: bestMethod, isaPlain, leanCached, leanGet, leanPut, leanTables,
  //@ multiHooks, truth
  function getMethod(mf, dispatch) {
    if (multiHooks.getMethod) return multiHooks.getMethod(mf, dispatch);
    // As `MultiFn.tables` where the hierarchy, which then never changes, has
    // no ancestors, and without preferences, which only it can have.
    // Only an answer that can change while the methods do not tells the
    // cache from a new walk (`leanCached`), so only such answers are cached:
    // the cache, a scan of entries, stays as small as their dispatch values,
    // and a function's, which only it equals, is a lookup by identity.
    const tables = leanTables.get(mf) ?? {methods: [], cache: [], fns: new Map()}, cache = leanCached(dispatch);
    const fn = typeof dispatch === "function";
    const cached = cache && ((fn ? tables.fns.get(dispatch) : undefined) ?? leanGet(tables.cache, dispatch));
    if (truth(cached)) return cached;
    const best = bestMethod(mf, dispatch, tables.methods, entry => entry.key, isaPlain, isaPlain);
    const target = best === null ? leanGet(tables.methods, mf.default_dispatch_val) : best.value;
    if (!truth(target)) return null;
    if (fn) tables.fns.set(dispatch, target); else if (cache) leanPut(tables.cache, dispatch, target);
    return target;
  }
  //@unit MultiFn.tables effect: multiHooks when: methods, MultiFnFactory, multiTables, prefers,
  //@ preferMethod, removeMethod, resetMulti requires: assoc, bestMethod, declarationJournal,
  //@ demandMulti, deref, equalPair, get, getMethodProtocol, isaIn, multiHooks, multiPrefers,
  //@ multiState, nth, resetMultiCache, toArray, truth
  multiHooks.getMethodProtocol = (mf, dispatch) => getMethodProtocol(mf, dispatch);
  // Only the tables in atoms run in a compiler session, which demands the
  // multimethod first (`setDeclarationDemand`).
  multiHooks.addMethod = (mf, dispatch, method) => {
    // Bundle installation must not demand itself or later method declarations.
    if (!declarationJournal) demandMulti(mf);
    multiState(mf.method_table, assoc(deref(mf.method_table), dispatch, method)); resetMultiCache(mf); return mf;
  };
  multiHooks.getMethod = (mf, dispatch) => {
    demandMulti(mf);
    for (;;) {
      const h = deref(mf.hierarchy);
      if (!equalPair(deref(mf.cached_hierarchy), h)) resetMultiCache(mf);
      const cached = get(deref(mf.method_cache), dispatch);
      if (truth(cached)) return cached;
      const best = bestMethod(mf, dispatch, toArray(deref(mf.method_table)), entry => nth(entry, 0), (x, y) => isaIn(h, x, y), (x, y) => multiPrefers(mf, x, y) || isaIn(h, x, y));
      const target = best === null ? get(deref(mf.method_table), mf.default_dispatch_val) : nth(best, 1);
      if (!truth(target)) return null;
      if (!equalPair(deref(mf.cached_hierarchy), deref(mf.hierarchy))) { resetMultiCache(mf); continue; }
      multiState(mf.method_cache, assoc(deref(mf.method_cache), dispatch, target)); return target;
    }
  };

  //@unit stateProtocol requires: protocolHooks
  // `resolved` is the resolved key (`methodKey`) of `key`, where protocol
  // implementations exist.
  function stateProtocol(value, key, resolved, args) {
    const method = protocolHooks.builtin?.(value, resolved, args.length + 1);
    if (!method) throw new Error("No protocol method " + key + " defined for this value");
    return method.call(value, value, ...args);
  }
  //@unit resetProtocol requires: methodKeys, stateProtocol
  function resetProtocol(value, state) { return stateProtocol(value, "IReset/-reset!", methodKeys[45], [state]); }
  //@unit swapProtocol requires: invalidArity, methodKeys, stateProtocol
  function swapProtocol(value, fn, ...args) {
    if (arguments.length < 2 || arguments.length > 5) return invalidArity(arguments.length);
    return stateProtocol(value, "ISwap/-swap!", methodKeys[59], [fn, ...args]);
  }
  //@unit notifyWatches requires: atoms, first, methodKeys, next, nth, protocolHooks, seq,
  //@ stateProtocol
  function notifyWatches(value, oldState, state) {
    const method = protocolHooks.builtin?.(value, methodKeys[64], 3);
    if (method) return method.call(value, value, oldState, state);
    if (!atoms.has(value)) return stateProtocol(value, "IWatchable/-notify-watches", methodKeys[64], [oldState, state]);
    // Hold the original persistent watch map through all synchronous callbacks.
    for (let s = seq(value.watches); s !== null; s = next(s)) {
      const entry = first(s);
      nth(entry, 1).call(null, nth(entry, 0), value, oldState, state);
    }
    return null;
  }
  //@unit addWatchProtocol requires: assoc, atoms, methodKeys, protocolHooks, stateProtocol
  function addWatchProtocol(value, key, fn) {
    const method = protocolHooks.builtin?.(value, methodKeys[63], 3);
    if (method) return method.call(value, value, key, fn);
    if (!atoms.has(value)) return stateProtocol(value, "IWatchable/-add-watch", methodKeys[63], [key, fn]);
    value.watches = assoc(value.watches, key, fn);
    return value;
  }
  //@unit removeWatchProtocol requires: atoms, dissoc, methodKeys, protocolHooks, stateProtocol
  function removeWatchProtocol(value, key) {
    const method = protocolHooks.builtin?.(value, methodKeys[65], 2);
    if (method) return method.call(value, value, key);
    if (!atoms.has(value)) return stateProtocol(value, "IWatchable/-remove-watch", methodKeys[65], [key]);
    return value.watches = dissoc(value.watches, key);
  }
  //@unit addWatch requires: addWatchProtocol
  function addWatch(value, key, fn) { addWatchProtocol(value, key, fn); return value; }
  //@unit removeWatch requires: removeWatchProtocol
  function removeWatch(value, key) { removeWatchProtocol(value, key); return value; }
  //@unit resetAtom requires: notifyWatches, truth, vector
  function resetAtom(value, state, pair) {
    const validator = value.validator;
    if (validator != null && !truth(validator.call(null, state))) throw new Error("Validator rejected reference state");
    // A validator may have changed the atom; capture old state only afterward.
    const oldState = value.state;
    value.state = state;
    if (value.watches != null) notifyWatches(value, oldState, state);
    return pair ? vector(oldState, state) : state;
  }
  //@unit reset requires: atoms, resetAtom, resetProtocol
  function reset(value, state) { return atoms.has(value) ? resetAtom(value, state, false) : resetProtocol(value, state); }
  //@unit resetVals requires: atoms, deref, resetAtom, resetProtocol, vector
  function resetVals(value, state) { return atoms.has(value) ? resetAtom(value, state, true) : vector(deref(value), resetProtocol(value, state)); }
  //@unit swapState requires: apply, atoms, deref, first, methodKeys, next, resetAtom, rest,
  //@ stateProtocol, toArray, vector
  function swapState(value, fn, args, pair, variadic = false) {
    if (atoms.has(value)) {
      const state = variadic ? apply(fn, value.state, first(args), first(next(args)), rest(next(args))) : fn.call(null, value.state, ...toArray(args));
      return resetAtom(value, state, pair);
    }
    const oldState = pair ? deref(value) : null;
    const inputs = variadic ? [fn, first(args), first(next(args)), rest(next(args))] : [fn, ...toArray(args)];
    const result = stateProtocol(value, "ISwap/-swap!", methodKeys[59], inputs);
    return pair ? vector(oldState, result) : result;
  }
  //@unit swap requires: apply, cons, fnRest, isAtomCell, resetAtom, swapState
  function swap(value, fn, ...args) {
    if (arguments.length < 2) return swapState(value, fn, cons(undefined, cons(undefined, null)), false, true);
    // An atom's `swap!` passes `fn` its arguments as they came, or past two
    // the first two and an indexed seq of the others, as upstream's `(apply
    // f state x y more)`, and builds no seq of them all for `swapState` to
    // walk (light-cljs-1013). Another atom (the Pants build's engine atoms)
    // takes `swapState`.
    if (isAtomCell(value)) return resetAtom(value, args.length <= 2 ? fn.call(null, value.state, ...args) : apply(fn, value.state, args[0], args[1], fnRest(args, 2)), false);
    return swapState(value, fn, fnRest(args, 0), false, args.length > 2);
  }
  //@unit swapVals requires: apply, cons, fnRest, isAtomCell, resetAtom, swapState
  function swapVals(value, fn, ...args) {
    if (arguments.length < 2) return swapState(value, fn, cons(undefined, cons(undefined, null)), true, true);
    if (isAtomCell(value)) return resetAtom(value, args.length <= 2 ? fn.call(null, value.state, ...args) : apply(fn, value.state, args[0], args[1], fnRest(args, 2)), true);
    return swapState(value, fn, fnRest(args, 0), true, args.length > 2);
  }
  //@unit compareAndSet requires: deref, equalPair, reset
  function compareAndSet(value, oldState, state) {
    // Both pinned CLJS runtimes use = here, including equal distinct collections.
    if (!equalPair(deref(value), oldState)) return false;
    reset(value, state);
    return true;
  }
  //@unit setValidator requires: deref, truth
  function setValidator(value, validator) {
    if (validator != null && !truth(validator.call(null, deref(value)))) throw new Error("Validator rejected reference state");
    return value.validator = validator;
  }
  //@unit getValidator
  function getValidator(value) { return value.validator; }
  //@unit volatile requires: Volatile
  function volatile(state) { return new Volatile(state); }
  //@unit volatileFactory requires: Volatile
  function volatileFactory(state) { return new Volatile(state); }
  //@unit isVolatile requires: volatiles
  function isVolatile(value) { return volatiles.has(value); }
  //@unit vreset requires: vresetProtocol
  function vreset(value, state) { return vresetProtocol(value, state); }
  //@unit vresetProtocol requires: methodKeys, protocolHooks, stateProtocol, volatiles
  function vresetProtocol(value, state) {
    const method = protocolHooks.builtin?.(value, methodKeys[62], 2);
    if (method) return method.call(value, value, state);
    if (volatiles.has(value)) return value.state = state;
    return stateProtocol(value, "IVolatile/-vreset!", methodKeys[62], [state]);
  }
  //@unit update requires: apply, assoc, fnRest, get
  function update(value, key, fn, ...args) {
    const old = get(value, key);
    return assoc(value, key, arguments.length >= 3 && arguments.length <= 6
      ? fn.call(null, old, ...args)
      : apply(fn, old, args[0], args[1], args[2], fnRest(args, 3)));
  }
  //@unit updatedMap requires: comparatorOf, constructorSlots, isArrayMap, isHashMap, kindHooks,
  //@ makeMap, mapFromEntries, mapKinds, mapTypes, meta, preserveMeta, priorLength,
  //@ publicCollectionSlots, recordTypes
  // The map of `original`'s kind with `entries`, which an update built: an
  // array map's count of entries is its pairs' (`arrayMapEntries`), a hash
  // map's its `cnt` (light-cljs-1036). A public constructor's array map
  // keeps its count's difference from its pairs in its result's slots and
  // field; a public hash map's count is a field, which its own operations
  // update.
  function updatedMap(original, entries, removeRecord = false) {
    const Type = !removeRecord && recordTypes.get(original), kind = mapKinds.get(original);
    // A sorted map's own updates build its tree (`treeMapAssoc`); these
    // entries are its own, in its comparator's order.
    if (kind === "sorted-map") return preserveMeta(original, makeMap(mapTypes["sorted-map"], entries, "sorted-map", comparatorOf(original)));
    const result = Type ? kindHooks.updatedRecord(Type, entries, kind)
      : mapFromEntries(entries, kind === "hash-map" || (entries.length > 8 && entries.length > priorLength(original)));
    const slots = publicCollectionSlots.get(original);
    if (slots?.cnt != null) {
      const cnt = slots.cnt + entries.length - priorLength(original);
      // A map of the classes has its count in its field (`arrayMapCount`,
      // `hashMapCount`), and an array map's slots keep its pairs, for its
      // public fields; another (the Pants build's engine maps) keeps both
      // in its slots.
      const nil = Array.isArray(entries) ? entries.find(e => e.key == null) : entries.nil;
      if (isHashMap(result)) result.cnt$LKEY = cnt;
      else constructorSlots(result, mapKinds.get(result) === "array-map" ? {cnt, arr: entries.flatMap(e => [e.key, e.value])}
        : {cnt, root: result, "has-nil?": nil != null, "nil-val": nil?.value}, meta(original), null);
      if (isArrayMap(result)) result.cnt$LKEY = cnt;
    }
    return preserveMeta(original, result);
  }
  //@unit priorLength requires: isArrayMap, isHashMap, maps
  // The count of entries an update of `original` starts from (`updatedMap`).
  function priorLength(original) {
    return isArrayMap(original) ? (original.arr$LKEY.length + 1) >> 1 : isHashMap(original) ? original.cnt$LKEY : maps.get(original)?.length ?? 0;
  }
  //@unit withMeta requires: collectionWithMeta, fnWithMeta
  function withMeta(value, metadata) {
    return typeof value === "function" ? fnWithMeta(value, metadata) : collectionWithMeta(value, metadata);
  }
  //@unit fnWithMeta requires: apply, makeIFn, metadataValues, metaFunctions
  // A function, or a function with metadata, with `metadata`.
  function fnWithMeta(value, metadata) {
    const original = metaFunctions.get(value) || value;
    const methods = {};
    for (let n = 0; n <= 20; n++) methods[n] = function (_self, ...args) { return original.call(null, ...args); };
    methods[21] = function (_self, ...args) { return apply(original, ...args); };
    const result = makeIFn(methods);
    metaFunctions.set(result, original);
    metadataValues.set(result, metadata);
    return result;
  }
  //@unit collectionWithMeta requires: canonicalVectorRoots, cellCopy, cellOf, chunkedConsCells,
  //@ constructorHashes, holdsVector, ifnValues, isCell, isChunkedCons, isEntryCell, isLazy, isView,
  //@ kindHooks, lazyCells, mapCellCopy, maps, meta, metaFunctions, methodKeys, protocolHooks,
  //@ protocolImplementations, publicCollectionSlots, queues, recordTypes, reifyValues, seqCells,
  //@ setCellOf, setComparators, setKinds, setMeta, setQueueCopy, sets, subvecSources, symbolCells,
  //@ treeSetMaps, vars, vectorCellCopy, vectorCells, viewAt, viewCells, viewOf
  // `withMeta` of a value other than a function, for callers that pass no
  // function.
  function collectionWithMeta(value, metadata) {
    if (value == null) return null;
    const method = protocolHooks.builtin?.(value, methodKeys[66], 2);
    if (method) return method.call(value, value, metadata);
    if (!recordTypes.has(value) && !isEntryCell(value) &&
        [vectorCells, maps, sets, seqCells, chunkedConsCells, viewCells, queues, lazyCells].some(store => store.has(value)) && meta(value) === metadata) return value;
    let result;
    if (isEntryCell(value)) return collectionWithMeta(kindHooks.entryVector(value), metadata);
    const kind = cellOf(value)?.cell$LKEY;
    if (metaFunctions.has(value)) return kindHooks.withMetaFn(value, metadata);
    else if (isLazy(value)) result = kindHooks.withMetaLazy(value, metadata);
    else if (kind === "range") result = kindHooks.withMetaRange(value, metadata);
    else if (isChunkedCons(value)) result = kindHooks.withMetaChunkedCons(value, metadata);
    else if (kind === "repeat") result = kindHooks.withMetaRepeat(value, metadata);
    else if (kind === "iterate") result = kindHooks.withMetaIterate(value);
    else if (viewOf(value)?.vec$LKEY) result = kindHooks.withMetaChunkedSeq(value, metadata);
    else if (kind === "cycle") result = kindHooks.withMetaCycle(value);
    else if (recordTypes.has(value)) result = kindHooks.withMetaRecord(value);
    else {
      const stores = [treeSetMaps, canonicalVectorRoots, publicCollectionSlots, constructorHashes, symbolCells, maps, sets, setKinds, setComparators, seqCells, viewCells, queues, vars, subvecSources];
      const vector = holdsVector(value);
      if (!vector && !stores.some(store => store.has(value)) && !reifyValues.has(value)) throw new Error("value does not support metadata");
      // A vector's, a view's, a symbol's or a map's copy is built by its
      // class, with its fields.
      const set = setCellOf(value) !== undefined;
      result = vector ? vectorCellCopy(value) : isView(value) ? viewAt(value, value.i$LKEY) : isCell(value) ? cellCopy(value) : maps.has(value) ? mapCellCopy(value)
        : setQueueCopy(value) ?? kindHooks.symbolCopy?.(value) ?? Object.create(Object.getPrototypeOf(value));
      // A copy of a reify value has no brand of its type: `reifyValues` holds it.
      // The class layout's tests, which hold nothing, are no stores.
      for (const store of [...stores, protocolImplementations]) if (store.set !== undefined && store !== viewCells && store !== symbolCells && store !== seqCells && store !== maps && !(set && (store === sets || store === setKinds)) && store.has(value)) store.set(result, store.get(value));
      if (reifyValues.has(value)) reifyValues.add(result);
      if (ifnValues.has(value)) ifnValues.set(result, ifnValues.get(value));
      if (ifnValues.has(value) || reifyValues.has(value) || recordTypes.has(value)) Object.defineProperties(result, Object.getOwnPropertyDescriptors(value));
    }
    setMeta(result, metadata);
    return result;
  }
  //@unit fnWithMeta.meta effect: fnWithMeta when: collectionWithMeta, withMetaProtocol requires: fnWithMeta,
  //@ kindHooks
  kindHooks.withMetaFn = fnWithMeta;
  //@unit SymbolCell.meta effect: symbol, SymbolValue when: collectionWithMeta requires: kindHooks,
  //@ SymbolCell, symbolOf
  // A symbol's metadata copy: a symbol cell of its prototype with its
  // fields, else undefined; `collectionWithMeta` then sets its `meta`.
  kindHooks.symbolCopy = value => {
    const symbol = symbolOf(value);
    if (symbol === undefined) return undefined;
    const Type = function () {};
    Type.prototype = Object.getPrototypeOf(value);
    return Reflect.construct(SymbolCell, [symbol.ns$LKEY, symbol.name$LKEY, symbol.str$LKEY, symbol.hash$LKEY, symbol.meta$LKEY], Type);
  };
  //@unit mapEntry.meta effect: mapEntry, clone when: collectionWithMeta requires: kindHooks,
  //@ makeVector, vectorStore
  // A map entry's metadata copy is a vector.
  kindHooks.entryVector = value => makeVector(vectorStore(value).tail$LKEY.slice());
  //@unit LazySeq.meta effect: LazySeq when: collectionWithMeta requires: kindHooks, lazyOf,
  //@ LazySeq, seq
  kindHooks.withMetaLazy = (value, metadata) => new LazySeq(metadata, () => seq(value), null, lazyOf(value).hash$LKEY);
  //@unit rangeNode.meta effect: rangeNode when: collectionWithMeta requires: cellOf,
  //@ constructorHashes, IntegerRangeValue, kindHooks, rangeChunks, RangeValue
  kindHooks.withMetaRange = (value, metadata) => {
    const c = cellOf(value), state = rangeChunks.get(c);
    return c.integer$LKEY ? IntegerRangeValue(metadata, c.first$LKEY, c.end$LKEY, c.step$LKEY, c.count$LKEY, constructorHashes.get(value))
      : RangeValue(metadata, c.first$LKEY, c.end$LKEY, c.step$LKEY, state?.chunk, state?.tail, constructorHashes.get(value));
  };
  //@unit ChunkedCons.meta effect: ChunkedCons when: collectionWithMeta requires: ChunkedCons,
  //@ chunkedConsOf, kindHooks
  kindHooks.withMetaChunkedCons = (value, metadata) => {
    const c = chunkedConsOf(value);
    return new ChunkedCons(c.chunk$LKEY, c.more$LKEY, metadata, c.hash$LKEY);
  };
  //@unit RepeatValue.meta effect: RepeatValue when: collectionWithMeta requires: cellOf, kindHooks,
  //@ RepeatValue
  kindHooks.withMetaRepeat = (value, metadata) => {
    const c = cellOf(value);
    return RepeatValue(metadata, c.remaining$LKEY, c.first$LKEY, c.next$LKEY ?? null, null);
  };
  //@unit iterateNode.meta effect: iterateNode when: collectionWithMeta requires: cellOf,
  //@ iterateNode, kindHooks
  kindHooks.withMetaIterate = value => {
    const c = cellOf(value);
    return iterateNode(c.f$LKEY, c.prev$LKEY, c.seed$LKEY, c.realized$LKEY, c.next$LKEY);
  };
  //@unit ChunkedSeqValue.meta effect: ChunkedSeqValue when: collectionWithMeta requires: ChunkedSeqValue,
  //@ kindHooks
  kindHooks.withMetaChunkedSeq = (value, metadata) =>
    ChunkedSeqValue(value.vec$LKEY, value.arr$LKEY, value.base$LKEY, value.i$LKEY, metadata, null);
  //@unit cycleNode.meta effect: cycleNode when: collectionWithMeta requires: cellOf, cycleNode,
  //@ kindHooks
  kindHooks.withMetaCycle = value => {
    const c = cellOf(value);
    return cycleNode(c.all$LKEY, c.prev$LKEY, c.current$LKEY, c.next$LKEY);
  };
  //@unit makeType.meta effect: recordMethods when: collectionWithMeta, updatedMap requires: equalPair,
  //@ kindHooks, mapKinds, maps, recordKeys, recordMaps, recordTypes
  // A record copy owns fresh mutable host fields. Copying accessor
  // descriptors would leave their closures pointing at the old instance.
  kindHooks.withMetaRecord = value => {
    const Type = recordTypes.get(value), result = new Type(...Type.members.map(member => value[member]));
    recordMaps.set(result, maps.get(value)); recordMaps.kinds.set(result, mapKinds.get(value));
    return result;
  };
  // A record of `entries`, for `updatedMap`.
  kindHooks.updatedRecord = (Type, entries, kind) => {
    const result = new Type(...recordKeys(Type).map(key => {
      const entry = entries.find(e => equalPair(e.key, key));
      return entry?.value;
    }));
    recordMaps.set(result, Object.freeze(entries)); recordMaps.kinds.set(result, kind);
    return result;
  };
  //@unit uuidPrototype requires: unkeyedJSON, uuids
  const uuidPrototype = Object.freeze({
    toString() { return uuids.get(this); },
    // JSON of a UUID (`UUIDCell`): its own properties but its keyed fields.
    toJSON() { return unkeyedJSON(this); }
  });
  //@unit UUIDType effect requires: builtinType, UUIDCell, uuidPrototype, uuidValue, viewType
  // A UUID is a class of its type's prototype (`UUIDCell`, light-cljs-1013).
  const UUIDType = viewType(builtinType(function (text, cachedHash) { return uuidValue(text, cachedHash); }, uuidPrototype), UUIDCell, 42);
  //@unit UUIDCell.type effect: UUIDCell requires: UUIDCell, UUIDType
  UUIDCell.prototype = UUIDType.prototype;
  //@unit uuid requires: uuidValue
  function uuid(text) {
    if (typeof text !== "string") throw new Error("UUID expects a string");
    return uuidValue(text.toLowerCase(), null);
  }
  //@unit uuidValue requires: UUIDCell
  function uuidValue(text, cachedHash) { return new UUIDCell(text, cachedHash); }
  //@unit tapListeners
  // Explicit embedding hooks. Scheduling is optional; the default dispatches
  // synchronously, with a finite drain budget even when taps enqueue more taps.
  const tapListeners = new Set(), tapQueue = [];
  //@unit tapDraining
  let tapDraining = false, tapDelivered = 0, tapPending = 0;
  //@unit tapLimit
  let tapLimit = 1024;
  //@unit defaultTapSchedule
  function defaultTapSchedule(task) { task.call(null); return true; }
  //@unit configureHostHooks requires: coreCells, defaultHostClock, defaultHostRandom,
  //@ defaultTapSchedule, tapLimit
  function configureHostHooks(options = {}) {
    for (const key of ["clock", "random", "tapSchedule"]) {
      if (Object.hasOwn(options, key) && options[key] !== null && typeof options[key] !== "function") throw new TypeError(key + " hook must be a function or null");
    }
    if (Object.hasOwn(options, "maxPendingTaps") && (!Number.isSafeInteger(options.maxPendingTaps) || options.maxPendingTaps < 1)) throw new TypeError("maxPendingTaps must be a positive integer");
    if (Object.hasOwn(options, "clock")) coreCells.hostClock = options.clock ?? defaultHostClock;
    if (Object.hasOwn(options, "random")) coreCells.hostRandom = options.random ?? defaultHostRandom;
    if (Object.hasOwn(options, "tapSchedule")) coreCells.execTapFn = options.tapSchedule ?? defaultTapSchedule;
    if (Object.hasOwn(options, "maxPendingTaps")) tapLimit = options.maxPendingTaps;
    return null;
  }
  //@unit defaultHostClock
  function defaultHostClock() {
    return typeof globalThis.performance?.now === "function" ? globalThis.performance.now() : Date.now();
  }
  //@unit defaultHostRandom
  function defaultHostRandom() { return Math.random(); }
  //@unit systemTime requires: coreCells, invalidArity
  function systemTime() {
    if (arguments.length) return invalidArity(arguments.length);
    const value = coreCells.hostClock.call(null);
    if (typeof value !== "number" || !Number.isFinite(value)) throw new TypeError("clock hook must return a finite number");
    return value;
  }
  //@unit hostRandom requires: coreCells
  function hostRandom() {
    const value = coreCells.hostRandom.call(null);
    if (typeof value !== "number" || !(value >= 0 && value < 1)) throw new TypeError("random hook must return a number in [0, 1)");
    return value;
  }
  //@unit hostRand requires: hostRandom, invalidArity
  function hostRand(n) {
    if (arguments.length > 1) return invalidArity(arguments.length);
    return arguments.length ? hostRandom() * n : hostRandom();
  }
  //@unit hostRandInt requires: hostRandom, invalidArity
  function hostRandInt(n) { if (arguments.length !== 1) return invalidArity(arguments.length); return Math.floor(hostRandom() * n); }
  //@unit hostRandNth requires: count, hostRandInt, invalidArity, nth
  function hostRandNth(coll) { if (arguments.length !== 1) return invalidArity(arguments.length); return nth(coll, hostRandInt(count(coll))); }
  //@unit hostShuffle requires: hostRandInt, invalidArity, toArray, vectorFromArray
  function hostShuffle(coll) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    const items = toArray(coll);
    for (let i = items.length - 1; i > 0; i--) { const j = hostRandInt(i + 1); [items[i], items[j]] = [items[j], items[i]]; }
    return vectorFromArray(items);
  }
  //@unit hostRandomUuid requires: hostRandInt, invalidArity, uuid
  function hostRandomUuid() {
    if (arguments.length) return invalidArity(arguments.length);
    return uuid("xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, digit => { const value = hostRandInt(16); return (digit === "x" ? value : (value & 3) | 8).toString(16); }));
  }
  //@unit addTap requires: invalidArity, tapListeners
  function addTap(listener) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    if (listener == null || typeof listener.call !== "function") throw new TypeError("add-tap requires a callable value");
    tapListeners.add(listener); return null;
  }
  //@unit removeTap requires: invalidArity, tapListeners
  function removeTap(listener) { if (arguments.length !== 1) return invalidArity(arguments.length); tapListeners.delete(listener); return null; }
  //@unit deliverTap
  function deliverTap(listeners, value) {
    for (const listener of listeners) { try { listener.call(null, value); } catch (_) { /* One listener cannot suppress the others. */ } }
  }
  //@unit tapGreater requires: coreCells, defaultTapSchedule, drainTaps, invalidArity, tapDelivered,
  //@ tapDraining, tapLimit, tapPending, tapQueue, truth
  function tapGreater(value) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    if (tapPending + tapQueue.length + (tapDraining ? tapDelivered : 0) >= tapLimit) return false;
    const schedule = coreCells.execTapFn;
    if (schedule !== defaultTapSchedule) {
      tapPending++;
      let active = true;
      const task = () => {
        if (!active) return null;
        active = false; tapPending--;
        // The same drain guard bounds a scheduler which chooses to run inline.
        tapQueue.push(value); drainTaps(); return null;
      };
      try {
        const admitted = truth(schedule.call(null, task));
        if (!admitted && active) { active = false; tapPending--; }
        return admitted;
      } catch (_) { if (active) { active = false; tapPending--; } return false; }
    }
    tapQueue.push(value); drainTaps(); return true;
  }
  //@unit drainTaps requires: deliverTap, tapDelivered, tapDraining, tapLimit, tapListeners,
  //@ tapQueue
  function drainTaps() {
    if (tapDraining) return;
    tapDraining = true; tapDelivered = 0;
    try {
      while (tapQueue.length && tapDelivered < tapLimit) {
        const value = tapQueue.shift(); tapDelivered++;
        deliverTap([...tapListeners], value);
      }
    } finally { tapQueue.length = 0; tapDraining = false; tapDelivered = 0; }
  }
  //@unit makeVar requires: apply, makeIFn, metadataValues, Var, vars
  function makeVar(get, symbol, metadata = null) {
    const methods = {};
    for (let n = 0; n <= 20; n++) methods[n] = function (_self, ...args) { return get.call(null).call(null, ...args); };
    methods[21] = function (_self, ...args) { return apply(get.call(null), ...args); };
    const value = makeIFn(methods);
    Object.setPrototypeOf(value, Var.prototype);
    vars.set(value, Object.freeze({get, symbol}));
    metadataValues.set(value, metadata);
    return value;
  }
  //@unit definitionVar requires: assoc, deferredVarMetadata, into, keyword, listFromArray, makeVar,
  //@ mapFromArray, name, namespace, symbolCore, truth
  function definitionVar(get, symbol, metadata = null, deferredMetadata = null) {
    const root = deferredMetadata ? null : get.call(null);
    const base = mapFromArray([keyword(null, "ns"), symbolCore(namespace(symbol)), keyword(null, "name"), symbolCore(name(symbol)),
      keyword(null, "doc"), null, keyword(null, "file"), null, keyword(null, "line"), null, keyword(null, "column"), null, keyword(null, "arglists"), listFromArray([])]);
    const value = makeVar(get, symbol, assoc(into(base, metadata), keyword(null, "ns"), symbolCore(namespace(symbol)), keyword(null, "name"), symbolCore(name(symbol)), keyword(null, "test"), truth(root) ? root.cljs$lang$test : null));
    if (deferredMetadata) deferredVarMetadata.set(value, deferredMetadata);
    return value;
  }
  //@unit Var requires: makeVar, metadataValues
  function Var(get, symbol, metadata) { const value = makeVar(get, symbol, metadata); metadataValues.set(value, metadata); return value; }
  //@unit test requires: get, keyword, meta, truth, vars
  function test(value) {
    const fn = vars.has(value) ? get(meta(value), keyword(null, "test"))
      : value == null ? null : value.cljs$lang$test;
    if (truth(fn)) { fn.call(null); return keyword(null, "ok"); }
    return keyword(null, "no-test");
  }
  //@unit varPrototype requires: ifnApply, ifnPrototype, meta, Var, vars
  const varPrototype = Object.freeze({
    constructor: Var,
    isMacro() { return vars.get(this).get.call(null).cljs$lang$macro; },
    get val() { return vars.get(this).get; },
    get sym() { return vars.get(this).symbol; },
    get _meta() { return meta(this); },
    apply(_self, args) { return ifnApply(this, args); },
    call(_self, ...args) { return ifnPrototype.call.call(this, _self, ...args); },
    toString() { return "#'" + vars.get(this).symbol; }
  });
  //@unit Var.prototype effect: Var requires: builtinType, Var, varPrototype
  // Extensible, like every builtin type's: `extend-type Var` installs methods
  // here, above the frozen base every Var inherits through it.
  builtinType(Var, varPrototype);
  //@unit namespaces compile-time
  // Namespace identities are session-owned, with source registration from emitted units.
  const namespaces = new Map();
  //@unit Namespace effect requires: equalPair, hash, installMethods, keyword, makeVar,
  //@ mapFromArray, mungeStr, str, symbol
  function Namespace(obj, name) {
    if (!(this instanceof Namespace)) return new Namespace(obj, name);
    this.obj = obj; this.name = name;
    installMethods(this, [["IEquiv/-equiv", 2, (_self, other) => other instanceof Namespace && equalPair(name, other.name)],
      ["IHash/-hash", 1, () => hash(name)]]);
  }
  Namespace.prototype.getName = function () { return this.name; };
  Namespace.prototype.toString = function () { return String(this.name); };
  Namespace.prototype.findInternedVar = function (local) {
    if (this.obj == null) return null;
    const key = mungeStr(str(local));
    if (!Object.prototype.hasOwnProperty.call(this.obj, key)) return null;
    return makeVar(() => this.obj[key], symbol(str(this.name), str(local)), mapFromArray([keyword(null, "ns"), this]), true);
  };
  //@unit createNs compile-time mutator requires: invalidArity, Namespace, namespaces, str, symbolOf
  function createNs(name, obj = null) {
    if (arguments.length !== 1 && arguments.length !== 2) return invalidArity(arguments.length);
    if (symbolOf(name) === undefined) throw new TypeError("create-ns requires a symbol");
    const key = str(name);
    if (!namespaces.has(key)) namespaces.set(key, new Namespace(obj, name));
    else if (obj != null) namespaces.get(key).obj = obj;
    return namespaces.get(key);
  }
  //@unit readerCoreNames compile-time
  // Pinned source names from core_public_bindings.tsv and core_var_metadata.tsv.
  // Qualification describes source identity and does not demand a core value.
  const readerCoreNames = new Set("* *1 *2 *3 *assert* *clojurescript-version* *command-line-args* *e *eval* *exec-tap-fn* *flush-on-newline* *loaded-libs* *main-cli-fn* *ns* *out* *print-dup* *print-err-fn* *print-fn* *print-fn-bodies* *print-length* *print-level* *print-meta* *print-namespace-maps* *print-newline* *print-readably* *unchecked-arrays* *unchecked-if* *warn-on-infer* + - --destructure-map -> ->> ->ArrayChunk ->ArrayIter ->Atom ->ChunkBuffer ->ChunkedCons ->ChunkedSeq ->Cons ->Cycle ->Delay ->ES6EntriesIterator ->ES6Iterator ->ES6IteratorSeq ->ES6SetEntriesIterator ->Eduction ->Empty ->EmptyList ->IndexedSeq ->IndexedSeqIterator ->IntegerRange ->IntegerRangeChunk ->Iterate ->KeySeq ->Keyword ->LazySeq ->List ->Many ->MapEntry ->MetaFn ->MultiFn ->MultiIterator ->Namespace ->PersistentArrayMap ->PersistentArrayMapIterator ->PersistentArrayMapSeq ->PersistentHashMap ->PersistentHashSet ->PersistentQueue ->PersistentQueueIter ->PersistentQueueSeq ->PersistentTreeMap ->PersistentTreeMapSeq ->PersistentTreeSet ->PersistentVector ->RSeq ->Range ->RangeIterator ->RangedIterator ->RecordIter ->Reduced ->Repeat ->SeqIter ->Single ->StringBufferWriter ->StringIter ->Subvec ->Symbol ->TaggedLiteral ->TransformerIterator ->UUID ->ValSeq ->Var ->Volatile -add-method -add-watch -as-transient -assoc -assoc! -assoc-n -assoc-n! -chunked-first -chunked-next -chunked-rest -clj->js -clone -comparator -compare -conj -conj! -contains-key? -count -default-dispatch-val -deref -deref-with-timeout -disjoin -disjoin! -dispatch-fn -dissoc -dissoc! -drop -drop-first -empty -entry-key -equiv -find -first -flush -get-method -hash -invoke -iterator -js->clj -key -key->js -kv-reduce -lookup -meta -methods -name -namespace -next -notify-watches -nth -peek -persistent! -pop -pop! -pr-writer -prefer-method -prefers -realized? -reduce -remove-method -remove-watch -reset -reset! -rest -rseq -seq -sorted-seq -sorted-seq-from -swap! -val -vreset! -with-meta -write .. / < <= = == > >= APersistentVector ASeq ArrayChunk ArrayIter Atom CHAR_MAP ChunkBuffer ChunkedCons ChunkedSeq Cons Cycle DEMUNGE_MAP DEMUNGE_PATTERN Delay ES6EntriesIterator ES6Iterator ES6IteratorSeq ES6SetEntriesIterator Eduction Empty EmptyList ExceptionInfo Fn IAssociative IAtom IChunk IChunkedNext IChunkedSeq ICloneable ICollection IComparable ICounted IDeref IDerefWithTimeout IDrop IEditableCollection IEmptyableCollection IEncodeClojure IEncodeJS IEquiv IFind IFn IHash IIndexed IIterable IKVReduce IList ILookup IMap IMapEntry IMeta IMultiFn INIT INamed INext IPending IPrintWithWriter IRecord IReduce IReset IReversible ISeq ISeqable ISequential ISet ISorted IStack ISwap ITransientAssociative ITransientCollection ITransientMap ITransientSet ITransientVector IUUID IVector IVolatile IWatchable IWithMeta IWriter IndexedSeq IndexedSeqIterator Inst IntegerRange IntegerRangeChunk Iterate KeySeq Keyword LazySeq List LongImpl MODULE_INFOS MODULE_URIS Many MapEntry MetaFn MultiFn MultiIterator NS_CACHE NaN? Namespace NodeSeq PROTOCOL_SENTINEL PersistentArrayMap PersistentArrayMapIterator PersistentArrayMapSeq PersistentHashMap PersistentHashSet PersistentQueue PersistentQueueIter PersistentQueueSeq PersistentTreeMap PersistentTreeMapSeq PersistentTreeSet PersistentVector RSeq Range RangeIterator RangedIterator RecordIter Reduced Repeat START SeqIter Single StringBufferWriter StringIter Subvec Symbol TaggedLiteral Throwable->map TransformerIterator UUID ValSeq Var Volatile abs aclone add-tap add-to-string-hash-cache add-watch aget alength alter-meta! amap ancestors and any? apply apply-to areduce array array-chunk array-index-of array-iter array-list array-map array-seq array? as-> aset assert assoc assoc! assoc-in associative? atom await bases binding bit-and bit-and-not bit-clear bit-count bit-flip bit-not bit-or bit-set bit-shift-left bit-shift-right bit-shift-right-zero-fill bit-test bit-xor boolean boolean? booleans bounded-count butlast byte bytes case cat char char? chars checked-aget checked-aget' checked-aset checked-aset' chunk chunk-append chunk-buffer chunk-cons chunk-first chunk-next chunk-rest chunked-seq chunked-seq? clj->js clone cloneable? coercive-= coercive-boolean coercive-not coercive-not= coll? comment comp comparator compare compare-and-set! complement completing concat cond cond-> cond->> condp conj conj! cons constantly contains? copy-arguments count counted? create-ns cycle dec declare dedupe default-dispatch-val defmacro defmethod defmulti defn defn- defonce defprotocol defrecord deftype delay delay? demunge deref derive descendants destructure disj disj! dispatch-fn dissoc dissoc! distinct distinct? divide doall dorun doseq dotimes doto double double-array double? doubles drop drop-last drop-while dt->et eduction empty empty? enable-console-print! ensure-reduced equiv-map es6-entries-iterator es6-iterable es6-iterator es6-iterator-seq es6-set-entries-iterator eval even? every-pred every? ex-cause ex-data ex-info ex-message exists? extend-protocol extend-type false? ffirst filter filterv find find-macros-ns find-ns find-ns-obj first flatten float float? floats flush fn fn? fnext fnil for force frequencies gen-apply-to gen-apply-to-simple gensym gensym_counter get get-global-hierarchy get-in get-method get-validator goog-define group-by halt-when hash hash-combine hash-double hash-keyword hash-long hash-map hash-ordered-coll hash-set hash-string hash-string* hash-unordered-coll ident? identical? identity if-let if-not if-some ifind? ifn? implements? import imul inc indexed? infinite? inst-ms inst-ms* inst? instance? int int-array int-rotate-left int? integer? interleave interpose into into-array ints is_proto_ isa? iter iterable? iterate iteration js->clj js-arguments js-comment js-debugger js-delete js-fn? js-in js-inline-comment js-invoke js-iterable? js-keys js-mod js-obj js-reserved js-str js-symbol? juxt keep keep-indexed key key->js key-test keys keyword keyword-identical? keyword? last lazy-cat lazy-seq let letfn list list* list? load-file load-file* locking long long-array longs loop m3-C1 m3-C2 m3-fmix m3-hash-int m3-hash-unencoded-chars m3-mix-H1 m3-mix-K1 m3-seed macroexpand macroexpand-1 make-array make-hierarchy map map-entry? map-indexed map? mapcat mapv max max-key memfn memoize merge merge-with meta methods min min-key missing-protocol mix-collection-hash mk-bound-fn mod munge munge-str name namespace nat-int? native-satisfies? neg-int? neg? newline next nfirst nil-iter nil? nnext not not-any? not-empty not-every? not-native not= ns-imports ns-interns ns-interns* ns-name ns-publics ns-unmap nth nthnext nthrest number? object-array object? odd? or parents parse-boolean parse-double parse-long parse-uuid partial partition partition-all partition-by partitionv partitionv-all peek persistent! persistent-array-map-seq pop pop! pos-int? pos? pr pr-seq-writer pr-sequential-writer pr-str pr-str* pr-str-with-opts pr-writer prefer-method prefers prim-seq print print-map print-meta? print-prefix-map print-str println println-str prn prn-str prn-str-with-opts qualified-ident? qualified-keyword? qualified-symbol? quot rand rand-int rand-nth random-sample random-uuid range ranged-iterator re-find re-matches re-pattern re-seq realized? record? reduce reduce-kv reduceable? reduced reduced? reductions refer-clojure refer-global regexp? reify rem remove remove-all-methods remove-method remove-tap remove-watch repeat repeatedly replace replicate req! require require-global require-macros reset! reset-meta! reset-vals! resolve rest reverse reversible? rseq rsubseq run! satisfies? second select-keys selector seq seq-iter seq-to-map-for-destructuring seq? seqable? sequence sequential? set set-from-indexed-seq set-lite-check set-print-err-fn! set-print-fn! set-validator! set? short shorts shuffle simple-benchmark simple-ident? simple-keyword? simple-symbol? some some-> some->> some-fn some-vals some? sort sort-by sorted-map sorted-map-by sorted-set sorted-set-by sorted? special-symbol? specify specify! split-at split-with splitv-at spread str str_ string-hash-cache string-hash-cache-count string-iter string-print string? subs subseq subvec supers swap! swap-vals! symbol symbol-identical? symbol? system-time tagged-literal tagged-literal? take take-last take-nth take-while tap> test this-as time to-array to-array-2d trampoline transduce transformer-iterator transient tree-seq true? truth_ type type->str unchecked-add unchecked-add-int unchecked-byte unchecked-char unchecked-dec unchecked-dec-int unchecked-divide-int unchecked-double unchecked-float unchecked-get unchecked-inc unchecked-inc-int unchecked-int unchecked-long unchecked-max unchecked-min unchecked-multiply unchecked-multiply-int unchecked-negate unchecked-negate-int unchecked-remainder-int unchecked-set unchecked-short unchecked-subtract unchecked-subtract-int undefined? underive unreduced unsafe-bit-and unsafe-cast unsigned-bit-shift-right update update-in update-keys update-vals uri? use use-macros uuid uuid? val vals var? vary-meta vec vec-lite vector vector-lite vector? volatile! volatile? vreset! vswap! when when-first when-let when-not when-some while with-meta with-out-str with-redefs write-all zero? zipmap".split(" "));
  //@unit registerReaderNamespace compile-time requires: createNs, symbol
  function registerReaderNamespace(name, context, added) {
    const ns = createNs(symbol(null, name));
    if (added && ns.readerContext) { for (const b of context.bindings) ns.readerContext.bindings.add(b); return ns; }
    ns.readerContext = {aliases: new Map(context.aliases ?? []),
      refers: new Map((context.refers ?? []).map(([local, ns, name]) => [local, [ns, name]])),
      excludes: new Set(context.excludes ?? []), bindings: new Set(context.bindings ?? [])};
    return ns;
  }
  //@unit macroReaderFunction compile-time requires: withMacroReaderContext
  function macroReaderFunction(fn) {
    const wrapped = function (...args) { return withMacroReaderContext(fn, args); };
    return Object.assign(wrapped, fn);
  }
  //@unit readerOverlay compile-time
  // Reader contexts are immutable views. Nested macro expansion extends the
  // current view without copying its namespace maps and sets.
  function readerOverlay(base, own) {
    if (own.size === 0) return base;
    return {has: key => own.has(key) || base.has(key),
      get: key => own.has(key) ? own.get(key) : base.get(key)};
  }
  //@unit withMacroReaderContext compile-time requires: coreCells, createNs, get, keyword, nth,
  //@ readerOverlay, str, toArray
  function withMacroReaderContext(fn, args) {
    const saved = coreCells.currentNs, env = get(args[1], keyword(null, "ns"));
    const name = get(env, keyword(null, "name"));
    if (name == null) return Reflect.apply(fn, null, args);
    const ns = createNs(name);
    const context = ns.readerContext ?? {aliases: new Map(), refers: new Map(), excludes: new Set(), bindings: new Set()};
    const aliases = new Map();
    for (const pair of toArray(get(env, keyword(null, "requires")))) aliases.set(str(nth(pair, 0)), str(nth(pair, 1)));
    const previous = ns.readerContext;
    const view = get(args[1], keyword("light-cljs", "reader-context"));
    const refers = new Map(), excludes = new Set(), bindings = new Set();
    let declared = context.declared, visible = context.visible ?? 0;
    if (view != null) {
      for (const entry of toArray(get(view, keyword(null, "refers")))) refers.set(nth(entry, 0), [nth(entry, 1), nth(entry, 2)]);
      for (const name of toArray(get(view, keyword(null, "excludes")))) excludes.add(name);
      for (const name of toArray(get(view, keyword(null, "bindings")))) bindings.add(name);
      // The compilation's declarations, registered on the namespace once each
      // by the views that first name them, in declaration order; a view sees
      // the first `visible` of them.
      const added = get(view, keyword(null, "declared"));
      if (added != null) {
        const generation = get(added, keyword(null, "generation"));
        if (ns.readerDeclared?.generation !== generation) ns.readerDeclared = {generation, index: new Map()};
        let position = get(added, keyword(null, "from"));
        for (const name of toArray(get(added, keyword(null, "names")))) ns.readerDeclared.index.set(name, position++);
        declared = ns.readerDeclared.index;
        visible = get(view, keyword(null, "visible"));
      }
    }
    ns.readerContext = {aliases: readerOverlay(context.aliases, aliases),
      refers: readerOverlay(context.refers, refers), excludes: readerOverlay(context.excludes, excludes),
      bindings: readerOverlay(context.bindings, bindings), declared, visible}; coreCells.currentNs = ns;
    try { return Reflect.apply(fn, null, args); }
    finally { ns.readerContext = previous; coreCells.currentNs = saved; }
  }
  //@unit findNs compile-time requires: invalidArity, Namespace, namespaces, str, symbolOf
  function findNs(name) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    if (name == null) return null;
    if (name instanceof Namespace) return name;
    if (symbolOf(name) === undefined) throw new TypeError("find-ns requires a symbol or Namespace");
    return namespaces.get(str(name)) ?? null;
  }
  //@unit nsName requires: invalidArity, symbolOf
  function nsName(value) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    // Pinned ns-name reads .name: metadata symbols expose their string name,
    // while explicitly constructed Namespaces preserve their supplied slot.
    if (symbolOf(value) !== undefined) return symbolOf(value).name$LKEY;
    return value.name;
  }
  //@unit theNs compile-time requires: findNs, invalidArity, str
  function theNs(name) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    const value = findNs(name);
    if (!value) throw new Error("No namespace: " + str(name));
    return value;
  }
  //@unit allNs compile-time requires: invalidArity, listFromArray, namespaces
  function allNs() { if (arguments.length) return invalidArity(arguments.length); return listFromArray([...namespaces.values()]); }
  //@unit compilerMacroVar compile-time requires: into, invalidArity, keyword, makeVar,
  //@ mapFromArray, name, namespace, str, symbolCore
  function compilerMacroVar(sym, metadata) {
    if (arguments.length !== 2) return invalidArity(arguments.length);
    const base = mapFromArray([keyword(null, "ns"), symbolCore(namespace(sym)), keyword(null, "name"), symbolCore(name(sym)), keyword(null, "macro"), true]);
    return makeVar(() => { throw new Error("compile-time macro cannot be consumed as a runtime value: " + str(sym)); }, sym, into(base, metadata), true);
  }
  //@unit rePattern
  function rePattern(pattern) {
    if (pattern instanceof RegExp) return pattern;
    if (typeof pattern !== "string") throw new TypeError("regex pattern requires a string");
    const prefix = /^\(\?([dgimsuxy]*)\)/.exec(pattern);
    return new RegExp(prefix ? pattern.slice(prefix[0].length) : pattern, prefix ? prefix[1] : "");
  }
  //@unit reMatch requires: vectorFromArray
  function reMatch(re, text, full) {
    if (typeof text !== "string") throw new TypeError("regex matching requires a string");
    const result = re.exec(text);
    if (result === null || (full && result[0] !== text)) return null;
    return result.length === 1 ? result[0] : vectorFromArray(Array.from(result));
  }
  //@unit deferredValue requires: truth
  function deferredValue(state, lazy = false) {
    // Assignment occurs only after success; failures leave the thunk installed.
    // Reentrant calls are ordinary calls, matching CLJS's observable behavior.
    if (lazy ? state.thunk != null : truth(state.thunk)) { state.value = state.thunk.call(null); state.thunk = null; }
    return state.value;
  }
  //@unit Delay requires: DelayCell
  function Delay(thunk, result) { return new DelayCell({thunk, value: result}); }
  //@unit Delay.kind effect: Delay requires: Delay, DelayCell, unkeyedJSON, viewType
  // A delay is a class of its type's prototype (`DelayCell`, light-cljs-1013),
  // whose JSON leaves its keyed field out.
  viewType(Delay, DelayCell, 40);
  Object.defineProperty(Delay.prototype, "toJSON", {configurable: true, writable: true, value() { return unkeyedJSON(this); }});
  //@unit LazySeq requires: LazySeqCell, metadataValues
  // Nil metadata is not stored: a lazy seq without an entry reads as one
  // with nil (`meta`, the `meta` field, the compiler bridge).
  function LazySeq(metadata, thunk, result, cachedHash) {
    const value = new LazySeqCell(thunk, result, cachedHash);
    if (metadata !== null) metadataValues.set(value, metadata);
    return value;
  }
  //@unit lazySequence requires: LazySeq
  function lazySequence(thunk) { return LazySeq(null, thunk, null, null); }
  //@unit force requires: delays, deref
  function force(value) { return delays.has(value) ? deref(value) : value; }
  //@unit nthrest requires: rest, seq
  function nthrest(value, n) {
    while (n > 0) { const current = seq(value); if (current == null) break; value = rest(current); n--; }
    return value;
  }
  //@unit nthnext requires: next, seq
  function nthnext(value, n) {
    value = seq(value);
    while (n > 0 && value != null) { value = next(value); n--; }
    return value;
  }
  //@unit reSeq requires: lazySequence, ReSeqCell, vectorFromArray
  function reSeq(re, text) {
    if (typeof text !== "string") throw new TypeError("regex matching requires a string");
    const match = re.exec(text);
    if (match === null) return null;
    const value = match.length === 1 ? match[0] : vectorFromArray(Array.from(match));
    // A lazy tail must not compute a count (or execute the next RegExp.exec).
    const tail = lazySequence(() => {
      const offset = match.index + Math.max(1, match[0].length);
      return offset <= text.length ? reSeq(re, text.slice(offset)) : null;
    });
    return new ReSeqCell(value, tail);
  }
  //@unit toArray requires: first, next, seq
  function toArray(value) {
    const result = [];
    for (let current = seq(value); current !== null; current = next(current)) result.push(first(current));
    return result;
  }
  //@unit vec requires: into, isMapEntry, isVector, keyProtocol, valProtocol, vector, withMeta
  function vec(value) {
    if (isMapEntry(value)) return vector(keyProtocol(value), valProtocol(value));
    if (isVector(value)) return withMeta(value, null);
    return into(vector(), value);
  }
  //@unit concatTwo requires: chunkCons, chunkedSeqValue, chunkFirst, chunkRest, cons, first,
  //@ lazySequence, methodKeys, protocolHooks, rest, seq
  function concatTwo(x, y) {
    return lazySequence(() => {
      const s = seq(x);
      if (s === null) return y;
      return (protocolHooks.implements?.(methodKeys[5], s) ?? chunkedSeqValue(s)) ? chunkCons(chunkFirst(s), concatTwo(chunkRest(s), y)) : cons(first(s), concatTwo(rest(s), y));
    });
  }
  //@unit concatMany requires: chunkCons, chunkedSeqValue, chunkFirst, chunkRest, cons, first,
  //@ lazySequence, methodKeys, next, protocolHooks, rest, seq
  function concatMany(current, remaining) {
    return lazySequence(() => {
      const s = seq(current);
      if (s !== null) return (protocolHooks.implements?.(methodKeys[5], s) ?? chunkedSeqValue(s)) ? chunkCons(chunkFirst(s), concatMany(chunkRest(s), remaining)) : cons(first(s), concatMany(rest(s), remaining));
      return remaining == null ? null : concatMany(first(remaining), next(remaining));
    });
  }
  //@unit treeSeq requires: cons, lazySequence, mapcat, truth
  function treeSeq(branch, children, root) {
    const walk = node => lazySequence(() => cons(node,
      truth(branch.call(null, node)) ? mapcat(walk, children.call(null, node)) : null));
    return walk(root);
  }
  //@unit flatten requires: filter, isSequential, rest, seq, treeSeq
  function flatten(value) { return filter(x => !isSequential(x), rest(treeSeq(isSequential, seq, value))); }
  //@unit concat requires: callable, concatMany, concatTwo, lazySequence
  const concat = callable(2, {0: () => lazySequence(() => null), 1: x => lazySequence(() => x), 2: concatTwo}, (x, y, more) => concatMany(concatTwo(x, y), more));
  //@unit es6Iterator requires: first, next, seq
  function es6Iterator(value) {
    let current = seq(value);
    return {next() {
      if (current === null) return {value: null, done: true};
      const value = first(current);
      current = next(current);
      return {value, done: false};
    }};
  }
  //@unit es6Iterator.host effect: es6Iterator, extendType, extendInstance, treeMapConstructor when: mapPrototype,
  //@ entryPrototype, setPrototype, vectorPrototype requires: es6Iterator, hostHooks
  hostHooks.iterator = es6Iterator;
  //@unit es6EntriesIterator requires: es6Iterator, nth
  function es6EntriesIterator(value, setEntries = false) {
    const iterator = es6Iterator(value);
    return {next() {
      const item = iterator.next();
      if (!item.done) item.value = setEntries ? [item.value, item.value] : [nth(item.value, 0), nth(item.value, 1)];
      return item;
    }};
  }
  //@unit es6EntriesIterator.host effect: es6EntriesIterator, extendType, extendInstance,
  //@ treeMapConstructor when: mapPrototype, setPrototype requires: es6EntriesIterator, hostHooks
  hostHooks.entries = es6EntriesIterator;
  //@unit sequenceIterator requires: displayStr, isSeqable, iterableValue, iteratorInit,
  //@ iteratorProtocol, methodKeys, protocolHooks, SeqIter
  function sequenceIterator(value) {
    if (protocolHooks.satisfies?.(methodKeys[27], value) ?? iterableValue(value)) return iteratorProtocol(value);
    if (value == null) return {hasNext() { return false; }, next() { return new Error("No such element"); }, remove() { return new Error("Unsupported operation"); }};
    if (typeof value === "string" || Array.isArray(value)) {
      let index = 0;
      return {hasNext() { return index < value.length; }, next() { return typeof value === "string" ? value.charAt(index++) : value[index++]; }, remove() { return new Error("Unsupported operation"); }};
    }
    if (isSeqable(value)) return new SeqIter(iteratorInit, value);
    throw new Error("Cannot create iterator from " + displayStr(value));
  }
  //@unit transformedIterator requires: invalidArity, isReduced
  function transformedIterator(xform, sources) {
    let buffer = [], offset = 0, ready = false, item, completed = false;
    const sink = function (acc, value) {
      switch (arguments.length) {
        case 0: return null;
        case 1: return acc;
        case 2: buffer.push(value); return acc;
        default: return invalidArity(arguments.length);
      }
    };
    const xf = xform.call(null, sink);
    return {
      hasNext() {
        if (ready) return true;
        while (offset === buffer.length) {
          buffer = []; offset = 0;
          if (completed) return false;
          // MultiIterator probes sources from left to right and stops at the
          // shortest. Request next values only after every probe succeeds.
          if (sources.every(source => source.hasNext())) {
            const result = sources.length === 1
              ? xf.call(null, null, sources[0].next())
              : xf.call(null, null, ...sources.map(source => source.next()));
            if (!isReduced(result)) continue;
          }
          xf.call(null, null);
          completed = true;
        }
        item = buffer[offset]; buffer[offset++] = null; ready = true;
        return true;
      },
      next() {
        if (!this.hasNext()) throw new Error("No such element");
        const value = item; item = null; ready = false;
        return value;
      }
    };
  }
  //@unit chunkIteratorSequence requires: arrayChunk, chunkCons, lazySequence
  function chunkIteratorSequence(iter) {
    return lazySequence(() => {
      if (!iter.hasNext()) return null;
      const values = [];
      // The CLJS condition probes the iterator BEFORE checking the chunk
      // size, caching one extra output for the following chunk.
      while (iter.hasNext() && values.length < 32) values.push(iter.next());
      return chunkCons(arrayChunk(values), chunkIteratorSequence(iter));
    });
  }
  //@unit sequence requires: callable, chunkIteratorSequence, emptyList, isSeq, seq,
  //@ sequenceIterator, toArray, transformedIterator
  const sequence = callable(2, {
    1: value => isSeq(value) ? value : seq(value) ?? emptyList,
    2: (xform, value) => chunkIteratorSequence(transformedIterator(xform, [sequenceIterator(value)]))
  }, (xform, value, more) => chunkIteratorSequence(transformedIterator(xform, [value, ...toArray(more)].map(sequenceIterator))));
  //@unit Eduction requires: EductionCell
  function Eduction(xform, coll) {
    // Retain only the view's inputs; each protocol call owns its traversal state.
    // This is not a collection payload with structural equality or metadata.
    return new EductionCell(xform, coll);
  }
  //@unit Eduction.kind effect: Eduction requires: Eduction, EductionCell, viewType
  // An eduction is a class of its type's prototype (`EductionCell`,
  // light-cljs-1013).
  viewType(Eduction, EductionCell, 43);
  //@unit eduction requires: callable, comp, Eduction, toArray
  const eduction = callable(0, {}, args => {
    const values = toArray(args);
    return new Eduction(comp(...values.slice(0, -1)), values.length ? values.at(-1) : null);
  });
  //@unit fnRest requires: IndexedSeqCell
  // The rest arguments from `start`: an indexed seq over one fresh array of
  // them, neither frozen nor stamped (docs/value-layout.md, section 2). An
  // array from 0 is the call's own (a direct call's `fnRest([b, c], 0)`, see
  // src/call_shapes.rs), which the seq takes as it is; `arguments` and a
  // later start are copied.
  function fnRest(args, start) {
    if (args.length <= start) return null;
    return new IndexedSeqCell(start === 0 && Array.isArray(args) ? args : Array.prototype.slice.call(args, start), 0, true, "indexed");
  }
  //@unit fnApplyTo requires: first, next, rest, seq
  function fnApplyTo(delegate, fixed, args) {
    const prefix = [];
    // The final rest preserves sequence identity and laziness. For a positive
    // prefix, CLJS uses rest after the last fixed argument, not next.
    for (let i = 0; i < fixed; i++) {
      prefix.push(first(args));
      args = i + 1 === fixed ? rest(args) : next(args);
    }
    return delegate.call(null, ...prefix, fixed === 0 ? seq(args) : args);
  }
  //@unit typeHooks
  const typeHooks = {};
  //@unit fnRest.install effect: fnRest when: protocolMethod requires: fnRest, typeHooks
  typeHooks.rest = fnRest;
  //@unit fnApplyTo.install effect: fnApplyTo when: ifnInvokeEntry requires: fnApplyTo, typeHooks
  typeHooks.applyTo = fnApplyTo;
  //@unit publicHashSet.install effect: publicHashSet when: PersistentHashSetValue requires: publicHashSet,
  //@ typeHooks
  typeHooks.hashSet = publicHashSet;
  //@unit publicArrayMap.install effect: publicArrayMap when: PersistentArrayMapValue requires: publicArrayMap,
  //@ typeHooks
  typeHooks.arrayMap = publicArrayMap;
  //@unit ifnInvokeEntry requires: typeHooks
  // This is the callable object ABI used by IFn and metadata wrappers.
  // The last method receives twenty values followed by a sequence tail.
  function ifnInvokeEntry(method) {
    const entry = function (...args) { return method.call(this, this, ...args); };
    const delegate = method.cljs$core$IFn$_invoke$arity$variadic;
    if (delegate) {
      entry.cljs$core$IFn$_invoke$arity$variadic = function (...args) { return delegate.call(this, this, ...args); };
      entry.cljs$lang$maxFixedArity = method.cljs$lang$maxFixedArity - 1;
      entry.cljs$lang$applyTo = args => typeHooks.applyTo(entry.cljs$core$IFn$_invoke$arity$variadic, entry.cljs$lang$maxFixedArity, args);
    }
    return entry;
  }
  //@unit makeIFn requires: ifnInvokeEntry, ifnPrototype, ifnValues, reifyValues
  function makeIFn(methods, invokeMethods = methods) {
    const value = Object.create(ifnPrototype);
    ifnValues.set(value, methods);
    for (const [arity, method] of Object.entries(invokeMethods)) {
      value["cljs$core$IFn$_invoke$arity$" + arity] = ifnInvokeEntry(method);
    }
    reifyValues.add(value);
    return value;
  }
  //@unit protocolKeys
  const protocolKeys = new WeakMap(), protocolImplementations = new WeakMap();
  //@unit protocolNames
  const protocolNames = new WeakMap(), protocolValues = new Map(), protocolFunctions = new Map(), protocolVariables = new Map(), protocolGetters = new Map();
  //@unit nativeMethods
  // The implementations `extend-type` gives host types, `object` and
  // `default`, by a protocol method key's text and then the type's name, and
  // the dispatch cells by a key's text (`dispatchCell`).
  const nativeMethods = new Map(), coreDispatchers = new Map();
  //@unit dispatchCell requires: coreDispatchers
  // The dispatch cell of a protocol method key's text (light-cljs-375): the
  // core protocol dispatcher `coreMethod` made for it (`dispatcher`), the
  // implementations `extend-type` gave host types for it (`natives`, the
  // key's entry of `nativeMethods`, which the declaration journal sets and
  // undoes with it) and its core protocol (`protocol`). A resolved key
  // (`methodKey`) holds its cell once `externalMethod` has looked it up, so
  // that a call reads them without a lookup by the key's text.
  function dispatchCell(text) { return coreDispatchers.get(text) ?? coreDispatchers.set(text, {}).get(text); }
  //@unit protocol mutator requires: protocolNames, protocolValues, setDeclaration
  function protocol(name) {
    const value = function () {};
    protocolNames.set(value, name); setDeclaration(protocolValues, name, value);
    return value;
  }
  //@unit nativeType
  function nativeType(value) { return value == null ? "null" : Array.isArray(value) ? "array" : typeof value; }
  //@unit directMethod requires: builtinSlots, demandMethod, protocolSentinel, protocolSlot
  function directMethod(value, key, arity) {
    demandMethod(key, arity, value);
    if (value == null) return undefined;
    const method = value[protocolSlot(key, arity)];
    if (method !== undefined && builtinSlots.has(method)) return undefined;
    return arity === 0 ? (method === protocolSentinel ? method : undefined) : method;
  }
  //@unit protocolStores requires: atoms, chunkedConsCells, delays, keywordCells, lazyCells, maps,
  //@ metaFunctions, multiFns, queues, reducedValues, seqCells, sets, symbolCells, transientCells,
  //@ vars, vectorCells, viewCells, volatiles
  const protocolStores = [multiFns, transientCells, keywordCells, symbolCells, vectorCells, maps, sets, seqCells, chunkedConsCells, viewCells, queues, lazyCells, delays, vars, atoms, volatiles, reducedValues, metaFunctions];
  //@unit ownedProtocolValue requires: protocolStores, storesPredicate
  const ownedProtocolValue = storesPredicate(...protocolStores);
  //@unit demandKey requires: demandMethod
  // The declaration demand of a method's resolved key (`methodKey`), for an
  // operation that reads the method's slot itself (`get`).
  function demandKey(key, arity, value) { demandMethod(key.key, arity, value); }
  //@unit keyMethod requires: builtinSlots, demandMethod, keySlot
  // `directMethod` of a method's resolved key (`methodKey`): its slot for
  // `arity`, read from the key.
  function keyMethod(value, key, arity) {
    demandMethod(key.key, arity, value);
    const method = value == null ? undefined : value[keySlot(key, arity)];
    return method !== undefined && builtinSlots.has(method) ? undefined : method;
  }
  //@unit keyMarker requires: demandMethod, protocolSentinel
  // `directMethod` of a resolved key's protocol (arity 0): whether the value
  // holds the protocol's marker.
  function keyMarker(value, key) {
    demandMethod(key.name, 0, value);
    return value != null && value[key.marker] === protocolSentinel;
  }
  //@unit builtin requires: keyMethod, protocolHooks
  // The value's own implementation of a resolved key (`methodKey`), else one
  // that `extend-type` of a host type, `object` or `default` gave
  // (`externalMethod`).
  function builtin(value, key, arity) {
    const method = keyMethod(value, key, arity);
    if (method != null) return method;
    return protocolHooks.externalMethod?.(value, key, arity);
  }
  //@unit externalMethod requires: builtinCoreImplementation, coreProtocolsByName, dispatchCell,
  //@ nativeMethods, nativeType
  // It takes a resolved key (`methodKey`, light-cljs-358), whose dispatch
  // cell (`dispatchCell`) it keeps, with the key's core protocol, on its
  // first call: the dispatchers and implementations `extend-type` installs
  // are read from the cell, which `coreMethod` and `extend-type` update.
  function externalMethod(value, key, arity) {
    const cell = key.cell ?? (key.cell = dispatchCell(key.key)), external = cell.dispatcher;
    const dispatch = external?.[nativeType(value)] ?? external?._;
    if (!nativeMethods.size && !dispatch) return undefined;
    const protocol = cell.protocol ?? (cell.protocol = coreProtocolsByName.get(key.name) ?? null);
    // Native-object fallback cannot override an owned collection's direct
    // core implementation. Its public operation handles the private payload.
    if (protocol && builtinCoreImplementation(protocol, value)) return undefined;
    return dispatch ?? cell.natives?.get(nativeType(value))?.get(arity) ?? cell.natives?.get("_")?.get(arity);
  }
  //@unit payloadStores requires: chunkedConsCells, lazyCells, maps, queues, seqCells, sets,
  //@ vectorCells, viewCells
  // `payloadStore`'s stores, numbered from 1, and the store each kind
  // (`payloads`) names, else 0: each entry its own, so that no index a host
  // gives `Array.prototype` is read (the Sol review of light-cljs-998).
  const payloadStores = (stores => {
    const byKind = Array(32).fill(0);
    stores.forEach((store, i) => {
      const bits = Object.getOwnPropertyDescriptor(store, "kindBits")?.value;
      for (let kind = 1; kind < 31; kind++) if (bits >> kind & 1) byKind[kind] = i + 1;
    });
    return {stores, byKind};
  })([viewCells, seqCells, vectorCells, maps, lazyCells, sets, queues, chunkedConsCells]);
  //@unit payloadStore requires: payloads, payloadStores
  // Which store of the runtime's collections and seqs holds a value, by a
  // number from 1 to 8, else 0. These are brand checks, which read no
  // property of the value, so no getter or Proxy trap of a host value runs.
  // A value is in one payload store (`countedValue`). The value's kind
  // (`payloads`) names an exact store at once; the stores that are not
  // exact are then probed in this order (light-cljs-990).
  const payloadStore = (({stores, byKind}) => {
    const probed = [];
    stores.forEach((store, i) => { if (Object.getOwnPropertyDescriptor(store, "exact")?.value !== true) probed.push(store, i + 1); });
    return value => {
      const number = byKind[payloads.kind(value)];
      if (number) return number;
      for (let i = 0; i < probed.length; i += 2) if (probed[i].has(value)) return probed[i + 1];
      return 0;
    };
  })(payloadStores);
  //@unit kindStore requires: holdsVector, payloads, payloadStores
  // `payloadStore` from the kind alone, else 0: probing the maps' store asks
  // if a host value is a record, which may run a Proxy's `getPrototypeOf`.
  // A vector cell, which has no payload kind, is store 3 (`vectorCells`).
  const kindStore = (byKind => value => byKind[payloads.kind(value)] || (holdsVector(value) ? 3 : 0))(payloadStores.byKind);
  //@unit storeNativeProtocols
  // The core protocols of `publicMethodInfo`'s `stores`.
  const storeNativeProtocols = new Set(["ISeqable", "ISeq", "INext", "IMeta", "ILookup"]);
  //@unit publicMethodInfo requires: coreProtocolNames, coreProtocolsByName, nativeImplementations,
  //@ protocolMasks, protocolSlot, storeNativeProtocols
  // The protocol of a key's text, which a resolved key (`methodKey`) keeps as its
  // `info`: only the private, fixed ABI belongs there. Membership, demanded
  // extensions and receiver properties must still be observed on every call.
  function publicMethodInfo(key) {
    const protocol = coreProtocolsByName.get(key.split("/")[0]), name = coreProtocolNames.get(protocol);
    // `stores` (`publicMethod`) only for the protocols whose native test of
    // the runtime's collections and seqs is which payload store holds the
    // value (`payloadStore`), so that one answer serves the store:
    // `seqableValue`, `seqValue`, `nextValue`, `metaValue` and `lookupValue`
    // read no payload.
    const stores = storeNativeProtocols.has(name) ? Array(9).fill(undefined) : null;
    return {protocol, name, mask: protocolMasks.get(name), marker: name && protocolSlot(name, 0), native: nativeImplementations.get(protocol), stores};
  }
  //@unit publicMethod requires: demandMethod, keyMethod, payloadStore, protocolSentinel,
  //@ publicDeclared, publicMethodInfo, satisfies
  // Of a resolved key (`methodKey`), whose `info` it fills in once.
  function publicMethod(value, key, arity, fast = false, directOnly = false) {
    const info = key.info || (key.info = publicMethodInfo(key.key)), protocol = info.protocol, mask = info.mask, native = info.native;
    // A runtime class's mask bits are its native tests' answers (see
    // `ownMethod`), which the payload stores' step below gives.
    let direct = !!(mask && (value?.[mask[0]] & mask[1])) && value.cljs$lang$kind$$LKEY === undefined;
    if (!direct && info.name) {
      demandMethod(info.name, 0, value);
      direct = value != null && value[info.marker] === protocolSentinel;
      // One of the runtime's collections or seqs (`payloadStore`) that
      // declares no core protocol of its own (light-cljs-988): the native
      // test alone decides, once. Where it is false, the value is the
      // runtime's own and not in the protocol (`satisfies`), so no
      // implementation exists; where it is true, the value's slot is the
      // only one, as an implementation that `extend-type` gave a host type,
      // `object` or `default` cannot override a native one
      // (`externalMethod`). Either way this gives what the steps below give,
      // without testing the value against every store up to three times.
      // `stores` keeps the test's answer by store, where it is the same for
      // every value the store holds.
      const store = direct ? 0 : payloadStore(value);
      if (store) {
        const stores = info.stores;
        if (!(stores ? stores[store] ?? (stores[store] = !!native(value)) : native?.(value))) return undefined;
        return keyMethod(value, key, arity) || undefined;
      }
    }
    if (!direct) direct = !!native?.(value);
    if (!direct && (directOnly || (fast && (Array.isArray(value) || typeof value === "string")))) return undefined;
    if (!direct && !satisfies(protocol, value)) return undefined;
    return publicDeclared(value, key, arity, keyMethod(value, key, arity));
  }
  //@unit storeProtocol requires: demandMethod, demandProtocol, protocolSentinel
  // What `satisfies` (`satisfying`) or `implementsProtocol` gives a value of
  // `store` (`kindStore`), with the same reads and demands: mask bit, then
  // marker, else the native test, as the value is the runtime's own
  // (`ownedProtocolValue`), which `extend-type` of host types, `object` or
  // `default` cannot reach (light-cljs-998).
  function storeProtocol(info, value, store, satisfying) {
    const name = info.name, mask = info.mask, stores = info.stores;
    if (satisfying) demandProtocol(name, value);
    if (mask && (value[mask[0]] & mask[1])) return true;
    demandMethod(name, 0, value);
    if (value[info.marker] === protocolSentinel) return true;
    return stores ? stores[store] ?? (stores[store] = !!info.native(value)) : !!info.native?.(value);
  }
  //@unit publicDeclared requires: IMeta, protocolHooks, publicMethodInfo
  // The rest of `publicMethod` once the value is in the key's protocol, given
  // the `method` its slot held: that, else one `extend-type` gave, else
  // nothing where the protocol is native to the value, else an error. A
  // method getter may have changed the receiver since membership testing.
  function publicDeclared(value, key, arity, method) {
    if (method == null) method = protocolHooks.externalMethod?.(value, key, arity);
    if (method) return method;
    const info = key.info || (key.info = publicMethodInfo(key.key));
    if (info.protocol === IMeta && typeof value === "function") return undefined;
    if (!info.native?.(value)) throw new Error("No protocol method " + key.key + " defined for this value");
    return undefined;
  }
  //@unit ownProtocol requires: keyMarker
  // `true` where the value declares the core protocol of a resolved key
  // (`methodKey`: its mask bit or marker, as `implementsProtocol` reads
  // them), else nothing, for the caller's native test (`?? countedValue(value)`).
  function ownProtocol(key, value) {
    return key.bit !== 0 && value != null && value[key.mask] & key.bit || keyMarker(value, key) ? true : undefined;
  }
  //@unit ownMethod requires: keyMarker, keyMethod, ownDeclared
  // `publicMethod` where no `extend-type` or `specify!` exists: only values of
  // the program's types, `reify` and the runtime's types built with
  // `installMethods` implement protocols, each declaring the protocols it
  // implements, and only their methods count. Declaring one without the
  // method is an error, except where the runtime implements it natively:
  // `IMeta` of a function or a `reify`. A function's `IMeta` slot counts
  // without the marker. (A record has the method of every operation the
  // program keeps.)
  // It takes a resolved key (`methodKey`).
  function ownMethod(value, key, arity) {
    const meta = key.meta && typeof value === "function";
    // The mask bit first: a value of a type the program defines has it, and
    // declaring the protocol demands the method (`keyMethod`) as the marker would.
    // A runtime class's mask bits (its keyed kind, docs/value-layout.md,
    // section 3) declare what it implements natively, which the operation
    // answers: as before they were there, only a marker declares a method
    // for its value here (light-cljs-1028). Program types are told by their
    // brands until their kinds are keyed (light-cljs-1013).
    const declared = key.bit !== 0 && value != null && (value[key.mask] & key.bit) !== 0 && value.cljs$lang$kind$$LKEY === undefined || keyMarker(value, key);
    return declared || meta ? ownDeclared(value, key, arity, keyMethod(value, key, arity)) : undefined;
  }
  //@unit ownDeclared requires: reifyValues
  // The rest of `ownMethod` once the value declares the key's protocol (or,
  // for `IMeta`, is a function), given the `method` its slot held: that, else
  // nothing for the natively implemented `IMeta` of a function or a `reify`,
  // else an error.
  function ownDeclared(value, key, arity, method) {
    if (method) return method;
    if (key.meta && (typeof value === "function" || reifyValues.has(value))) return undefined;
    // A runtime class's value (its keyed kind) declares the protocols it
    // implements natively on its prototype (docs/value-layout.md, section
    // 3), with slots for some operations only: where no program method is
    // there, the operation answers it natively.
    if (value != null && value.cljs$lang$kind$$LKEY !== undefined) return undefined;
    throw new Error("No protocol method " + key.key + " defined for this value");
  }
  //@unit metadataMethod requires: get, meta, symbolCore, truth
  // A protocol method's implementation in the receiver's metadata, or null.
  function metadataMethod(receiver, metadataName) {
    const implementation = get(meta(receiver), symbolCore(metadataName));
    return truth(implementation) ? implementation : null;
  }
  //@unit metadataMethod.install effect: metadataMethod when: protocolMethod requires: metadataMethod,
  //@ typeHooks
  typeHooks.metadataMethod = metadataMethod;
  //@unit protocolMethod mutator requires: demandMethod, invalidArity, nativeType,
  //@ protocolFunctions, protocolGetters, protocolKeys, protocolSlot, protocolVariables,
  //@ setDeclaration, typeHooks
  // `fast` (light-cljs-265) makes a dispatcher that calls a slot, else its first argument.
  function protocolMethod(key, metadataName, arities, viaMetadata = false, variadic = null, getter = null, fast = null) {
    const rest = variadic == null ? -1 : variadic + 2, fixed = arities ? arities.filter(n => n !== rest) : [];
    const single = rest < 0 && fixed.length === 1 ? fixed[0] : 0, slots = new Map((arities ?? []).map(n => [n, protocolSlot(key, n)]));
    // Saved method values retain their direct slot while dynamic dispatch
    // observes the current method Var, as the CLJS macro expansion does.
    const external = (receiver, args) => {
      const current = getter ? getter() : protocolVariables.get(metadataName) ?? method;
      const implementation = current[nativeType(receiver)] ?? current._;
      if (implementation != null) return implementation.apply(null, args);
      throw new Error("No protocol method " + metadataName + " defined for this value");
    };
    const dispatch = function (receiver) {
      let args = arguments, arity = args.length;
      if (single) {
        if (arity !== single) { args = Array.from({length: single}, (_, i) => arguments[i]); arity = single; }
      } else if (rest >= 0 && !fixed.includes(arity)) {
        // CLJS accepts these declarations with a warning. Its macro's literal
        // signature expansion forwards the unbound ampersand and packed tail.
        args = [...Array.from({length: variadic}, (_, i) => arguments[i]), undefined, typeHooks.rest(arguments, variadic)];
        arity = rest;
      } else if (rest < 0 && fixed.length > 1 && !fixed.includes(arity)) return invalidArity(arity);
      if (viaMetadata) {
        const implementation = typeHooks.metadataMethod(receiver, metadataName);
        if (implementation != null) return implementation.call(null, ...args);
      }
      demandMethod(key, arity, receiver);
      const direct = receiver == null ? undefined : receiver[slots.get(arity) ?? protocolSlot(key, arity)];
      return direct != null ? direct.apply(receiver, args) : external(receiver, args);
    };
    const method = fast ? fast(function (receiver) { return single || slots.has(arguments.length) ? external(receiver, arguments) : dispatch.apply(null, arguments); }, ...fixed.map(n => slots.get(n))) : dispatch;
    protocolKeys.set(method, key); setDeclaration(protocolFunctions, key, metadataName); setDeclaration(protocolVariables, metadataName, method);
    setDeclaration(protocolGetters, key, getter ?? (() => protocolVariables.get(metadataName)));
    return method;
  }
  //@unit extendType mutator requires: coreDispatchers, coreProtocolsByName, dispatchCell,
  //@ installMethods, invalidArity, kindOps, kindTables, nativeMethods, patchDeclaration,
  //@ protocolFunctions, protocolGetters, protocolSentinel, protocolValues, setDeclaration
  function extendType(target, methods) {
    let result = null;
    if (typeof target === "string") {
      const grouped = new Map();
      for (const [key, arity, method] of methods) {
        if (arity === 0) {
          const value = protocolValues.get(key) ?? coreProtocolsByName.get(key);
          if (value) { patchDeclaration(value, target, {value: true, writable: true, configurable: true, enumerable: true}); result = true; }
        }
        if (!grouped.has(key)) grouped.set(key, new Map());
        grouped.get(key).set(arity, method);
      }
      for (const [key, implementations] of grouped) {
        if (implementations.has(0)) continue;
        const dispatch = protocolGetters.get(key)?.() ?? coreDispatchers.get(key)?.dispatcher;
        const implementation = function (...args) {
          const method = implementations.size === 1 ? implementations.values().next().value : implementations.get(args.length);
          return method ? method.call(null, ...args) : invalidArity(args.length);
        };
        result = implementation;
        if (dispatch) patchDeclaration(dispatch, target, {value: implementation, writable: true, configurable: true, enumerable: true});
        if (!protocolFunctions.has(key)) {
          // A key's implementations go in its entry and its dispatch cell
          // alike, each through the journal (`dispatchCell`).
          if (!nativeMethods.has(key)) {
            const natives = new Map();
            setDeclaration(nativeMethods, key, natives);
            patchDeclaration(dispatchCell(key), "natives", {value: natives, writable: true, configurable: true, enumerable: true});
          }
          setDeclaration(nativeMethods.get(key), target, implementations);
        }
      }
    } else {
      if (typeof target !== "function" || target.prototype == null) throw new TypeError("extend-type requires a type constructor");
      // A runtime kind's type, or `Object`, which every type inherits from.
      if (target === Object || target.prototype[kindOps] !== undefined) kindTables.fill(kindTables[0]);
      installMethods(target.prototype, methods);
      result = methods.length ? (methods.at(-1)[1] === 0 ? protocolSentinel : methods.at(-1)[2]) : null;
    }
    return result;
  }
  //@unit extendInstance mutator requires: installMethods, kindOps, kindTables, protocolSentinel
  function extendInstance(target, methods, returnTarget) {
    if (target != null && target[kindOps] !== undefined) kindTables.fill(kindTables[0]);
    installMethods(target, methods);
    return returnTarget ? target : methods.length ? (methods.at(-1)[1] === 0 ? protocolSentinel : methods.at(-1)[2]) : null;
  }
  //@unit installMethods mutator requires: declarationJournal, ifnInvokeEntry, ifnPrototype,
  //@ ifnValues, ifnVariadics, invalidArity, mungeMember, patchDeclaration, protocolImplementations,
  //@ protocolKeys, protocolSentinel, protocolSlot
  function installMethods(value, methods) {
    if (value != null && typeof value !== "object" && typeof value !== "function") return value;
    const oldImplementations = protocolImplementations.get(value), oldCalls = ifnValues.get(value);
    const transaction = declarationJournal;
    const implementations = new Map(Array.from(oldImplementations ?? [], ([key, entries]) => [key, new Map(entries)])), calls = {}, invokes = {}, objectVariadics = new Map();
    if (transaction) transaction.push({
      undo: () => { if (oldImplementations) protocolImplementations.set(value, oldImplementations); else protocolImplementations.delete(value); if (oldCalls) ifnValues.set(value, oldCalls); else ifnValues.delete(value); },
      redo: () => { protocolImplementations.set(value, implementations); if (Object.keys(calls).length || ifnVariadics.has(calls)) ifnValues.set(value, calls); },
      snapshot: () => { const methods = protocolImplementations.get(value), functions = ifnValues.get(value); return () => { if (methods) protocolImplementations.set(value, methods); else protocolImplementations.delete(value); if (functions) ifnValues.set(value, functions); else ifnValues.delete(value); }; }
    });
    for (const [dispatch, arity, method, invoke = method, variadic] of methods) {
      const key = typeof dispatch === "string" ? dispatch : protocolKeys.get(dispatch);
      if (!implementations.has(key)) implementations.set(key, new Map());
      if (variadic != null && key.startsWith("Object/")) objectVariadics.set(key, [variadic, method]);
      else implementations.get(key).set(arity, method);
      if (!key.startsWith("Object/") && key !== "IFn/-invoke") patchDeclaration(value, protocolSlot(key, arity), {value: arity === 0 ? protocolSentinel : method, writable: true, configurable: true});
      if (key === "IFn/-invoke") {
        if (variadic != null) ifnVariadics.set(calls, method);
        else calls[arity - 1] = method;
        invokes[arity - 1] = invoke;
      }
    }
    for (const [key, entries] of implementations) if (key.startsWith("Object/")) {
      patchDeclaration(value, mungeMember(key.slice(7)), {value: function (...args) {
        const rest = objectVariadics.get(key);
        const method = !rest && entries.size === 1 ? entries.values().next().value : entries.get(args.length + 1) ?? rest?.[1];
        if (!method) return invalidArity(args.length);
        // The pinned CLJS multi-arity Object dispatcher calls the variadic
        // entry as a property of its delegate function, losing the instance.
        const receiver = rest && entries.size && !entries.has(args.length + 1) ? method : this;
        return method.call(receiver, receiver, ...args);
      }, writable: true, configurable: true});
    }
    if (Object.keys(calls).length || ifnVariadics.has(calls)) {
      ifnValues.set(value, calls);
      let call = ifnPrototype.call;
      const variadic = ifnVariadics.get(calls);
      if (variadic) {
        call = Object.keys(calls).length ? function (_self, ...args) { return ifnPrototype.call.call(this, _self, ...args); } : variadic;
        call.cljs$lang$maxFixedArity = variadic.cljs$lang$maxFixedArity;
        call.cljs$lang$applyTo = variadic.cljs$lang$applyTo;
        call.cljs$core$IFn$_invoke$arity$variadic = variadic.cljs$core$IFn$_invoke$arity$variadic;
        for (const [arity, method] of Object.entries(calls)) call["cljs$core$IFn$_invoke$arity$" + (+arity + 1)] = method;
      }
      patchDeclaration(value, "call", {value: call, writable: true, configurable: true});
      patchDeclaration(value, "apply", {value: ifnPrototype.apply, writable: true, configurable: true});
      for (const [arity, method] of Object.entries(invokes)) patchDeclaration(value, "cljs$core$IFn$_invoke$arity$" + arity, {value: ifnInvokeEntry(method), writable: true, configurable: true, enumerable: true});
    }
    protocolImplementations.set(value, implementations);
    return value;
  }
  //@unit makeReify requires: installMethods, reifyValues
  function makeReify(methods) { const value = installMethods({}, methods); reifyValues.add(value); return value; }
  //@unit reifySites
  const reifySites = new Map();
  //@unit reify requires: makeType, metadataValues, reifySites, reifyValues
  // The methods read the captured locals under `members` (`decorateType`),
  // which the emitter munges the same way.
  function reify(site, fields, values, methods, metadata = null) {
    let Type = reifySites.get(site);
    if (!Type) {
      Type = makeType(fields, [
        ["IMeta", 0, null], ["IWithMeta", 0, null],
        ["IMeta/-meta", 1, function () { return metadataValues.has(this) ? metadataValues.get(this) : null; }],
        ["IWithMeta/-with-meta", 2, function (_self, metadata) {
          const result = new Type(...Type.members.map(member => this[member]));
          reifyValues.add(result);
          metadataValues.set(result, metadata);
          return result;
        }], ...methods()], false, null, null, null, function (...values) {
        // Own fields like a type's, whatever their names (`__proto__`).
        Type.members.forEach((member, i) => Object.defineProperty(this, member, {value: values[i], writable: true, enumerable: true, configurable: true}));
      });
      reifySites.set(site, Type);
    }
    const value = new Type(...values);
    metadataValues.set(value, metadata);
    reifyValues.add(value);
    return value;
  }
  //@unit mungeCharacters effect requires: memberMunging
  const mungeCharacters = {...memberMunging, "/": "_SLASH_"};
  // These two upstream CHAR_MAP entries are two-character strings, while the
  // public function iterates one UTF-16 character at a time.
  delete mungeCharacters['"']; delete mungeCharacters["\\"];
  mungeCharacters['\\"'] = "_DOUBLEQUOTE_"; mungeCharacters["\\\\"] = "_BSLASH_";
  //@unit demungeCharacters requires: mungeCharacters
  const demungeCharacters = Object.fromEntries(Object.entries(mungeCharacters).map(([a, b]) => [b, a]));
  //@unit demungePattern requires: demungeCharacters
  const demungePattern = new RegExp(Object.keys(demungeCharacters).sort((a, b) => b.length - a.length).join("|") + "|\\$", "g");
  //@unit reservedNames
  const reservedNames = new Set("arguments abstract await boolean break byte case catch char class const continue debugger default delete do double else enum export extends final finally float for function goto if implements import in instanceof int interface let long native new package private protected public return short static super switch synchronized this throw throws transient try typeof var void volatile while with yield methods null constructor".split(" "));
  //@unit mungeStr requires: mungeCharacters
  function mungeStr(value) { let result = ""; for (let i = 0; i < value.length; i++) result += mungeCharacters[value.charAt(i)] ?? value.charAt(i); return result; }
  //@unit munge requires: mungeStr, reservedNames, str, symbolCore, symbolOf
  function munge(value) { let result = mungeStr(str(value)); result = result === ".." ? "_DOT__DOT_" : reservedNames.has(result) ? result + "$" : result; return symbolOf(value) !== undefined ? symbolCore(result) : result; }
  //@unit demunge requires: demungeCharacters, demungePattern, str, symbolCore, symbolOf
  function demunge(value) { let result = str(value); result = result === "_DOT__DOT_" ? ".." : result.replace(/\$$/, "").replace(demungePattern, key => key === "$" ? "/" : demungeCharacters[key]); return symbolOf(value) !== undefined ? symbolCore(result) : result; }
  //@unit mapEntry
  // A map entry is a vector of its key and value of type `MapEntryType`: one
  // `makeVector` builds where the program can build vectors
  // (`mapEntry.vector`), and else one with the store it would build, a tail
  // of two items (`mapEntry.lean`), so that it keeps no vector type. The
  // shared runtime defines it twice, the second time with `makeVector`.
  let mapEntry;
  //@unit mapEntry.lean effect: mapEntry unless: makeVector requires: EntryCell, mapEntry
  mapEntry = (key, value) => {
    return new EntryCell(key, value);
  };
  //@unit mapEntry.vector effect: mapEntry when: makeVector requires: EntryCell, mapEntry
  // The same as `mapEntry.lean`.
  mapEntry = (key, value) => {
    return new EntryCell(key, value);
  };
  //@unit recordKeys requires: keyword
  // A record type's field keywords, interned once per type (the members its
  // constructor assigns the fields to are `members`, see `decorateType`).
  function recordKeys(Type) {
    return Type.fieldKeys ??= Type.fieldNames.map(name => keyword(null, name));
  }
  //@unit recordFieldIndex requires: equalPair, recordKeys
  // The index of the field `key` names, or -1, for `dissoc`, which upstream
  // tests with a set of the field keywords. Keyword literals are interned, so
  // a field's own keyword is found by identity; other keys compare with `=`.
  function recordFieldIndex(Type, key) {
    const keys = recordKeys(Type), index = keys.indexOf(key);
    return index < 0 ? keys.findIndex(field => equalPair(key, field)) : index;
  }
  //@unit recordKeywordFieldIndex requires: keywordFqn
  // Defrecord's lookup/contains use keyword case; assoc uses keyword identity.
  // General IEquiv dispatch can incorrectly match a different field (or even
  // a non-keyword key), and repeats protocol work for each preceding field.
  // A key's index is that of the field its keyword spelling names, -1 for a
  // non-keyword (whose spelling is null). A keyword's spelling never changes,
  // so the type remembers the answer for the first 32 keywords it is asked
  // about (`fieldIndex`, by identity) and reads no spelling for them again.
  function recordKeywordFieldIndex(Type, key) {
    const known = Type.fieldIndex ??= new Map();
    let index = known.get(key), spelling;
    if (index == null && (index = Type.fieldNames.indexOf(spelling = keywordFqn(key)), spelling !== null && known.size < 32)) known.set(key, index);
    return index;
  }
  //@unit recordEntries requires: keyword, mapEntry, toArray
  // Surface records supply protocol methods just as the upstream macro does.
  // Raw defrecord* only emits a constructor; its explicit body installs methods.
  function recordEntries(value, Type) {
    return [...Type.fieldNames.map((name, i) => mapEntry(keyword(null, name), value[Type.members[i]])), ...toArray(value.__extmap)];
  }
  //@unit recordCopy
  function recordCopy(value, Type, metadata, extmap, cachedHash) {
    const Constructor = Type.currentConstructor?.() ?? Type;
    return new Constructor(...Type.members.map(member => value[member]), metadata, extmap, cachedHash);
  }
  //@unit recordMethodTable
  // Builders of record methods, each added by a unit kept where it can run.
  const recordMethodTable = [];
  //@unit recordMethods requires: recordMethodTable
  // Every record protocol's marker, and the methods of the table.
  function recordMethods(Type) {
    const protocols = "ICloneable IHash IEquiv IMeta IWithMeta ILookup ICounted ICollection IAssociative IMap ISeqable IIterable IPrintWithWriter IKVReduce".split(" ");
    return [["IRecord", 0, null], ...protocols.map(key => [key, 0, null]), ...recordMethodTable.flatMap(methods => methods(Type))];
  }
  //@unit recordMethods.install effect: recordMethods when: decorateType requires: recordMethods,
  //@ typeHooks
  typeHooks.recordMethods = recordMethods;
  //@unit recordMethods.clone effect: recordMethods when: clone requires: recordCopy,
  //@ recordMethodTable
  recordMethodTable.push(Type => [
    ["ICloneable/-clone", 1, function () { return recordCopy(this, Type, this.__meta, this.__extmap, this.__hash); }]]);
  //@unit recordEntriesHash requires: hash, nth
  // `result` plus each entry's key hash xor value hash (light-cljs-267).
  function recordEntriesHash(entries, result) {
    for (const entry of entries) result = (result + (hash(nth(entry, 0)) ^ hash(nth(entry, 1)))) | 0;
    return result;
  }
  //@unit recordMethods.hash effect: recordMethods when: hash, hashProtocol requires: recordEntries,
  //@ recordEntriesHash, recordMethodTable, stringHash, typeNames
  recordMethodTable.push(Type => [
    ["IHash/-hash", 1, function () {
      if (this.__hash != null) return this.__hash;
      return this.__hash = recordEntriesHash(recordEntries(this, Type), 0) ^ stringHash(typeNames.get(Type) ?? "");
    }]]);
  //@unit recordMethods.equiv effect: recordMethods when: arrayIndex, arrayMapDissoc, entryIndex,
  //@ equalPair, equivProtocol, objectEquiv requires: equalPair, recordMethodTable
  recordMethodTable.push(Type => [
    ["IEquiv/-equiv", 2, function (_self, other) {
      return other != null && this.constructor === other.constructor &&
        Type.members.every(member => equalPair(this[member], other[member])) && equalPair(this.__extmap, other.__extmap);
    }]]);
  //@unit recordMethods.meta effect: recordMethods when: meta, metaSupported requires: recordMethodTable
  recordMethodTable.push(() => [
    ["IMeta/-meta", 1, function () { return this.__meta; }]]);
  //@unit recordMethods.withMeta effect: recordMethods when: collectionWithMeta, withMetaSupported
  //@ requires: recordCopy, recordMethodTable
  recordMethodTable.push(Type => [
    ["IWithMeta/-with-meta", 2, function (_self, metadata) { return recordCopy(this, Type, metadata, this.__extmap, this.__hash); }]]);
  //@unit recordMethods.lookup effect: recordMethods when: get, lookupProtocol requires: get,
  //@ protocolSlot, recordKeywordFieldIndex, recordMethodTable
  // `(-lookup r k)` calls the three-argument method `r` has, as `get` would.
  recordMethodTable.push((Type, slot = protocolSlot("ILookup/-lookup", 3)) => [
    ["ILookup/-lookup", 2, function (_self, key) { return this[slot](this, key, null); }],
    ["ILookup/-lookup", 3, function (_self, key, fallback) {
      const index = recordKeywordFieldIndex(Type, key); return index < 0 ? get(this.__extmap, key, fallback) : this[Type.members[index]];
    }]]);
  //@unit recordMethods.count effect: recordMethods when: count, countProtocol requires: count,
  //@ recordMethodTable
  recordMethodTable.push(Type => [
    ["ICounted/-count", 1, function () { return Type.fieldNames.length + count(this.__extmap); }]]);
  //@unit recordMethods.conj effect: recordMethods when: conj, conjProtocol requires: assoc, conj,
  //@ isVector, nth, recordMethodTable, reduce
  recordMethodTable.push(() => [
    ["ICollection/-conj", 2, function (_self, entry) {
      if (isVector(entry)) return assoc(this, nth(entry, 0), nth(entry, 1));
      return reduce((result, pair) => conj(result, pair), this, entry);
    }]]);
  //@unit recordMethods.contains effect: recordMethods when: contains, containsProtocol requires: contains,
  //@ protocolHooks, recordKeywordFieldIndex, recordMethodTable
  // `contains?` of a missing extmap is false unless `extend-type` could have
  // given nil `-contains-key?` (`protocolHooks.extend`).
  recordMethodTable.push(Type => [
    ["IAssociative/-contains-key?", 2, function (_self, key) {
      if (recordKeywordFieldIndex(Type, key) >= 0) return true;
      const extmap = this.__extmap;
      return !(extmap == null && !protocolHooks.extend) && contains(extmap, key);
    }]]);
  //@unit recordMethods.assoc effect: recordMethods when: assoc, assocProtocol requires: assoc,
  //@ recordCopy, recordKeywordFieldIndex, recordMethodTable
  recordMethodTable.push(Type => [
    ["IAssociative/-assoc", 3, function (_self, key, value) {
      const index = recordKeywordFieldIndex(Type, key);
      if (index < 0) return recordCopy(this, Type, this.__meta, assoc(this.__extmap, key, value), null);
      const Constructor = Type.currentConstructor?.() ?? Type;
      return new Constructor(...Type.members.map((member, i) => i === index ? value : this[member]), this.__meta, this.__extmap, null);
    }]]);
  //@unit recordMethods.dissoc effect: recordMethods when: dissoc, dissocProtocol requires: collectionWithMeta,
  //@ dissoc, into, mapFromArray, recordCopy, recordFieldIndex, recordMethodTable, seq
  recordMethodTable.push(Type => [
    ["IMap/-dissoc", 2, function (_self, key) {
      if (recordFieldIndex(Type, key) >= 0) return dissoc(collectionWithMeta(into(mapFromArray([]), this), this.__meta), key);
      const extmap = dissoc(this.__extmap, key);
      return recordCopy(this, Type, this.__meta, seq(extmap) === null ? null : extmap, null);
    }]]);
  //@unit recordMethods.seq effect: recordMethods when: lazyBranch, reduce, rseq, seq, seqBranch,
  //@ seqProtocol, vectorFirst, vectorRest requires: recordEntries, recordMethodTable, seq,
  //@ vectorFromArray
  recordMethodTable.push(Type => [
    ["ISeqable/-seq", 1, function () { return seq(vectorFromArray(recordEntries(this, Type))); }]]);
  //@unit recordMethods.iterator effect: recordMethods when: iterator, iteratorProtocol requires: iteratorInit,
  //@ recordMethodTable, SeqIter
  recordMethodTable.push(() => [
    ["IIterable/-iterator", 1, function () { return new SeqIter(iteratorInit, this); }]]);
  //@unit recordMethods.print effect: recordMethods when: printWriterImpl, prWriter requires: printHooks,
  //@ recordEntries, recordMethodTable, typeNames
  recordMethodTable.push(Type => [
    ["IPrintWithWriter/-pr-writer", 3, function (_self, writer, options) {
      const {sequentialWriter, writer: printOne} = printHooks;
      return sequentialWriter(writer, (entry, target, opts) => sequentialWriter(target, printOne, "", " ", "", opts, entry),
        "#" + (typeNames.get(Type) ?? "").replaceAll("/", ".") + "{", ", ", "}", options, recordEntries(this, Type));
    }]]);
  //@unit recordMethods.kvReduce effect: recordMethods when: kvReduceProtocol, reduceKv requires: nth,
  //@ recordMethodTable, reduce
  recordMethodTable.push(() => [
    ["IKVReduce/-kv-reduce", 3, function (_self, fn, init) {
      return reduce((result, entry) => fn.call(null, result, nth(entry, 0), nth(entry, 1)), init, this);
    }]]);
  //@unit TypeBrand
  // A type instance's brand: a private field holding its record type, or
  // `null` for another type. Like the stores, copying fields or inheriting a
  // prototype cannot forge it, and setting it writes no WeakMap. The base
  // class returns the instance as `this`, and `TypeBrand` adds the field.
  // A binding, not a declaration, so terse output renames it.
  const TypeBrand = class extends (class { constructor(value) { return value; } }) {
    #t;
    constructor(value, type = null) { super(value); this.#t = type; }
    // The brand of `value`, or `undefined` for a value without one.
    static type(value) { if (Object(value) === value && #t in value) return value.#t; }
  };
  //@unit typeFields.brands effect: TypeBrand when: typeFields requires: TypeBrand, typeFields
  typeFields.has = value => TypeBrand.type(value) !== undefined;
  //@unit recordTypes.brands effect: recordMethods when: recordTypes requires: recordTypes,
  //@ TypeBrand
  // Only a program with records keeps `recordMethods`; in any other no
  // instance has a record type.
  recordTypes.has = value => !!TypeBrand.type(value);
  recordTypes.get = value => TypeBrand.type(value) ?? undefined;
  //@unit maps.records effect: recordMethods when: maps requires: maps, recordMaps, recordTypes
  // A record is a map without entries of its own until `updatedRecord` gives
  // it some (`recordMaps`), so constructing one writes no store: these
  // answer for it, with `none` as its entries (a record type is a function).
  maps.has = (has => value => has(value) || recordTypes.has(value))(maps.has);
  maps.get = ((get, none) => value => get(value) ?? recordMaps.get(value) ?? (recordTypes.get(value) && none))(maps.get, Object.freeze([]));
  //@unit mapKinds.records effect: recordMethods when: mapKinds requires: mapKinds, recordMaps,
  //@ recordTypes
  mapKinds.has = (has => value => has(value) || recordTypes.has(value))(mapKinds.has);
  mapKinds.get = (get => value => get(value) ?? recordMaps.kinds.get(value) ?? (recordTypes.has(value) ? "record" : undefined))(mapKinds.get);
  //@unit decorateType requires: installMethods, mungeMember, protocolMaskNames, typeHooks,
  //@ typeNames
  // Gives a type its methods; returns the masks its markers declare. The
  // methods read fields under `members`, the names the constructor
  // assigns them to, munged once per type.
  function decorateType(Type, fields, methods, record, name, currentConstructor) {
    const masks = [0, 0];
    Type.fieldNames = fields;
    Type.members = fields.map(mungeMember);
    if (currentConstructor) Type.currentConstructor = currentConstructor;
    if (name) typeNames.set(Type, name);
    const implementations = record ? [...methods, ...typeHooks.recordMethods(Type)] : methods;
    for (const [key, arity] of implementations) {
      const index = arity === 0 ? protocolMaskNames.lastIndexOf(key) : -1;
      if (index >= 0) masks[index >> 5] |= 1 << (index % 32);
    }
    installMethods(Type.prototype, implementations);
    return masks;
  }
  //@unit makeType requires: decorateType, TypeBrand
  // `init`, which the emitter (or `reify`) writes, assigns the arguments to
  // the munged fields and a record's three slots; `masks` sets a raw type's
  // masks (`typeMasks`). A construction munges no name and writes no WeakMap.
  function makeType(fields, methods, record, masks, name, currentConstructor, init) {
    let implicitMasks;
    const Type = function () {
      init?.apply(this, arguments);
      if (masks) masks(this);
      else {
        this.cljs$lang$protocol_mask$partition0$$LKEY = implicitMasks[0];
        this.cljs$lang$protocol_mask$partition1$$LKEY = implicitMasks[1];
      }
      new TypeBrand(this, record === true ? Type : null);
    };
    implicitMasks = decorateType(Type, fields, methods, record === true, name, currentConstructor);
    return Type;
  }
  //@unit defineType requires: decorateType
  // At `Advanced` the emitter writes each type's constructor, which ends with
  // `new TypeBrand(this)` (a record's `(this, Type)`); its masks are its
  // prototype's. `names` is "ns/T field ...", or an array of them where one
  // holds a space.
  function defineType(Type, names, methods, record, currentConstructor) {
    const [name, ...fields] = names.split?.(" ") ?? names;
    decorateType(Type, fields, methods, record, name, currentConstructor)
      .forEach((mask, i) => Type.prototype["cljs$lang$protocol_mask$partition" + i + "$$LKEY"] = mask);
    Type.cljs$lang$type = true;
    if (!record) Type.cljs$lang$ctorStr = name;
    return Type;
  }
  //@unit ClassBrand
  // The brand of an instance of a type the emitter writes as a class
  // (light-cljs-346): a private field of the class's own, which V8's mid
  // tier adds without the two calls `TypeBrand` costs. The class's prototype
  // holds its check under `#c`; an object made from the prototype or given
  // an instance's fields fails it, and an instance moved to another
  // prototype loses its brand. Until a class type exists no prototype is
  // read, so no proxy's trap runs (the macro host's runtime has none); then
  // at most 64 are, as a proxy's `getPrototypeOf` trap can answer with
  // proxies without end. Its own class rather than a `privateStore`, which
  // a small program would keep only for this.
  const ClassBrand = class extends (class { constructor(value) { return value; } }) {
    #c;
    static #some = false;
    constructor(prototype, brand) { super(prototype); this.#c = brand; ClassBrand.#some = true; }
    static type(value) {
      if (ClassBrand.#some) for (let p = Object.getPrototypeOf(value), n = 64; p !== null && n--; p = Object.getPrototypeOf(p)) if (#c in p) return p.#c(value);
    }
  };
  //@unit ClassBrand.types effect: ClassBrand requires: ClassBrand, TypeBrand
  TypeBrand.type = (type => value => {
    const brand = type(value);
    return brand === undefined && Object(value) === value ? ClassBrand.type(value) : brand;
  })(TypeBrand.type);
  //@unit defineClass requires: ClassBrand, defineType
  // `defineType` of `class { #b; constructor(a0) {...} static c = (a0) => #b
  // in a0; }`: the prototype's brand answers with the check `c`.
  function defineClass(Type, names, methods, record, currentConstructor) {
    const check = Type.c, type = record ? Type : null;
    delete Type.c;
    new ClassBrand(Type.prototype, value => check(value) ? type : undefined);
    return defineType(Type, names, methods, record, currentConstructor);
  }
  //@unit typeMasks requires: isArrayMap, maps
  // What sets the masks of a raw type's mask map, for `makeType`.
  function typeMasks(masks) {
    if (masks == null) return null;
    // A mask map is a literal, built by a builder: an array map's keys and
    // values (light-cljs-1036), or the Pants build's engine map's entries.
    const name = key => "cljs$lang$protocol_mask$partition" + key + "$$LKEY", entries = [];
    if (isArrayMap(masks)) { const arr = masks.arr$LKEY; for (let i = 0; i < arr.length; i += 2) entries.push([name(arr[i]), arr[i + 1]]); }
    else for (const entry of maps.get(masks) ?? []) entries.push([name(entry.key), entry.value]);
    return value => { for (const [key, mask] of entries) value[key] = mask; };
  }
  //@unit recordFromMap requires: dissoc, get, into, isRecord, mapFromArray, recordKeys, seq
  function recordFromMap(Type, map) {
    const keys = recordKeys(Type);
    let extmap = dissoc(map, ...keys);
    if (isRecord(map)) extmap = into(mapFromArray([]), extmap);
    return new Type(...keys.map(key => get(map, key)), null, seq(extmap) === null ? null : extmap, null);
  }
  //@unit ifnMethods requires: ifnValues
  function ifnMethods(value) {
    return ifnValues.get(value) ?? (value == null ? undefined : ifnValues.get(Object.getPrototypeOf(value)));
  }
  //@unit ifnPrototype requires: ifnApply, ifnMethods, ifnVariadics, invalidArity
  const ifnPrototype = Object.freeze({
    call(_self, ...args) {
      const methods = ifnMethods(this), keys = Object.keys(methods);
      const variadic = ifnVariadics.get(methods);
      const method = variadic ? methods[args.length] ?? variadic : keys.length === 1 ? methods[keys[0]] : methods[args.length];
      if (!method) return invalidArity(args.length);
      // The pinned overloaded dispatcher calls its variadic delegate as a
      // function property, just like overloaded Object methods. Single-method
      // variadics and fixed entries keep the actual instance receiver.
      const receiver = variadic && keys.length && !methods[args.length] ? variadic : this;
      return method.call(receiver, receiver, ...args);
    },
    apply(_self, args) { return ifnApply(this, args); }
  });
  //@unit ifnApply
  // Unlike Function.apply, an IFn apply must bundle the 21st and later values
  // exactly once. Keep that path independent of .call's already-expanded ABI.
  function ifnApply(value, args) {
    const values = Array.from(args);
    return value.call(null, ...values.slice(0, 20), ...(values.length > 20 ? [values.slice(20)] : []));
  }
  //@unit boundedCount requires: count, countedValue, methodKeys, next, protocolHooks, seq
  function boundedCount(n, value) {
    if (protocolHooks.satisfies?.(methodKeys[11], value) ?? (value == null || countedValue(value))) return count(value);
    let total = 0;
    for (let current = seq(value); n > 0 && current !== null; n--, current = next(current)) total++;
    return total;
  }
  //@unit applyTo requires: first, rest, seq
  function applyTo(fn, n, args) {
    args = seq(args);
    if (!Number.isInteger(n) || n < 0 || n > 20) throw new Error("Only up to 20 arguments supported on functions");
    const values = [];
    for (let i = 0; i < n; i++, args = rest(args)) values.push(first(args));
    return fn.call(null, ...values);
  }
  //@unit apply requires: applyTo, boundedCount, cons, seq, toArray
  function apply(fn, ...args) {
    // The upstream variadic default also receives calls with fewer than two
    // arguments. Keep its missing leading slots rather than adding validation.
    if (args.length === 0) args = [undefined, undefined, undefined, undefined, null];
    const hasApplyTo = fn.cljs$lang$applyTo;
    let tail = args.pop();
    const prefix = args;
    if (prefix.length >= 4) tail = seq(tail);
    if (hasApplyTo) {
      let arglist = tail;
      for (let i = prefix.length - 1; i >= 0; i--) arglist = cons(prefix[i], arglist);
      const fixed = fn.cljs$lang$maxFixedArity;
      const n = prefix.length + boundedCount(fixed + 1 - prefix.length, tail);
      return n <= fixed ? applyTo(fn, n, arglist) : fn.cljs$lang$applyTo(arglist);
    }
    const values = prefix.concat(toArray(tail));
    if (values.length > 20) return fn.apply(fn, values);
    const entry = fn["cljs$core$IFn$_invoke$arity$" + values.length];
    return entry ? entry.call(fn, ...values) : fn.call(fn, ...values);
  }
  //@unit listStar requires: cons, seq
  function listStar(...args) {
    if (args.length === 0) args = [undefined, undefined, undefined, undefined, null];
    let result = args.pop();
    if (args.length === 0) return seq(result);
    if (args.length >= 4) result = seq(result);
    for (let i = args.length - 1; i >= 0; i--) result = cons(args[i], result);
    return result;
  }
  //@unit iterateNode requires: IterateCell
  function iterateNode(fn, previous, value, realized, following = null) {
    return new IterateCell(fn, previous, value, realized, following);
  }
  //@unit iterate requires: iterateNode
  function iterate(fn, seed) { return iterateNode(fn, null, seed, true); }
  //@unit ChunkBuffer requires: ChunkBufferCell
  // A chunk buffer (`ChunkBufferCell`): an array it fills from `end`.
  function ChunkBuffer(array, end) { return new ChunkBufferCell(array, end); }
  //@unit chunkBuffer requires: ChunkBuffer
  function chunkBuffer(capacity) { return new ChunkBuffer(new Array(capacity), 0); }
  //@unit chunkAppend requires: chunkBufferOf
  function chunkAppend(buffer, item) { const state = chunkBufferOf(buffer); state.buf$LKEY[state.end$LKEY] = item; return ++state.end$LKEY; }
  //@unit chunk requires: arrayChunk, chunkBufferOf
  function chunk(buffer) { const state = chunkBufferOf(buffer), value = arrayChunk(state.buf$LKEY, 0, state.end$LKEY); state.buf$LKEY = null; return value; }
  //@unit arrayChunk requires: ArrayChunkCell
  // Chunk values expose indexed/count/reduce protocols, without being seqs:
  // an `ArrayChunkCell`, whose items are `array`'s from `offset` to `end`.
  function arrayChunk(array, offset = 0, end = array.length) { return new ArrayChunkCell(array, offset, end, null, null); }
  //@unit chunkNth requires: chunkOf, missing
  function chunkNth(chunk, i, fallback = missing) {
    const state = chunkOf(chunk), size = state.end$LKEY - state.off$LKEY, integer = state.step$LKEY !== null;
    if (integer && fallback === missing) return state.first$LKEY + i * state.step$LKEY;
    if (i >= 0 && i < size) return integer ? state.first$LKEY + i * state.step$LKEY : state.arr$LKEY[state.off$LKEY + i];
    if (fallback !== missing) return fallback;
    // ArrayChunk's two-argument -nth reads the backing array directly.
    return state.arr$LKEY?.[state.off$LKEY + i];
  }
  //@unit arrayChunk.nth effect: arrayChunk, ownedChunkFirst, dropFirstChunk when: nth requires: chunkNth,
  //@ kindHooks
  kindHooks.nthChunk = chunkNth;
  //@unit makeTransient.nth effect: makeTransient when: nth requires: kindHooks, transientNth
  kindHooks.nthTransient = transientNth;
  //@unit makeTransient.count effect: makeTransient when: count requires: editableTransient,
  //@ kindHooks
  // A transient vector's entries are its store; a map's or set's an array.
  kindHooks.countTransient = value => {
    const state = editableTransient(value, "count");
    // A transient array map counts its pairs (`TransientArrayMapCell`).
    return state === value ? value.cljs$lang$kind$$LKEY === 31 ? value.arr$LKEY.length >> 1 : state.cnt$LKEY : state.entries.cnt ?? state.entries.length;
  };
  // The chunk operations call a value's keyed slot first, as the slot-first
  // wrappers do (`nth.slots`, light-cljs-1029): a lazy pipeline's step reads
  // its source's chunk without the protocol dispatcher's lookups. A value
  // whose slot is gone, or gives `noBranch`, takes the dispatcher.
  //@unit chunkFirst requires: chunkedFirstProtocol, noBranch, slotFirst
  function chunkFirst(value) {
    if (slotFirst.on && value != null && value.cljs$lang$kind$$LKEY !== undefined) {
      const method = value.cljs$core$IChunkedSeq$_chunked_first$arity$1$LKEY;
      if (method) { const result = method.call(value, value); if (result !== noBranch) return result; }
    }
    return chunkedFirstProtocol(value);
  }
  //@unit chunkRest requires: chunkedRestProtocol, noBranch, slotFirst
  function chunkRest(value) {
    if (slotFirst.on && value != null && value.cljs$lang$kind$$LKEY !== undefined) {
      const method = value.cljs$core$IChunkedSeq$_chunked_rest$arity$1$LKEY;
      if (method) { const result = method.call(value, value); if (result !== noBranch) return result; }
    }
    return chunkedRestProtocol(value);
  }
  //@unit chunkNext requires: chunkedNextProtocol, chunkedSeqValue, chunkRest, methodKeys, noBranch,
  //@ protocolHooks, seq, slotFirst
  function chunkNext(value) {
    if (slotFirst.on && value != null && value.cljs$lang$kind$$LKEY !== undefined) {
      const method = value.cljs$core$IChunkedNext$_chunked_next$arity$1$LKEY;
      if (method) { const result = method.call(value, value); if (result !== noBranch) return result; }
    }
    return (protocolHooks.implements?.(methodKeys[4], value) ?? chunkedSeqValue(value)) ? chunkedNextProtocol(value) : seq(chunkRest(value));
  }
  //@unit chunkDropFirst requires: dropFirstProtocol, noBranch, slotFirst
  // `-drop-first` of a chunk, as `chunkFirst`.
  function chunkDropFirst(chunk) {
    if (slotFirst.on && chunk != null && chunk.cljs$lang$kind$$LKEY !== undefined) {
      const method = chunk.cljs$core$IChunk$_drop_first$arity$1$LKEY;
      if (method) { const result = method.call(chunk, chunk); if (result !== noBranch) return result; }
    }
    return dropFirstProtocol(chunk);
  }
  //@unit ownedChunkFirst requires: arrayChunk, cellOf, chunkedConsOf, forceRangeChunk,
  //@ publicCollectionSlots, viewOf
  function ownedChunkFirst(value) {
    const chunked = chunkedConsOf(value);
    if (chunked !== undefined) return chunked.chunk$LKEY;
    const cell = cellOf(value);
    if (cell?.cell$LKEY === "range") {
      if (cell.integer$LKEY) {
        const chunk = arrayChunk([], 0, Math.min(32, cell.count$LKEY));
        chunk.first$LKEY = cell.first$LKEY; chunk.step$LKEY = cell.step$LKEY;
        return chunk;
      }
      return forceRangeChunk(cell).chunk;
    }
    const slice = viewOf(value);
    if (slice.view$LKEY === "chunked" || publicCollectionSlots.has(value)) return arrayChunk(slice.arr$LKEY, slice.i$LKEY);
    const end = Math.min(slice.arr$LKEY.length, slice.i$LKEY + 32 - slice.i$LKEY % 32);
    return arrayChunk(slice.arr$LKEY, slice.i$LKEY, end);
  }
  //@unit rangeChunks
  const rangeChunks = new WeakMap();
  //@unit forceRangeChunk requires: arrayChunk, emptyList, rangeChunks, rangeNode
  function forceRangeChunk(cell) {
    if (!rangeChunks.has(cell) || rangeChunks.get(cell).chunk == null) {
      const step = cell.step$LKEY, end = cell.end$LKEY, values = []; let current = cell.first$LKEY;
      do { values.push(current); current += step; }
      while (values.length < 32 && (step > 0 ? current < end : current > end));
      const tail = (step > 0 ? current < end : current > end) ? rangeNode(current, end, step, false) : emptyList;
      const state = rangeChunks.get(cell) ?? {};
      state.chunk = arrayChunk(values);
      if (values.length === 32 && tail !== emptyList) state.tail = tail;
      rangeChunks.set(cell, state);
    }
    return rangeChunks.get(cell);
  }
  //@unit ownedChunkRest requires: cellOf, chunkedConsOf, emptyList, forceRangeChunk, kindHooks,
  //@ rangeNode, viewAt, viewOf
  function ownedChunkRest(value) {
    const chunked = chunkedConsOf(value);
    if (chunked !== undefined) return chunked.more$LKEY ?? emptyList;
    const cell = cellOf(value);
    if (cell?.cell$LKEY === "range") return cell.integer$LKEY ? (cell.count$LKEY <= 32 ? emptyList : rangeNode(cell.first$LKEY + 32 * cell.step$LKEY, cell.end$LKEY, cell.step$LKEY, true)) : forceRangeChunk(cell).tail ?? emptyList;
    const slice = viewOf(value);
    if (slice.vec$LKEY) return kindHooks.chunkedVectorRest(slice);
    const end = Math.min(slice.arr$LKEY.length, slice.i$LKEY + 32 - slice.i$LKEY % 32);
    return end < slice.arr$LKEY.length ? viewAt(slice, end) : emptyList;
  }
  //@unit ChunkedSeqValue.chunkRest effect: ChunkedSeqValue when: ownedChunkRest requires: chunkedVectorRest,
  //@ kindHooks
  // A chunked seq with a vector (`ChunkedSeqCell`) continues past its leaf.
  kindHooks.chunkedVectorRest = chunkedVectorRest;
  //@unit chunkedVectorRest requires: ChunkedSeqCell, count, emptyList, vectorChunk
  // The chunks after a chunked seq's leaf.
  function chunkedVectorRest(slice) {
    const vec = slice.vec$LKEY, end = slice.base$LKEY + slice.arr$LKEY.length;
    return end < count(vec) ? new ChunkedSeqCell(vectorChunk(vec, end), 0, vec, end) : emptyList;
  }
  //@unit chunkStore requires: chunkOf, protocolHooks
  // A runtime chunk's store where no protocol implementation exists, which
  // then gives the items and count that `nth` and `count` give; else
  // undefined (light-cljs-237).
  function chunkStore(chunk) { return protocolHooks.publicMethod ? undefined : chunkOf(chunk); }
  //@unit chunkItem requires: nth
  // Item `i` of a chunk, below its count: from `store`, its `chunkStore`.
  function chunkItem(chunk, store, i) { return store === undefined ? nth(chunk, i) : store.step$LKEY !== null ? store.first$LKEY + i * store.step$LKEY : store.arr$LKEY[store.off$LKEY + i]; }
  //@unit dropFirstChunk requires: arrayChunk, chunkOf, count, protocolHooks
  // A runtime chunk without its first item.
  function dropFirstChunk(value) {
    const c = chunkOf(value), integer = c.step$LKEY !== null;
    const size = protocolHooks.publicMethod ? count(value) : c.end$LKEY - c.off$LKEY;
    if (integer ? size <= 1 : size === 0) throw new Error("-drop-first of empty chunk");
    const result = arrayChunk(c.arr$LKEY, c.off$LKEY + 1, c.end$LKEY);
    if (integer) { result.first$LKEY = c.first$LKEY + c.step$LKEY; result.step$LKEY = c.step$LKEY; }
    return result;
  }
  //@unit chunkedConsFirst requires: chunkStore, nth
  // The first item of a chunked cons (`chunkedConsOf`): where no protocol
  // implementation exists, a runtime chunk's items and count are its
  // store's, as `nth`, `count` and `-drop-first` give them.
  function chunkedConsFirst(chunked) {
    const chunk = chunked.chunk$LKEY, c = chunkStore(chunk);
    return c === undefined ? nth(chunk, 0) : c.step$LKEY !== null ? c.first$LKEY + 0 * c.step$LKEY : c.arr$LKEY?.[c.off$LKEY];
  }
  //@unit chunkedConsRest requires: chunkDropFirst, ChunkedCons, chunkStore, count, dropFirstChunk,
  //@ emptyList
  // The rest of a chunked cons, as `chunkedConsFirst` reads its chunk.
  function chunkedConsRest(chunked) {
    const chunk = chunked.chunk$LKEY, more = chunked.more$LKEY, c = chunkStore(chunk);
    if (c === undefined) return count(chunk) > 1 ? ChunkedCons(chunkDropFirst(chunk), more, null, null) : more ?? emptyList;
    return c.end$LKEY - c.off$LKEY > 1 ? ChunkedCons(dropFirstChunk(chunk), more, null, null) : more ?? emptyList;
  }
  //@unit ChunkedCons.firstSeq effect: ChunkedCons when: firstSeq requires: chunkedConsFirst,
  //@ kindHooks
  // A chunked cons's first and rest where `first` and `rest` take them from
  // the seq they have (`firstSeq`, `restSeq`): only a program that builds
  // chunked conses keeps their chunk operations.
  kindHooks.chunkedConsFirst = chunkedConsFirst;
  //@unit ChunkedCons.restSeq effect: ChunkedCons when: restSeq requires: chunkedConsRest, kindHooks
  kindHooks.chunkedConsRest = chunkedConsRest;
  //@unit ChunkedCons requires: ChunkedConsCell, metadataValues
  // A chunked cons (`ChunkedConsCell`). Nil metadata is not stored, as for
  // LazySeq.
  function ChunkedCons(chunk, more, metadata, cachedHash) {
    const value = new ChunkedConsCell(chunk, more, cachedHash);
    if (metadata !== null) metadataValues.set(value, metadata);
    return value;
  }
  //@unit chunkCons requires: ChunkedCons, count
  function chunkCons(chunk, tail) { return count(chunk) === 0 ? tail : ChunkedCons(chunk, tail, null, null); }
  //@unit dropOwned requires: cellOf, holdsVector, kindHooks, maps, rangeNode, repeat, seq, viewAt,
  //@ viewOf
  function dropOwned(value, n) {
    if (value == null) return null;
    if (maps.has(value)) return dropOwned(seq(value), n);
    const cell = cellOf(value);
    if (cell?.cell$LKEY === "repeat") return cell.remaining$LKEY === -1 ? value : repeat(cell.remaining$LKEY - n, cell.first$LKEY);
    if (cell?.cell$LKEY === "range") return n > 0 ? n < cell.count$LKEY ? rangeNode(cell.first$LKEY + cell.step$LKEY * n, cell.end$LKEY, cell.step$LKEY, true) : null : value;
    const slice = viewOf(value);
    if (slice?.vec$LKEY) return kindHooks.dropChunkedSeq(slice, n);
    if (holdsVector(value)) return kindHooks.dropVector(value, n);
    const offset = slice.i$LKEY + n;
    return offset < slice.arr$LKEY.length ? viewAt(slice, offset) : null;
  }
  //@unit ChunkedSeqValue.drop effect: ChunkedSeqValue when: dropOwned requires: ChunkedSeqCell,
  //@ count, kindHooks, vectorChunk
  // A chunked seq with a vector (`ChunkedSeqCell`) drops into later leaves.
  kindHooks.dropChunkedSeq = (slice, n) => {
    const vec = slice.vec$LKEY, offset = slice.i$LKEY + n;
    if (offset < slice.arr$LKEY.length) return new ChunkedSeqCell(slice.arr$LKEY, offset, vec, slice.base$LKEY);
    const i = slice.base$LKEY + offset, newOffset = i % 32;
    return i < count(vec) ? new ChunkedSeqCell(vectorChunk(vec, i), newOffset, vec, i - newOffset) : null;
  };
  //@unit makeVector.drop effect: makeVector, mapEntry when: dropOwned requires: ChunkedSeqCell,
  //@ IndexedSeqCell, kindHooks, vectorChunk, vectorStore
  // As `makeVector.seq`, from index `n`.
  kindHooks.dropVector = (value, n) => {
    const store = vectorStore(value);
    return n >= store.cnt$LKEY ? null : store.cnt$LKEY <= 32 ? new IndexedSeqCell(vectorChunk(value, 0), n, true, "indexed")
      : new ChunkedSeqCell(vectorChunk(value, n), n & 31, value, n & -32);
  };
  //@unit numberArgument
  function numberArgument(value) {
    if (typeof value !== "number") throw new Error("Assert failed: (number? n)");
  }
  //@unit collectionArity requires: invalidArity
  function collectionArity(name, args) {
    if (args !== 2) return invalidArity(args);
  }
  //@unit take requires: collectionArity, cons, ensureReduced, first, invalidArity, lazySequence,
  //@ numberArgument, reducingFunction, rest, seq, transform
  function take(n, coll) {
    if (arguments.length !== 1 && arguments.length !== 2) return invalidArity(arguments.length);
    numberArgument(n);
    if (arguments.length === 1) return transform(rf => {
      let remaining = n;
      return reducingFunction(rf, (result, input) => {
        const prior = remaining; remaining--;
        const ret = prior > 0 ? rf.call(null, result, input) : result;
        return remaining > 0 ? ret : ensureReduced(ret);
      });
    });
    collectionArity("take", arguments.length);
    return lazySequence(() => {
      const s = n > 0 ? seq(coll) : null;
      return s === null ? null : cons(first(s), take(n - 1, rest(s)));
    });
  }
  //@unit drop requires: collectionArity, dropProtocol, dropValue, emptyList, invalidArity,
  //@ lazySequence, methodKeys, numberArgument, protocolHooks, reducingFunction, rest, seq,
  //@ transform
  function drop(n, coll) {
    if (arguments.length !== 1 && arguments.length !== 2) return invalidArity(arguments.length);
    numberArgument(n);
    if (arguments.length === 1) return transform(rf => {
      let remaining = n;
      return reducingFunction(rf, (result, input) => { const prior = remaining; remaining--; return prior > 0 ? result : rf.call(null, result, input); });
    });
    collectionArity("drop", arguments.length);
    if (protocolHooks.implements?.(methodKeys[13], coll) ?? dropValue(coll)) return (n > 0 ? dropProtocol(coll, Math.ceil(n)) : seq(coll)) ?? emptyList;
    return lazySequence(() => { let s = seq(coll), remaining = n; while (remaining > 0 && s !== null) { s = seq(rest(s)); remaining--; } return s; });
  }
  //@unit mapSeq requires: apply, arrayChunk, callable, chunkCons, chunkedSeqValue, chunkFirst,
  //@ chunkItem, chunkRest, chunkStore, cons, count, first, invalidArity, lazySequence,
  //@ listFromArray, mapManySteps, methodKeys, protocolHooks, rest, seq, transform
  function mapSeq(fn, ...colls) {
    if (arguments.length === 0) return lazySequence(() => null);
    if (arguments.length === 1) return transform(rf => callable(2, {
      0: () => rf.call(null), 1: result => rf.call(null, result),
      2: (result, input) => rf.call(null, result, fn.call(null, input))
    }, (result, input, inputs) => rf.call(null, result, apply(fn, input, inputs))));
    if (!colls.length) return invalidArity(arguments.length);
    // The variadic branch maps a lazy sequence of argument tuples and uses
    // apply. This preserves bounded rest arguments and host receiver behavior.
    if (colls.length > 3) return mapSeq(args => apply(fn, args), mapManySteps(listFromArray(colls)));
    return lazySequence(() => {
      const sources = [];
      for (const coll of colls) { const s = seq(coll); sources.push(s); if (colls.length > 3 && s === null) return null; }
      if (sources.some(value => value === null)) return null;
      if (sources.length === 1 && (protocolHooks.implements?.(methodKeys[5], sources[0]) ?? chunkedSeqValue(sources[0]))) {
        const chunk = chunkFirst(sources[0]), store = chunkStore(chunk), size = count(chunk), items = [];
        for (let i = 0; i < size; i++) items.push(fn.call(null, chunkItem(chunk, store, i)));
        return chunkCons(arrayChunk(items), mapSeq(fn, chunkRest(sources[0])));
      }
      return cons(fn.call(null, ...sources.map(first)), mapSeq(fn, ...sources.map(rest)));
    });
  }
  //@unit mapManySteps requires: cons, every, first, lazySequence, mapSeq, rest, seq
  function mapManySteps(collections) {
    return lazySequence(() => {
      const sources = mapSeq(seq, collections);
      return every(x => x, sources) ? cons(mapSeq(first, sources), mapManySteps(mapSeq(rest, sources))) : null;
    });
  }
  //@unit indexedSequence requires: arrayChunk, chunkCons, chunkedSeqValue, chunkFirst, chunkItem,
  //@ chunkRest, chunkStore, cons, count, first, lazySequence, methodKeys, protocolHooks, rest, seq
  function indexedSequence(fn, coll, index, keepOnly, indexed = true) {
    return lazySequence(() => {
      const s = seq(coll), at = index;
      if (s !== null) {
        if (protocolHooks.implements?.(methodKeys[5], s) ?? chunkedSeqValue(s)) {
          const chunk = chunkFirst(s), store = chunkStore(chunk), size = count(chunk), values = [];
          for (let i = 0; i < size; i++) { const value = indexed ? fn.call(null, at + i, chunkItem(chunk, store, i)) : fn.call(null, chunkItem(chunk, store, i)); if (!keepOnly || value != null) values.push(value); }
          return chunkCons(arrayChunk(values), indexedSequence(fn, chunkRest(s), at + size, keepOnly, indexed));
        }
        const value = indexed ? fn.call(null, at, first(s)) : fn.call(null, first(s)), tail = rest(s);
        if (!keepOnly || value != null) return cons(value, indexedSequence(fn, tail, at + 1, keepOnly, indexed));
        return indexedSequence(fn, tail, at + 1, keepOnly, indexed);
      }
      return null;
    });
  }
  //@unit mapIndexed requires: collectionArity, indexedSequence, indexedTransform
  function mapIndexed(fn, coll) { if (arguments.length === 1) return indexedTransform(fn, false, true); collectionArity("map-indexed", arguments.length); return indexedSequence(fn, coll, 0, false); }
  //@unit keepIndexed requires: collectionArity, indexedSequence, indexedTransform
  function keepIndexed(fn, coll) { if (arguments.length === 1) return indexedTransform(fn, true, true); collectionArity("keep-indexed", arguments.length); return indexedSequence(fn, coll, 0, true); }
  //@unit keep requires: collectionArity, indexedSequence, indexedTransform
  function keep(fn, coll) { if (arguments.length === 1) return indexedTransform(fn, true, false); collectionArity("keep", arguments.length); return indexedSequence(fn, coll, 0, true, false); }
  //@unit filter requires: arrayChunk, chunkCons, chunkedSeqValue, chunkFirst, chunkItem, chunkRest,
  //@ chunkStore, collectionArity, cons, count, first, lazySequence, methodKeys, protocolHooks,
  //@ reducingFunction, rest, seq, transform, truth
  function filter(pred, coll) {
    if (arguments.length === 1) return transform(rf => reducingFunction(rf, (result, input) => truth(pred.call(null, input)) ? rf.call(null, result, input) : result));
    collectionArity("filter", arguments.length);
    return lazySequence(() => {
      const s = seq(coll);
      if (s !== null) {
        if (protocolHooks.implements?.(methodKeys[5], s) ?? chunkedSeqValue(s)) {
          const chunk = chunkFirst(s), store = chunkStore(chunk), size = count(chunk), values = [];
          for (let i = 0; i < size; i++) { const value = chunkItem(chunk, store, i); if (truth(pred.call(null, value))) values.push(value); }
          return chunkCons(arrayChunk(values), filter(pred, chunkRest(s)));
        }
        // Upstream obtains rest before invoking the predicate.
        const value = first(s), tail = rest(s);
        if (truth(pred.call(null, value))) return cons(value, filter(pred, tail));
        return filter(pred, tail);
      }
      return null;
    });
  }
  //@unit remove requires: collectionArity, complement, filter
  function remove(pred, coll) { if (arguments.length === 1) return filter(complement(pred)); collectionArity("remove", arguments.length); return filter(complement(pred), coll); }
  //@unit takeWhile requires: collectionArity, cons, first, lazySequence, reduced, reducingFunction,
  //@ rest, seq, transform, truth
  function takeWhile(pred, coll) {
    if (arguments.length === 1) return transform(rf => reducingFunction(rf, (result, input) => truth(pred.call(null, input)) ? rf.call(null, result, input) : reduced(result)));
    collectionArity("take-while", arguments.length);
    return lazySequence(() => { const s = seq(coll); return s !== null && truth(pred.call(null, first(s))) ? cons(first(s), takeWhile(pred, rest(s))) : null; });
  }
  //@unit dropWhile requires: collectionArity, first, lazySequence, reducingFunction, rest, seq,
  //@ transform, truth
  function dropWhile(pred, coll) {
    if (arguments.length === 1) return transform(rf => {
      let dropping = true;
      return reducingFunction(rf, (result, input) => {
        if (dropping && truth(pred.call(null, input))) return result;
        dropping = false; return rf.call(null, result, input);
      });
    });
    collectionArity("drop-while", arguments.length);
    return lazySequence(() => { let s = seq(coll); while (s !== null && truth(pred.call(null, first(s)))) s = seq(rest(s)); return s; });
  }
  //@unit repeatedly requires: cons, invalidArity, lazySequence, take
  function repeatedly(n, fn) {
    if (arguments.length === 1) { fn = n; return lazySequence(() => cons(fn.call(null), repeatedly(fn))); }
    if (arguments.length === 2) return take(n, repeatedly(fn));
    return invalidArity(arguments.length);
  }
  //@unit replicate requires: repeat, take
  function replicate(n, value) { return take(n, repeat(value)); }
  //@unit takeNth requires: collectionArity, cons, drop, first, invalidArity, lazySequence,
  //@ numberArgument, reducingFunction, seq, transform
  function takeNth(n, coll) {
    if (arguments.length !== 1 && arguments.length !== 2) return invalidArity(arguments.length);
    numberArgument(n);
    if (arguments.length === 1) return transform(rf => {
      let index = -1;
      return reducingFunction(rf, (result, input) => ++index % n === 0 ? rf.call(null, result, input) : result);
    });
    collectionArity("take-nth", arguments.length);
    return lazySequence(() => { const s = seq(coll); return s === null ? null : cons(first(s), takeNth(n, drop(n, s))); });
  }
  //@unit dropLast requires: drop, invalidArity, mapSeq
  function dropLast(n, coll) { if (arguments.length === 1) { coll = n; n = 1; } else if (arguments.length !== 2) return invalidArity(arguments.length); return mapSeq((x, _) => x, coll, drop(n, coll)); }
  //@unit takeLast requires: drop, next, seq
  function takeLast(n, coll) { let s = seq(coll), lead = seq(drop(n, coll)); while (lead !== null) { s = next(s); lead = next(lead); } return s; }
  //@unit splitAt requires: drop, take, vector
  function splitAt(n, coll) { return vector(take(n, coll), drop(n, coll)); }
  //@unit splitWith requires: dropWhile, takeWhile, vector
  function splitWith(pred, coll) { return vector(takeWhile(pred, coll), dropWhile(pred, coll)); }
  //@unit splitVAt requires: drop, take, vec, vector
  function splitVAt(n, coll) { return vector(vec(take(n, coll)), drop(n, coll)); }
  //@unit partitionV requires: concat, cons, count, invalidArity, lazySequence, list, nthrest, seq,
  //@ take, vec
  function partitionV(n, step, pad, coll) {
    const arity = arguments.length;
    if (arity === 2) { coll = step; step = n; } else if (arity === 3) coll = pad; else if (arity !== 4) return invalidArity(arity);
    return lazySequence(() => {
      const s = seq(coll); if (s === null) return null;
      const p = vec(take(n, s));
      if (n === count(p)) return cons(p, arity === 4 ? partitionV(n, step, pad, nthrest(s, step)) : partitionV(n, step, nthrest(s, step)));
      return arity === 4 ? list(vec(take(n, concat(p, pad)))) : null;
    });
  }
  //@unit partitionVAll requires: cons, drop, invalidArity, lazySequence, partitionAll, seq, take,
  //@ vec
  function partitionVAll(n, step, coll) {
    if (arguments.length === 2) { coll = step; step = n; } else if (arguments.length === 1) return partitionAll(n); else if (arguments.length !== 3) return invalidArity(arguments.length);
    return lazySequence(() => { const s = seq(coll); return s === null ? null : cons(vec(take(n, coll)), partitionVAll(n, step, drop(step, s))); });
  }
  //@unit jsIterable
  const jsIterable = value => value != null && value[Symbol.iterator] != null;
  //@unit iteration requires: cons, deref, get, installMethods, isReduced, keyword, lazySequence,
  //@ listFromArray, seqToMap, truth
  function iteration(step, ...options) {
    const opts = seqToMap(listFromArray(options)), option = (name, fallback) => get(opts, keyword(null, name), fallback);
    const somef = option("somef", value => value != null), vf = option("vf", value => value), kf = option("kf", value => value), initk = option("initk", null);
    const sequence = ret => {
      if (!truth(somef.call(null, ret))) return null;
      const value = vf.call(null, ret), key = kf.call(null, ret);
      return cons(value, key == null ? null : lazySequence(() => sequence(step.call(null, key))));
    };
    return installMethods({}, [["ISeqable", 0, null], ["IReduce", 0, null],
      ["ISeqable/-seq", 1, () => sequence(step.call(null, initk))],
      ["IReduce/-reduce", 3, (_self, rf, init) => {
        let acc = init, ret = step.call(null, initk);
        while (truth(somef.call(null, ret))) {
          acc = rf.call(null, acc, vf.call(null, ret));
          if (isReduced(acc)) return deref(acc);
          const key = kf.call(null, ret); if (key == null) return acc;
          ret = step.call(null, key);
        }
        return acc;
      }]]);
  }
  //@unit partition requires: concat, cons, count, drop, invalidArity, lazySequence, list, seq, take
  function partition(n, step, pad, coll) {
    const arity = arguments.length;
    if (arity === 2) { coll = step; step = n; } else if (arity === 3) coll = pad; else if (arity !== 4) return invalidArity(arity);
    return lazySequence(() => {
      const s = seq(coll); if (s === null) return null;
      const p = take(n, s);
      if (n === count(p)) return cons(p, arity === 4 ? partition(n, step, pad, drop(step, s)) : partition(n, step, drop(step, s)));
      return arity === 4 ? list(take(n, concat(p, pad))) : null;
    });
  }
  //@unit partitionAll requires: bufferedTransform, cons, drop, invalidArity, lazySequence, seq,
  //@ take
  function partitionAll(n, step, coll) {
    if (arguments.length === 2) { coll = step; step = n; } else if (arguments.length === 1) return bufferedTransform({kind: "count", n}); else if (arguments.length !== 3) return invalidArity(arguments.length);
    return lazySequence(() => { const s = seq(coll); return s === null ? null : cons(take(n, s), partitionAll(n, step, drop(step, s))); });
  }
  //@unit partitionBy requires: bufferedTransform, collectionArity, cons, count, drop, equalPair,
  //@ first, lazySequence, next, seq, takeWhile
  function partitionBy(fn, coll) {
    if (arguments.length === 1) return bufferedTransform({kind: "key", fn});
    collectionArity("partition-by", arguments.length);
    return lazySequence(() => {
      const s = seq(coll); if (s === null) return null;
      const value = first(s), key = fn.call(null, value), run = cons(value, takeWhile(x => equalPair(key, fn.call(null, x)), next(s)));
      return cons(run, partitionBy(fn, lazySequence(() => drop(count(run), s))));
    });
  }
  //@unit interleave requires: cons, emptyList, first, lazySequence, rest, seq
  function interleave(...colls) {
    if (!colls.length) return emptyList;
    if (colls.length === 1) return lazySequence(() => colls[0]);
    return lazySequence(() => {
      const ss = [];
      for (const coll of colls) { const s = seq(coll); ss.push(s); if (colls.length > 2 && s === null) return null; }
      if (ss.some(s => s === null)) return null;
      let result = interleave(...ss.map(rest));
      for (let i = ss.length - 1; i >= 0; i--) result = cons(first(ss[i]), result);
      return result;
    });
  }
  //@unit interpose requires: collectionArity, drop, interleave, isReduced, reducingFunction,
  //@ repeat, transform
  function interpose(sep, coll) {
    if (arguments.length === 1) return transform(rf => {
      let started = false;
      return reducingFunction(rf, (result, input) => {
        if (started) { result = rf.call(null, result, sep); if (isReduced(result)) return result; }
        else started = true;
        return rf.call(null, result, input);
      });
    });
    collectionArity("interpose", arguments.length);
    return drop(1, interleave(repeat(sep), coll));
  }
  //@unit dorun requires: invalidArity, next, seq
  function dorun(n, coll) {
    if (arguments.length === 1) { coll = n; n = Infinity; } else if (arguments.length !== 2) return invalidArity(arguments.length);
    while (seq(coll) !== null && n > 0) { coll = next(coll); n--; }
    return null;
  }
  //@unit doall requires: dorun, invalidArity
  function doall(n, coll) { if (arguments.length === 1) { dorun(n); return n; } if (arguments.length === 2) { dorun(n, coll); return coll; } return invalidArity(arguments.length); }
  //@unit last requires: first, next, seq
  function last(coll) { let s = seq(coll); while (s !== null) { const tail = next(s); if (tail === null) return first(s); s = tail; } return null; }
  //@unit butlast requires: seq, toArray, vectorFromArray
  function butlast(coll) { const values = toArray(coll); values.pop(); return seq(vectorFromArray(values)); }
  //@unit mapv requires: conjBangOwn, into, mapSeq, persistentBang, reduce, transient, vector
  // As upstream: of one collection, a reduction into a transient vector.
  function mapv(fn, ...colls) {
    return colls.length === 1 ? persistentBang(reduce((v, x) => conjBangOwn(v, fn.call(null, x)), transient(vector()), colls[0]))
      : into(vector(), mapSeq(fn, ...colls));
  }
  //@unit filterv requires: conjBangOwn, persistentBang, reduce, transient, truth, vector
  function filterv(fn, coll) { return persistentBang(reduce((v, x) => truth(fn.call(null, x)) ? conjBangOwn(v, x) : v, transient(vector()), coll)); }
  //@unit cat requires: isReduced, reduce, reduced, reducingFunction
  function cat(rf) {
    return reducingFunction(rf, (result, input) => reduce((acc, value) => {
      const ret = rf.call(null, acc, value);
      return isReduced(ret) ? reduced(ret) : ret;
    }, result, input));
  }
  //@unit mapcat requires: apply, cat, comp, concat, invalidArity, mapSeq
  function mapcat(fn, ...colls) {
    if (arguments.length === 1) return comp(mapSeq(fn), cat);
    if (!colls.length) return invalidArity(arguments.length);
    return apply(concat, mapSeq(fn, ...colls));
  }
  //@unit rangeNode requires: IntegerRangeCell, RangeCell
  function rangeNode(start, end, step, integer, suppliedCount) {
    // A step builds one object, whose `rest` is a getter of its prototype
    // (`rangeRest`), and no closure (light-cljs-237); only an integer
    // range has a count.
    return integer ? new IntegerRangeCell(start, end, step, arguments.length >= 5 ? suppliedCount : Math.ceil((end - start) / step))
      : new RangeCell(start, end, step);
  }
  //@unit IndexedSeqValue requires: IndexedSeqCell, metadataValues
  function IndexedSeqValue(arr, i, metadata) {
    // Positional construction retains the supplied public array and
    // observes subsequent host-array writes.
    const value = new IndexedSeqCell(arr, i, true, "indexed");
    metadataValues.set(value, metadata);
    return value;
  }
  //@unit primSeq requires: IndexedSeqValue, invalidArity
  function primSeq(prim, start) {
    if (arguments.length === 1) start = 0;
    else if (arguments.length !== 2) return invalidArity(arguments.length);
    return start < prim.length ? IndexedSeqValue(prim, start, null) : null;
  }
  //@unit RangeValue requires: constructorSlots, rangeChunks, rangeNode
  function RangeValue(metadata, start, end, step, chunk, chunkNext, cachedHash) {
    const value = rangeNode(start, end, step, false);
    const state = {chunk, tail: chunkNext};
    rangeChunks.set(value, state);
    return constructorSlots(value, {start, end, step}, metadata, cachedHash);
  }
  //@unit IntegerRangeValue requires: constructorSlots, rangeNode
  function IntegerRangeValue(metadata, start, end, step, cnt, cachedHash) {
    return constructorSlots(rangeNode(start, end, step, true, cnt), {start, end, step, cnt}, metadata, cachedHash);
  }
  //@unit range requires: emptyList, invalidArity, isInteger, rangeNode, repeat
  function range(start, end, step = 1) {
    if (arguments.length === 0) { start = 0; end = Number.MAX_VALUE; }
    if (arguments.length === 1) { end = start; start = 0; }
    if (arguments.length > 3) return invalidArity(arguments.length);
    if (step > 0 ? end <= start : step < 0 ? end >= start : end === start) return emptyList;
    if (!(step > 0 || step < 0)) return repeat(start);
    return rangeNode(start, end, step, isInteger(start) && isInteger(end) && isInteger(step));
  }
  //@unit callable requires: fnApplyTo, fnRest
  // Construct the same fixed/variadic split as an emitted CLJS function. This
  // lets combinators hand an unbounded tail to a variadic target unchanged.
  function callable(fixed, methods, variadic) {
    const fn = function (...args) {
      const method = methods[args.length];
      return method ? method(...args) : variadic(...Array.from({length: fixed}, (_, i) => args[i]), fnRest(args, fixed));
    };
    for (const [arity, method] of Object.entries(methods)) fn["cljs$core$IFn$_invoke$arity$" + arity] = method;
    fn.cljs$lang$maxFixedArity = fixed;
    fn.cljs$lang$applyTo = args => fnApplyTo(variadic, fixed, args);
    fn.cljs$core$IFn$_invoke$arity$variadic = variadic;
    return fn;
  }
  //@unit directOrApply requires: apply, missing
  function directOrApply(fn, args, tail) { return tail === missing ? fn.call(null, ...args) : apply(fn, ...args, tail); }
  //@unit complement requires: apply, callable
  function complement(fn) {
    const negate = value => value == null || value === false;
    return callable(2, {0: () => negate(fn.call(null)), 1: x => negate(fn.call(null, x)), 2: (x, y) => negate(fn.call(null, x, y))},
      (x, y, zs) => negate(apply(fn, x, y, zs)));
  }
  //@unit constantly requires: callable
  function constantly(x) { return callable(2, {0: () => x, 1: () => x, 2: () => x}, () => x); }
  //@unit predicateCombination requires: callable, first, next, seq, truth
  function predicateCombination(predicates, small, existential) {
    // The first three arguments visit each predicate in turn. CLJS's 1-3
    // predicate variants then visit each tail argument; the 4+ variant instead
    // traverses that tail once per predicate. Neither tail is materialized.
    const fixed = args => {
      if (!args.length) return existential ? null : true;
      let result = null;
      for (let ps = seq(predicates); ps !== null; ps = next(ps)) {
        const predicate = first(ps);
        for (const arg of args) {
          result = predicate.call(null, arg);
          if (truth(result) === existential) return existential ? result : false;
        }
      }
      // Fixed-arity some-fn with 1-3 predicates returns the final false/nil
      // value; its variadic and 4+ predicate variants use some, returning nil.
      return existential ? small ? result : null : true;
    };
    const tail = args => {
      if (small) {
        for (let xs = seq(args); xs !== null; xs = next(xs)) {
          const value = first(xs);
          for (let ps = seq(predicates); ps !== null; ps = next(ps)) {
            const result = first(ps).call(null, value);
            if (truth(result) === existential) return existential ? result : false;
          }
        }
      } else {
        for (let ps = seq(predicates); ps !== null; ps = next(ps)) {
          const predicate = first(ps);
          for (let xs = seq(args); xs !== null; xs = next(xs)) {
            const result = predicate.call(null, first(xs));
            if (truth(result) === existential) return existential ? result : false;
          }
        }
      }
      return existential ? null : true;
    };
    return callable(3, {0: () => existential ? null : true, 1: x => fixed([x]),
      2: (x, y) => fixed([x, y]), 3: (x, y, z) => fixed([x, y, z])},
    (x, y, z, args) => {
      const result = fixed([x, y, z]);
      return truth(result) === existential ? result : tail(args);
    });
  }
  //@unit predicateConstructor requires: callable, cons, listFromArray, predicateCombination
  function predicateConstructor(existential) {
    const build = (...ps) => predicateCombination(listFromArray(ps), true, existential);
    return callable(3, {1: p => build(p), 2: (p, q) => build(p, q), 3: (p, q, r) => build(p, q, r)},
      (p, q, r, ps) => predicateCombination(cons(p, cons(q, cons(r, ps))), false, existential));
  }
  //@unit someFn requires: predicateConstructor
  const someFn = predicateConstructor(true), everyPred = predicateConstructor(false);
  //@unit memoize requires: apply, assoc, callable, get, mapFromArray, missing
  function memoize(fn) {
    let cache = mapFromArray([]);
    return callable(0, {}, args => {
      const cached = get(cache, args, missing);
      if (cached !== missing) return cached;
      const result = apply(fn, args);
      cache = assoc(cache, args, result);
      return result;
    });
  }
  //@unit trampolineLoop requires: isFn
  function trampolineLoop(fn) {
    for (;;) {
      const result = fn.call(null);
      if (!isFn(result)) return result;
      fn = result;
    }
  }
  //@unit trampoline requires: apply, callable, trampolineLoop
  const trampoline = callable(1, {1: trampolineLoop}, (fn, args) => trampolineLoop(() => apply(fn, args)));
  //@unit comp requires: callable, directOrApply, identity, missing
  function comp(...fns) {
    if (!fns.length) return identity;
    if (fns.length === 1) return fns[0];
    const run = (args, tail) => {
      let result = directOrApply(fns[fns.length - 1], args, tail);
      for (let i = fns.length - 2; i >= 0; i--) result = fns[i].call(null, result);
      return result;
    };
    if (fns.length > 3) return callable(0, {}, args => run([], args));
    return callable(3, {0: () => run([], missing), 1: x => run([x], missing), 2: (x, y) => run([x, y], missing), 3: (x, y, z) => run([x, y, z], missing)},
      (x, y, z, tail) => run([x, y, z], tail));
  }
  //@unit partialFixed requires: apply, callable
  function partialFixed(fn, prefix) {
    if (!prefix.length) return fn;
    const run = (...args) => fn.call(null, ...prefix, ...args);
    return callable(3, {0: () => run(), 1: x => run(x), 2: (x, y) => run(x, y), 3: (x, y, z) => run(x, y, z)},
      (x, y, z, tail) => apply(fn, ...prefix, x, y, z, tail));
  }
  //@unit partial requires: apply, callable, concat, partialFixed
  // Constructors need the same sequence ABI as their returned functions: apply
  // may supply an infinite tail which construction must only capture.
  const partial = callable(4, {
    0: () => partialFixed(undefined, [undefined, undefined, undefined]),
    1: fn => fn,
    2: (fn, a) => partialFixed(fn, [a]),
    3: (fn, a, b) => partialFixed(fn, [a, b]),
    4: (fn, a, b, c) => partialFixed(fn, [a, b, c])
  }, (fn, a, b, c, more) => callable(0, {}, args => apply(fn, a, b, c, concat(more, args))));
  //@unit juxtFixed requires: callable, directOrApply, missing, vectorFromArray
  function juxtFixed(fns) {
    const run = (args, tail) => vectorFromArray(fns.map(fn => directOrApply(fn, args, tail)));
    return callable(3, {0: () => run([], missing), 1: x => run([x], missing), 2: (x, y) => run([x, y], missing), 3: (x, y, z) => run([x, y, z], missing)},
      (x, y, z, tail) => run([x, y, z], tail));
  }
  //@unit juxt requires: callable, directOrApply, juxtFixed, listStar, mapSeq, missing, toArray,
  //@ vectorFromArray
  const juxt = callable(3, {
    0: () => juxtFixed([undefined, undefined, undefined]),
    1: f => juxtFixed([f]),
    2: (f, g) => juxtFixed([f, g]),
    3: (f, g, h) => juxtFixed([f, g, h])
  }, (f, g, h, more) => {
    const fns = listStar(f, g, h, more);
    const run = (args, tail) => vectorFromArray(toArray(mapSeq(fn => directOrApply(fn, args, tail), fns)));
    return callable(3, {0: () => run([], missing), 1: x => run([x], missing), 2: (x, y) => run([x, y], missing), 3: (x, y, z) => run([x, y, z], missing)},
      (x, y, z, tail) => run([x, y, z], tail));
  });
  //@unit fnil requires: apply, callable, invalidArity
  function fnil(fn, ...defaults) {
    if (defaults.length < 1 || defaults.length > 3) return invalidArity(arguments.length);
    const patch = args => args.map((x, i) => i < defaults.length && x == null ? defaults[i] : x);
    const run = (...args) => fn.call(null, ...patch(args));
    const methods = {2: (x, y) => run(x, y), 3: (x, y, z) => run(x, y, z)};
    if (defaults.length === 1) methods[1] = x => run(x);
    return callable(3, methods, (x, y, z, tail) => apply(fn, ...patch([x, y, z]), tail));
  }
  //@unit into requires: conj, conjBangOwn, conjProtocol, editableValue, invalidArity, meta,
  //@ methodKeys, persistentBang, protocolHooks, reduce, transduce, transient, vector,
  //@ withMetaProtocol
  function into(to, from, source) {
    switch (arguments.length) {
      case 0: return vector();
      case 1: return to;
      case 2: {
        if (protocolHooks.implements?.(methodKeys[14], to) ?? editableValue(to))
          return withMetaProtocol(persistentBang(reduce(conjBangOwn, transient(to), from)), meta(to));
        return reduce(to == null ? conj : conjProtocol, to, from);
      }
      case 3: {
        const editable = protocolHooks.implements?.(methodKeys[14], to) ?? editableValue(to), metadata = editable ? meta(to) : null;
        const rf = !editable ? conj : function (result, input) {
          if (arguments.length === 2) return conjBangOwn(result, input);
          if (arguments.length === 1) return withMetaProtocol(persistentBang(result), metadata);
          return invalidArity(arguments.length);
        };
        return transduce(from, rf, editable ? transient(to) : to, source);
      }
      default: return invalidArity(arguments.length);
    }
  }
  //@unit exceptionMessages
  // Match the CLJS constructor: preserve the supplied message/data/cause values
  // and Error identity, including when callers construct ExceptionInfo directly.
  const exceptionMessages = new WeakMap();
  //@unit compilerExceptionMessage compile-time requires: exceptionMessages
  const compilerExceptionMessage = value => exceptionMessages.get(value);
  //@unit ExceptionInfo effect requires: exceptionMessages
  function ExceptionInfo(message, data, cause) {
    const error = new Error(message);
    const target = this == null ? globalThis : Object(this);
    target.message = message;
    target.data = data;
    target.cause = cause;
    if (typeof message === "string") exceptionMessages.set(target, message);
    for (const key of ["name", "description", "number", "fileName", "lineNumber", "columnNumber", "stack"])
      target[key] = error[key];
    return target;
  }
  // The printer installs `toString`.
  Object.setPrototypeOf(ExceptionInfo.prototype, Error.prototype);
  //@unit exInfo requires: ExceptionInfo, invalidArity
  function exInfo(message, data, cause) {
    if (arguments.length !== 2 && arguments.length !== 3) return invalidArity(arguments.length);
    return new ExceptionInfo(message, data, arguments.length === 2 ? null : cause);
  }
  //@unit exData requires: ExceptionInfo
  const exData = value => value instanceof ExceptionInfo ? value.data : null;
  //@unit exMessage
  const exMessage = value => value instanceof Error ? value.message : null;
  //@unit exCause requires: ExceptionInfo
  const exCause = value => value instanceof ExceptionInfo ? value.cause : null;
  //@unit throwableToMap requires: exCause, ExceptionInfo, exData, exMessage, get, keyword,
  //@ mapFromArray, symbol, truth, vectorFromArray
  function throwableToMap(error) {
    const k = name => keyword(null, name), via = [];
    let root = null;
    for (let current = error; truth(current); current = exCause(current)) {
      root = current;
      const pairs = [k("type"), current instanceof ExceptionInfo ? symbol("cljs.core", "ExceptionInfo")
        : current instanceof Error ? symbol("js", current.name) : null];
      const message = exMessage(current), data = exData(current);
      if (truth(message)) pairs.push(k("message"), message);
      if (truth(data)) pairs.push(k("data"), data);
      via.push(mapFromArray(pairs));
    }
    const pairs = [k("via"), vectorFromArray(via), k("trace"), null];
    const message = exMessage(root), data = exData(root);
    const phase = get(exData(error), keyword("clojure.error", "phase"));
    if (truth(message)) pairs.push(k("cause"), message);
    if (truth(data)) pairs.push(k("data"), data);
    if (truth(phase)) pairs.push(k("phase"), phase);
    return mapFromArray(pairs);
  }
  //@unit printCore requires: printItems
  function printCore(...values) { return printItems(values); }
  //@unit setPrintFn requires: coreCells, invalidArity
  function setPrintFn(fn) { if (arguments.length !== 1) return invalidArity(arguments.length); return coreCells.printFn = fn; }
  //@unit setPrintErrFn requires: coreCells, invalidArity
  function setPrintErrFn(fn) { if (arguments.length !== 1) return invalidArity(arguments.length); return coreCells.printErrFn = fn; }
  //@unit enableConsolePrint requires: coreCells, invalidArity
  function enableConsolePrint() {
    if (arguments.length !== 0) return invalidArity(arguments.length);
    coreCells.printNewline = false;
    coreCells.printFn = (...args) => globalThis.console.log(...args);
    coreCells.printErrFn = (...args) => globalThis.console.error(...args);
    return null;
  }
  //@unit stringPrint requires: coreCells, invalidArity
  function stringPrint(text) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    if (coreCells.printFn == null) throw new Error("No *print-fn* fn set for evaluation environment");
    coreCells.printFn.call(null, text); return null;
  }
  //@unit printOption requires: coreCells, printHooks
  function printOption(options, name, root) { return options == null ? coreCells[root] : printHooks.option(options, name); }
  //@unit flush requires: invalidArity
  function flush() { if (arguments.length !== 0) return invalidArity(arguments.length); return null; }
  //@unit newline requires: coreCells, invalidArity, printOption, stringPrint, truth
  function newline(options = null) {
    if (arguments.length > 1) return invalidArity(arguments.length);
    stringPrint("\n");
    return truth(printOption(options, "flush-on-newline", "flushOnNewline")) ? coreCells.flush.call(null) : null;
  }
  //@unit pr requires: prStr, stringPrint
  function pr(...values) { return stringPrint(prStr(...values)); }
  //@unit println requires: coreCells, newline, printItems, truth
  function println(...values) { printItems(values); return truth(coreCells.printNewline) ? newline() : null; }
  //@unit prn requires: coreCells, newline, pr, truth
  function prn(...values) { pr(...values); return truth(coreCells.printNewline) ? newline() : null; }
  //@unit prnStr requires: prStr
  function prnStr(...values) { return prStr(...values) + "\n"; }
  //@unit printStr requires: coreCells, prStr
  function printStr(...values) { const saved = coreCells.printReadably; coreCells.printReadably = false; try { return prStr(...values); } finally { coreCells.printReadably = saved; } }
  //@unit printlnStr requires: printStr
  function printlnStr(...values) { return printStr(...values) + "\n"; }
  //@unit TaggedLiteral effect requires: equalPair, hash, installMethods, keyword, printWriter,
  //@ prStr, str, write
  function TaggedLiteral(tag, form) {
    this.tag = tag; this.form = form;
    installMethods(this, [["IEquiv", 0, null], ["IHash", 0, null], ["ILookup", 0, null], ["IPrintWithWriter", 0, null],
      ["IEquiv/-equiv", 2, (_self, other) => other instanceof TaggedLiteral && equalPair(tag, other.tag) && equalPair(form, other.form)],
      ["IHash/-hash", 1, () => 31 * hash(tag) + hash(form)],
      ["ILookup/-lookup", 2, (_self, key) => equalPair(key, keyword(null, "tag")) ? tag : equalPair(key, keyword(null, "form")) ? form : null],
      ["ILookup/-lookup", 3, (_self, key, notFound) => equalPair(key, keyword(null, "tag")) ? tag : equalPair(key, keyword(null, "form")) ? form : notFound],
      ["IPrintWithWriter/-pr-writer", 3, (_self, writer, options) => { write(writer, "#" + str(tag) + " "); return printWriter(form, writer, options); }]]);
  }
  TaggedLiteral.prototype.toString = function () { return prStr(this); };
  //@unit taggedLiteral requires: symbolOf, TaggedLiteral
  function taggedLiteral(tag, form) { if (symbolOf(tag) === undefined) throw new Error("Assert failed: (symbol? tag)"); return new TaggedLiteral(tag, form); }
  //@unit printItems requires: coreCells, prStr, stringPrint
  function printItems(values) {
    const saved = coreCells.printReadably;
    coreCells.printReadably = false;
    try { return stringPrint(prStr(...values)); }
    finally { coreCells.printReadably = saved; }
  }
  //@unit stringWriter requires: StringBufferWriter
  function stringWriter() {
    const buffer = {text: "", append(text) { this.text += text; return this; }};
    return {buffer, writer: new StringBufferWriter(buffer)};
  }
  //@unit StringBufferWriter requires: stringBufferWriters
  function StringBufferWriter(sb) {
    const writer = Object.create(StringBufferWriter.prototype);
    writer.sb = sb;
    stringBufferWriters.add(writer);
    return writer;
  }
  //@unit prStrStar requires: flushWriter, prWriter, stringWriter
  function prStrStar(value) {
    // The public helper calls the protocol directly with nil options. In
    // particular, it does not use pr-str's generic native/nil printer.
    const {buffer, writer} = stringWriter();
    prWriter(value, writer, null);
    flushWriter(writer);
    return buffer.text;
  }
  //@unit printMetaQ requires: invalidArity, printsMeta
  function printMetaQ(options, value) {
    if (arguments.length !== 2) return invalidArity(arguments.length);
    return printsMeta(options, value);
  }
  //@unit printsMeta requires: meta, metaValue, methodKeys, printOption, protocolHooks, truth
  function printsMeta(options, value) {
    return truth(printOption(options, "meta", "printMeta")) && (protocolHooks.implements?.(methodKeys[34], value) ?? metaValue(value)) && meta(value) != null;
  }
  //@unit prSequentialWriter requires: invalidArity, printSequential
  function prSequentialWriter(writer, printOne, begin, separator, end, options, coll) {
    if (arguments.length !== 7) return invalidArity(arguments.length);
    return printSequential(writer, printOne, begin, separator, end, options, coll);
  }
  //@unit printSequential requires: coreCells, printHooks, printOption, protocolHooks, truth, write
  // The printer's own entries pass on the options they receive (see
  // `printOptions`). Without protocol implementations, which could see the
  // seq of an array, an array is walked by index, other collections through
  // `printHooks.seq`, which the units that can pass it install.
  // A map's keys and values, flat (`printMapEntries`), are walked two by
  // two (`step` 2), and `printOne` takes each key and its value.
  function printSequential(writer, printOne, begin, separator, end, options, coll, step = 1) {
    const saved = coreCells.printLevel;
    coreCells.printLevel = saved == null ? null : saved - 1;
    try {
      if (coreCells.printLevel != null && coreCells.printLevel < 0) return write(writer, "#");
      write(writer, begin);
      const items = Array.isArray(coll) && !protocolHooks.publicMethod;
      let current = items ? coll.length ? 0 : null : printHooks.seq(coll), remaining = printOption(options, "print-length", "printLength"), printed = false;
      while (current != null && (remaining == null || remaining !== 0)) {
        if (printed) write(writer, separator);
        if (step === 2) printOne.call(null, coll[current], writer, options, coll[current + 1]);
        else printOne.call(null, items ? coll[current] : printHooks.first(current), writer, options);
        printed = true;
        current = items ? current + step < coll.length ? current + step : null : printHooks.next(current); if (remaining != null) remaining--;
      }
      if (current != null) {
        if (printed) write(writer, separator);
        const marker = options == null ? null : printHooks.option(options, "more-marker"); write(writer, truth(marker) ? marker : "...");
      }
      return write(writer, end);
    } finally { coreCells.printLevel = saved; }
  }
  //@unit printMapEntries requires: kindHooks, printSequential, printWriter, write
  // Only keys of the kinds that keyword and symbol builders print lift a
  // namespace (`kindHooks.namespaceOf`).
  // `entries` may be a map's keys and values, flat (`pairs`, light-cljs-1036),
  // where no protocol implementation walks an array's seq
  // (`printSequential`).
  function printMapEntries(entries, writer, options, prefix = "", printKey = printWriter, pairs = false) {
    const ns = !prefix && kindHooks.namespaceOf?.(entries, pairs);
    // JavaScript object tags precede the map printer's depth suppression.
    if (prefix) write(writer, prefix);
    const begin = (ns ? "#:" + ns : "") + "{";
    if (pairs) return printSequential(writer, (key, target, opts, value) => {
      printKey(ns ? kindHooks.unqualified(key) : key, target, opts); write(target, " "); return printWriter(value, target, opts);
    }, begin, ", ", "}", options, entries, 2);
    return printSequential(writer, (entry, target, opts) => {
      printKey(ns ? kindHooks.unqualified(entry.key) : entry.key, target, opts); write(target, " "); return printWriter(entry.value, target, opts);
    }, begin, ", ", "}", options, entries);
  }
  //@unit makeKeyword.namespaceOf effect: makeKeyword, SymbolValue, symbol when: printMapEntries
  //@ requires: coreCells, freshKeyword, keywordOf, kindHooks, name, namedOf, symbol, truth
  // The namespace `*print-namespace-maps*` lifts out of keys that all have
  // it, else a falsy value; of a map's entries, or its keys and values,
  // flat (`pairs`).
  kindHooks.namespaceOf = (entries, pairs) => {
    if (!truth(coreCells.printNamespaceMaps)) return false;
    const names = [];
    if (pairs) for (let i = 0; i < entries.length; i += 2) names.push(namedOf(entries[i]));
    else for (const entry of entries) names.push(namedOf(entry.key));
    const ns = names.length && names[0]?.ns$LKEY;
    return ns && names.every(name => name?.ns$LKEY === ns) && ns;
  };
  kindHooks.unqualified = key => keywordOf(key) !== undefined ? freshKeyword(null, name(key)) : symbol(null, name(key));
  //@unit printNumber
  function printNumber(value) {
    return Number.isNaN(value) ? "##NaN" : value === Infinity ? "##Inf" : value === -Infinity ? "##-Inf" : Object.is(value, -0) ? "-0.0" : String(value);
  }
  //@unit printHooks
  // The printer's hooks, which the units that can use them fill in.
  const printHooks = {};
  //@unit printHooks.defaults effect: printHooks when: entryPrototype, mapPrototype,
  //@ sequencePrototype, setPrototype, transientPrototype, vectorPrototype requires: printHooks
  // The printer for code that must not keep it; `printWriterImpl.install`
  // replaces it where the program can print. A collection's `toString`...
  printHooks.collection = value => Object.prototype.toString.call(value);
  //@unit printHooks.messages effect: printHooks when: bestMethod, derive, MultiFn, preferMethod
  //@ requires: printHooks, printNumber
  // ... and the values in messages.
  printHooks.text = value => value == null ? "nil" : typeof value === "number" ? printNumber(value) : String(value);
  //@unit printWriter requires: printHooks, printWriterImpl
  function printWriter(value, writer, options) {
    return options == null ? printWriterImpl(value, writer, options) : printHooks.optionsWriter(value, writer, options);
  }
  //@unit printJsKey requires: printHooks, printWriter, write
  // A key of a JavaScript object prints as a keyword where it can be one. The
  // keyword is built only where options or protocol implementations could
  // see it, or the program keeps the keyword type (`printHooks.jsKeyword`).
  function printJsKey(key, writer, options) {
    if (!/^[A-Za-z_*+?!\-'][\w*+?!\-']*$/.test(key)) return printWriter(key, writer, options);
    return printHooks.jsKeyword ? printWriter(printHooks.jsKeyword(key), writer, options) : write(writer, ":" + key);
  }
  //@unit freshKeyword.jsKey effect: installMethods, KeywordValue, printMetaQ, prSequentialWriter,
  //@ prSeqWriter, prStrWithOpts, prWriter, prWriterFn when: printJsKey requires: freshKeyword,
  //@ printHooks
  printHooks.jsKeyword = key => freshKeyword(null, key);
  //@unit printWriterImpl requires: coreCells, kindHooks, methodKeys, printHost, printNumber,
  //@ printOption, protocolHooks, truth, typeNames, write
  // The printer is polymorphic over the kinds the program builds: the units
  // building a kind of value add its printer to `kindHooks.print`, which
  // falls back to the host values' `printHost`.
  function printWriterImpl(value, writer, options, skipMetadata = false) {
    if (value == null) return write(writer, "nil");
    if (!skipMetadata) kindHooks.printMetadata?.(value, writer, options);
    const writerMethod = protocolHooks.publicMethod?.(value, methodKeys[41], 3);
    if (writerMethod) return writerMethod.call(value, value, writer, options);
    const readable = truth(printOption(options, "readably", "printReadably"));
    if (typeof value === "string") {
      // CLJS escapes its seven named characters and preserves other UTF-16 units.
      const escapes = {'"': '\\"', '\\': '\\\\', '\b': '\\b', '\f': '\\f', '\n': '\\n', '\r': '\\r', '\t': '\\t'};
      return write(writer, readable ? '"' + value.replace(/[\\"\b\f\n\r\t]/g, ch => escapes[ch]) + '"' : value);
    }
    if (typeof value === "number") return write(writer, printNumber(value));
    if (typeof value === "boolean") return write(writer, String(value));
    if (typeof value === "function") return write(writer, value.cljs$lang$type && typeNames.has(value) ? typeNames.get(value) : "#object[" + (value.name || "Function") + (truth(coreCells.printFnBodies) ? ' "' + String(value) + '"' : "") + "]");
    return (kindHooks.print ?? printHost)(value, writer, options);
  }
  //@unit printHost requires: printJsKey, printMapEntries, printSequential, printWriter, write
  // Host values: no kind's printer took them.
  function printHost(value, writer, options) {
    if (value instanceof Error) return write(writer, "#object[" + (value.constructor?.name || "Object") + " " + String(value) + "]");
    if (value instanceof Date) return write(writer, '#inst "' + value.toISOString().replace(/Z$/, "-00:00") + '"');
    if (value instanceof RegExp) return write(writer, '#"' + value.source + '"');
    if (Array.isArray(value)) return printSequential(writer, printWriter, "#js [", " ", "]", options, value);
    if (typeof value === "object" && Object.getPrototypeOf(value) === null) return write(writer, "#object[Object]");
    if (typeof value === "object" && value.constructor === Object) return printMapEntries(Object.keys(value).map(key => ({key, value: value[key]})), writer, options, "#js ", printJsKey);
    if (typeof value === "symbol") return write(writer, "#object[" + String(value) + "]");
    return write(writer, "#object[" + (value.constructor?.cljs$lang$ctorStr?.replaceAll("/", ".") || value.constructor?.name || "Object") + " " + String(value) + "]");
  }
  //@unit printsMeta.print effect: Atom, ChunkedCons, collectionWithMeta, compilerFormBridge, Cons,
  //@ cons, listFromArray, cycleNode, emptyList, EmptyListValue, fnWithMeta, IndexedSeqValue,
  //@ installMethods, iterateNode, LazySeq, ListValue, makeIFn, makeMap, makeReify, makeSet,
  //@ recordMethods, makeVar, makeVector, mapEntry, mapProjection, queueFromArray, rangeNode, reify,
  //@ RepeatValue, reSeq, symbol, SymbolValue, treeMapConstructor, treeMapSeqConstructor,
  //@ treeSetConstructor, view, ChunkedSeqValue when: printWriterImpl requires: kindHooks, meta,
  //@ printsMeta, printWriter, write
  // Only values of these kinds, and of types with protocol implementations,
  // have metadata to print.
  kindHooks.printMetadata = (value, writer, options) => {
    if (printsMeta(options, value)) { write(writer, "^"); printWriter(meta(value), writer, options); write(writer, " "); }
  };
  //@unit printKind requires: printHost
  // The kind printer chain with `print` for the values `test` takes: each kind's
  // printer passes other values on, to `printHost` last.
  const printKind = (previous = printHost, test, print) => (value, writer, options) => (test(value) ? print : previous)(value, writer, options);
  //@unit ExceptionInfo.print effect: ExceptionInfo when: printWriterImpl requires: ExceptionInfo,
  //@ kindHooks, print, printKind, truth, write
  kindHooks.print = printKind(kindHooks.print, value => value instanceof ExceptionInfo, (value, writer) => write(writer, value.toString()));
  ExceptionInfo.prototype.toString = function () {
    return "#error {:message " + print(this.message, true)
      + (truth(this.data) ? ", :data " + print(this.data, true) : "")
      + (truth(this.cause) ? ", :cause " + print(this.cause, true) : "") + "}";
  };
  //@unit makeTransient.print effect: makeTransient when: printWriterImpl requires: isTransient,
  //@ kindHooks, printKind, transientState, write
  kindHooks.print = printKind(kindHooks.print, value => isTransient(value), (value, writer) => {
    const state = transientState(value), kind = value.cljs$lang$kind$$LKEY;
    return write(writer, "#object[cljs.core." + (kind === 31 ? "TransientArrayMap" : kind === 32 ? "TransientHashMap" : state === value ? "TransientVector" : "TransientHashSet") + "]");
  });
  //@unit Atom.print effect: Atom, Volatile when: printWriterImpl requires: atoms, kindHooks,
  //@ printKind, printWriter, volatiles, write
  kindHooks.print = printKind(kindHooks.print, value => atoms.has(value) || volatiles.has(value), (value, writer, options) => {
    write(writer, "#object[cljs.core." + (atoms.has(value) ? "Atom" : "Volatile") + " {:val "); printWriter(value.state, writer, options); return write(writer, "}]");
  });
  //@unit Delay.print effect: Delay when: printWriterImpl requires: delays, kindHooks, printKind,
  //@ printWriter, write
  kindHooks.print = printKind(kindHooks.print, value => delays.has(value), (value, writer, options) => {
    const state = delays.get(value);
    write(writer, "#object[cljs.core.Delay {:status " + (state.thunk == null ? ":ready" : ":pending") + ", :val "); printWriter(state.value, writer, options); return write(writer, "}]");
  });
  //@unit makeKeyword.print effect: makeKeyword, makeVar, SymbolValue, symbol when: printWriterImpl
  //@ requires: kindHooks, namedOf, printKind, vars, write
  kindHooks.print = printKind(kindHooks.print, value => namedOf(value) !== undefined || vars.has(value), (value, writer) => write(writer, String(value)));
  //@unit uuidValue.print effect: uuidValue when: printWriterImpl requires: kindHooks, printKind,
  //@ uuids, write
  kindHooks.print = printKind(kindHooks.print, value => uuids.has(value), (value, writer) => write(writer, '#uuid "' + uuids.get(value) + '"'));
  //@unit makeVector.print effect: makeVector, mapEntry when: printWriterImpl requires: holdsVector,
  //@ kindHooks, printKind, printSequential, printWriter, protocolHooks, vectorStorage
  // A vector prints the items `seq` walks, through `seq` where protocol
  // implementations could see it.
  kindHooks.print = printKind(kindHooks.print, value => holdsVector(value), (value, writer, options) => {
    if (protocolHooks.publicMethod) return printSequential(writer, printWriter, "[", " ", "]", options, value);
    const storage = vectorStorage(value) ?? [];
    return printSequential(writer, printWriter, "[", " ", "]", options, Array.isArray(storage) ? storage : Array.from(storage));
  });
  //@unit cons.print effect: ChunkedCons, compilerFormBridge, Cons, cons, listFromArray, cycleNode,
  //@ emptyList, EmptyListValue, IndexedSeqValue, iterateNode, LazySeq, ListValue, mapProjection,
  //@ queueFromArray, rangeNode, RepeatValue, reSeq, treeMapSeqConstructor, view, ChunkedSeqValue
  //@ when: printWriterImpl requires: isCell, isChunkedCons, isLazy, isView, kindHooks, printKind,
  //@ printSequential, printWriter, queues
  kindHooks.print = printKind(kindHooks.print, value => isCell(value) || isChunkedCons(value) || isView(value) || queues.has(value) || isLazy(value),
    (value, writer, options) => printSequential(writer, printWriter, queues.has(value) ? "#queue [" : "(", " ", queues.has(value) ? "]" : ")", options, value));
  //@unit makeMap.print effect: ArrayMapCell, makeMap, recordMethods, treeMapConstructor when: printWriterImpl
  //@ requires: isArrayMap, isHashMap, isTreeMap, kindHooks, mapPairs, maps, mapTraversal,
  //@ printKind, printMapEntries, printWriter, protocolHooks
  // An array map's and a hash map's keys and values, flat (`mapPairs`),
  // where no protocol implementation walks an array's seq
  // (`printSequential`); else entries.
  kindHooks.print = printKind(kindHooks.print, value => maps.has(value), (value, writer, options) => !protocolHooks.publicMethod && (isArrayMap(value) || isHashMap(value) || isTreeMap(value))
    ? printMapEntries(mapPairs(value), writer, options, "", printWriter, true) : printMapEntries(mapTraversal(value), writer, options));
  //@unit makeSet.print effect: makeSet, treeSetConstructor when: printWriterImpl requires: kindHooks,
  //@ printKind, printSequential, printWriter, sets, setTraversal
  kindHooks.print = printKind(kindHooks.print, value => sets.has(value),
    (value, writer, options) => printSequential(writer, (e, target, opts) => printWriter(e.key, target, opts), "#{", " ", "}", options, setTraversal(value)));
  //@unit printSequential.seq effect: ChunkedCons, compilerFormBridge, Cons, cons, listFromArray,
  //@ cycleNode, emptyList, EmptyListValue, IndexedSeqValue, installMethods, iterateNode, LazySeq,
  //@ ListValue, mapProjection, prSequentialWriter, queueFromArray, rangeNode, RepeatValue, reSeq,
  //@ treeMapSeqConstructor, view, ChunkedSeqValue when: printSequential requires: first, next,
  //@ printHooks, seq
  // What `printSequential` walks other than arrays: the seqs these build, and
  // what the public entry and protocol implementations pass.
  printHooks.seq = seq;
  printHooks.first = first;
  printHooks.next = next;
  //@unit prSeqWriter requires: invalidArity, printSeq
  function prSeqWriter(values, writer, options) {
    if (arguments.length !== 3) return invalidArity(arguments.length);
    return printSeq(values, writer, options);
  }
  //@unit prWriterFn requires: invalidArity, printWriter
  // Upstream's private `pr-writer`, which libraries' `-pr-writer` methods
  // call to print a nested value: `:alt-impl`, metadata and the protocol.
  function prWriterFn(value, writer, options) {
    if (arguments.length !== 3) return invalidArity(arguments.length);
    return printWriter(value, writer, options);
  }
  //@unit printSeq requires: first, next, printWriter, write
  function printSeq(values, writer, options) {
    printWriter(first(values), writer, options);
    for (let s = next(values); s != null; s = next(s)) { write(writer, " "); printWriter(first(s), writer, options); }
    return null;
  }
  //@unit prStrWithOpts requires: invalidArity, printText
  function prStrWithOpts(values, options) {
    if (arguments.length !== 2) return invalidArity(arguments.length);
    return printText(values, options);
  }
  //@unit printText requires: flushWriter, printSeq, seq, stringWriter
  function printText(values, options) {
    if (seq(values) == null) return "";
    const {buffer, writer} = stringWriter(); printSeq(values, writer, options); flushWriter(writer); return buffer.text;
  }
  //@unit prnStrWithOpts requires: invalidArity, prStrWithOpts
  function prnStrWithOpts(values, options) { if (arguments.length !== 2) return invalidArity(arguments.length); return prStrWithOpts(values, options) + "\n"; }
  //@unit print requires: coreCells, flushWriter, printWriterImpl, stringWriter
  function print(value, readable) {
    const saved = coreCells.printReadably; coreCells.printReadably = readable;
    try { const {buffer, writer} = stringWriter(); printWriterImpl(value, writer, null, true); flushWriter(writer); return buffer.text; }
    finally { coreCells.printReadably = saved; }
  }
  //@unit printWriterImpl.install effect: printWriterImpl when: bestMethod, derive, entryPrototype,
  //@ Eduction, mapPrototype, MultiFn, preferMethod, sequencePrototype, setPrototype,
  //@ transientPrototype, vectorPrototype requires: print, printHooks, printSequential, printWriter
  // Only a program that can print has collection `toString`, where it keeps a
  // collection type, and prints values into messages.
  printHooks.collection = value => print(value, true);
  printHooks.text = value => print(value, false);
  printHooks.writer = printWriter;
  printHooks.sequentialWriter = printSequential;
  //@unit prStr requires: printValues
  function prStr(...values) { return printValues(values, null); }
  //@unit printValues requires: flushWriter, printHooks, printWriter, protocolHooks, stringWriter,
  //@ write
  // `printText` of an array of arguments, by index where no protocol
  // implementation could see its seq.
  function printValues(values, options) {
    if (protocolHooks.publicMethod) return printHooks.seqText(values, options);
    if (!values.length) return "";
    const {buffer, writer} = stringWriter();
    values.forEach((value, index) => { if (index) write(writer, " "); printWriter(value, writer, options); });
    flushWriter(writer); return buffer.text;
  }
  //@unit printText.install effect: installMethods when: printValues requires: printHooks, printText
  printHooks.seqText = printText;
  //@unit printOptions effect: printMetaQ, prSequentialWriter, prSeqWriter, prStrWithOpts, prWriter,
  //@ prWriterFn requires: get, keyword, printHooks
  // Options other than nil reach the printer only from these entries.
  printHooks.option = (options, name) => get(options, keyword(null, name));
  //@unit printOptions.writer effect: printMetaQ, prSequentialWriter, prSeqWriter, prStrWithOpts,
  //@ prWriter, prWriterFn when: printWriter requires: assoc, get, keyword, printHooks,
  //@ printWriterImpl, truth
  printHooks.optionsWriter = (value, writer, options) => {
    const alt = get(options, keyword(null, "alt-impl"));
    if (truth(alt)) return alt.call(null, value, writer, assoc(options, keyword(null, "fallback-impl"), printWriterImpl));
    return printWriterImpl(value, writer, options);
  };
  //@unit printForHost requires: prStr
  // The Rust display API returns UTF-8. Keep canonical JS printing lossless,
  // but represent isolated surrogate units as escapes at this text boundary.
  function printForHost(value) {
    return prStr(value).replace(/[\uD800-\uDBFF][\uDC00-\uDFFF]|[\uD800-\uDFFF]/g,
      units => units.length === 2 ? units : "\\u" + units.charCodeAt(0).toString(16).padStart(4, "0"));
  }
  //@unit str requires: kindHooks, maps, recordTypes, sequential, sets
  function str(...values) { return values.map(value => value == null ? "" : typeof value === "string" ? value : sequential(value) || (maps.has(value) && !recordTypes.has(value)) || sets.has(value) ? kindHooks.strCollection(value) : String(value)).join(""); }
  //@unit str.collections effect: makeVector, mapEntry, emptyList, Cons, ListValue, EmptyListValue,
  //@ RepeatValue, cons, listFromArray, compilerFormBridge, mapProjection, cycleNode,
  //@ treeMapSeqConstructor, reSeq, iterateNode, ChunkedCons, rangeNode, view, ChunkedSeqValue,
  //@ IndexedSeqValue, queueFromArray, LazySeq, makeMap, treeMapConstructor, recordMethods, makeSet,
  //@ treeSetConstructor when: str requires: kindHooks, print
  kindHooks.strCollection = value => print(value, true);
  //@unit displayStr
  // `str` of one value for error messages, through `toString` for collections.
  const displayStr = value => value == null ? "" : String(value);

  //@unit variadicCore mutator requires: fnApplyTo
  function variadicCore(fn, fixed, delegate) {
    fn.cljs$lang$maxFixedArity = fixed;
    fn.cljs$lang$applyTo = args => fnApplyTo(delegate, fixed, args);
    fn.cljs$core$IFn$_invoke$arity$variadic = delegate;
    return fn;
  }
  //@unit foldSeq requires: first, next, seq
  function foldSeq(operation, x, y, more) {
    let result = operation(x, y);
    for (let s = seq(more); s !== null; s = next(s)) result = operation(result, first(s));
    return result;
  }
  //@unit add.variadic install: add requires: add, foldSeq, variadicCore
  variadicCore(add, 2, (x, y, more) => foldSeq((a, b) => a + b, x, y, more));
  //@unit subtract.variadic install: subtract requires: foldSeq, subtract, variadicCore
  variadicCore(subtract, 2, (x, y, more) => foldSeq((a, b) => a - b, x, y, more));
  //@unit multiply.variadic install: multiply requires: foldSeq, multiply, variadicCore
  variadicCore(multiply, 2, (x, y, more) => foldSeq((a, b) => a * b, x, y, more));
  //@unit divide.variadic install: divide requires: divide, foldSeq, variadicCore
  variadicCore(divide, 2, (x, y, more) => foldSeq((a, b) => a / b, x, y, more));
  //@unit equal.variadic install: equal requires: equal, equalPair, first, next, seq, variadicCore
  variadicCore(equal, 2, (x, y, more) => {
    if (!equalPair(x, y)) return false;
    for (let s = seq(more); s !== null; s = next(s)) {
      const item = first(s);
      if (!equalPair(y, item)) return false;
      y = item;
    }
    return true;
  });
  //@unit vector.variadic install: vector requires: toArray, variadicCore, vector, vectorFromArray
  variadicCore(vector, 0, args => vectorFromArray(toArray(args)));
  //@unit list.variadic install: list requires: list, listFromArray, toArray, variadicCore
  variadicCore(list, 0, args => listFromArray(toArray(args)));
  //@unit hashMap.variadic install: hashMap requires: hashMap, mapFromArray, toArray, variadicCore
  variadicCore(hashMap, 0, args => mapFromArray(toArray(args), "hash-map"));
  //@unit arrayMap.variadic install: arrayMap requires: arrayMap, arrayMapOfPairs, toArray,
  //@ variadicCore
  variadicCore(arrayMap, 0, args => arrayMapOfPairs(toArray(args)));
  //@unit hashSet.variadic install: hashSet requires: hashSet, setFromArray, toArray, variadicCore
  variadicCore(hashSet, 0, args => setFromArray(toArray(args)));
  //@unit mapSeq.variadic install: mapSeq requires: apply, cons, mapManySteps, mapSeq, variadicCore
  // Applied collection arguments remain a sequence. Materializing the rest
  // here would demand collections after the shortest input has already ended.
  variadicCore(mapSeq, 4, (fn, c1, c2, c3, colls) =>
    mapSeq(args => apply(fn, args), mapManySteps(cons(c1, cons(c2, cons(c3, colls))))));
  //@unit mapv.variadic install: mapv requires: apply, into, mapSeq, mapv, variadicCore, vector
  variadicCore(mapv, 4, (fn, c1, c2, c3, colls) =>
    into(vector(), apply(mapSeq, fn, c1, c2, c3, colls)));
  //@unit str.variadic install: str requires: first, next, seq, str, variadicCore
  variadicCore(str, 1, (x, more) => {
    let result = str(x);
    for (let s = seq(more); s !== null; s = next(s)) result += str(first(s));
    return result;
  });

  //@unit printCore.variadic install: printCore requires: printCore, printItems, toArray,
  //@ variadicCore
  variadicCore(printCore, 0, args => printItems(toArray(args)));
  //@unit minKey.variadic install: minKey requires: keyExtreme, minKey, variadicCore
  variadicCore(minKey, 3, (key, x, y, more) => keyExtreme(true, key, x, y, more));
  //@unit maxKey.variadic install: maxKey requires: keyExtreme, maxKey, variadicCore
  variadicCore(maxKey, 3, (key, x, y, more) => keyExtreme(false, key, x, y, more));
  //@unit isDistinct.variadic install: isDistinct requires: distinctMore, isDistinct, variadicCore
  variadicCore(isDistinct, 2, distinctMore);

  //@unit swap.variadic install: swap requires: cons, swap, swapState, variadicCore
  variadicCore(swap, 4, (value, fn, x, y, more) => swapState(value, fn, cons(x, cons(y, more)), false, true));
  //@unit swapVals.variadic install: swapVals requires: cons, swapState, swapVals, variadicCore
  variadicCore(swapVals, 4, (value, fn, x, y, more) => swapState(value, fn, cons(x, cons(y, more)), true, true));
  //@unit merge.variadic install: merge requires: merge, mergeMaps, missing, variadicCore
  variadicCore(merge, 0, values => mergeMaps(values, missing));
  //@unit mergeWith.variadic install: mergeWith requires: mergeMaps, mergeWith, variadicCore
  variadicCore(mergeWith, 1, (fn, values) => mergeMaps(values, fn));
  //@unit spreadArgs requires: cons, first, next, seq
  function spreadArgs(args) {
    if (args == null) return null;
    const following = next(args), item = first(args);
    return following == null ? seq(item) : cons(item, spreadArgs(following));
  }
  //@unit listStar.variadic install: listStar requires: cons, listStar, spreadArgs, variadicCore
  variadicCore(listStar, 4, (a, b, c, d, more) => cons(a, cons(b, cons(c, cons(d, spreadArgs(more))))));
  //@unit updateIn.variadic install: updateIn requires: apply, updateIn, updatePath, variadicCore
  variadicCore(updateIn, 6, (value, path, fn, x, y, z, more) => updatePath(value, path, old => apply(fn, old, x, y, z, more)));
  //@unit update.variadic install: update requires: apply, assoc, get, update, variadicCore
  variadicCore(update, 6, (value, key, fn, x, y, z, more) => assoc(value, key, apply(fn, get(value, key), x, y, z, more)));

  //@unit jsSet
  // Host access follows JavaScript property conversion and receiver semantics.
  function jsSet(object, key, value) {
    if (object == null) throw new TypeError("Cannot set properties of null or undefined");
    Reflect.set(Object(object), key, value, object);
    return value;
  }
  //@unit jsDelete
  function jsDelete(object, key) {
    if (object == null) throw new TypeError("Cannot delete properties of null or undefined");
    return Reflect.deleteProperty(Object(object), key);
  }
  //@unit aget
  function aget(array, index, ...indices) {
    let value = array[index];
    for (const nextIndex of indices) value = value[nextIndex];
    return value;
  }
  //@unit aset requires: jsSet
  function aset(array, index, value, ...more) {
    if (more.length === 0) return jsSet(array, index, value);
    return aset(array[index], value, ...more);
  }
  //@unit makeArray requires: invalidArity
  function makeArray(type, size, ...dimensions) {
    if (arguments.length === 0) return invalidArity(0);
    if (arguments.length === 1) return new Array(type);
    const result = new Array(size);
    if (dimensions.length) for (let i = 0; i < result.length; i++) result[i] = makeArray(null, ...dimensions);
    return result;
  }
  //@unit aclone
  function aclone(array) {
    const result = new Array(array.length);
    for (let i = 0; i < result.length; i++) result[i] = array[i];
    return result;
  }
  //@unit toArray2d requires: count, first, next, seq, toArray
  function toArray2d(coll) {
    const result = new Array(count(coll));
    let i = 0;
    for (let s = seq(coll); s !== null; s = next(s)) result[i++] = toArray(first(s));
    return result;
  }
  //@unit typedArray requires: first, intoArray, invalidArity, isSeq, makeArray, next, seq
  function typedArray(size, init) {
    if (arguments.length === 1) return typeof size === "number" ? typedArray(size, null) : intoArray(size);
    if (arguments.length !== 2) return invalidArity(arguments.length);
    const result = makeArray(size);
    if (isSeq(init)) {
      // The upstream loop advances after each store, even after the last slot.
      let s = seq(init);
      for (let i = 0; s !== null && i < size; i++, s = next(s)) result[i] = first(s);
    } else {
      for (let i = 0; i < size; i++) result[i] = init;
    }
    return result;
  }
  //@unit intArray requires: typedArray
  function intArray(...args) { return typedArray(...args); }
  //@unit longArray requires: typedArray
  function longArray(...args) { return typedArray(...args); }
  //@unit doubleArray requires: typedArray
  function doubleArray(...args) { return typedArray(...args); }
  //@unit objectArray requires: typedArray
  function objectArray(...args) { return typedArray(...args); }
  //@unit intoArray requires: invalidArity, toArray
  function intoArray(type, coll) {
    if (arguments.length !== 1 && arguments.length !== 2) return invalidArity(arguments.length);
    return toArray(arguments.length === 1 ? type : coll);
  }
  //@unit jsObj
  function jsObj(...pairs) {
    // goog.object/create accepts a single array of interleaved entries.
    if (pairs.length === 1 && Array.isArray(pairs[0])) pairs = pairs[0];
    if (pairs.length % 2) throw new Error("Uneven number of arguments");
    const result = {};
    for (let i = 0; i < pairs.length; i += 2) result[pairs[i]] = pairs[i + 1];
    return result;
  }
  //@unit jsKeys
  function jsKeys(object) { const keys = []; for (const key in object) keys.push(key); return keys; }
  //@unit jsInvoke
  function jsInvoke(object, key, ...args) { return object[key].apply(object, args); }
  //@unit keyToJs requires: cljToJs, encodeJsProtocol, methodKeys, namedOf, protocolHooks, prStr
  function keyToJs(key, primitive = cljToJs) {
    if (protocolHooks.satisfies?.(methodKeys[17], key)) return encodeJsProtocol(key);
    return typeof key === "string" || typeof key === "number" || namedOf(key) !== undefined ? primitive.call(null, key) : prStr(key);
  }
  //@unit cljToJs requires: encodeJsProtocol, first, get, isColl, isMap, keyToJs, keyword,
  //@ keywordOf, listFromArray, mapSeq, methodKeys, name, next, nth, protocolHooks, seq, seqToMap,
  //@ str, symbolOf, toArray
  function cljToJs(value, ...options) {
    const opts = seqToMap(listFromArray(options)), keywordFn = get(opts, keyword(null, "keyword-fn"), name);
    const convert = value => {
      if (value == null) return null;
      if (protocolHooks.satisfies?.(methodKeys[17], value)) return encodeJsProtocol(value);
      if (keywordOf(value) !== undefined) return keywordFn.call(null, value);
      if (symbolOf(value) !== undefined) return str(value);
      if (isMap(value)) {
        const result = {};
        for (let s = seq(value); s !== null; s = next(s)) { const entry = first(s); result[keyToJs(nth(entry, 0), convert)] = convert(nth(entry, 1)); }
        return result;
      }
      if (isColl(value)) return toArray(mapSeq(convert, value));
      return value;
    };
    return convert(value);
  }
  //@unit jsToClj requires: arrayMap, assoc, doall, empty, encodeClojureProtocol, get, into, isColl,
  //@ isMapEntry, isSeq, keyword, keywordCore, listFromArray, mapEntry, mapFromArray, mapSeq,
  //@ methodKeys, nth, protocolHooks, seqToMap, truth, vectorFromArray
  function jsToClj(value, ...options) {
    if (arguments.length === 1) options = [keyword(null, "keywordize-keys"), false];
    const opts = seqToMap(listFromArray(options)), keywordize = truth(get(opts, keyword(null, "keywordize-keys")));
    const convert = value => {
      if (protocolHooks.satisfies?.(methodKeys[16], value)) return encodeClojureProtocol(value, arrayMap(...options));
      if (isSeq(value)) return doall(mapSeq(convert, value));
      if (isMapEntry(value)) return mapEntry(convert(nth(value, 0)), convert(nth(value, 1)));
      if (isColl(value)) return into(empty(value), mapSeq(convert, value));
      if (Array.isArray(value)) return vectorFromArray(value.map(convert));
      if (value != null && value.constructor === Object) {
        let result = mapFromArray([]);
        for (const key of Object.keys(value)) result = assoc(result, keywordize ? keywordCore(key) : key, convert(value[key]));
        return result;
      }
      return value;
    };
    return convert(value);
  }
  //@unit checkedIndex
  function checkedIndex(array, index) {
    if (!(array != null && (Array.isArray(array) || (typeof array === "object" && typeof array.length === "number"))) ||
        typeof index !== "number" || index < 0 || !(index < array.length)) throw new Error("Assert failed: array index out of bounds");
  }
  //@unit checkedAget requires: checkedIndex, coreCells
  function checkedAget(array, index, ...indices) {
    try { if (coreCells.assertions) checkedIndex(array, index); }
    catch (error) { if (coreCells.printErrFn) coreCells.printErrFn.call(null, error); }
    const value = array[index];
    return indices.length ? checkedAget(value, ...indices) : value;
  }
  //@unit checkedAset requires: checkedAget, checkedIndex, coreCells, jsSet
  function checkedAset(array, index, value, ...more) {
    if (more.length) return checkedAset(checkedAget(array, index), value, ...more);
    try { if (coreCells.assertions) checkedIndex(array, index); }
    catch (error) { if (coreCells.printErrFn) coreCells.printErrFn.call(null, error); }
    return jsSet(array, index, value);
  }
  //@unit checkedAgetStrict requires: checkedIndex, coreCells
  function checkedAgetStrict(array, index, ...indices) {
    if (coreCells.assertions) checkedIndex(array, index);
    const value = array[index];
    return indices.length ? checkedAgetStrict(value, ...indices) : value;
  }
  //@unit checkedAsetStrict requires: checkedIndex, coreCells, jsSet
  function checkedAsetStrict(array, index, value, ...more) {
    if (coreCells.assertions) checkedIndex(array, index);
    if (more.length) return checkedAsetStrict(array[index], value, ...more);
    return jsSet(array, index, value);
  }

  //@unit coreMethod mutator requires: dispatchCell, invalidArity, protocolHooks
  // A protocol implementation (`protocolHooks`), else `fallback` where the
  // protocol's own test (`countedValue`) answers it.
  function coreMethod(key, arities, fallback, native) {
    // The key, resolved (`methodKey`) as the dispatcher is made where the
    // protocol machinery (`installMethods`), which alone resolves keys, has
    // loaded, else on the first call that can find an implementation (a
    // chunk can load the machinery after the dispatcher's own).
    let resolved = protocolHooks.methodKey?.(key);
    const dispatch = function (value, ...args) {
      const arity = arities.length === 1 ? arities[0] : arguments.length;
      if (!arities.includes(arity)) return invalidArity(arguments.length);
      if (arities.length === 1) args = args.slice(0, arity - 1);
      const method = protocolHooks.builtin?.(value, resolved || (resolved = protocolHooks.methodKey(key)), arity);
      if (method != null) return method.call(value, value, ...args);
      if (native(value)) return fallback(value, ...args);
      throw new Error("No protocol method " + key + " defined for this value");
    };
    dispatchCell(key).dispatcher = dispatch;
    return dispatch;
  }
  //@unit everyValue
  const everyValue = () => true;
  //@unit noValue
  const noValue = () => false;
  //@unit write effect requires: coreMethod, writerValue
  const write = coreMethod("IWriter/-write", [2], (writer, text) => writer.sb.append(text), writerValue);
  //@unit flushWriter effect requires: coreMethod, writerValue
  const flushWriter = coreMethod("IWriter/-flush", [1], () => null, writerValue);
  //@unit prWriter effect requires: coreMethod, printableValue, printWriterImpl
  const prWriter = coreMethod("IPrintWithWriter/-pr-writer", [3], (value, writer, options) => printWriterImpl(value, writer, options, true), printableValue);
  //@unit clone effect requires: canonicalVectorRoots, cellCopy, cellOf, cloneablePayload,
  //@ constructorHashes, coreMethod, holdsVector, IntegerRangeValue, isCell, isView, mapCellCopy,
  //@ maps, meta, metadataValues, protocolStores, rangeChunks, RangeValue, seqCells, setCellOf,
  //@ setComparators, setKinds, setQueueCopy, sets, subvecSources, transientCells, treeSetMaps,
  //@ vectorCellCopy, vectorCells, viewAt, viewCells
  const clone = coreMethod("ICloneable/-clone", [1], value => {
    if (cellOf(value)?.cell$LKEY === "range") {
      const c = cellOf(value), state = rangeChunks.get(c);
      return c.integer$LKEY ? IntegerRangeValue(meta(value), c.first$LKEY, c.end$LKEY, c.step$LKEY, c.count$LKEY, constructorHashes.get(value))
        : RangeValue(meta(value), c.first$LKEY, c.end$LKEY, c.step$LKEY, state?.chunk, state?.tail, constructorHashes.get(value));
    }
    if (!cloneablePayload(value)) throw new Error("No protocol method ICloneable.-clone defined for this value");
    const set = setCellOf(value) !== undefined;
    const result = holdsVector(value) ? vectorCellCopy(value) : isView(value) ? viewAt(value, value.i$LKEY) : isCell(value) ? cellCopy(value) : maps.has(value) ? mapCellCopy(value)
      : setQueueCopy(value) ?? Object.create(Object.getPrototypeOf(value));
    // The class layout's tests, which hold nothing, are no stores.
    for (const store of [...protocolStores, treeSetMaps, canonicalVectorRoots, constructorHashes, metadataValues, setKinds, setComparators, subvecSources]) if (store.set !== undefined && store !== vectorCells && store !== transientCells && store !== viewCells && store !== seqCells && store !== maps && !(set && (store === sets || store === setKinds)) && store.has(value)) store.set(result, store.get(value));
    return result;
  }, cloneablePayload);
  //@unit indexedNth requires: isTransient, missing, nth, transientNth
  // The `-nth` of the runtime's indexed values.
  function indexedNth(value, index, notFound) { return isTransient(value) ? transientNth(value, index, arguments.length === 2 ? missing : notFound) : nth(...arguments); }
  //@unit seqProtocol effect requires: coreMethod, seq, seqableValue
  const seqProtocol = coreMethod("ISeqable/-seq", [1], seq, seqableValue);
  //@unit firstProtocol effect requires: coreMethod, first, seqValue
  const firstProtocol = coreMethod("ISeq/-first", [1], first, seqValue);
  //@unit restProtocol effect requires: coreMethod, rest, seqValue
  const restProtocol = coreMethod("ISeq/-rest", [1], rest, seqValue);
  //@unit nextProtocol effect requires: coreMethod, next, nextValue
  const nextProtocol = coreMethod("INext/-next", [1], next, nextValue);
  //@unit countProtocol effect requires: coreMethod, count, countedValue
  const countProtocol = coreMethod("ICounted/-count", [1], count, value => value == null || countedValue(value));
  //@unit nthProtocol effect requires: coreMethod, indexedNth, indexedValue
  const nthProtocol = coreMethod("IIndexed/-nth", [2, 3], indexedNth, indexedValue);
  //@unit lookupProtocol effect requires: coreMethod, get, lookupValue
  const lookupProtocol = coreMethod("ILookup/-lookup", [2, 3], get, lookupValue);
  //@unit containsProtocol effect requires: associativeValue, contains, coreMethod
  const containsProtocol = coreMethod("IAssociative/-contains-key?", [2], contains, associativeValue);
  //@unit upstreamMembers
  // The upstream fields and methods of the runtime's types (`first`, `cnt`,
  // `meta`, `sval`, ...), which their `.fields` effects add, are compiler-owned
  // names, which Closure renames and removes upstream: the effects wait for
  // this unit, which an `Advanced` program keeps only where it names one of
  // them (`UPSTREAM_MEMBERS` in src/runtime_slice.rs), or for `resetMeta`,
  // which assigns `meta`.
  const upstreamMembers = null;
  //@unit publicFields mutator requires: constructorHashes, ownedProtocolValue, ownMeta
  // The upstream deftype fields; `read` answers all but `meta` (from `meta`)
  // and the hash, and `write` sets a mutable field other than the hash.
  function publicFields(Type, fields, read, meta = null, write = null) {
    // `mungeMember` of a field name, which holds only letters, `_`, `-` and `?`.
    for (const field of fields) Object.defineProperty(Type.prototype, field.replaceAll("-", "_").replaceAll("?", "_QMARK_"), {
      get() {
        if (field === "meta") return meta(this);
        if (field === "__hash" || field === "_hash") return ownMeta(this) ? this.hash$LKEY : constructorHashes.has(this) ? constructorHashes.get(this) : null;
        return read(this, field);
      },
      set: ["__hash", "_hash", "next", "_next", "current", "seed", "chunk", "chunk-next"].includes(field) ? function (value) {
        if (!ownedProtocolValue(this) || !(this instanceof Type)) throw new Error("Expected public core value");
        if (field === "__hash" || field === "_hash") { if (ownMeta(this)) this.hash$LKEY = value; else constructorHashes.set(this, value); }
        else write(this, field, value);
      } : function (value) {
        // A host field shadow cannot mutate an immutable branded payload.
        Object.defineProperty(this, field.replaceAll("-", "_").replaceAll("?", "_QMARK_"), {value, writable: true, configurable: true, enumerable: true});
      }
    });
  }
  //@unit cellStateField requires: cellOf, iterateUnrealizedSeed
  // The `write` of Repeat, Cycle and Iterate.
  function cellStateField(value, field, newValue) {
    const cell = cellOf(value);
    if (field === "current") cell.current$LKEY = newValue;
    else if (field === "seed") { cell.seed$LKEY = newValue; cell.realized$LKEY = newValue !== iterateUnrealizedSeed; }
    else cell.next$LKEY = newValue;
  }
  //@unit rangeChunkField requires: cellOf, rangeChunks
  // The `write` of Range.
  function rangeChunkField(value, field, newValue) {
    const c = cellOf(value), state = rangeChunks.get(c) ?? {};
    state[field === "chunk" ? "chunk" : "tail"] = newValue;
    rangeChunks.set(c, state);
  }
  //@unit slotField requires: missing, publicCollectionSlots
  // A field a positional constructor supplied, else the count, else `missing`.
  function slotField(value, field, counted) {
    const slots = publicCollectionSlots.get(value);
    if (slots && field in slots) return slots[field];
    return field === "cnt" || field === "count" ? counted(value) : missing;
  }
  //@unit rangeField requires: cellOf, rangeChunks
  function rangeField(value, field) {
    const c = cellOf(value);
    if (["start", "end", "step", "cnt"].includes(field)) return c[field === "start" ? "first$LKEY" : field === "cnt" ? "count$LKEY" : field + "$LKEY"];
    return rangeChunks.has(c) ? rangeChunks.get(c)[field === "chunk" ? "chunk" : "tail"] : null;
  }
  //@unit seqMethods requires: count, equalPair, es6Iterator, nth
  const seqMethods = {
    indexOf(item, start = 0) {
      const length = count(this);
      for (let i = start < 0 ? Math.max(0, length + start) : start; i < length; i++) if (equalPair(nth(this, i), item)) return i;
      return -1;
    },
    lastIndexOf(item, start = count(this)) {
      const length = count(this);
      for (let i = start < 0 ? length + start : Math.min(length - 1, start); i >= 0; i--) if (equalPair(nth(this, i), item)) return i;
      return -1;
    },
    [Symbol.iterator]() { return es6Iterator(this); }
  };
  //@unit seqIndexes mutator requires: seqMethods
  function seqIndexes(Type) {
    Object.defineProperties(Type.prototype, Object.getOwnPropertyDescriptors({
      indexOf: seqMethods.indexOf,
      lastIndexOf: seqMethods.lastIndexOf
    }));
  }
  //@unit SymbolValue.fields effect: SymbolValue when: resetMeta, upstreamMembers requires: symbolOf,
  //@ SymbolValue
  // Upstream's fields read and write the keyed ones (`_hash` the cache,
  // null until computed; `_meta` the metadata, null on a value that is no
  // symbol).
  for (const [field, key] of Object.entries({ns: "ns$LKEY", name: "name$LKEY", str: "str$LKEY", _hash: "hash$LKEY", _meta: "meta$LKEY"})) Object.defineProperty(SymbolValue.prototype, field, {
    get() { const value = symbolOf(this); return value === undefined ? field === "_meta" ? null : undefined : value[key]; },
    set(value) {
      if (symbolOf(this) !== this) throw new Error("Expected public core value");
      this[key] = value;
    }
  });
  //@unit Cons.fields effect: Cons when: resetMeta, upstreamMembers requires: cellOf, Cons,
  //@ constructorHashes, metadataValues
  for (const [field, key] of Object.entries({first: "first$LKEY", rest: "rest$LKEY"})) Object.defineProperty(Cons.prototype, field, {
    get() { return cellOf(this)[key]; },
    set(value) {
      const cell = cellOf(this);
      if (cell === undefined) throw new Error("Expected public core value");
      cell[key] = value;
    }
  });
  Object.defineProperty(Cons.prototype, "__hash", {
    get() { return constructorHashes.has(this) ? constructorHashes.get(this) : null; },
    set(value) {
      if (!(cellOf(this)?.cell$LKEY === "cons")) throw new Error("Expected public core value");
      constructorHashes.set(this, value);
    }
  });
  Object.defineProperty(Cons.prototype, "meta", {
    get() { return metadataValues.has(this) ? metadataValues.get(this) : null; }, set(value) {
      if (!(cellOf(this)?.cell$LKEY === "cons")) throw new Error("Expected public core value");
      metadataValues.set(this, value);
    }
  });
  //@unit Cons.members effect: Cons requires: Cons, seqIndexes, seqMethods
  seqIndexes(Cons);
  Cons.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit Reduced.fields effect: Reduced when: resetMeta, upstreamMembers requires: Reduced,
  //@ reducedValues
  Object.defineProperty(Reduced.prototype, "val", {
    get() { return reducedValues.get(this); }, set(value) {
      if (!reducedValues.has(this)) throw new Error("Expected Reduced");
      reducedValues.reset(this, value);
    }
  });
  //@unit ListValue.fields effect: ListValue when: resetMeta, upstreamMembers requires: cellOf,
  //@ ListValue, meta, publicFields
  publicFields(ListValue, ["meta", "first", "rest", "count", "__hash"], (value, field) => cellOf(value)[{first: "first$LKEY", rest: "rest$LKEY", count: "count$LKEY"}[field]], meta);
  //@unit ListValue.members effect: ListValue requires: ListValue, seqIndexes, seqMethods
  seqIndexes(ListValue);
  ListValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit EmptyListValue.fields effect: EmptyListValue when: resetMeta, upstreamMembers requires: EmptyListValue,
  //@ meta, publicFields
  publicFields(EmptyListValue, ["meta"], null, meta);
  //@unit EmptyListValue.members effect: EmptyListValue requires: EmptyListValue, seqIndexes,
  //@ seqMethods
  seqIndexes(EmptyListValue);
  EmptyListValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit KeywordValue.fields effect: KeywordValue when: resetMeta, upstreamMembers requires: keywordOf,
  //@ KeywordValue
  // Upstream's fields read the keyed ones; `_hash` is null until computed.
  // Writing `ns`, `name` or `fqn` makes a shadow property, as before, and
  // `_hash` sets the cache.
  for (const field of ["ns", "name", "fqn", "_hash"]) Object.defineProperty(KeywordValue.prototype, field, {
    get() { const value = keywordOf(this); return value === undefined ? undefined : value[field === "_hash" ? "hash$LKEY" : field + "$LKEY"]; },
    set: field === "_hash" ? function (value) {
      if (keywordOf(this) !== this) throw new Error("Expected public core value");
      this.hash$LKEY = value;
    } : function (value) {
      Object.defineProperty(this, field, {value, writable: true, configurable: true, enumerable: true});
    }
  });
  //@unit RepeatValue.fields effect: RepeatValue when: resetMeta, upstreamMembers requires: cellOf,
  //@ cellStateField, meta, publicFields, RepeatValue
  publicFields(RepeatValue, ["meta", "count", "val", "next", "__hash"], (value, field) => {
    const c = cellOf(value);
    return field === "count" ? c.remaining$LKEY : field === "val" ? c.first$LKEY : c.next$LKEY ?? null;
  }, meta, cellStateField);
  //@unit RepeatValue.members effect: RepeatValue requires: RepeatValue, seqIndexes
  seqIndexes(RepeatValue);
  //@unit CycleValue.fields effect: CycleValue when: resetMeta, upstreamMembers requires: cellOf,
  //@ cellStateField, cycleCurrent, CycleValue, meta, publicFields
  publicFields(CycleValue, ["meta", "all", "prev", "current", "_next"], (value, field) => {
    const c = cellOf(value);
    return field === "all" ? c.all$LKEY : field === "prev" ? c.prev$LKEY : field === "current" ? c.current$LKEY : c.next$LKEY;
  }, meta, cellStateField);
  CycleValue.prototype.currentval = function () { return cycleCurrent(cellOf(this)); };
  //@unit IterateValue.fields effect: IterateValue when: resetMeta, upstreamMembers requires: cellOf,
  //@ cellStateField, IterateValue, meta, publicFields
  publicFields(IterateValue, ["meta", "f", "prev-seed", "seed", "next"], (value, field) => {
    const c = cellOf(value);
    return field === "f" ? c.f$LKEY : field === "prev-seed" ? c.prev$LKEY : field === "seed" ? c.seed$LKEY : c.next$LKEY;
  }, meta, cellStateField);
  //@unit PersistentVectorValue.fields effect: PersistentVectorValue when: resetMeta,
  //@ upstreamMembers requires: canonicalVectorRoots, hostHooks, meta, missing,
  //@ PersistentVectorValue, protocolHooks, publicFields, slotField, vectorCount, vectorRoots,
  //@ vectorStore, vectorTailOffset, viewTrie
  publicFields(PersistentVectorValue, ["meta", "cnt", "shift", "root", "tail", "__hash"], (value, field) => {
    const slot = slotField(value, field, protocolHooks.extend ? hostHooks.count : vectorCount);
    if (slot !== missing) return slot;
    // A copy of the tail and a token of the root keep the store private;
    // a view (`VectorViewCell`) gives those of the trie of its items.
    const store = viewTrie(vectorStore(value));
    if (field === "tail") return store.tail$LKEY.slice(0, store.cnt$LKEY - vectorTailOffset(store.cnt$LKEY));
    if (field === "shift") return store.shift$LKEY;
    if (field === "root") {
      if (!canonicalVectorRoots.has(value)) { const root = {}; vectorRoots.set(root, store.root$LKEY); canonicalVectorRoots.set(value, root); }
      return canonicalVectorRoots.get(value);
    }
  }, meta);
  //@unit SubvecValue.fields effect: SubvecValue when: resetMeta, upstreamMembers requires: count,
  //@ meta, missing, publicFields, slotField, subvecSources, SubvecValue
  publicFields(SubvecValue, ["meta", "start", "end", "__hash"], (value, field) => {
    const slot = slotField(value, field, count);
    return slot !== missing ? slot : field === "start" ? 0 : count(value);
  }, meta);
  Object.defineProperty(SubvecValue.prototype, "v", {get() { return subvecSources.get(this); }});
  //@unit ChunkedSeqValue.fields effect: ChunkedSeqValue when: resetMeta, upstreamMembers requires: ChunkedSeqValue,
  //@ meta, publicFields, viewOf
  // A chunked seq's fields are its own (`ChunkedSeqCell`): upstream's `i` and
  // `off` are its `base` and `i`.
  publicFields(ChunkedSeqValue, ["vec", "node", "i", "off", "meta", "__hash"], (value, field) => {
    const v = viewOf(value);
    return field === "vec" ? v.vec$LKEY : field === "node" ? v.arr$LKEY : field === "i" ? v.base$LKEY : v.i$LKEY;
  }, meta);
  //@unit ChunkedSeqValue.members effect: ChunkedSeqValue requires: ChunkedSeqValue, seqIndexes,
  //@ seqMethods
  seqIndexes(ChunkedSeqValue);
  ChunkedSeqValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit PersistentQueueValue.fields effect: PersistentQueueValue when: resetMeta, upstreamMembers
  //@ requires: count, meta, missing, PersistentQueueValue, publicFields, queues, seq, slotField,
  //@ vector
  publicFields(PersistentQueueValue, ["meta", "count", "front", "rear", "__hash"], (value, field) => {
    const slot = slotField(value, field, count);
    return slot !== missing ? slot : field === "front" ? seq(queues.get(value)) : vector();
  }, meta);
  //@unit PersistentQueueValue.members effect: PersistentQueueValue requires: PersistentQueueValue,
  //@ seqMethods
  PersistentQueueValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit PersistentArrayMapValue.fields effect: PersistentArrayMapValue when: resetMeta,
  //@ upstreamMembers requires: hostHooks, mapCount, mapPairs, meta, missing,
  //@ PersistentArrayMapValue, protocolHooks, publicFields, slotField
  publicFields(PersistentArrayMapValue, ["meta", "cnt", "arr", "__hash"], (value, field) => {
    const slot = slotField(value, field, protocolHooks.extend ? hostHooks.count : mapCount);
    return slot !== missing ? slot : mapPairs(value).slice();
  }, meta);
  //@unit PersistentHashMapValue.fields effect: PersistentHashMapValue when: resetMeta,
  //@ upstreamMembers requires: contains, get, hostHooks, mapCount, meta, missing,
  //@ PersistentHashMapValue, protocolHooks, publicFields, slotField
  publicFields(PersistentHashMapValue, ["meta", "cnt", "root", "has-nil?", "nil-val", "__hash"], (value, field) => {
    const slot = slotField(value, field, protocolHooks.extend ? hostHooks.count : mapCount);
    return slot !== missing ? slot : field === "root" ? value : field === "has-nil?" ? contains(value, null) : get(value, null);
  }, meta);
  //@unit PersistentHashSetValue.fields effect: PersistentHashSetValue when: resetMeta,
  //@ upstreamMembers requires: arrayMapFromArray, kindHooks, meta, missing, PersistentHashSetValue,
  //@ publicFields, setKinds, setTraversal, slotField
  publicFields(PersistentHashSetValue, ["meta", "hash-map", "__hash"], (value, field) => {
    const slot = slotField(value, field);
    if (slot !== missing) return slot;
    // A map of the set's kind: an array set's is an array map.
    const pairs = setTraversal(value).flatMap(e => [e.key, null]), kind = setKinds.get(value);
    return kind === "array-map" ? arrayMapFromArray(pairs) : kindHooks.hashSetMap(pairs, kind);
  }, meta);
  //@unit putMember.setMap effect: publicHashSet, putMember when: resetMeta, upstreamMembers
  //@ requires: kindHooks, mapFromArray
  // Only promotion and the public constructor build sets of other kinds.
  kindHooks.hashSetMap = mapFromArray;
  //@unit IndexedSeqValue.fields effect: IndexedSeqValue when: resetMeta, upstreamMembers requires: IndexedSeqValue,
  //@ meta, publicFields, viewOf
  publicFields(IndexedSeqValue, ["arr", "i", "meta"], (value, field) => {
    const v = viewOf(value);
    return field === "arr" ? v.arr$LKEY : v.i$LKEY;
  }, meta);
  //@unit IndexedSeqValue.members effect: IndexedSeqValue requires: IndexedSeqValue, seqIndexes,
  //@ seqMethods
  seqIndexes(IndexedSeqValue);
  IndexedSeqValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit RangeValue.fields effect: RangeValue when: resetMeta, upstreamMembers requires: cellOf,
  //@ forceRangeChunk, meta, publicFields, rangeChunkField, rangeField, RangeValue
  publicFields(RangeValue, ["meta", "start", "end", "step", "chunk", "chunk-next", "__hash"], rangeField, meta, rangeChunkField);
  RangeValue.prototype.forceChunk = function () { forceRangeChunk(cellOf(this)); return null; };
  //@unit RangeValue.members effect: RangeValue requires: RangeValue, seqIndexes, seqMethods
  seqIndexes(RangeValue);
  RangeValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit IntegerRangeValue.fields effect: IntegerRangeValue when: resetMeta, upstreamMembers
  //@ requires: IntegerRangeValue, meta, publicFields, rangeField
  publicFields(IntegerRangeValue, ["meta", "start", "end", "step", "cnt", "__hash"], rangeField, meta);
  //@unit IntegerRangeValue.members effect: IntegerRangeValue requires: IntegerRangeValue,
  //@ seqIndexes, seqMethods
  seqIndexes(IntegerRangeValue);
  IntegerRangeValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit treeMapConstructor.fields effect: treeMapConstructor when: resetMeta, upstreamMembers
  //@ requires: comparatorOf, meta, publicFields, treeMapConstructor
  // Its own fields (light-cljs-1035): the tree a public constructor gave it
  // is the one it gives back.
  publicFields(treeMapConstructor, ["tree", "cnt", "meta", "__hash"], (value, field) => field === "tree" ? value.tree$LKEY : value.cnt$LKEY, meta);
  Object.defineProperty(treeMapConstructor.prototype, "comp", {get() { return comparatorOf(this); }});
  //@unit treeSetConstructor.fields effect: treeSetConstructor when: resetMeta, upstreamMembers
  //@ requires: makeMap, meta, publicCollectionSlots, publicFields, setComparators, sets,
  //@ treeMapConstructor, treeSetConstructor, treeSetMaps
  publicFields(treeSetConstructor, ["meta", "tree-map", "__hash"], (value, field) => {
    const slots = publicCollectionSlots.get(value);
    if (slots && field in slots) return slots[field];
    if (!treeSetMaps.has(value)) treeSetMaps.set(value, makeMap(treeMapConstructor, sets.get(value).map(e => ({key: e.key, value: null, get hash() { return e.hash; }})), "sorted-map", setComparators.get(value)));
    return treeSetMaps.get(value);
  }, meta);
  //@unit treeMapSeqConstructor.fields effect: treeMapSeqConstructor when: resetMeta,
  //@ upstreamMembers requires: meta, publicFields, treeMapSeqConstructor
  // Its own fields, which a public constructor gave it as its slots
  // (light-cljs-1035).
  publicFields(treeMapSeqConstructor, ["meta", "stack", "ascending?", "cnt", "__hash"],
    (value, field) => field === "stack" ? value.stack$LKEY : field === "cnt" ? value.cnt$LKEY : value.ascending$LKEY, meta);
  //@unit MapEntryType.fields effect: MapEntryType when: resetMeta, upstreamMembers requires: MapEntryType
  // A map entry's `key` and `val` (upstream's MapEntry fields) and a sorted
  // map's node's `left` and `right` (its BlackNode's and RedNode's,
  // `TreeNodeCell`), undefined for another map entry. A node is never
  // written: a write gives the value an own field of that name, as the
  // other types' immutable fields (`publicFields`), and leaves its entry and
  // its tree as they were.
  for (const [name, read] of [["key", node => node.tail$LKEY?.[0]], ["val", node => node.tail$LKEY?.[1]], ["left", node => node.left$LKEY], ["right", node => node.right$LKEY]])
    Object.defineProperty(MapEntryType.prototype, name, {configurable: true, get() { return read(this); },
      set(value) { Object.defineProperty(this, name, {value, writable: true, configurable: true, enumerable: true}); }});
  //@unit treeMapSeqConstructor.members effect: treeMapSeqConstructor requires: seqMethods,
  //@ treeMapSeqConstructor
  treeMapSeqConstructor.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit Eduction.members effect: Eduction requires: completing, count, Eduction, eductions,
  //@ es6Iterator, installMethods, printHooks, seq, sequence, sequenceIterator, sequentialIndex,
  //@ transduce, transformedIterator
  installMethods(Eduction.prototype, [
    ["ISequential", 0, null], ["IIterable", 0, null], ["ISeqable", 0, null], ["IReduce", 0, null], ["IPrintWithWriter", 0, null],
    ["IIterable/-iterator", 1, value => {
      const {xform, coll} = eductions.get(value);
      return transformedIterator(xform, [sequenceIterator(coll)]);
    }],
    ["ISeqable/-seq", 1, value => {
      const {xform, coll} = eductions.get(value);
      return seq(sequence(xform, coll));
    }],
    ["IReduce/-reduce", 2, (value, rf) => {
      const {xform, coll} = eductions.get(value);
      // Inner completion flushes buffers without completing the outer reducer.
      return transduce(xform, completing(rf), coll);
    }],
    ["IReduce/-reduce", 3, (value, rf, init) => {
      const {xform, coll} = eductions.get(value);
      return transduce(xform, completing(rf), init, coll);
    }],
    ["IPrintWithWriter/-pr-writer", 3, (value, writer, options) => {
      return printHooks.sequentialWriter(writer, printHooks.writer, "(", " ", ")", options, value);
    }]
  ]);
  Object.defineProperties(Eduction.prototype, Object.getOwnPropertyDescriptors({
    indexOf(value, start = 0) { return sequentialIndex(this, value, start, false); },
    lastIndexOf(value, start = count(this)) { return sequentialIndex(this, value, start, true); },
    [Symbol.iterator]() { return es6Iterator(this); }
  }));
  //@unit Eduction.fields effect: Eduction when: resetMeta, upstreamMembers requires: Eduction,
  //@ eductions
  Object.defineProperties(Eduction.prototype, Object.getOwnPropertyDescriptors({
    get xform() { return eductions.get(this).xform; },
    set xform(value) { eductions.get(this).xform = value; },
    get coll() { return eductions.get(this).coll; },
    set coll(value) { eductions.get(this).coll = value; }
  }));
  //@unit Delay.fields effect: Delay when: resetMeta, upstreamMembers requires: Delay, delays
  for (const [field, key] of Object.entries({f: "thunk", value: "value"})) Object.defineProperty(Delay.prototype, field, {
    configurable: true, get() { return delays.get(this)[key]; }, set(value) { delays.get(this)[key] = value; }
  });
  //@unit LazySeq.fields effect: LazySeq when: resetMeta, upstreamMembers requires: isLazy, lazyOf,
  //@ LazySeq, lazyStep, metadataValues
  for (const [field, key] of Object.entries({fn: "fn$LKEY", s: "s$LKEY", __hash: "hash$LKEY"})) Object.defineProperty(LazySeq.prototype, field, {
    configurable: true, get() { return lazyOf(this)[key]; }, set(value) { lazyOf(this)[key] = value; }
  });
  Object.defineProperty(LazySeq.prototype, "meta", {
    // `LazySeq` stores no nil metadata.
    configurable: true, get() { return metadataValues.has(this) || !isLazy(this) ? metadataValues.get(this) : null; },
    set(value) { metadataValues.set(this, value); }
  });
  Object.defineProperties(LazySeq.prototype, Object.getOwnPropertyDescriptors({
    sval() { return lazyStep(lazyOf(this)); }
  }));
  //@unit LazySeq.members effect: LazySeq requires: LazySeq, objectEquiv, seqMethods
  Object.defineProperties(LazySeq.prototype, Object.getOwnPropertyDescriptors({
    equiv(other) { return objectEquiv(this, other); },
    indexOf: seqMethods.indexOf,
    lastIndexOf: seqMethods.lastIndexOf,
    [Symbol.iterator]: seqMethods[Symbol.iterator]
  }));
  //@unit NodeSeqType.members effect: NodeSeqType requires: NodeSeqType, seqMethods
  NodeSeqType.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit PersistentArrayMapSeqType.members effect: PersistentArrayMapSeqType requires: PersistentArrayMapSeqType,
  //@ seqMethods
  PersistentArrayMapSeqType.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit ArrayNodeSeqType.members effect: ArrayNodeSeqType requires: ArrayNodeSeqType, seqMethods
  ArrayNodeSeqType.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit RSeqType.members effect: RSeqType requires: RSeqType, seqMethods
  RSeqType.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit ChunkedCons.members effect: ChunkedCons requires: ChunkedCons, seqMethods
  ChunkedCons.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit ChunkedCons.fields effect: ChunkedCons when: resetMeta, upstreamMembers requires: ChunkedCons,
  //@ chunkedConsOf, meta
  for (const [field, key] of Object.entries({chunk: "chunk$LKEY", more: "more$LKEY", __hash: "hash$LKEY"})) Object.defineProperty(ChunkedCons.prototype, field, {
    configurable: true, get() { return chunkedConsOf(this)[key]; },
    set: field === "__hash" ? function (value) { chunkedConsOf(this).hash$LKEY = value; } : undefined
  });
  Object.defineProperty(ChunkedCons.prototype, "meta", {get() { return meta(this); }});
  //@unit ChunkBuffer.fields effect: ChunkBuffer when: resetMeta, upstreamMembers requires: chunk,
  //@ chunkAppend, ChunkBuffer, chunkBufferOf
  for (const [field, key] of Object.entries({buf: "buf$LKEY", end: "end$LKEY"})) Object.defineProperty(ChunkBuffer.prototype, field, {
    configurable: true, get() { return chunkBufferOf(this)[key]; }, set(value) { chunkBufferOf(this)[key] = value; }
  });
  Object.defineProperties(ChunkBuffer.prototype, Object.getOwnPropertyDescriptors({
    add(value) { return chunkAppend(this, value); },
    chunk() { return chunk(this); }
  }));
  //@unit arrayChunk.fields effect: arrayChunk when: resetMeta, upstreamMembers requires: arrayChunk,
  //@ chunkOf
  for (const [field, key] of Object.entries({arr: "arr$LKEY", off: "off$LKEY", end: "end$LKEY"})) Object.defineProperty(arrayChunk.prototype, field, {
    configurable: true, get() { return chunkOf(this)[key]; }, set(value) { chunkOf(this)[key] = value; }
  });
  //@unit realized effect requires: cellOf, coreMethod, delays, isCell, lazyOf, pendingValue, truth
  const realized = coreMethod("IPending/-realized?", [1], value => isCell(value) ? cellOf(value).realized$LKEY === true : delays.has(value) ? !truth(delays.get(value).thunk) : !truth(lazyOf(value).fn$LKEY), pendingValue);
  //@unit typeStatics
  // The statics of the runtime's types (`EMPTY`, `HASHMAP_THRESHOLD`,
  // `fromArray`, `fromArrays`), compiler-owned names upstream, which Closure
  // removes where nothing reads them: their `.statics` effects wait for this
  // unit, which an `Advanced` program keeps only where it names one of them
  // (`TYPE_STATICS` in src/runtime_slice.rs) or reads an empty literal's
  // `EMPTY` (light-cljs-247). The reader reads the queue's, which waits for it
  // too.
  const typeStatics = null;
  //@unit ListValue.statics effect: ListValue when: typeStatics requires: emptyList, ListValue
  ListValue.EMPTY = emptyList;
  //@unit PersistentHashSetValue.statics effect: PersistentHashSetValue when: typeStatics requires: makeSet,
  //@ PersistentHashSetValue
  PersistentHashSetValue.EMPTY = makeSet(PersistentHashSetValue, [], "array-map");
  //@unit PersistentQueueValue.statics effect: PersistentQueueValue when: reader, typeStatics
  //@ requires: PersistentQueueValue, queueFromArray
  PersistentQueueValue.EMPTY = queueFromArray([]);
  //@unit PersistentVectorValue.statics effect: PersistentVectorValue when: typeStatics requires: makeVector,
  //@ PersistentVectorValue
  PersistentVectorValue.EMPTY = makeVector([]);
  //@unit PersistentArrayMapValue.statics effect: PersistentArrayMapValue when: typeStatics
  //@ requires: makeMap, PersistentArrayMapValue
  PersistentArrayMapValue.EMPTY = makeMap(PersistentArrayMapValue, [], "array-map");
  PersistentArrayMapValue.HASHMAP_THRESHOLD = 8;
  //@unit PersistentHashMapValue.statics effect: PersistentHashMapValue when: typeStatics requires: aclone,
  //@ displayStr, hashTrie, makeMap, PersistentHashMapValue, putEntry
  PersistentHashMapValue.EMPTY = makeMap(PersistentHashMapValue, hashTrie([]), "hash-map");
  PersistentHashMapValue.fromArray = (array, noClone) => {
    const items = noClone ? array : aclone(array), entries = [], length = items.length;
    const edit = {};
    for (let i = 0; i < length; i += 2) putEntry(entries, items[i], items[i + 1], "hash-map", edit);
    return makeMap(PersistentHashMapValue, hashTrie(entries), "hash-map");
  };
  PersistentHashMapValue.fromArrays = (keys, values) => {
    const length = keys.length, entries = [], edit = {};
    for (let i = 0; i < length; i++) {
      if (values.length <= i) throw new Error("No value supplied for key: " + displayStr(keys[i]));
      putEntry(entries, keys[i], values[i], "hash-map", edit);
    }
    return makeMap(PersistentHashMapValue, hashTrie(entries), "hash-map");
  };
  //@unit positionalFactory requires: invalidArity
  function positionalFactory(Type, arity) {
    return function (...args) {
      if (args.length !== arity) return invalidArity(args.length);
      return Reflect.construct(Type, args);
    };
  }
  //@unit coreCells requires: flush
  // Mutable core cells: dynamic Vars, host hooks, and operations the runtime reads
  // through a cell so `set!` reaches it. The `core` hub extends this object.
  const coreCells = {
    replOne: null, replTwo: null, replThree: null, replError: null, sourceFile: null,
    uncheckedArrays: false, commandLineArgs: null, printFn: null, printErrFn: null, printNewline: true,
    printReadably: true, printLength: null, printLevel: null, printNamespaceMaps: false,
    printMeta: false, printDup: false, flushOnNewline: true, printFnBodies: false,
    readerCurrentInputValue: null, currentNs: null, assertions: true,
    hostClock: null, hostRandom: null, execTapFn: null, rand: null, flush};
  //@unit coreCells.hostClock effect: coreCells when: systemTime requires: coreCells,
  //@ defaultHostClock
  // The initial value of a cell that only its readers need: these units, and
  // a program reading the cell, whose slice keeps the unit (`RuntimeRoots`).
  coreCells.hostClock = defaultHostClock;
  //@unit coreCells.hostRandom effect: coreCells when: hostRandom requires: coreCells,
  //@ defaultHostRandom
  coreCells.hostRandom = defaultHostRandom;
  //@unit coreCells.execTapFn effect: coreCells when: tapGreater requires: coreCells,
  //@ defaultTapSchedule
  coreCells.execTapFn = defaultTapSchedule;
  //@unit coreCells.rand effect: coreCells when: randomSample requires: coreCells, hostRand
  coreCells.rand = hostRand;
  //@unit removeAllMethods effect requires: coreMethod, multiFnValue, resetMulti
  // Core operations without a function of their own, one unit each.
  const removeAllMethods = coreMethod("IMultiFn/-reset", [1], resetMulti, multiFnValue);
  //@unit addMethodProtocol effect requires: addMethod, coreMethod, multiFnValue
  const addMethodProtocol = coreMethod("IMultiFn/-add-method", [3], addMethod, multiFnValue);
  //@unit removeMethodProtocol effect requires: coreMethod, multiFnValue, removeMethod
  const removeMethodProtocol = coreMethod("IMultiFn/-remove-method", [2], removeMethod, multiFnValue);
  //@unit preferMethodProtocol effect requires: coreMethod, multiFnValue, preferMethod
  const preferMethodProtocol = coreMethod("IMultiFn/-prefer-method", [3], preferMethod, multiFnValue);
  //@unit getMethodProtocol effect requires: coreMethod, getMethod, multiFnValue
  const getMethodProtocol = coreMethod("IMultiFn/-get-method", [2], getMethod, multiFnValue);
  //@unit methods effect requires: coreMethod, demandMulti, deref, multiFnValue
  const methods = coreMethod("IMultiFn/-methods", [1], mf => { demandMulti(mf); return deref(mf.method_table); }, multiFnValue);
  //@unit prefers effect requires: coreMethod, demandMulti, deref, multiFnValue
  const prefers = coreMethod("IMultiFn/-prefers", [1], mf => { demandMulti(mf); return deref(mf.prefer_table); }, multiFnValue);
  //@unit defaultDispatchVal effect requires: coreMethod, multiFnValue
  const defaultDispatchVal = coreMethod("IMultiFn/-default-dispatch-val", [1], mf => mf.default_dispatch_val, multiFnValue);
  //@unit dispatchFn effect requires: coreMethod, multiFnValue
  const dispatchFn = coreMethod("IMultiFn/-dispatch-fn", [1], mf => mf.dispatch_fn, multiFnValue);
  //@unit asTransientProtocol effect requires: asTransient, coreMethod, editableValue
  const asTransientProtocol = coreMethod("IEditableCollection/-as-transient", [1], asTransient, editableValue);
  //@unit persistentBangProtocol effect requires: coreMethod, persistTransient, transientValue
  const persistentBangProtocol = coreMethod("ITransientCollection/-persistent!", [1], persistTransient, transientValue);
  //@unit conjBangProtocol effect requires: conjTransient, coreMethod, transientValue
  const conjBangProtocol = coreMethod("ITransientCollection/-conj!", [2], conjTransient, transientValue);
  //@unit assocBangProtocol effect requires: assocTransient, coreMethod, transientAssociativeValue
  const assocBangProtocol = coreMethod("ITransientAssociative/-assoc!", [3], assocTransient, transientAssociativeValue);
  //@unit assocNBangProtocol effect requires: assocTransient, coreMethod, transientVectorValue
  const assocNBangProtocol = coreMethod("ITransientVector/-assoc-n!", [3], (value, index, item) => assocTransient(value, index, item, true), transientVectorValue);
  //@unit dissocBangProtocol effect requires: coreMethod, removeTransient, transientMapValue
  const dissocBangProtocol = coreMethod("ITransientMap/-dissoc!", [2], (value, key) => removeTransient(value, key, "dissoc!"), transientMapValue);
  //@unit disjoinBangProtocol effect requires: coreMethod, removeTransient, transientSetValue
  const disjoinBangProtocol = coreMethod("ITransientSet/-disjoin!", [2], (value, key) => removeTransient(value, key, "disj!"), transientSetValue);
  //@unit popBangProtocol effect requires: coreMethod, popTransient, transientVectorValue
  const popBangProtocol = coreMethod("ITransientVector/-pop!", [1], popTransient, transientVectorValue);
  //@unit chunkedNextProtocol effect requires: chunkedConsOf, chunkedSeqValue, coreMethod,
  //@ ownedChunkRest, seq
  const chunkedNextProtocol = coreMethod("IChunkedNext/-chunked-next", [1], value => {
    const chunked = chunkedConsOf(value);
    return chunked !== undefined ? chunked.more$LKEY ?? null : seq(ownedChunkRest(value));
  }, chunkedSeqValue);
  //@unit dropProtocol effect requires: coreMethod, dropOwned, dropValue
  const dropProtocol = coreMethod("IDrop/-drop", [2], dropOwned, dropValue);
  //@unit dropFirstProtocol effect requires: chunkValue, coreMethod, dropFirstChunk
  const dropFirstProtocol = coreMethod("IChunk/-drop-first", [1], dropFirstChunk, chunkValue);
  //@unit second requires: first, next
  const second = x => first(next(x));
  //@unit ffirst requires: first
  const ffirst = x => first(first(x));
  //@unit nfirst requires: first, next
  const nfirst = x => next(first(x));
  //@unit fnext requires: first, next
  const fnext = x => first(next(x));
  //@unit nnext requires: next
  const nnext = x => next(next(x));
  //@unit reverse requires: conj, emptyList, reduce
  const reverse = x => reduce((s, value) => conj(s, value), emptyList, x);
  //@unit some requires: first, next, seq, truth
  const some = (pred, coll) => { for (let s = seq(coll); s !== null; s = next(s)) { const value = pred.call(null, first(s)); if (truth(value)) return value; } return null; };
  //@unit notEvery requires: every
  const notEvery = (pred, coll) => !every(pred, coll);
  //@unit notAny requires: some, truth
  const notAny = (pred, coll) => !truth(some(pred, coll));
  //@unit notEmpty requires: seq
  const notEmpty = coll => seq(coll) === null ? null : coll;
  //@unit run requires: reduce
  const run = (proc, coll) => { reduce((_x, value) => proc.call(null, value), null, coll); return null; };
  //@unit isUri
  const isUri = value => typeof globalThis.goog?.Uri === "function" && value instanceof globalThis.goog.Uri;
  //@unit isIterable requires: iterableValue, methodKeys, protocolHooks
  const isIterable = value => protocolHooks.satisfies?.(methodKeys[27], value) ?? iterableValue(value);
  //@unit writeAll requires: write
  const writeAll = (writer, ...texts) => { for (const text of texts) write(writer, text); return null; };
  //@unit keyProtocol effect requires: coreMethod, entryKey, mapEntryValue
  const keyProtocol = coreMethod("IMapEntry/-key", [1], entryKey, mapEntryValue);
  //@unit valProtocol effect requires: coreMethod, entryVal, mapEntryValue
  const valProtocol = coreMethod("IMapEntry/-val", [1], entryVal, mapEntryValue);
  //@unit entryKey requires: isEntryCell, nth
  // `-key` and `-val` of the runtime's map entries, which are all that
  // implement IMapEntry where no protocol implementation exists.
  function entryKey(entry) {
    if (!isEntryCell(entry)) throw new Error("No protocol method IMapEntry/-key defined for this value");
    return nth(entry, 0);
  }
  //@unit entryVal requires: isEntryCell, nth
  function entryVal(entry) {
    if (!isEntryCell(entry)) throw new Error("No protocol method IMapEntry/-val defined for this value");
    return nth(entry, 1);
  }
  //@unit protocolHooks.builtin effect: installMethods when: Loadable.dispatch, addWatchProtocol,
  //@ arrayIndex, arrayMapDissoc, assoc, collectionWithMeta, compareKeys, conj, conjBangOwn,
  //@ conjMeta, conjTransient, coreMethod, count, derefProtocol, disj, dissoc, entryIndex,
  //@ equalPair, firstSeq, hash, iterator, metaSupported, name, namespace, notifyWatches,
  //@ objectEquiv, peek, pop, removeWatchProtocol, restSeq, rseq, stateProtocol, vresetProtocol,
  //@ withMetaSupported requires: builtin, protocolHooks
  protocolHooks.builtin = builtin;
  //@unit protocolHooks.keyMethod effect: installMethods when: arrayIndex, arrayMapDissoc,
  //@ entryIndex requires: keyMethod, protocolHooks
  // `builtin`'s first half, which `entryIndex` reads a keyword key's slot
  // with (light-cljs-989).
  protocolHooks.keyMethod = keyMethod;
  //@unit protocolHooks.external effect: extendType when: arrayIndex, arrayMapDissoc, builtin,
  //@ entryIndex, publicDeclared requires: externalMethod, protocolHooks
  // Only `extend-type` gives implementations that are not the value's own.
  protocolHooks.externalMethod = externalMethod;
  //@unit protocolHooks.extend effect: extendType, extendInstance when: contains, containsProtocol,
  //@ entryPrototype, ownMap, ownSet, resetMeta, treeMapSeqConstructor, upstreamMembers,
  //@ vectorIndexOf, vectorPrototype requires: protocolHooks
  // Only `extend-type` and `specify!` can give the runtime's own values
  // protocol implementations, which their host-facing methods and fields then
  // answer through the generic operations (`hostHooks`).
  protocolHooks.extend = true;
  //@unit protocolHooks.ownMethod effect: installMethods when: SubvecValue, chunkStore, contains,
  //@ count, dropFirstChunk, empty, find, first, get, lazyBranch, meta, next, nth, printSequential,
  //@ printValues, printWriterImpl, reduce, reduceKv, rest, rseq, seq, seqBranch, subvec,
  //@ vectorFirst, vectorRest requires: ownDeclared, ownMethod, protocolHooks
  // Every protocol implementation goes through `installMethods`.
  protocolHooks.publicMethod = ownMethod;
  protocolHooks.declaredMethod = ownDeclared;
  //@unit protocolHooks.publicMethod effect: extendType, extendInstance when: SubvecValue,
  //@ chunkStore, contains, count, dropFirstChunk, empty, find, first, get, lazyBranch, meta, next,
  //@ nth, printSequential, printValues, printWriterImpl, reduce, reduceKv, rest, rseq, seq,
  //@ seqBranch, subvec, vectorFirst, vectorRest requires: protocolHooks, publicDeclared,
  //@ publicMethod
  // ... and only `extend-type` and `specify!` give the runtime's values and
  // host values implementations, which `publicMethod` finds.
  protocolHooks.publicMethod = publicMethod;
  protocolHooks.declaredMethod = publicDeclared;
  //@unit protocolHooks.ownSatisfies effect: installMethods when: accumulatingCount, boundedCount,
  //@ cljToJs, cons.equiv, isAssociative, isCloneable, isColl, isCounted, isEmpty, isFn, isIFind,
  //@ isIFn, isIndexed, isInst, isIterable, isList, isMap, isRecord, isReduceable, isReversible,
  //@ isSeq, isSeqable, isSequential, isSet, isSorted, isVector, jsToClj, keyToJs, makeMap.equiv,
  //@ makeSet.equiv, mapConj, KeySeqCell, nth, reduce, sequenceIterator requires: ownProtocol,
  //@ protocolHooks
  // The generic operations test core protocols by name: without `extend-type`
  // or `specify!` the protocols a value declares, then their native tests ...
  protocolHooks.satisfies = ownProtocol;
  //@unit protocolHooks.ownImplements effect: installMethods when: chunkNext, concatMany, concatTwo,
  //@ cons, count, drop, filter, hash, indexedSequence, into, isChunkedSeq, isMapEntry, isUuid,
  //@ mapSeq, name, namespace, nth, printsMeta requires: ownProtocol, protocolHooks
  protocolHooks.implements = ownProtocol;
  //@unit protocolHooks.satisfies effect: extendType, extendInstance when: accumulatingCount,
  //@ boundedCount, cljToJs, cons.equiv, isAssociative, isCloneable, isColl, isCounted, isEmpty,
  //@ isFn, isIFind, isIFn, isIndexed, isInst, isIterable, isList, isMap, isRecord, isReduceable,
  //@ isReversible, isSeq, isSeqable, isSequential, isSet, isSorted, isVector, jsToClj, keyToJs,
  //@ makeMap.equiv, makeSet.equiv, mapConj, KeySeqCell, nth, reduce, sequenceIterator requires: kindStore,
  //@ protocolHooks, publicMethodInfo, satisfies, storeProtocol
  // ... and with them also what those give the runtime's and host values.
  // Each reads the protocol of a resolved key (`methodKey`) from its `info`.
  // A value of a payload store's kind (`kindStore`) takes one step.
  protocolHooks.satisfies = (key, value) => {
    const info = key.info || (key.info = publicMethodInfo(key.key)), store = info.name ? kindStore(value) : 0;
    return store ? storeProtocol(info, value, store, true) : satisfies(info.protocol, value);
  };
  //@unit protocolHooks.implements effect: extendType, extendInstance when: chunkNext, concatMany,
  //@ concatTwo, cons, count, drop, filter, hash, indexedSequence, into, isChunkedSeq, isMapEntry,
  //@ isUuid, mapSeq, name, namespace, nth, printsMeta requires: implementsProtocol, kindStore,
  //@ protocolHooks, publicMethodInfo, storeProtocol
  protocolHooks.implements = (key, value) => {
    const info = key.info || (key.info = publicMethodInfo(key.key)), store = info.name ? kindStore(value) : 0;
    return store ? storeProtocol(info, value, store, false) : implementsProtocol(info.protocol, value);
  };
  //@unit protocolHooks.countProtocol effect: installMethods when: accumulatingCount requires: countProtocol,
  //@ protocolHooks
  // One install per core dispatcher the generic operations call.
  protocolHooks.countProtocol = countProtocol;
  //@unit protocolHooks.nthProtocol effect: installMethods when: mapConj, nth requires: nthProtocol,
  //@ protocolHooks
  protocolHooks.nthProtocol = nthProtocol;
  //@unit protocolHooks.firstProtocol effect: installMethods when: KeySeqCell requires: firstProtocol,
  //@ protocolHooks
  protocolHooks.firstProtocol = firstProtocol;
  //@unit protocolHooks.nextProtocol effect: installMethods when: KeySeqCell requires: nextProtocol,
  //@ protocolHooks
  protocolHooks.nextProtocol = nextProtocol;
  //@unit protocolHooks.keyProtocol effect: installMethods when: KeySeqCell requires: keyProtocol,
  //@ protocolHooks
  protocolHooks.keyProtocol = keyProtocol;
  //@unit protocolHooks.valProtocol effect: installMethods when: KeySeqCell requires: protocolHooks,
  //@ valProtocol
  protocolHooks.valProtocol = valProtocol;
  //@unit protocolHooks.atom effect: installMethods when: getGlobalHierarchy requires: atom,
  //@ protocolHooks
  // Protocol implementations can see the global hierarchy's `atom` look up
  // its options, and `isa?` build `(supers child)`.
  protocolHooks.atom = atom;
  //@unit protocolHooks.supers effect: installMethods when: isaIn requires: protocolHooks, supers,
  //@ toArray
  protocolHooks.supers = child => toArray(supers(child));
  //@unit protocolHooks.deref effect: installMethods when: reducedValue requires: deref,
  //@ protocolHooks
  // A protocol implementation can give the value of a `reduced`.
  protocolHooks.deref = deref;
  //@unit protocolHooks.withMeta effect: installMethods when: withMetaProtocol requires: fnWithMeta,
  //@ protocolHooks
  // A function with a protocol implementation reaches `-with-meta`.
  protocolHooks.fnWithMeta = fnWithMeta;
  //@unit Loadable.type effect: Loadable when: decorateType, protocolMaskNames, protocolSlot
  //@ requires: defineType, Loadable
  // A deftype's IDeref where a program can look up protocols by mask or slot.
  defineType(Loadable, "light.lazy/Loadable importer getter", [
    ["IDeref", 0, null], ["IDeref/-deref", 1, Loadable.prototype.deref]]);
  //@unit Loadable.dispatch effect: Loadable when: installMethods requires: Loadable, methodKeys,
  //@ protocolHooks
  // Where `extend-type` or `specify!` can give a loadable another IDeref, `@l`
  // and `load` use it, as a deftype's would.
  Loadable.prototype.deref = (value => function () {
    const method = protocolHooks.builtin?.(this, methodKeys[12], 1);
    return method ? method.call(this, this) : value.call(this);
  })(Loadable.prototype.deref);
  //@unit findProtocol effect requires: coreMethod, find, findValue
  const findProtocol = coreMethod("IFind/-find", [2], find, findValue);
  //@unit sortedSeqProtocol effect requires: coreMethod, identity, mapSeq, rseq, seq, sets,
  //@ sortedValue, truth
  const sortedSeqProtocol = coreMethod("ISorted/-sorted-seq", [2], (value, ascending) => { const s = truth(ascending) ? seq(value) : rseq(value); return sets.has(value) ? mapSeq(identity, s) : s; }, sortedValue);
  //@unit sortedSeqFromProtocol effect requires: coreMethod, sortedSeqFrom, sortedValue
  const sortedSeqFromProtocol = coreMethod("ISorted/-sorted-seq-from", [3], sortedSeqFrom, sortedValue);
  //@unit entryKeyProtocol effect requires: coreMethod, keyProtocol, sets, sortedValue
  const entryKeyProtocol = coreMethod("ISorted/-entry-key", [2], (value, entry) => sets.has(value) ? entry : keyProtocol(entry), sortedValue);
  //@unit comparatorProtocol effect requires: comparatorOf, coreMethod, sortedValue
  const comparatorProtocol = coreMethod("ISorted/-comparator", [1], value => comparatorOf(value), sortedValue);
  //@unit chunkedFirstProtocol effect requires: chunkedSeqValue, coreMethod, ownedChunkFirst
  const chunkedFirstProtocol = coreMethod("IChunkedSeq/-chunked-first", [1], ownedChunkFirst, chunkedSeqValue);
  //@unit chunkedRestProtocol effect requires: chunkedSeqValue, coreMethod, ownedChunkRest
  const chunkedRestProtocol = coreMethod("IChunkedSeq/-chunked-rest", [1], ownedChunkRest, chunkedSeqValue);
  //@unit invokeProtocol effect requires: coreMethod, ifnValue
  const invokeProtocol = coreMethod("IFn/-invoke", Array.from({length: 22}, (_, i) => i + 1), (value, ...args) => value.call(null, ...args), ifnValue);
  //@unit evalCore compile-time
  const evalCore = Object.defineProperty(() => { throw new Error("compiler eval service is not installed"); }, "name", {value: "eval"});
  //@unit isCloneable requires: cloneablePayload, methodKeys, protocolHooks
  const isCloneable = value => protocolHooks.satisfies?.(methodKeys[6], value) ?? cloneablePayload(value);
  //@unit isDelay requires: delays
  const isDelay = value => delays.has(value);
  //@unit derefWithTimeoutProtocol effect requires: coreMethod, noValue
  const derefWithTimeoutProtocol = coreMethod("IDerefWithTimeout/-deref-with-timeout", [3], () => null, noValue);
  //@unit reduceProtocol effect requires: coreMethod, reduce, reduceValue
  const reduceProtocol = coreMethod("IReduce/-reduce", [2, 3], function (value, rf, init) { return arguments.length === 2 ? reduce(rf, value) : reduce(rf, init, value); }, reduceValue);
  //@unit kvReduceProtocol effect requires: coreMethod, kvReduceValue, reduceKv
  const kvReduceProtocol = coreMethod("IKVReduce/-kv-reduce", [3], (value, rf, init) => reduceKv(rf, init, value), kvReduceValue);
  //@unit assocProtocol effect requires: assoc, associativeValue, coreMethod
  const assocProtocol = coreMethod("IAssociative/-assoc", [3], assoc, associativeValue);
  //@unit assocNProtocol effect requires: assoc, coreMethod, vectorValue
  const assocNProtocol = coreMethod("IVector/-assoc-n", [3], assoc, vectorValue);
  //@unit dissocProtocol effect requires: coreMethod, dissoc, mapValue
  const dissocProtocol = coreMethod("IMap/-dissoc", [2], dissoc, mapValue);
  //@unit conjProtocol effect requires: collectionValue, conj, coreMethod
  const conjProtocol = coreMethod("ICollection/-conj", [2], conj, collectionValue);
  //@unit emptyProtocol effect requires: coreMethod, empty, emptyableValue
  const emptyProtocol = coreMethod("IEmptyableCollection/-empty", [1], empty, emptyableValue);
  //@unit disjoinProtocol effect requires: coreMethod, disj, setValue
  const disjoinProtocol = coreMethod("ISet/-disjoin", [2], disj, setValue);
  //@unit peekProtocol effect requires: coreMethod, peek, stackValue
  const peekProtocol = coreMethod("IStack/-peek", [1], peek, stackValue);
  //@unit popProtocol effect requires: coreMethod, pop, stackValue
  const popProtocol = coreMethod("IStack/-pop", [1], pop, stackValue);
  //@unit rseqProtocol effect requires: coreMethod, reversibleValue, rseq
  const rseqProtocol = coreMethod("IReversible/-rseq", [1], rseq, reversibleValue);
  //@unit compareProtocol effect requires: comparableValue, compareKeys, coreMethod
  const compareProtocol = coreMethod("IComparable/-compare", [2], compareKeys, comparableValue);
  //@unit iteratorProtocol effect requires: coreMethod, iterableValue, iterator
  const iteratorProtocol = coreMethod("IIterable/-iterator", [1], iterator, iterableValue);
  //@unit imul
  const imul = Math.imul;
  //@unit nameProtocol effect requires: coreMethod, name, namedValue
  const nameProtocol = coreMethod("INamed/-name", [1], value => { if (!namedValue(value)) throw new Error("No protocol method INamed.-name defined for this value"); return name(value); }, namedValue);
  //@unit namespaceProtocol effect requires: coreMethod, namedValue, namespace
  const namespaceProtocol = coreMethod("INamed/-namespace", [1], namespace, namedValue);
  //@unit hashProtocol effect requires: coreMethod, everyValue, hash
  const hashProtocol = coreMethod("IHash/-hash", [1], hash, everyValue);
  //@unit equivProtocol effect requires: coreMethod, equalPair, everyValue
  const equivProtocol = coreMethod("IEquiv/-equiv", [2], equalPair, everyValue);
  //@unit type
  const type = value => value == null ? null : value.constructor;
  //@unit identical
  const identical = (a, b) => a === b;
  //@unit isUuid requires: methodKeys, protocolHooks, uuidImplementation
  const isUuid = value => protocolHooks.implements?.(methodKeys[60], value) ?? uuidImplementation(value);
  //@unit isInst requires: instValue, methodKeys, protocolHooks
  const isInst = value => protocolHooks.satisfies?.(methodKeys[67], value) ?? instValue(value);
  //@unit isTaggedLiteral requires: TaggedLiteral
  const isTaggedLiteral = value => value instanceof TaggedLiteral;
  //@unit encodeJsProtocol effect requires: coreMethod, noValue
  const encodeJsProtocol = coreMethod("IEncodeJS/-clj->js", [1], value => value, noValue);
  //@unit encodeKeyProtocol effect requires: coreMethod, noValue
  const encodeKeyProtocol = coreMethod("IEncodeJS/-key->js", [1], value => value, noValue);
  //@unit encodeClojureProtocol effect requires: coreMethod, noValue
  const encodeClojureProtocol = coreMethod("IEncodeClojure/-js->clj", [2], value => value, noValue);
  //@unit instMs requires: instMsProtocol
  const instMs = value => instMsProtocol(value);
  //@unit instMsProtocol effect requires: coreMethod, instValue
  const instMsProtocol = coreMethod("Inst/inst-ms*", [1], value => value.getTime(), instValue);
  //@unit isVar requires: vars
  const isVar = value => vars.has(value);
  //@unit isInstance
  const isInstance = (type, value) => value instanceof type;
  //@unit reFind requires: reMatch
  const reFind = (re, text) => reMatch(re, text, false);
  //@unit reMatches requires: reMatch
  const reMatches = (re, text) => reMatch(re, text, true);
  //@unit isRegex
  const isRegex = value => value instanceof RegExp;
  //@unit consCore requires: cons
  const consCore = Object.defineProperty((head, tail) => cons(head, tail), "name", {value: "cons"});
  //@unit arraySeq requires: IndexedSeqValue, invalidArity, view
  const arraySeq = function (array, start = 0) { if (arguments.length > 2) return invalidArity(arguments.length); return array != null && start < array.length ? view(IndexedSeqValue, array, start, true) : null; };
  //@unit array
  const array = (...args) => args;
  //@unit alength
  const alength = array => array.length;
  //@unit jsIn
  const jsIn = (key, object) => key in object;
  //@unit uncheckedGet
  const uncheckedGet = (object, key) => object[key];
  //@unit IndexedSeqFactory requires: IndexedSeqValue, positionalFactory
  const IndexedSeqFactory = positionalFactory(IndexedSeqValue, 3);
  //@unit UUIDFactory requires: positionalFactory, UUIDType
  const UUIDFactory = positionalFactory(UUIDType, 2);
  //@unit RangeFactory requires: positionalFactory, RangeValue
  const RangeFactory = positionalFactory(RangeValue, 7);
  //@unit IntegerRangeFactory requires: IntegerRangeValue, positionalFactory
  const IntegerRangeFactory = positionalFactory(IntegerRangeValue, 6);
  //@unit PersistentTreeMapFactory requires: positionalFactory, treeMapConstructor
  const PersistentTreeMapFactory = positionalFactory(treeMapConstructor, 5);
  //@unit PersistentTreeSetFactory requires: positionalFactory, treeSetConstructor
  const PersistentTreeSetFactory = positionalFactory(treeSetConstructor, 3);
  //@unit PersistentTreeMapSeqFactory requires: positionalFactory, treeMapSeqConstructor
  const PersistentTreeMapSeqFactory = positionalFactory(treeMapSeqConstructor, 5);
  //@unit EductionFactory requires: Eduction, positionalFactory
  const EductionFactory = positionalFactory(Eduction, 2);
  //@unit MultiFnFactory requires: MultiFn, positionalFactory
  const MultiFnFactory = positionalFactory(MultiFn, 8);
  //@unit SeqIterFactory requires: positionalFactory, SeqIter
  const SeqIterFactory = positionalFactory(SeqIter, 2);
  //@unit RangedIteratorFactory requires: positionalFactory, RangedIterator
  const RangedIteratorFactory = positionalFactory(RangedIterator, 6);
  //@unit NamespaceFactory requires: Namespace, positionalFactory
  const NamespaceFactory = positionalFactory(Namespace, 2);
  //@unit TaggedLiteralFactory requires: positionalFactory, TaggedLiteral
  const TaggedLiteralFactory = positionalFactory(TaggedLiteral, 2);

  //@unit readerJsLiterals requires: jsObject
  // compilerForms replaces these with builders that mark literals for macros.
  const readerJsLiterals = {array: items => Array.from(items), object: jsObject};
  //@unit compilerForms compile-time effect requires: compilerFormBridge, readerJsLiterals
  const compilerForms = compilerFormBridge();
  readerJsLiterals.array = compilerForms.jsArray; readerJsLiterals.object = compilerForms.jsObject;
  //@unit reader requires: conj, contains, coreCells, first, freshKeyword, gensym, hashMap, hashSet,
  //@ holdsVector, inspect, into, isCell, isChunkedCons, isRecord, isView, keywordOf, listFromArray,
  //@ mapFromArray, maps, merge, meta, next, nsName, PersistentQueueValue, prStr, readerJsLiterals,
  //@ rePattern, seq, setFromArray, sets, str, symbol, symbolOf, toArray, uuid, uuids,
  //@ vectorFromArray, withMeta
  // First-party bounded EDN and explicit synchronous input capability runtime.
  const reader = (rt => {
  const inspect = rt.inspect;
  const streams = new WeakMap(), tags = new Map(), builtinTags = new Map(), readerMaps = new WeakMap();
  const absent = Object.freeze({}), discarded = Object.freeze({});
  const MAX_INPUT = 1048576, MAX_NODES = 100000, MAX_DEPTH = 32;
  let defaultTag = null, activeDepth = 0;
  function arity(n) { throw new Error('Invalid arity: ' + n); }
  function located(state, message, category = 'reader') {
    const error = new Error(message.slice(0,1024) + ' at ' + state.name + ':' + state.line + ':' + state.column);
    error.source = state.name; error.line = state.line; error.column = state.column;
    error.offset = state.offset; error.category = category;
    return error;
  }
  function fail(state, message, category) { throw located(state, message, category); }
  function duplicate(state, kind, key) {
    const message = kind + ' literal contains duplicate key: ' + prStr(key).slice(0,256);
    const error = located(state,message); error.message = message; throw error;
  }
  function stringReader(text, name = '<string-reader>') {
    if (typeof text !== 'string' || typeof name !== 'string') throw new TypeError('Expected input string and source name');
    const stream = Object.freeze({});
    streams.set(stream, {text, position: 0, total: text.length, offset: 0, line: 1, column: 1, cr: false,
      baseLine: 1, name: name.slice(0,256), provider: null, eof: true, busy: false, failed: false, closed: false});
    if (text.length > MAX_INPUT) fail(streams.get(stream), 'Reader input limit exceeded', 'limit');
    return stream;
  }
  function inputProvider(name, provider) {
    if (typeof name !== 'string' || typeof provider !== 'function') throw new TypeError('Expected source name and input provider');
    const stream = stringReader('', name), state = streams.get(stream);
    state.provider = provider; state.eof = false;
    return stream;
  }
  function stateOf(stream) {
    const state = streams.get(stream);
    if (!state) throw new TypeError('Expected input stream');
    if (state.closed) fail(state, 'Input stream is closed', 'host');
    if (state.busy) fail(state, 'input stream is already in use', 'host');
    return state;
  }
  function peek(s) {
    while (s.position === s.text.length && !s.eof) {
      let chunk;
      try { chunk = s.provider(); }
      catch (_) { fail(s, 'Input provider failed', 'host'); }
      if (chunk == null) { s.eof = true; break; }
      if (typeof chunk !== 'string' || chunk.length === 0) fail(s, 'Input provider must return a nonempty string or nil', 'host');
      s.total += chunk.length;
      if (s.total > MAX_INPUT) fail(s, 'Reader input limit exceeded', 'limit');
      // Drop consumed chunks while retaining absolute locations.
      s.text = chunk; s.position = 0; s.baseLine = s.line;
    }
    return s.position === s.text.length ? null : s.text.charAt(s.position);
  }
  function take(s) {
    const ch = peek(s); if (ch == null) return null;
    ++s.position; ++s.offset;
    if (ch === '\r') { ++s.line; s.column = 1; s.cr = true; }
    else if (ch === '\n') { if (!s.cr) ++s.line; s.column = 1; s.cr = false; }
    else if (ch === '\f' && s.cr) { s.column = 1; s.cr = false; }
    else { ++s.column; s.cr = false; }
    return ch;
  }
  const whitespace = ch => ch !== null && /[\s,]/.test(ch);
  const delimiter = ch => ch === null || whitespace(ch) || /[\[\]{}()";^\\]/.test(ch);
  function skip(s) {
    for (;;) {
      while (whitespace(peek(s))) take(s);
      if (peek(s) !== ';') return;
      while (peek(s) !== null && !['\r', '\n'].includes(take(s))) {}
    }
  }
  function token(s, code = false) {
    let result = '';
    while (!delimiter(peek(s)) && !(code && peek(s) !== null && /['@`~]/.test(peek(s)))) result += take(s);
    if (!result || /[@`~]/.test(result)) fail(s, 'Invalid EDN token');
    return result;
  }
  function named(s, text, keyword = false) {
    if (!text || text.startsWith(':') || text.endsWith(':') || /[\s\[\]{}()";^\\]/.test(text)) fail(s, 'Invalid name');
    const slash = text.indexOf('/');
    if (text !== '/' && (slash === 0 || slash === text.length - 1 || text.indexOf('/', slash + 1) !== -1)) fail(s, 'Invalid name');
    return (keyword ? rt.keyword : rt.symbol)(slash > 0 ? text.slice(0, slash) : null, slash > 0 ? text.slice(slash + 1) : text);
  }
  function number(s, text) {
    if (/[NM]$/.test(text)) fail(s, 'Unsupported numeric extension');
    let result;
    const ratio = /^([+-]?\d+)\/(\d+)$/.exec(text);
    if (ratio) { if (Number(ratio[2]) === 0) fail(s, 'Invalid ratio'); result = Number(ratio[1]) / Number(ratio[2]); }
    else if (/^[+-]?0[xX][\da-fA-F]+$/.test(text)) result = (text[0] === '-' ? -1 : 1) * parseInt(text.replace(/^[+-]/, '').slice(2), 16);
    else {
      const radix = /^([+-]?)(\d+)[rR]([\da-zA-Z]+)$/.exec(text);
      if (radix) {
        const base = Number(radix[2]);
        if (base < 2 || base > 36 || [...radix[3].toLowerCase()].some(ch => parseInt(ch, 36) >= base)) fail(s, 'Invalid radix integer');
        result = (radix[1] === '-' ? -1 : 1) * parseInt(radix[3], base);
      } else if (/^[+-]?0\d+$/.test(text)) {
        if (/[89]/.test(text)) fail(s, 'Invalid octal integer');
        result = (text[0] === '-' ? -1 : 1) * parseInt(text.replace(/^[+-]/, ''), 8);
      } else if (/^[+-]?\d+(?:\.\d*)?(?:[eE][+-]?\d+)?$/.test(text)) result = Number(text);
      else fail(s, 'Invalid number');
    }
    if (!Number.isFinite(result)) fail(s, 'Numeric overflow');
    return result;
  }
  function quoted(s, regex = false) {
    let result = ''; take(s);
    for (;;) {
      const ch = take(s);
      if (ch == null) fail(s, 'Unexpected EOF in string');
      if (ch === '"') return result;
      if (ch !== '\\') { result += ch; continue; }
      const escaped = take(s), escapes = {t:'\t', r:'\r', n:'\n', b:'\b', f:'\f', '"':'"', '\\':'\\'};
      if (Object.prototype.hasOwnProperty.call(escapes, escaped)) result += escapes[escaped];
      else if (escaped === 'u') {
        let digits = ''; for (let i = 0; i < 4; ++i) digits += take(s) ?? '';
        if (!/^[\da-fA-F]{4}$/.test(digits)) fail(s, 'Invalid Unicode escape');
        result += String.fromCharCode(parseInt(digits, 16));
      } else if (escaped !== null && /[0-7]/.test(escaped)) {
        let digits = escaped;
        while (digits.length < 3 && peek(s) !== null && /[0-7]/.test(peek(s))) digits += take(s);
        if (parseInt(digits, 8) > 255) fail(s, 'Invalid octal escape');
        result += String.fromCharCode(parseInt(digits, 8));
      } else if (regex && escaped !== null) result += '\\' + escaped;
      else fail(s, 'Unsupported string escape');
    }
  }
  function character(s) {
    take(s); const first = take(s);
    if (first === null) fail(s, 'Unexpected EOF in character');
    let text = first; while (!delimiter(peek(s))) text += take(s);
    const names = {newline:'\n', space:' ', tab:'\t', backspace:'\b', formfeed:'\f', return:'\r'};
    if (text.length === 1) return text;
    if (names[text] !== undefined) return names[text];
    if (/^u[\da-fA-F]{4}$/.test(text)) return String.fromCharCode(parseInt(text.slice(1), 16));
    if (/^o[0-7]{1,3}$/.test(text) && parseInt(text.slice(1), 8) <= 255) return String.fromCharCode(parseInt(text.slice(1), 8));
    fail(s, 'Invalid character');
  }
  function instant(s, value) {
    try { return parseTimestamp(value); } catch (_) { fail(s, 'Invalid instant'); }
  }
  function timestampFields(value) {
    const m = typeof value === 'string' && /^(\d{4})(?:-(\d{2})(?:-(\d{2})(?:T(\d{2})(?::(\d{2})(?::(\d{2})(?:\.(\d+))?)?)?)?)?)?(?:Z|([-+])(\d{2}):(\d{2}))?$/.exec(value);
    if (!m) throw new Error('Unrecognized date/time syntax: ' + str(value).slice(0,1024));
    const [year, month, day, hour, minute, second] = [Number(m[1]),Number(m[2] ?? 1),Number(m[3] ?? 1),Number(m[4] ?? 0),Number(m[5] ?? 0),Number(m[6] ?? 0)];
    const days = [31,year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0) ? 29 : 28,31,30,31,30,31,31,30,31,30,31];
    const check = (low, n, high, msg) => { if (!(low <= n && n <= high)) throw new Error(msg + ' Failed:  ' + low + '<=' + n + '<=' + high); return n; };
    check(1,month,12,'timestamp month field must be in range 1..12');
    check(1,day,days[month-1],'timestamp day field must be in range 1..last day in month');
    check(0,hour,23,'timestamp hour field must be in range 0..23');
    check(0,minute,59,'timestamp minute field must be in range 0..59');
    check(0,second,minute === 59 ? 60 : 59,'timestamp second field must be in range 0..60');
    const milliseconds = Number((m[7] ?? '').slice(0,3).padEnd(3,'0'));
    const offset = (m[8] === '-' ? -1 : 1) * (Number(m[9] ?? 0)*60+Number(m[10] ?? 0));
    return [year,month,day,hour,minute,second,milliseconds,offset];
  }
  function parseTimestamp(value) {
    const [year,month,day,hour,minute,second,milliseconds,offset] = timestampFields(value);
    return new Date(Date.UTC(year,month-1,day,hour,minute,second,milliseconds)-offset*60000);
  }
  function jsLiteral(s, value) {
        const data = inspect(value);
        if (data?.kind === 'vector') return rt.jsLiterals.array(data.items);
        if (data?.kind === 'map') {
          const pairs = [];
          for (const [key, value] of data.entries) {
            const name = inspect(key);
            if (typeof key !== 'string' && !(name?.kind === 'keyword' && name.namespace === null)) fail(s, 'Invalid JS object literal key');
            pairs.push(typeof key === 'string' ? key : name.name, value);
          }
          return rt.jsLiterals.object(pairs);
        }
        fail(s, 'Invalid JS literal');
  }
  function options(value, profile, string = false) {
    const result = {readers: new Map(profile === 'edn' || (profile === 'reader' && value !== undefined) ? [] : tags), fallback: profile === 'edn' || (profile === 'reader' && value !== undefined) ? null : defaultTag,
      eof: string && profile !== 'code' && value === undefined ? null : absent, profile};
    if (value === undefined) return result;
    const map = inspect(value);
    if (map?.kind !== 'map' || isRecord(value)) throw new TypeError('Expected reader options map');
    for (const [key, value] of map.entries) {
      const k = inspect(key);
      if (k?.kind !== 'keyword' || k.namespace !== null) throw new TypeError('Invalid reader option');
      if (k.name === 'eof') result.eof = value;
      else if (profile !== 'code' && k.name === 'default') {
        if (value != null && typeof value.call !== 'function') throw new TypeError('Expected default reader function');
        result.fallback = value;
      } else if (profile !== 'code' && k.name === 'readers') {
        if (value == null) continue;
        let entries = typeof value === 'object' && value !== null ? readerMaps.get(value) : undefined;
        if (entries === undefined) {
          const readers = inspect(value);
          if (readers?.kind !== 'map' || isRecord(value)) throw new TypeError('Expected readers map');
          entries = readers.entries.map(([tag, handler]) => {
            const n = inspect(tag);
            if (n?.kind !== 'symbol') throw new TypeError('Expected symbol reader key');
            if (handler != null && typeof handler.call !== 'function') throw new TypeError('Expected reader function');
            return [n.spelling, handler];
          });
          // Only immutable privately branded maps have stable validated entries.
          if (rt.ednKind(value) === 'map') readerMaps.set(value,entries);
        }
        for (const [tag, handler] of entries) result.readers.set(tag,handler);
      } else throw new TypeError('Unsupported reader option: ' + k.name);
    }
    return result;
  }
  function validate(s, root, depth, allowBuiltins) {
    const active = new WeakSet(); let nodes = 0;
    function visit(value, d) {
      if (++nodes > MAX_NODES || d + activeDepth > MAX_DEPTH) fail(s, 'EDN result limit exceeded', 'limit');
      if (value === null || ['boolean', 'number', 'string'].includes(typeof value)) return;
      if (typeof value !== 'object') fail(s, 'Unsupported EDN reader result');
      if (active.has(value)) fail(s, 'Cyclic EDN reader result', 'limit');
      active.add(value);
      try {
        const kind = rt.ednKind(value);
        if (allowBuiltins && ['instant', 'uuid'].includes(kind)) return;
        if (kind === 'seq') {
          const seen = new WeakSet(); let cursor = seq(value);
          while (cursor !== null) {
            if (++nodes > MAX_NODES || seen.has(cursor)) fail(s, 'EDN sequence limit exceeded', 'limit');
            seen.add(cursor); if (rt.ednKind(cursor) !== 'seq') fail(s, 'Unsupported EDN sequence tail');
            visit(first(cursor), d + 1); cursor = next(cursor);
          }
        } else {
          if (!['symbol', 'keyword', 'vector', 'map', 'set'].includes(kind)) fail(s, 'Unsupported EDN reader result');
          const data = inspect(value);
          if (!data || !['symbol', 'keyword', 'vector', 'map', 'set'].includes(data.kind)) fail(s, 'Unsupported EDN reader result');
          if (data.items) for (const item of data.items) visit(item, d + 1);
          if (data.entries) for (const pair of data.entries) { visit(pair[0], d + 1); visit(pair[1], d + 1); }
        }
        const metadata = meta(value); if (metadata != null) visit(metadata, d + 1);
      } finally { active.delete(value); }
    }
    visit(root, depth);
  }
  function parse(s, opts) {
    let nodes = 0, anonymousArgs = null, suppressTags = 0;
    const splices = new WeakMap();
    // Capture this invocation's active view. Nested/tag reads can bind *ns*
    // independently without changing qualification in the enclosing read.
    const current = opts.profile === 'code' ? coreCells.currentNs : null;
    const currentName = current === null ? null : str(nsName(current));
    const context = current?.readerContext;
    function alias(name) {
      const resolved = context?.aliases.get(name);
      if (resolved === undefined) fail(s, 'Missing namespace alias: ' + name);
      return resolved;
    }

    function required(depth) {
      let value; do { value = form(depth); } while (value === discarded);
      if (value === absent) fail(s, 'Unexpected EOF');
      return value;
    }
    function collection(depth, end, kind, namespace) {
      const values = [];
      for (;;) {
        skip(s); const ch = peek(s);
        if (ch == null) fail(s, 'Unexpected EOF in collection');
        if (ch === end) { take(s); break; }
        const value = form(depth + 1);
        if (splices.has(value)) values.push(...splices.get(value));
        else if (value !== discarded) values.push(value);
      }
      if (kind === 'map') {
        if (values.length % 2) fail(s, 'Map requires an even number of forms');
        let seen = hashSet();
        for (let i = 0; i < values.length; i += 2) {
          if (namespace !== undefined) {
            const name = inspect(values[i]);
            if (name && ['keyword','symbol'].includes(name.kind) && (name.namespace === null || name.namespace === '_'))
              values[i] = (name.kind === 'keyword' ? rt.keyword : rt.symbol)(name.namespace === '_' ? null : namespace, name.name);
          }
          if (contains(seen, values[i])) duplicate(s, 'Map', values[i]);
          seen = conj(seen, values[i]);
        }
      }
      if (kind === 'set') {
        let seen = hashSet(); for (const value of values) { if (contains(seen, value)) duplicate(s, 'Set', value); seen = conj(seen, value); }
      }
      return ({vector:rt.vectorFromArray, list:rt.listFromArray, map:rt.mapFromArray, set:rt.setFromArray})[kind](values);
    }
    function metadata(depth) {
      take(s); let metadata = required(depth + 1); const kind = inspect(metadata);
      if (kind?.kind === 'keyword') metadata = hashMap(metadata, true);
      else if (typeof metadata === 'string' || kind?.kind === 'symbol') metadata = hashMap(rt.keyword(null, 'tag'), metadata);
      else if (kind?.kind !== 'map') fail(s, 'Invalid metadata');
      const value = required(depth + 1);
      try { return withMeta(value, merge(meta(value), metadata)); }
      catch (_) { fail(s, 'Invalid metadata target'); }
    }
    function dispatch(depth) {
      take(s); const ch = peek(s);
      if (ch === '{') { take(s); return collection(depth, '}', 'set'); }
      if (ch === '_') { take(s); required(depth + 1); return discarded; }
      if (ch === '^') return metadata(depth);
      if (ch === '#') {
        take(s); const name = token(s);
        if (name === 'Inf') return Infinity; if (name === '-Inf') return -Infinity; if (name === 'NaN') return NaN;
        fail(s, 'Invalid symbolic value');
      }
      if (ch === ':') {
        take(s); const auto = peek(s) === ':';
        if (auto) { if (opts.profile !== 'code') fail(s, 'Auto-resolved namespaced maps are unsupported'); take(s); }
        const spelling = peek(s) === '{' || whitespace(peek(s)) ? '' : token(s, opts.profile === 'code');
        if (spelling.includes('/') || spelling.includes(':') || (!auto && !spelling)) fail(s, 'Invalid map namespace');
        const namespace = auto ? (spelling ? alias(spelling) : currentName) : spelling;
        skip(s); if (take(s) !== '{') fail(s, 'Expected namespaced map');
        return collection(depth, '}', 'map', namespace);
      }
      if (opts.profile === 'code' && ch === '?') {
        take(s); const splice = peek(s) === '@'; if (splice) take(s);
        skip(s); if (take(s) !== '(') fail(s, 'Expected reader conditional list');
        let selected = discarded;
        for (;;) {
          skip(s); if (peek(s) === ')') { take(s); break; }
          if (peek(s) === null) fail(s, 'Unexpected EOF in reader conditional');
          const feature = inspect(required(depth + 1));
          if (feature?.kind !== 'keyword') fail(s, 'Expected reader conditional feature keyword');
          const choose = selected === discarded && ['cljs','default'].includes(feature.name);
          if (!choose) ++suppressTags;
          let branch; try { branch = required(depth + 1); } finally { if (!choose) --suppressTags; }
          if (choose) selected = branch;
        }
        if (splice && selected !== discarded) {
          const kind = rt.ednKind(selected);
          if (!['vector','seq'].includes(kind)) fail(s, 'Reader conditional splice requires sequential data');
          const marker = Object.freeze({}); splices.set(marker, toArray(selected)); return marker;
        }
        return selected;
      }
      if (opts.profile === 'code' && ch === '(') {
        if (anonymousArgs !== null) fail(s, 'Nested anonymous function shorthand');
        anonymousArgs = {args: new Map(), max: 0}; take(s);
        let body, args;
        try { body = collection(depth, ')', 'list'); args = anonymousArgs; } finally { anonymousArgs = null; }
        const parameters = [];
        for (let n = 1; n <= args.max; ++n) parameters.push(args.args.get(n) ?? gensym('p' + n + '__'));
        if (args.args.has(-1)) parameters.push(rt.symbol(null, '&'), args.args.get(-1));
        return rt.listFromArray([rt.symbol(null, 'fn*'), rt.vectorFromArray(parameters), body]);
      }
      if (opts.profile === 'code' && ch === '"') return rePattern(quoted(s, true));
      if (opts.profile === 'code' && ch === "'") { take(s); return rt.listFromArray([rt.symbol(null, 'var'), required(depth + 1)]); }
      if (ch == null || whitespace(ch) || /[=!?('"+\-]/.test(ch)) fail(s, 'Unsupported dispatch syntax');
      const tag = token(s, opts.profile === 'code'); if (tag.startsWith(':')) fail(s, 'Invalid data tag');
      const symbol = named(s, tag), value = required(depth + 1);
      if (suppressTags) return value;
      let handler = opts.readers.get(tag), result;
      if (handler != null) {
        if (handler === builtinTags.get(tag)) {
          if (tag === 'inst') result = instant(s, value);
          else if (tag === 'uuid') { if (typeof value !== 'string' || !/^[\da-fA-F]{8}-(?:[\da-fA-F]{4}-){3}[\da-fA-F]{12}$/.test(value)) fail(s,'Invalid UUID'); result = uuid(value); }
          else if (tag === 'js') return jsLiteral(s,value);
          else if (tag === 'queue') { if (inspect(value)?.kind !== 'vector') fail(s,'Queue literal requires a vector'); return into(PersistentQueueValue.EMPTY,value); }
        } else result = handler.call(null, value);
      }
      else if (opts.profile === 'code' && tag === 'inst') {
        result = instant(s,value);
      } else if (opts.profile === 'code' && tag === 'uuid') {
        if (typeof value !== 'string' || !/^[\da-fA-F]{8}-(?:[\da-fA-F]{4}-){3}[\da-fA-F]{12}$/.test(value)) fail(s, 'Invalid UUID');
        result = uuid(value);
      } else if (opts.fallback != null) result = opts.fallback.call(null, symbol, value);
      else fail(s, 'No reader function for tag ' + tag);
      validate(s, result, depth + 1, opts.profile !== 'edn');
      return result;
    }
    function syntaxQuote(value, gensyms = new Map(), depth = 0) {
      if (depth + activeDepth > MAX_DEPTH) fail(s, 'Syntax quote depth limit exceeded', 'limit');
      const data = inspect(value);
      const sym = name => rt.symbol('cljs.core', name);
      if (data?.kind === 'symbol') {
        let resolved = value;
        // Special forms stay unqualified even where the namespace binds them.
        const special = data.namespace === null && '& . case* catch def deftype* do finally fn* if let* letfn* loop* monitor-enter monitor-exit new quote recur reify* set! throw try var'.split(' ').includes(data.name);
        if (data.namespace === null && !special) {
          if (data.name.endsWith('#')) { if (!gensyms.has(data.name)) gensyms.set(data.name, gensym(data.name.slice(0,-1) + '__')); resolved = gensyms.get(data.name); }
          else resolved = rt.symbol(opts.coreNames.has(data.name) ? 'cljs.core' : currentName, data.name);
        }
        if (data.namespace !== null && context?.aliases.has(data.namespace)) resolved = rt.symbol(context.aliases.get(data.namespace), data.name);
        if (data.namespace === null && !special && !data.name.endsWith('#')) {
          const bound = context?.bindings.has(data.name) || context?.declared?.get(data.name) < context?.visible;
          if (bound || context?.excludes.has(data.name)) resolved = rt.symbol(currentName, data.name);
          const referred = context?.refers.get(data.name);
          if (referred && !bound) resolved = rt.symbol(...referred);
        }
        return rt.listFromArray([rt.symbol(null,'quote'), resolved]);
      }
      if (!data || !['list','seq','vector','map','set'].includes(data.kind)) return value;
      const items = data.entries ? data.entries.flat() : data.items;
      if ((data.kind === 'list' || data.kind === 'seq') && items.length && inspect(items[0])?.kind === 'symbol') {
        const head = inspect(items[0]).name;
        if (head === 'unquote') { if (items.length !== 2) fail(s,'Invalid unquote'); return items[1]; }
        if (head === 'unquote-splicing') fail(s,'Unquote splicing requires a collection');
      }
      const pieces = items.map(item => {
        const data = inspect(item);
        if ((data?.kind === 'list' || data?.kind === 'seq') && inspect(data.items[0])?.name === 'unquote-splicing') {
          if (data.items.length !== 2) fail(s,'Invalid unquote splice'); return data.items[1];
        }
        return rt.listFromArray([sym('list'), syntaxQuote(item, gensyms, depth + 1)]);
      });
      let result = data.kind === 'list' && items.length === 0 ? rt.listFromArray([sym('list')]) :
        rt.listFromArray([sym('sequence'),rt.listFromArray([sym('seq'),rt.listFromArray([sym('concat'),...pieces])])]);
      if (data.kind === 'vector') result = rt.listFromArray([sym('vec'),result]);
      if (data.kind === 'map' || data.kind === 'set') result = rt.listFromArray([sym('apply'),sym(data.kind === 'map' ? 'hash-map' : 'hash-set'),result]);
      const metadata = meta(value);
      if (metadata !== null) result = rt.listFromArray([sym('with-meta'),result,syntaxQuote(metadata,gensyms,depth + 1)]);
      return result;
    }
    function form(depth) {
      if (++nodes > MAX_NODES || depth + activeDepth > MAX_DEPTH) fail(s, 'Reader limit exceeded', 'limit');
      skip(s); const ch = peek(s);
      if (ch === null) return absent;
      if (ch === '"') return quoted(s);
      if (ch === '\\') return character(s);
      if (ch === '#') return dispatch(depth);
      if (ch === '^') return metadata(depth);
      if (ch === '[' || ch === '(' || ch === '{') { take(s); return collection(depth, {'[':']','(':')','{':'}'}[ch], {'[':'vector','(':'list','{':'map'}[ch]); }
      if (opts.profile === 'code' && ch === "'") { take(s); return rt.listFromArray([rt.symbol(null,'quote'), required(depth + 1)]); }
      if (opts.profile === 'code' && ch === '@') { take(s); return rt.listFromArray([rt.symbol('cljs.core','deref'), required(depth + 1)]); }
      if (opts.profile === 'code' && ch === '`') { take(s); return syntaxQuote(required(depth + 1)); }
      if (opts.profile === 'code' && ch === '~') { take(s); const splice = peek(s) === '@'; if (splice) take(s); return rt.listFromArray([rt.symbol(null,splice ? 'unquote-splicing' : 'unquote'), required(depth + 1)]); }
      if (/[\])}'@`~]/.test(ch)) fail(s, 'Unsupported reader syntax');
      const text = token(s, opts.profile === 'code');
      if (opts.profile === 'code' && anonymousArgs !== null && text.startsWith('%')) {
        const n = text === '%&' ? -1 : text === '%' ? 1 : /^%(?:0|-[0-9]+|[1-9]\d*)$/.test(text) ? Number(text.slice(1)) : NaN;
        if (!Number.isSafeInteger(n) || n > MAX_NODES) fail(s, 'Invalid anonymous argument');
        if (!anonymousArgs.args.has(n)) anonymousArgs.args.set(n, gensym(n === -1 ? 'rest__' : 'p' + n + '__'));
        anonymousArgs.max = Math.max(anonymousArgs.max,n); return anonymousArgs.args.get(n);
      }
      if (opts.profile === 'code' && text.startsWith('::')) {
        const local = text.slice(2), slash = local.indexOf('/');
        const namespace = slash < 0 ? currentName : alias(local.slice(0, slash));
        return named(s, namespace + '/' + (slash < 0 ? local : local.slice(slash + 1)), true);
      }
      if (text === 'nil') return null; if (text === 'true') return true; if (text === 'false') return false;
      if (/^[+-]?\d/.test(text)) return number(s, text);
      if (text.startsWith(':')) return named(s, text.slice(1), true);
      return named(s, text);
    }
    let value; do { value = form(0); } while (value === discarded);
    if (splices.has(value)) fail(s,'Splicing reader conditional outside collection');
    return value;
  }
  function readWith(profile, args, coreNames = null) {
    let stream, option;
    if (args.length === 0) stream = coreCells.readerCurrentInputValue;
    else if (args.length === 1) {
      if (profile === 'code' && inspect(args[0])?.kind === 'map') {
        option = args[0]; stream = coreCells.readerCurrentInputValue;
      } else stream = args[0];
    }
    else if (args.length === 2) [option, stream] = args;
    else if ((profile === 'reader' || profile === 'edn') && args.length === 4) {
      stream = args[0]; option = args[3];
    } else if (profile === 'code' && (args.length === 3 || args.length === 4)) {
      stream = args[0]; option = hashMap(rt.keyword(null,'eof'), args[1] !== false && args[1] != null ? absent : args[2]);
    } else arity(args.length);
    const opts = options(option, profile), state = stateOf(stream); opts.coreNames = coreNames;
    if (profile === 'reader') {
      if (args.length < 2) opts.eof = null;
      else if (args.length === 4) opts.eof = args[1] !== false && args[1] != null ? absent : args[2];
    } else if (profile === 'edn') {
      if (args.length === 1) opts.eof = null;
      else if (args.length === 4) opts.eof = args[1] !== false && args[1] != null ? absent : args[2];
    }
    if (state.failed) { if (opts.eof !== absent) return opts.eof; fail(state, 'EOF while reading'); }
    state.busy = true; ++activeDepth;
    try { const value = parse(state, opts); if (value !== absent) return value; if (opts.eof !== absent) return opts.eof; fail(state, 'EOF while reading'); }
    catch (error) { state.failed = true; throw error; }
    finally { state.busy = false; --activeDepth; }
  }
  function readStringWith(profile, args, coreNames = null) {
    if (args.length !== 1 && args.length !== 2) arity(args.length);
    const text = args[args.length - 1];
    if (profile !== 'code' && (text == null || text === false)) return null;
    const opts = options(args.length === 2 ? args[0] : undefined, profile, true); opts.coreNames = coreNames;
    const stream = stringReader(text, profile === 'edn' ? '<edn-read-string>' : '<read-string>');
    // readWith receives explicit options so preserve the no-option EDN nil EOF.
    const state = streams.get(stream); ++activeDepth;
    try { const value = parse(state, opts); if (value !== absent) return value; if (opts.eof !== absent) return opts.eof; fail(state,'EOF while reading'); }
    finally { --activeDepth; }
  }
  function readLine(stream = coreCells.readerCurrentInputValue) {
    if (arguments.length > 1) arity(arguments.length);
    const s = stateOf(stream); s.busy = true;
    try {
      let result = '', ch;
      while ((ch = take(s)) !== null) {
        if (ch === '\n') return result;
        if (ch === '\r') { if (peek(s) === '\n') take(s); return result; }
        result += ch;
      }
      return result.length ? result : null;
    } finally { s.busy = false; }
  }
  function tagName(tag) { const name = inspect(tag); if (name?.kind !== 'symbol') throw new TypeError('Expected data reader symbol'); return name.spelling; }
  function register(tag, handler) {
    if (arguments.length !== 2) arity(arguments.length);
    if (handler != null && typeof handler.call !== 'function') throw new TypeError('Expected reader function');
    const name = tagName(tag), previous = tags.get(name) ?? null; tags.set(name, handler); return previous;
  }
  function deregister(tag) { if (arguments.length !== 1) arity(arguments.length); const name = tagName(tag), previous = tags.get(name) ?? null; tags.delete(name); return previous; }
  function registerDefault(handler) { if (arguments.length !== 1) arity(arguments.length); if (handler != null && typeof handler.call !== 'function') throw new TypeError('Expected reader function'); const previous = defaultTag; defaultTag = handler; return previous; }
  function deregisterDefault() { if (arguments.length !== 0) arity(arguments.length); const previous = defaultTag; defaultTag = null; return previous; }
  function withInput(text, thunk) {
    if (arguments.length !== 2) arity(arguments.length);
    const previous = coreCells.readerCurrentInputValue, scoped = stringReader(text, '<with-in-str>');
    coreCells.readerCurrentInputValue = scoped;
    try { return thunk.call(null); } finally {
      const state = streams.get(scoped); state.closed = true; state.provider = null; state.text = '';
      coreCells.readerCurrentInputValue = previous;
    }
  }
  function compilerInputWindow(more = false) {
    const s = stateOf(coreCells.readerCurrentInputValue);
    if ((more || s.position === s.text.length) && !s.eof) {
      const chunk = s.provider();
      if (chunk == null) s.eof = true;
      else {
        if (typeof chunk !== 'string' || !chunk.length) fail(s, 'Invalid input chunk', 'host');
        if (s.text.length + chunk.length > MAX_INPUT) fail(s, 'Reader input limit exceeded', 'limit');
        s.text += chunk;
      }
    }
    return [s.text, s.position, s.eof, s.baseLine];
  }
  function compilerInputConsume(units) {
    const s = stateOf(coreCells.readerCurrentInputValue);
    if (units < 0 || s.position + units > s.text.length) throw new Error('Invalid input cursor');
    for (let i = 0; i < units; ++i) take(s);
  }
  function compilerInputRecover() {
    const s = stateOf(coreCells.readerCurrentInputValue);
    if (s.failed) { compilerInputConsume(s.text.length - s.position); s.failed = false; }
  }
  function compilerInputDiscard() {
    const s = stateOf(coreCells.readerCurrentInputValue);
    compilerInputConsume(s.text.length - s.position); s.failed = false;
  }
  function compilerInputCompact() {
    const s = stateOf(coreCells.readerCurrentInputValue);
    const prefix = s.text.slice(0, s.position);
    const end = Math.max(prefix.lastIndexOf('\n'), prefix.lastIndexOf('\r')) + 1;
    if (end) {
      const lines = (prefix.slice(0, end).match(/\r\n|\r|\n/g) ?? []).length;
      s.baseLine += lines; s.text = s.text.slice(end); s.position -= end;
    }
    s.total = s.text.length;
  }
  function close(stream) { if (arguments.length !== 1) arity(arguments.length); const s = stateOf(stream); s.closed = true; s.provider = null; s.text = ''; return null; }
  const builtinLocation = {name: '<tag-parser>', line: 1, column: 1, offset: 0};
  builtinTags.set('inst', value => instant(builtinLocation,value));
  builtinTags.set('uuid', value => { if (typeof value !== 'string') throw new TypeError('UUID literal requires a string'); return uuid(value); });
  builtinTags.set('queue', value => { if (inspect(value)?.kind !== 'vector') throw new TypeError('Queue literal requires a vector'); return into(PersistentQueueValue.EMPTY,value); });
  builtinTags.set('js', value => jsLiteral(builtinLocation,value));
  for (const [tag, handler] of builtinTags) tags.set(tag,handler);
  return {ednReadString: (...args) => readStringWith('edn', args), ednRead: (...args) => readWith('edn', args),
    readerReadString: (...args) => readStringWith('reader', args), readerRead: (...args) => readWith('reader', args),
    codeProfile: coreNames => ({readString: (...args) => readStringWith('code', args, coreNames),
      read: (...args) => readWith('code', args, coreNames)}),
    readLine: function () { if (arguments.length) arity(arguments.length); return readLine(); },
    compilerInputWindow, compilerInputConsume, compilerInputRecover, compilerInputDiscard, compilerInputCompact,
    readerStringReader: stringReader, readerInputProvider: inputProvider, readerReadLine: readLine,
    readerParseTimestampFields: function (value) { if (arguments.length !== 1) arity(arguments.length); return rt.vectorFromArray(timestampFields(value)); },
    readerParseTimestamp: function (value) { if (arguments.length !== 1) arity(arguments.length); return parseTimestamp(value); },
    readerInitialTagTable: function () { if (arguments.length) arity(arguments.length); return rt.mapFromArray([...builtinTags].flatMap(([tag,f]) => [rt.symbol(null,tag),f])); },
    readerRegisterTagParser: register, readerDeregisterTagParser: deregister,
    readerRegisterDefaultTagParser: registerDefault, readerDeregisterDefaultTagParser: deregisterDefault,
    readerWithInput: withInput, readerCurrentInput: () => coreCells.readerCurrentInputValue, readerClose: close};
  })({keyword: freshKeyword, symbol, vectorFromArray, listFromArray, mapFromArray, setFromArray, inspect, jsLiterals: readerJsLiterals, ednKind: value => symbolOf(value) !== undefined ? 'symbol' : keywordOf(value) !== undefined ? 'keyword' : holdsVector(value) ? 'vector' : maps.has(value) ? 'map' : sets.has(value) ? 'set' : isCell(value) || isChunkedCons(value) || isView(value) ? 'seq' : uuids.has(value) ? 'uuid' : value instanceof Date ? 'instant' : null});

  //@unit codeReader compile-time requires: reader, readerCoreNames
  // Code reading qualifies symbols by `*ns*`: reflection, unlike EDN data.
  const codeReader = reader.codeProfile(readerCoreNames);
  //@unit loadFile requires: invalidArity
  const loadFile = function () { if (arguments.length !== 1) return invalidArity(arguments.length); throw new Error("load-file requires a compiler session and an explicit file provider"); };
  //@unit core requires: abs, aclone, add, addMethodProtocol, addTap, addWatch, addWatchProtocol,
  //@ aget, alength, allNs, alterMeta, ancestors, apply, applyTo, array, arrayChunk, ArrayList,
  //@ arrayMap, ArrayNode, ArrayNodeIterator, ArrayNodeSeqType, arraySeq, ASeq, aset, assoc,
  //@ assocBang, assocBangProtocol, assocIn, assocNBangProtocol, assocNProtocol, assocProtocol,
  //@ asTransientProtocol, Atom, atom, atomFactory, bases, bitAnd, bitAndNot, bitClear, bitCount,
  //@ bitFlip, BitmapIndexedNode, bitNot, bitOr, bitSet, bitShiftLeft, bitShiftRight,
  //@ bitShiftRightZeroFill, bitTest, bitXor, boolean, booleans, boundedCount, Box, butlast, byte,
  //@ bytes, cat, char, chars, checkedAget, checkedAgetStrict, checkedAset, checkedAsetStrict,
  //@ chunk, chunkAppend, ChunkBuffer, chunkBuffer, chunkCons, ChunkedCons, chunkedFirstProtocol,
  //@ chunkedNextProtocol, chunkedRestProtocol, ChunkedSeqValue, chunkFirst, chunkNext, chunkRest,
  //@ cljToJs, clone, codeReader, comp, comparator, comparatorProtocol, compareAndSet, compareKeys,
  //@ compareProtocol, compilerMacroVar, complement, completing, concat, configureHostHooks, conj,
  //@ conjBang, conjBangProtocol, conjProtocol, Cons, consCore, constantly, contains,
  //@ containsProtocol, coreCells, count, countProtocol, createNs, cycle, CycleValue, dec, dedupe,
  //@ defaultDispatchVal, Delay, demunge, deref, derefProtocol, derefWithTimeoutProtocol, derive,
  //@ descendants, disj, disjBang, disjoinBangProtocol, disjoinProtocol, dispatchFn, dissoc,
  //@ dissocBang, dissocBangProtocol, dissocProtocol, distinct, divide, doall, dorun, double,
  //@ doubleArray, doubles, drop, dropFirstProtocol, dropLast, dropProtocol, dropWhile, Eduction,
  //@ eduction, EductionFactory, empty, EmptyListValue, emptyProtocol, enableConsolePrint,
  //@ encodeClojureProtocol, encodeJsProtocol, encodeKeyProtocol, ensureReduced, entryKeyProtocol,
  //@ equal, equivMap, equivProtocol, es6Iterator, evalCore, every, everyPred, exCause,
  //@ ExceptionInfo, exData, exInfo, exMessage, ffirst, filter, filterv, find, findNs, findProtocol,
  //@ first, firstProtocol, flatten, float, floats, flushWriter, Fn, fnext, fnil, force,
  //@ frequencies, gensym, get, getGlobalHierarchy, getIn, getMethodProtocol, getValidator, greater,
  //@ greaterEqual, groupBy, haltWhen, hash, HashCollisionNode, hashDouble, hashMap, HashMapIter,
  //@ HashMapLite, hashOrderedColl, hashProtocol, hashSet, HashSetIter, hashUnorderedColl,
  //@ hostRandInt, hostRandNth, hostRandomUuid, hostShuffle, IAssociative, IAtom, IChunk,
  //@ IChunkedNext, IChunkedSeq, ICloneable, ICollection, IComparable, ICounted, identical,
  //@ identity, IDeref, IDerefWithTimeout, IDrop, IEditableCollection, IEmptyableCollection,
  //@ IEncodeClojure, IEncodeJS, IEquiv, IFind, IFn, IHash, IIndexed, IIterable, IKVReduce, IList,
  //@ ILookup, IMap, IMapEntry, IMeta, implementsProtocol, imul, IMultiFn, INamed, inc,
  //@ IndexedSeqFactory, IndexedSeqValue, INext, Inst, instMs, instMsProtocol, int, intArray,
  //@ IntegerRangeFactory, IntegerRangeValue, interleave, interpose, into, intoArray, ints,
  //@ invokeProtocol, IPending, IPrintWithWriter, IRecord, IReduce, IReset, IReversible, isa, isAny,
  //@ isArray, isAssociative, isBoolean, isChar, isChunkedSeq, isCloneable, isColl, isCounted,
  //@ isDelay, isDistinct, isDouble, isEmpty, ISeq, ISeqable, ISequential, ISet, isEven, isFalse,
  //@ isFloat, isFn, isIdent, isIFind, isIFn, isIndexed, isInfinite, isInst, isInstance, isInt,
  //@ isInteger, isIterable, isJsSymbol, isKeyword, isList, isMap, isMapEntry, isNaNCore,
  //@ isNaturalInt, isNegative, isNegativeInt, isNil, isNumber, isObject, isOdd, ISorted,
  //@ isPositive, isPositiveInt, isQualifiedIdent, isQualifiedKeyword, isQualifiedSymbol, isRecord,
  //@ isReduceable, isReduced, isRegex, isReversible, isSeq, isSeqable, isSequential, isSet,
  //@ isSimpleIdent, isSimpleKeyword, isSimpleSymbol, isSome, isSorted, isSpecialSymbol, isString,
  //@ isSymbol, IStack, isTaggedLiteral, isTrue, isUndefined, isUri, isUuid, isVar, isVector,
  //@ isVolatile, ISwap, isZero, iterate, IterateValue, iteration, iteratorInit, iteratorProtocol,
  //@ iteratorSeq, iteratorStart, ITransientAssociative, ITransientCollection, ITransientMap,
  //@ ITransientSet, ITransientVector, IUUID, IVector, IVolatile, IWatchable, IWithMeta, IWriter,
  //@ jsDelete, jsIn, jsInvoke, jsIterable, jsKeys, jsMod, jsObj, jsSet, jsToClj, juxt, keep,
  //@ keepIndexed, keyProtocol, keys, keyToJs, keywordCore, keywordIdentical, KeywordValue,
  //@ kvReduceProtocol, last, LazySeq, less, lessEqual, list, listStar, ListValue, loadFile, long,
  //@ longArray, longs, lookupProtocol, macroexpand, macroexpand1, macroReaderFunction, makeArray,
  //@ makeHierarchy, mapcat, MapEntryType, mapIndexed, mapSeq, mapv, max, maxKey, memoize, merge,
  //@ mergeWith, meta, metaProtocol, methods, min, minKey, mkBoundFn, mod, MultiFn, MultiFnFactory,
  //@ multiply, munge, mungeStr, name, nameProtocol, Namespace, namespace, NamespaceFactory,
  //@ namespaceProtocol, NeverEquiv, newline, next, nextProtocol, nfirst, nnext, NodeSeqType, not,
  //@ notAny, notEmpty, notEqual, notEvery, notifyWatches, nsName, nth, nthnext, nthProtocol,
  //@ nthrest, numericEqual, objectArray, ObjMap, ObjMapIterator, parents, parseBoolean,
  //@ parseDouble, parseLong, parseUuid, partial, partition, partitionAll, partitionBy, partitionV,
  //@ partitionVAll, peek, peekProtocol, PersistentArrayMapSeqType, PersistentArrayMapValue,
  //@ persistentBang, persistentBangProtocol, PersistentHashMapValue, PersistentHashSetValue,
  //@ PersistentQueueValue, PersistentTreeMapFactory, PersistentTreeMapSeqFactory,
  //@ PersistentTreeSetFactory, PersistentVectorValue, pop, popBang, popBangProtocol, popProtocol,
  //@ pr, preferMethodProtocol, prefers, primSeq, printCore, printForHost, println, printlnStr,
  //@ printMetaQ, printStr, prn, prnStr, prnStrWithOpts, protocolSentinel, prSequentialWriter,
  //@ prSeqWriter, prStr, prStrStar, prStrWithOpts, prWriter, prWriterFn, quot, randomSample, range,
  //@ RangedIterator, rangedIterator, RangedIteratorFactory, RangeFactory, RangeValue, reader,
  //@ realized, reduce, Reduced, reduced, reduceKv, reduceProtocol, reductions, reFind,
  //@ registerReaderNamespace, rem, reMatches, remove, removeAllMethods, removeMethodProtocol,
  //@ removeTap, removeWatch, removeWatchProtocol, rePattern, repeat, repeatedly, RepeatValue,
  //@ replace, replicate, requiredGet, reSeq, reset, resetMeta, resetProtocol, resetVals, rest,
  //@ restProtocol, reverse, rotateLeft, rseq, rseqProtocol, RSeqType, rsubseq, run, satisfies,
  //@ second, selectKeys, seq, SeqIter, seqIter, SeqIterFactory, seqProtocol, seqToMap, sequence,
  //@ sequenceIterator, set, SetLite, setPrintErrFn, setPrintFn, setValidator, short, shorts, some,
  //@ someFn, someVals, sort, sortBy, sortedMap, sortedMapBy, sortedSeqFromProtocol,
  //@ sortedSeqProtocol, sortedSet, sortedSetBy, splitAt, splitVAt, splitWith, str,
  //@ StringBufferWriter, stringHash, stringPrint, subs, subseq, subtract, subvec, SubvecValue,
  //@ supers, swap, swapProtocol, swapVals, symbolCore, symbolIdentical, SymbolValue, systemTime,
  //@ TaggedLiteral, taggedLiteral, TaggedLiteralFactory, take, takeLast, takeNth, takeWhile,
  //@ tapGreater, test, theNs, throwableToMap, toArray, toArray2d, trampoline, transduce, transient,
  //@ TransientArrayMapType, TransientHashMapType, TransientHashSetType, TransientVectorType,
  //@ treeMapConstructor, treeMapSeqConstructor, treeSeq, treeSetConstructor, type, uncheckedAdd,
  //@ uncheckedAddInt, uncheckedByte, uncheckedChar, uncheckedDec, uncheckedDecInt,
  //@ uncheckedDivideInt, uncheckedDouble, uncheckedFloat, uncheckedGet, uncheckedInc,
  //@ uncheckedIncInt, uncheckedInt, uncheckedLong, uncheckedMultiply, uncheckedMultiplyInt,
  //@ uncheckedNegate, uncheckedNegateInt, uncheckedRemainderInt, uncheckedShort, uncheckedSubtract,
  //@ uncheckedSubtractInt, underive, unreduced, unsignedBitShiftRight, update, updateIn,
  //@ updateKeys, updateVals, uuid, UUIDFactory, UUIDType, valProtocol, vals, Var, varyMeta, vec,
  //@ vector, VectorLite, VectorLiteIterator, VectorNode, Volatile, volatile, volatileFactory,
  //@ vreset, vresetProtocol, withMeta, withMetaProtocol, write, writeAll, zipmap
  // Every core operation by runtime name, for the shared runtime and reflection;
  // Advanced output reads the values directly (src/runtime_manifest.rs).
  const core = Object.assign(coreCells, {
   EmptyList: EmptyListValue, PersistentVector: PersistentVectorValue,
    Subvec: SubvecValue, MapEntry: MapEntryType,
    PersistentArrayMap: PersistentArrayMapValue,
    PersistentHashMap: PersistentHashMapValue,
    PersistentTreeMap: treeMapConstructor,
    PersistentHashSet: PersistentHashSetValue,
    PersistentTreeSet: treeSetConstructor, List: ListValue,
    Cons, IndexedSeq: IndexedSeqValue,
    ChunkedSeq: ChunkedSeqValue, LazySeq,
    PersistentQueue: PersistentQueueValue, Repeat: RepeatValue,
    Cycle: CycleValue, Iterate: IterateValue, Range: RangeValue,
    IntegerRange: IntegerRangeValue, NodeSeq: NodeSeqType,
    PersistentArrayMapSeq: PersistentArrayMapSeqType, ArrayNodeSeq: ArrayNodeSeqType,
    RSeq: RSeqType, BlackNode: MapEntryType, RedNode: MapEntryType,
    ArrayList, ArrayNode, ArrayNodeIterator, BitmapIndexedNode, Box, HashCollisionNode,
    HashMapIter, HashMapLite, HashSetIter, NeverEquiv, ObjMap, ObjMapIterator, SetLite,
    VectorLite, VectorLiteIterator, VectorNode,
    PersistentTreeMapSeq: treeMapSeqConstructor, Keyword: KeywordValue,
    Symbol: SymbolValue, Reduced, UUID: UUIDType,
    Delay, Eduction,
    TransientVector: TransientVectorType,
    TransientArrayMap: TransientArrayMapType,
    TransientHashMap: TransientHashMapType,
    TransientHashSet: TransientHashSetType, ArrayChunk: arrayChunk, IMultiFn,
    MultiFn, makeHierarchy, getGlobalHierarchy, isa, parents, ancestors, descendants, derive,
    underive, bases, supers, removeAllMethods, addMethod: addMethodProtocol,
    removeMethod: removeMethodProtocol, preferMethod: preferMethodProtocol,
    getMethod: getMethodProtocol, methods, prefers, defaultDispatchVal, dispatchFn, IChunk,
    IChunkedNext, IDrop, IEditableCollection, ITransientCollection, ITransientAssociative,
    ITransientMap, ITransientVector, ITransientSet, transient, persistentBang, conjBang, assocBang,
    dissocBang, disjBang, popBang, asTransientProtocol, persistentBangProtocol, conjBangProtocol,
    assocBangProtocol, assocNBangProtocol, dissocBangProtocol, disjoinBangProtocol, popBangProtocol,
    chunkedNextProtocol, dropProtocol, dropFirstProtocol, chunkFirst, chunkRest, chunkNext,
    chunkCons, ChunkedCons, arrayChunk, chunkBuffer, ChunkBuffer, chunkAppend, chunk, second,
    ffirst, nfirst, fnext, nnext, reverse, some, notEvery, notAny, notEmpty, run, isUri, cat, drop,
    repeatedly, replicate, mapIndexed, mapcat, filter, remove, keep, keepIndexed, takeWhile,
    dropWhile, takeLast, dropLast, takeNth, splitAt, splitWith, partition, partitionAll,
    partitionBy, interleave, interpose, dorun, doall, boundedCount, last, butlast, mapv, filterv,
    IMapEntry, IFind, ISorted, IChunkedSeq, isIterable, writeAll, keyProtocol, valProtocol,
    findProtocol, sortedSeqProtocol, sortedSeqFromProtocol, entryKeyProtocol, comparatorProtocol,
    chunkedFirstProtocol, chunkedRestProtocol, invokeProtocol, subtract, multiply, divide, inc, dec,
    less, lessEqual, greater, greaterEqual, numericEqual, max, min, abs, int, long, uncheckedInt,
    uncheckedLong, byte, short, float, double, char, booleans, bytes, chars, shorts, ints, floats,
    doubles, longs, uncheckedByte, uncheckedShort, uncheckedFloat, uncheckedDouble, uncheckedChar,
    uncheckedAdd, uncheckedAddInt, uncheckedSubtract, uncheckedSubtractInt, uncheckedMultiply,
    uncheckedMultiplyInt, uncheckedDivideInt, uncheckedInc, uncheckedIncInt, uncheckedDec,
    uncheckedDecInt, uncheckedNegate, uncheckedNegateInt, uncheckedRemainderInt, jsMod, mod, quot,
    rem, bitXor, bitAnd, bitOr, bitAndNot, bitNot, bitClear, bitFlip, bitSet, bitTest, bitShiftLeft,
    bitShiftRight, bitShiftRightZeroFill, unsignedBitShiftRight, bitCount, isZero, isPositive,
    isNegative, isEven, isOdd, isInteger, isInt, isPositiveInt, isNegativeInt, isNaturalInt,
    isNumber, isFloat, isDouble, isInfinite, isNaN: isNaNCore, boolean, not, isBoolean, isTrue,
    isFalse, isNil, isSome, isUndefined, notEqual, identity, isAny, isString, isChar, isKeyword,
    isSymbol, isIdent, isSimpleIdent, isQualifiedIdent, isSimpleKeyword, isQualifiedKeyword,
    isSimpleSymbol, isQualifiedSymbol, keywordIdentical, symbolIdentical, isFn, isIFn, isArray,
    isObject, isColl, isSeq, isList, isSeqable, isSequential, isVector, isMap, isSet, isAssociative,
    isIFind, isCounted, isIndexed, isReduceable, isReversible, isSorted, isRecord, isMapEntry,
    isChunkedSeq, isEmpty, keyword: keywordCore, symbol: symbolCore, vector, list, gensym,
    macroexpand, macroexpand1, eval: evalCore, hashMap, arrayMap, sortedMap, sortedMapBy, sortedSet,
    sortedSetBy, subseq, rsubseq, mkBoundFn, hashSet, dissoc, disj, conj, assoc, seq, seqToMap,
    first, rest, next, name, namespace, subs, count, nth, get, getIn, assocIn, updateIn, selectKeys,
    merge, mergeWith, zipmap, contains, find, every, set, isCloneable, equal, reduce, completing,
    transduce, ensureReduced, haltWhen, dedupe, randomSample, add, reduced, isReduced, unreduced,
    deref, meta, withMeta, varyMeta, resetMeta, alterMeta, atom, reset, swap, Atom, atomFactory,
    Volatile, volatile, volatileFactory, isVolatile, vreset, vresetProtocol, derefProtocol,
    resetVals, swapVals, compareAndSet, setValidator, getValidator, addWatch, removeWatch,
    notifyWatches, addWatchProtocol, removeWatchProtocol, resetProtocol, swapProtocol, update,
    IPending, realized, force, isDelay, nthrest, nthnext, IDeref, IDerefWithTimeout, Inst, IUUID,
    ASeq, IAtom, IReset, ISwap, IVolatile, IWatchable, derefWithTimeoutProtocol, IReduce, IKVReduce,
    IVector, IMap, ICollection, IEmptyableCollection, ISet, IStack, IReversible, IComparable,
    IIterable, reduceKv, subvec, equivMap, SeqIter, RangedIterator, seqIter, rangedIterator,
    reduceProtocol, kvReduceProtocol, assocProtocol, assocNProtocol, dissocProtocol, conjProtocol,
    emptyProtocol, disjoinProtocol, peekProtocol, popProtocol, rseqProtocol, compareProtocol,
    iteratorProtocol, hashOrderedColl, hashUnorderedColl, hashDouble, stringHash, rotateLeft, imul,
    compare: compareKeys, INamed, nameProtocol, namespaceProtocol, IHash, IEquiv, hashProtocol,
    equivProtocol, type, Fn, IList, IFn, ISeqable, ISeq, INext, ICounted, IIndexed, ILookup,
    ISequential, IAssociative, seqProtocol, firstProtocol, restProtocol, nextProtocol,
    countProtocol, nthProtocol, lookupProtocol, containsProtocol, protocolSentinel, IMeta,
    IWithMeta, IRecord, ICloneable, IPrintWithWriter, IWriter, StringBufferWriter, clone, prWriter,
    write, flushWriter, satisfies, implements: implementsProtocol, metaProtocol, withMetaProtocol,
    empty, peek, pop, rseq, keys, vals, repeat, cycle, prStr, prStrStar, str, uuid, identical,
    isUuid, isInst, printCore, someVals, requiredGet, minKey, maxKey, reductions, replace, distinct,
    isDistinct, sort, sortBy, someFn, everyPred, memoize, parseLong, parseDouble, parseUuid,
    parseBoolean, isJsSymbol, isSpecialSymbol, groupBy, frequencies, trampoline, comparator,
    updateVals, updateKeys, setPrintFn, setPrintErrFn, stringPrint, enableConsolePrint,
    prStrWithOpts, prnStrWithOpts, prSequentialWriter, prSeqWriter, prWriterFn, printMetaQ, printForHost,
    println, prn, pr, newline, prnStr, printStr, printlnStr, TaggedLiteral, taggedLiteral,
    isTaggedLiteral, splitVAt, partitionV, partitionVAll, jsIterable, iteration, cljToJs, jsToClj,
    keyToJs, IEncodeJS, IEncodeClojure, munge, mungeStr, demunge, treeSeq, flatten,
    iter: sequenceIterator, es6Iterator, es6IteratorSeq: iteratorSeq, encodeJsProtocol,
    encodeKeyProtocol, encodeClojureProtocol, Var, test, hash, instMs, instMsProtocol, isVar,
    ExceptionInfo, exInfo, exData, exMessage, exCause, throwableToMap, isInstance, rePattern, reSeq,
    reFind, reMatches, isRegex, vec, concat, sequence, eduction, apply, applyTo, into,
    cons: consCore, listStar, iterate, take, range, toArray, mapSeq, primSeq, arraySeq, array, aget,
    aset, alength, aclone, makeArray, intoArray, toArray2d, intArray, longArray, doubleArray,
    objectArray, jsObj, jsKeys, jsInvoke, jsDelete, jsIn, uncheckedGet, uncheckedSet: jsSet,
    checkedAget, checkedAset, checkedAgetStrict, checkedAsetStrict, constantly, complement, comp,
    partial, juxt, fnil, iteratorInit, iteratorStart, IndexedSeqFactory, UUIDFactory, RangeFactory,
    IntegerRangeFactory, PersistentTreeMapFactory, PersistentTreeSetFactory,
    PersistentTreeMapSeqFactory, EductionFactory, MultiFnFactory, SeqIterFactory,
    RangedIteratorFactory, NamespaceFactory, TaggedLiteralFactory,
    ednReadString: reader.ednReadString, ednRead: reader.ednRead,
    readerReadString: reader.readerReadString, readerRead: reader.readerRead,
    readString: codeReader.readString, read: codeReader.read, readLine: reader.readLine,
    compilerInputWindow: reader.compilerInputWindow,
    compilerInputConsume: reader.compilerInputConsume,
    compilerInputRecover: reader.compilerInputRecover,
    compilerInputDiscard: reader.compilerInputDiscard,
    compilerInputCompact: reader.compilerInputCompact,
    readerStringReader: reader.readerStringReader, readerInputProvider: reader.readerInputProvider,
    readerReadLine: reader.readerReadLine,
    readerParseTimestampFields: reader.readerParseTimestampFields,
    readerParseTimestamp: reader.readerParseTimestamp,
    readerInitialTagTable: reader.readerInitialTagTable,
    readerRegisterTagParser: reader.readerRegisterTagParser,
    readerDeregisterTagParser: reader.readerDeregisterTagParser,
    readerRegisterDefaultTagParser: reader.readerRegisterDefaultTagParser,
    readerDeregisterDefaultTagParser: reader.readerDeregisterDefaultTagParser,
    readerWithInput: reader.readerWithInput, readerCurrentInput: reader.readerCurrentInput,
    readerClose: reader.readerClose, configureHostHooks, systemTime, addTap, removeTap, tapGreater,
    randInt: hostRandInt, randNth: hostRandNth, shuffle: hostShuffle, randomUuid: hostRandomUuid,
    loadFile, Namespace, registerReaderNamespace, macroReaderFunction, createNs, findNs, nsName,
    theNs, allNs, compilerMacroVar
  });
  //@unit namespaces.cljsUser compile-time effect: namespaces requires: coreCells, createNs, symbol
  coreCells.currentNs = createNs(symbol(null, "cljs.user"));
  //@unit namespaces.cljsCore compile-time effect: namespaces requires: createNs, symbol
  createNs(symbol(null, "cljs.core"));
  //@unit core.seal effect: core requires: core
  Object.seal(core);
  //@exports
  return Object.freeze({setDeclarationDemand, declarationReceiver, declarationTransaction, applyDeclarationPatches, snapshotDeclarationPatches, restoreDeclarationPatches, compilerExceptionMessage, compilerForms, keyword, keywordFqn, get, symbol, vector, vectorFromArray, listFromArray, mapFromArray, arrayMapFromArray, mapFromArrays, mapShape, mapFromShape, hashMapShape, hashMapFromShape, setFromArray, arraySetFromArray, constantMapFromArray, constantSetFromArray, literalSetFromArray, setShape, setFromShape, queueFromArray, jsObject, makeVar, definitionVar, makeIFn, protocol, protocolPredicate, protocolMethod, metadataMethod, extendType, extendInstance, makeReify, reify, makeType, typeMasks, recordMethods, recordFromMap, Loadable, fnRest, fnApplyTo, displayStr, setMacroexpandServices, compilerFormBridge, core, inspect, hash, hashed});
})()
