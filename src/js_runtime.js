// Original bounded runtime for emitted JavaScript. This file is a factory
// expression, embedded once in each compilation unit that needs owned values.
(() => {
  "use strict";

  //@unit keywords
  // Brands and payloads stay private: copying fields or inheriting a value's
  // prototype cannot turn a host object into a language value.
  const keywords = new WeakMap();
  //@unit internedKeywords
  const internedKeywords = new Map();
  //@unit vectors
  const vectors = new WeakMap();
  //@unit symbols
  const symbols = new WeakMap();
  //@unit maps
  const maps = new WeakMap();
  //@unit mapKinds
  const mapKinds = new WeakMap();
  //@unit sortedComparators
  const sortedComparators = new WeakMap();
  //@unit mapEntries
  const mapEntries = new WeakSet();
  //@unit sets
  const sets = new WeakMap();
  //@unit setKinds
  const setKinds = new WeakMap();
  //@unit transients
  const transients = new WeakMap();
  //@unit cells
  const cells = new WeakMap();
  //@unit views
  const views = new WeakMap();
  //@unit reducedValues
  const reducedValues = new WeakMap();
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
  //@unit uuids
  const uuids = new WeakMap();
  //@unit uuidHashes
  const uuidHashes = new WeakMap();
  //@unit vars
  const vars = new WeakMap();
  //@unit atoms
  const atoms = new WeakMap(), volatiles = new WeakMap();
  //@unit queues
  const queues = new WeakMap();
  //@unit lazyValues
  const lazyValues = new WeakMap(), delays = new WeakMap();
  //@unit eductions
  const eductions = new WeakMap();
  //@unit ifnValues
  const ifnValues = new WeakMap(), ifnVariadics = new WeakMap();
  //@unit metaFunctions
  const metaFunctions = new WeakMap();
  //@unit typeNames
  const typeNames = new WeakMap();
  //@unit typeFields
  const typeFields = new WeakMap(), recordTypes = new WeakMap(), reifyValues = new WeakSet();
  //@unit builtinType mutator
  // Called once per type, before any of its values exist. Extension prototypes
  // are public and mutable; immutable collection payloads and their base
  // callability remain private and frozen.
  function builtinType(Type, base) {
    Type.prototype = Object.create(base);
    Object.defineProperty(Type.prototype, "constructor", {value: Type, writable: true, configurable: true});
    return Type;
  }
  //@unit Delay.type effect: Delay requires: builtinType, Delay
  builtinType(Delay, Object.prototype);
  //@unit Eduction.type effect: Eduction requires: builtinType, Eduction
  builtinType(Eduction, Object.prototype);
  //@unit ChunkBuffer.type effect: ChunkBuffer requires: builtinType, ChunkBuffer
  builtinType(ChunkBuffer, Object.prototype);
  //@unit arrayChunk.type effect: arrayChunk requires: arrayChunk, builtinType
  builtinType(arrayChunk, Object.prototype);
  //@unit StringBufferWriter.type effect: StringBufferWriter requires: builtinType,
  //@ StringBufferWriter
  builtinType(StringBufferWriter, Object.prototype);
  //@unit reducedPrototype
  const reducedPrototype = Object.freeze({});
  //@unit Reduced.type effect: Reduced requires: builtinType, Reduced, reducedPrototype
  builtinType(Reduced, reducedPrototype);
  //@unit sequencePrototype requires: count, objectEquiv, print, sequentialIndex
  const sequencePrototype = Object.freeze({
    equiv(other) { return objectEquiv(this, other); },
    toString() { return print(this, true); },
    indexOf(value, start = 0) { return sequentialIndex(this, value, start, false); },
    lastIndexOf(value, start = count(this)) { return sequentialIndex(this, value, start, true); }
  });
  //@unit ListValue.type effect: ListValue requires: builtinType, ListValue, sequencePrototype
  builtinType(ListValue, sequencePrototype);
  //@unit EmptyListValue.type effect: EmptyListValue requires: builtinType, EmptyListValue,
  //@ sequencePrototype
  builtinType(EmptyListValue, sequencePrototype);
  //@unit Cons.type effect: Cons requires: builtinType, Cons, sequencePrototype
  builtinType(Cons, sequencePrototype);
  //@unit IndexedSeqValue.type effect: IndexedSeqValue requires: builtinType, IndexedSeqValue,
  //@ sequencePrototype
  builtinType(IndexedSeqValue, sequencePrototype);
  //@unit ChunkedSeqValue.type effect: ChunkedSeqValue requires: builtinType, ChunkedSeqValue,
  //@ sequencePrototype
  builtinType(ChunkedSeqValue, sequencePrototype);
  //@unit ChunkedCons.type effect: ChunkedCons requires: builtinType, ChunkedCons, sequencePrototype
  builtinType(ChunkedCons, sequencePrototype);
  //@unit LazySeq.type effect: LazySeq requires: builtinType, LazySeq, sequencePrototype
  builtinType(LazySeq, sequencePrototype);
  //@unit PersistentQueueValue.type effect: PersistentQueueValue requires: builtinType,
  //@ PersistentQueueValue, sequencePrototype
  builtinType(PersistentQueueValue, sequencePrototype);
  //@unit RepeatValue.type effect: RepeatValue requires: builtinType, RepeatValue, sequencePrototype
  builtinType(RepeatValue, sequencePrototype);
  //@unit CycleValue.type effect: CycleValue requires: builtinType, CycleValue, sequencePrototype
  builtinType(CycleValue, sequencePrototype);
  //@unit IterateValue.type effect: IterateValue requires: builtinType, IterateValue,
  //@ sequencePrototype
  builtinType(IterateValue, sequencePrototype);
  //@unit RangeValue.type effect: RangeValue requires: builtinType, RangeValue, sequencePrototype
  builtinType(RangeValue, sequencePrototype);
  //@unit IntegerRangeValue.type effect: IntegerRangeValue requires: builtinType, IntegerRangeValue,
  //@ sequencePrototype
  builtinType(IntegerRangeValue, sequencePrototype);
  //@unit NodeSeqType effect requires: builtinType, sequencePrototype
  const NodeSeqType = builtinType(function () {}, sequencePrototype);
  //@unit treeMapSeqConstructor.type effect: treeMapSeqConstructor requires: builtinType,
  //@ sequencePrototype, treeMapSeqConstructor
  builtinType(treeMapSeqConstructor, sequencePrototype);
  //@unit emptyList effect requires: cells, EmptyListValue
  const emptyList = Object.create(EmptyListValue.prototype);
  cells.set(emptyList, Object.freeze({kind: "list", count: 0}));
  //@unit missing
  const missing = Object.freeze({});
  //@unit truth
  const truth = value => value != null && value !== false;
  //@unit invalidArity
  const invalidArity = n => { throw new Error("Invalid arity: " + n); };
  //@unit outOfBounds
  const outOfBounds = () => { throw new Error("Index out of bounds"); };

  //@unit keywordPrototype requires: get, invalidArity, keywords, objectEquiv
  const keywordPrototype = Object.freeze({
    equiv(other) { return objectEquiv(this, other); },
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    call(_self, collection, notFound) {
      if (!keywords.has(this)) throw new Error("Expected keyword");
      switch (arguments.length) {
        case 2: return get(collection, this);
        case 3: return get(collection, this, notFound);
        default: return invalidArity(arguments.length - 1);
      }
    },
    toString() {
      const value = keywords.get(this);
      if (!value) throw new Error("Expected keyword");
      return ":" + value.spelling;
    }
  });
  //@unit KeywordValue.type effect: KeywordValue requires: builtinType, keywordPrototype,
  //@ KeywordValue
  builtinType(KeywordValue, keywordPrototype);

  //@unit symbolPrototype requires: get, invalidArity, objectEquiv, symbols
  const symbolPrototype = Object.freeze({
    equiv(other) { return objectEquiv(this, other); },
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    call(_self, collection, notFound) {
      if (!symbols.has(this)) throw new Error("Expected symbol");
      switch (arguments.length) {
        case 2: return get(collection, this);
        case 3: return get(collection, this, notFound);
        default: return invalidArity(arguments.length - 1);
      }
    },
    toString() {
      const value = symbols.get(this);
      if (!value) throw new Error("Expected symbol");
      return value.spelling;
    }
  });
  //@unit SymbolValue.type effect: SymbolValue requires: builtinType, symbolPrototype, SymbolValue
  builtinType(SymbolValue, symbolPrototype);

  //@unit SymbolValue requires: constructorHashes, invalidArity, metadataValues, symbols
  // Positional constructors retain the supplied slots; unlike cons/symbol they
  // do not normalize a tail or derive the printed spelling from name/namespace.
  function SymbolValue(namespace, name, spelling, cachedHash, metadata) {
    if (arguments.length !== 5) return invalidArity(arguments.length);
    const value = Object.create(SymbolValue.prototype);
    symbols.set(value, Object.freeze({namespace, name, spelling}));
    constructorHashes.set(value, cachedHash);
    metadataValues.set(value, metadata);
    return value;
  }

  //@unit Cons requires: cells, constructorHashes, invalidArity, metadataValues
  function Cons(metadata, head, tail, cachedHash) {
    if (arguments.length !== 4) return invalidArity(arguments.length);
    const value = Object.create(Cons.prototype);
    cells.set(value, Object.freeze({kind: "cons", head, tail}));
    constructorHashes.set(value, cachedHash);
    metadataValues.set(value, metadata);
    return value;
  }

  //@unit constructorSlots requires: constructorHashes, metadataValues, publicCollectionSlots
  function constructorSlots(value, slots, metadata, cachedHash) {
    publicCollectionSlots.set(value, slots);
    metadataValues.set(value, metadata);
    constructorHashes.set(value, cachedHash);
    return value;
  }
  //@unit ListValue requires: cells, constructorSlots
  function ListValue(metadata, head, tail, suppliedCount, cachedHash) {
    const value = Object.create(ListValue.prototype);
    cells.set(value, Object.freeze({kind: "list", head, tail, count: suppliedCount}));
    return constructorSlots(value, {}, metadata, cachedHash);
  }
  //@unit EmptyListValue requires: cells, metadataValues
  function EmptyListValue(metadata) {
    const value = Object.create(EmptyListValue.prototype);
    cells.set(value, Object.freeze({kind: "list", count: 0}));
    metadataValues.set(value, metadata);
    return value;
  }
  //@unit KeywordValue requires: constructorHashes, makeKeyword
  function KeywordValue(namespace, name, spelling, cachedHash) {
    const value = makeKeyword(namespace, name, spelling);
    constructorHashes.set(value, cachedHash);
    return value;
  }
  //@unit CycleValue requires: cycleNode, metadataValues
  function CycleValue(metadata, all, previous, current, following) {
    const value = cycleNode(all, previous, current, following);
    metadataValues.set(value, metadata);
    return value;
  }
  //@unit RepeatValue requires: cells, constructorHashes, emptyList, metadataValues
  function RepeatValue(metadata, remaining, head, following, cachedHash) {
    const value = Object.create(RepeatValue.prototype), state = {following};
    cells.set(value, Object.freeze({kind: "repeat", remaining, head, state,
      get tail() { return state.following ?? (remaining === -1 ? value : remaining > 1
        ? (state.following = RepeatValue(null, remaining - 1, head, null, null)) : emptyList); }}));
    metadataValues.set(value, metadata); constructorHashes.set(value, cachedHash);
    return value;
  }
  //@unit IterateValue requires: iterateNode, iterateUnrealizedSeed, metadataValues
  function IterateValue(metadata, fn, previous, seed, following) {
    const value = iterateNode(fn, previous, seed, seed !== iterateUnrealizedSeed, following);
    metadataValues.set(value, metadata);
    return value;
  }
  //@unit PersistentVectorValue requires: constructorSlots, makeVector, vectorRoots
  // Root slots are opaque canonical tokens, never an upstream trie-node ABI.
  function PersistentVectorValue(metadata, cnt, shift, root, tail, cachedHash) {
    const prefix = vectorRoots.get(root) ?? [], tailOffset = cnt < 32 ? 0 : ((cnt - 1) >>> 5) << 5;
    const items = Array.from({length: cnt});
    for (let i = 0; i < cnt; i++) Object.defineProperty(items, i, {enumerable: true,
      get: () => i < tailOffset ? prefix[i] : tail[i - tailOffset]});
    const value = makeVector(items);
    return constructorSlots(value, {cnt, shift, root, tail}, metadata, cachedHash);
  }
  //@unit SubvecValue requires: constructorSlots, makeVector, nth, subvecSources
  function SubvecValue(metadata, source, start, end, cachedHash) {
    const items = Array.from({length: Math.max(0, end - start)});
    for (let i = 0; i < items.length; i++) Object.defineProperty(items, i, {enumerable: true, get: () => nth(source, start + i)});
    const value = makeVector(items);
    Object.setPrototypeOf(value, SubvecValue.prototype);
    subvecSources.set(value, source);
    return constructorSlots(value, {start, end}, metadata, cachedHash);
  }
  //@unit ChunkedSeqValue requires: constructorSlots, view
  function ChunkedSeqValue(source, node, index, offset, metadata, cachedHash) {
    const value = view(node, offset, false, "chunked");
    return constructorSlots(value, {vec: source, node, i: index, off: offset}, metadata, cachedHash);
  }
  //@unit PersistentQueueValue requires: constructorSlots, queueFromArray
  function PersistentQueueValue(metadata, cnt, front, rear, cachedHash) {
    const value = queueFromArray([]);
    return constructorSlots(value, {count: cnt, front, rear}, metadata, cachedHash);
  }
  //@unit PersistentArrayMapValue requires: constructorSlots, hash, makeMap
  function PersistentArrayMapValue(metadata, cnt, array, cachedHash) {
    const entries = [];
    // Positional arrays retain duplicate keys and their first-match lookup.
    for (let i = 0; i < array.length; i += 2) entries.push(Object.freeze({get key() { return array[i]; }, get value() { return array[i + 1]; }, get hash() { return hash(array[i]); }}));
    return constructorSlots(makeMap(entries, "array-map"), {cnt, arr: array}, metadata, cachedHash);
  }
  //@unit PersistentHashMapValue requires: constructorSlots, hashRoots, makeMap, maps
  function PersistentHashMapValue(metadata, cnt, root, hasNil, nilValue, cachedHash) {
    const entries = root == null ? [] : (maps.get(root) ?? []).filter(e => e.key != null).slice();
    if (root != null && hashRoots.has(maps.get(root))) hashRoots.set(entries, hashRoots.get(maps.get(root)));
    if (hasNil) entries.unshift(Object.freeze({key: null, value: nilValue, hash: 0}));
    return constructorSlots(makeMap(entries, "hash-map"), {cnt, root, "has-nil?": hasNil, "nil-val": nilValue}, metadata, cachedHash);
  }
  //@unit PersistentHashSetValue requires: constructorSlots, ensureHashRoot, makeSet, mapKinds, maps
  function PersistentHashSetValue(metadata, source, cachedHash) {
    const entries = (maps.get(source) ?? []).map(e => Object.freeze({key: e.key, hash: e.hash}));
    if (mapKinds.get(source) === "hash-map") ensureHashRoot(entries);
    return constructorSlots(makeSet(entries, mapKinds.get(source)), {"hash-map": source}, metadata, cachedHash);
  }

  //@unit Reduced requires: invalidArity, reduced
  function Reduced(value) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    return reduced(value);
  }

  //@unit symbol requires: symbols, SymbolValue
  function symbol(namespace, name) {
    const value = Object.create(SymbolValue.prototype);
    symbols.set(value, Object.freeze({namespace, name,
      spelling: namespace == null ? name : namespace + "/" + name}));
    return value;
  }

  //@unit symbolCore requires: invalidArity, keywords, symbol, symbols, vars
  function symbolCore(namespace, name) {
    switch (arguments.length) {
      case 1: {
        if (symbols.has(namespace)) return namespace;
        if (vars.has(namespace)) return vars.get(namespace).symbol;
        const kw = keywords.get(namespace);
        if (kw) namespace = kw.spelling;
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
  //@unit setMacroexpandServices requires: macroexpandServices
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

  //@unit cons requires: cells, Cons, count, implementsProtocol, ISeq, ListValue, seq
  function cons(head, tail, kind = "cons") {
    if (kind === "cons") {
      if (tail == null) kind = "list";
      else if (!implementsProtocol(ISeq, tail)) tail = seq(tail);
    }
    const value = Object.create((kind === "list" ? ListValue : Cons).prototype);
    cells.set(value, Object.freeze({kind, head, tail, ...(kind === "list" ? {count: count(tail) + 1} : {})}));
    return value;
  }

  //@unit listFromArray requires: cons, emptyList
  function listFromArray(items) {
    let result = emptyList;
    for (let i = items.length - 1; i >= 0; i--) result = cons(items[i], result, "list");
    return result;
  }

  //@unit list requires: listFromArray
  function list(...items) { return listFromArray(items); }

  //@unit view requires: ChunkedSeqValue, IndexedSeqValue, NodeSeqType, treeMapSeqConstructor, views
  function view(storage, offset, indexed, kind = "indexed", tail = null) {
    const value = Object.create((kind === "indexed" ? IndexedSeqValue : kind === "chunked" ? ChunkedSeqValue : kind === "hash-map" ? NodeSeqType : kind === "sorted-map" ? treeMapSeqConstructor : IndexedSeqValue).prototype);
    views.set(value, Object.freeze({storage, offset, indexed, kind, tail}));
    return value;
  }

  //@unit seq requires: cells, concat, deferredValue, iteratorSeq, lazyValues, mapEntries,
  //@ MapEntryType, mapKinds, maps, mapTraversal, publicCollectionSlots, publicMethod, queues, sets,
  //@ setTraversal, subvecSources, treeMapPush, treeMapSeqConstructor, vector, vectors, view, views
  function seq(value) {
    if (value == null) return null;
    const method = publicMethod(value, "ISeqable/-seq", 1, true);
    if (method) return method.call(value, value);
    if (value == null || (cells.get(value)?.kind === "list" && cells.get(value).count === 0 && !publicCollectionSlots.has(value))) return null;
    const lazy = lazyValues.get(value);
    if (lazy) {
      let current = deferredValue(lazy, true);
      // Flatten nested LazySeq results iteratively, retaining the raw final
      // value. CLJS calls seq on that raw value again on each traversal.
      while (lazyValues.has(current)) {
        current = deferredValue(lazyValues.get(current), true);
        // Compress as we advance, so discarded wrappers and their captured
        // source heads can be collected even through a long empty prefix.
        lazy.value = current;
      }
      lazy.value = current;
      return seq(current);
    }
    const queue = queues.get(value);
    if (queue && publicCollectionSlots.has(value)) { const q = publicCollectionSlots.get(value); return seq(concat(q.front, q.rear)); }
    if (queue) return queue.length ? view(queue, 0, false, "queue") : null;
    if (Array.isArray(value)) return value.length ? view(value, 0, true) : null;
    if (cells.has(value)) return value;
    if (views.has(value)) { const v = views.get(value); return v.offset < v.storage.length ? value : null; }
    const items = vectors.get(value);
    if (items && publicCollectionSlots.has(value) && !subvecSources.has(value)) { const slots = publicCollectionSlots.get(value); if (slots.cnt === 0) return null; if (slots.cnt <= 32) return view(slots.tail, 0, true); }
    if (items) return items.length ? view(items, 0, items.length <= 32, items.length <= 32 ? "indexed" : "chunked") : null;
    if (typeof value === "string") return value.length ? view(value, 0, true) : null;
    const treeSlots = publicCollectionSlots.get(value);
    if (maps.has(value) && treeSlots && "tree" in treeSlots) return treeSlots.cnt > 0
      ? treeMapSeqConstructor(null, treeMapPush(treeSlots.tree, null, true), true, treeSlots.cnt, null) : null;
    const entries = mapTraversal(value);
    if (entries) return entries.length
      ? view(Object.freeze(entries.map(entry => {
          const pair = vector(entry.key, entry.value);
          Object.setPrototypeOf(pair, MapEntryType.prototype);
          mapEntries.add(pair);
          return pair;
        })), 0, false, mapKinds.get(value)) : null;
    const members = sets.get(value);
    if (members) return members.length
      ? view(Object.freeze(setTraversal(value).map(entry => entry.key)), 0, false, "set") : null;
    if (value[Symbol.iterator] != null) return iteratorSeq(value[Symbol.iterator]());
    throw new Error(String(value) + " is not ISeqable");
  }

  //@unit iteratorSeq requires: installMethods
  // CLJS reprobes a nil tail; only successful nonempty tails are memoized.
  function iteratorSeq(iterator) {
    const item = iterator.next();
    if (item.done) return null;
    return installMethods({}, [["ISeqable", 0, null], ["ISeq", 0, null],
      ["ISeqable/-seq", 1, function () { return this; }],
      ["ISeq/-first", 1, () => item.value],
      ["ISeq/-rest", 1, (() => { let tail = null; return () => {
        if (tail === null) tail = iteratorSeq(iterator);
        return tail;
      }; })()]]);
  }
  //@unit first requires: builtin, cells, chunks, implementsProtocol, ISeq, lazyValues,
  //@ publicCollectionSlots, publicMethod, queues, seq, views
  function first(value) {
    if (value == null) return null;
    const method = implementsProtocol(ISeq, value) ? publicMethod(value, "ISeq/-first", 1) : null;
    if (method) return method.call(value, value);
    if (queues.has(value) && publicCollectionSlots.has(value)) return first(publicCollectionSlots.get(value).front);
    if (chunks.get(value)?.integer) return chunks.get(value).start;
    if (lazyValues.has(value)) { seq(value); return first(lazyValues.get(value).value); }
    const current = seq(value);
    if (current === null) return null;
    const cell = cells.get(current);
    if (cell) return cell.head;
    const slice = views.get(current);
    if (slice) return slice.storage[slice.offset];
    const firstMethod = builtin(current, "ISeq/-first", 1);
    if (firstMethod) return firstMethod.call(current, current);
    throw new Error("ISeq.-first not supported on this type");
  }

  //@unit rest requires: builtin, cells, ChunkedSeqValue, emptyList, implementsProtocol, ISeq,
  //@ lazyValues, meta, ownedChunkRest, publicCollectionSlots, publicMethod, seq, view, views,
  //@ withMeta
  function rest(value) {
    if (value == null) return emptyList;
    const method = implementsProtocol(ISeq, value) ? publicMethod(value, "ISeq/-rest", 1) : null;
    if (method) return method.call(value, value);
    if (lazyValues.has(value)) { seq(value); return rest(lazyValues.get(value).value); }
    const current = seq(value);
    if (current === null) return emptyList;
    const cell = cells.get(current);
    if (cell) return cell.kind === "list" ? cell.count === 1 ? emptyList : cell.tail : cell.tail ?? emptyList;
    const slice = views.get(current);
    const slots = publicCollectionSlots.get(current);
    if (slice && slots?.vec) return slice.offset + 1 < slice.storage.length
      ? ChunkedSeqValue(slots.vec, slots.node, slots.i, slice.offset + 1, null, null) : ownedChunkRest(current);
    if (!slice) {
      const restMethod = builtin(current, "ISeq/-rest", 1);
      if (restMethod) return restMethod.call(current, current);
      throw new Error("ISeq.-rest not supported on this type");
    }
    const tail = slice.offset + 1 < slice.storage.length
      ? view(slice.storage, slice.offset + 1, slice.indexed, slice.kind, slice.tail) : slice.tail ?? emptyList;
    return slice.kind === "queue" ? withMeta(tail, meta(current)) : tail;
  }

  //@unit next requires: cells, implementsProtocol, INext, lazyValues, publicCollectionSlots,
  //@ publicMethod, rest, seq
  function next(value) {
    if (value == null) return null;
    const method = implementsProtocol(INext, value) ? publicMethod(value, "INext/-next", 1) : null;
    if (method) return method.call(value, value);
    if (lazyValues.has(value)) { seq(value); return next(lazyValues.get(value).value); }
    const cell = cells.get(value);
    if (cell?.kind === "list") return cell.count === 1 || cell.count === 0 && !publicCollectionSlots.has(value) ? null : cell.tail;
    if (cell?.kind === "repeat") return cell.state?.following ?? (cell.remaining === -1 ? value : cell.remaining > 1 ? cell.tail : null);
    return seq(rest(value));
  }

  //@unit seqToMap requires: first, into, mapFromArray, next, seq, toArray
  function seqToMap(value) {
    // Upstream first probes next, then realizes exactly this argument sequence.
    // A singleton is its value (including nil); an odd trailing value is conjoined.
    if (next(value) === null) return seq(value) === null ? mapFromArray([]) : first(value);
    const values = toArray(value), trailing = values.length % 2 ? values.pop() : null;
    const base = mapFromArray(values, "array-map");
    return trailing == null ? base : into(base, trailing);
  }

  //@unit reduced requires: Reduced, reducedValues
  function reduced(value) {
    const marker = Object.create(Reduced.prototype);
    reducedValues.set(marker, value);
    return marker;
  }

  //@unit isReduced requires: reducedValues
  function isReduced(value) { return reducedValues.has(value); }
  //@unit ensureReduced requires: isReduced, reduced
  function ensureReduced(value) { return isReduced(value) ? value : reduced(value); }
  //@unit unreduced requires: isReduced, reducedValues
  function unreduced(value) { return isReduced(value) ? reducedValues.get(value) : value; }
  //@unit deref requires: derefProtocol
  function deref(value) { return derefProtocol(value); }
  //@unit derefProtocol requires: atoms, builtin, deferredValue, delays, isReduced, reducedValues,
  //@ vars, volatiles
  function derefProtocol(value) {
    const method = builtin(value, "IDeref/-deref", 1);
    if (method) return method.call(value, value);
    if (delays.has(value)) return deferredValue(delays.get(value));
    if (vars.has(value)) return vars.get(value).get.call(null);
    if (atoms.has(value) || volatiles.has(value)) return value.state;
    if (!isReduced(value)) throw new Error("value does not support deref");
    return reducedValues.get(value);
  }

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

  //@unit isIdent requires: keywords, symbols
  const isIdent = x => keywords.has(x) || symbols.has(x);
  //@unit isCollectionValue requires: cells, directMethod, lazyValues, maps, queues, sets, vectors,
  //@ views
  const isCollectionValue = x => !!directMethod(x, "ICollection", 0) || vectors.has(x) || cells.has(x) || views.has(x) || queues.has(x) || lazyValues.has(x) || maps.has(x) || sets.has(x);
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
  //@unit isKeyword requires: keywords
  const isKeyword = x => keywords.has(x);
  //@unit isSymbol requires: symbols
  const isSymbol = x => symbols.has(x);
  //@unit isSimpleIdent requires: isIdent, namespace
  const isSimpleIdent = x => isIdent(x) && namespace(x) == null;
  //@unit isQualifiedIdent requires: isIdent, namespace
  const isQualifiedIdent = x => isIdent(x) && namespace(x) != null;
  //@unit isSimpleKeyword requires: keywords, namespace
  const isSimpleKeyword = x => keywords.has(x) && namespace(x) == null;
  //@unit isQualifiedKeyword requires: keywords, namespace
  const isQualifiedKeyword = x => keywords.has(x) && namespace(x) != null;
  //@unit isSimpleSymbol requires: namespace, symbols
  const isSimpleSymbol = x => symbols.has(x) && namespace(x) == null;
  //@unit isQualifiedSymbol requires: namespace, symbols
  const isQualifiedSymbol = x => symbols.has(x) && namespace(x) != null;
  //@unit keywordIdentical requires: equalPair, keywords
  const keywordIdentical = (x, y) => x === y || (keywords.has(x) && keywords.has(y) && equalPair(x, y));
  //@unit symbolIdentical requires: equalPair, symbols
  const symbolIdentical = (x, y) => x === y || (symbols.has(x) && symbols.has(y) && equalPair(x, y));
  //@unit isFn requires: Fn, satisfies
  const isFn = x => typeof x === "function" || satisfies(Fn, x);
  //@unit isIFn requires: IFn, isFn, satisfies
  const isIFn = x => isFn(x) || satisfies(IFn, x);
  //@unit isArray
  const isArray = x => Array.isArray(x);
  //@unit isObject requires: isCollectionValue, isIdent, reducedValues, uuids, vars
  const isObject = x => x != null && !isCollectionValue(x) && !isIdent(x) && !uuids.has(x) && !vars.has(x) && !reducedValues.has(x) && x.constructor === Object;
  //@unit isColl requires: ICollection, satisfies
  const isColl = x => x != null && satisfies(ICollection, x);
  //@unit isSeq requires: ISeq, satisfies
  const isSeq = x => x != null && satisfies(ISeq, x);
  //@unit isList requires: IList, satisfies
  const isList = x => satisfies(IList, x);
  //@unit isSeqable requires: ISeqable, satisfies
  const isSeqable = x => x == null || satisfies(ISeqable, x) || Array.isArray(x) || typeof x === "string" || x[Symbol.iterator] != null;
  //@unit isSequential requires: ISequential, satisfies
  const isSequential = x => satisfies(ISequential, x);
  //@unit isVector requires: IVector, satisfies
  const isVector = x => satisfies(IVector, x);
  //@unit isMap requires: IMap, satisfies
  const isMap = x => x != null && satisfies(IMap, x);
  //@unit isSet requires: ISet, satisfies
  const isSet = x => x != null && satisfies(ISet, x);
  //@unit isAssociative requires: IAssociative, satisfies
  const isAssociative = x => satisfies(IAssociative, x);
  //@unit isIFind requires: IFind, satisfies
  const isIFind = x => satisfies(IFind, x);
  //@unit isCounted requires: ICounted, satisfies
  const isCounted = x => satisfies(ICounted, x);
  //@unit isIndexed requires: IIndexed, satisfies
  const isIndexed = x => satisfies(IIndexed, x);
  //@unit isReduceable requires: IReduce, satisfies
  const isReduceable = x => satisfies(IReduce, x);
  //@unit isReversible requires: IReversible, satisfies
  const isReversible = x => satisfies(IReversible, x);
  //@unit isSorted requires: ISorted, satisfies
  const isSorted = x => satisfies(ISorted, x);
  //@unit isRecord requires: IRecord, satisfies
  const isRecord = x => satisfies(IRecord, x);
  //@unit isMapEntry requires: IMapEntry, implementsProtocol
  const isMapEntry = x => implementsProtocol(IMapEntry, x);
  //@unit isChunkedSeq requires: IChunkedSeq, implementsProtocol
  const isChunkedSeq = x => implementsProtocol(IChunkedSeq, x);
  //@unit isEmpty requires: countProtocol, ICounted, satisfies, seq
  const isEmpty = x => x == null || (satisfies(ICounted, x) ? countProtocol(x) === 0 : seq(x) === null);

  //@unit reduce requires: builtinCoreImplementation, cells, chunks, deref, first, IIterable,
  //@ invalidArity, IReduce, isReduced, iterator, iteratorProtocol, lazyValues, next,
  //@ publicCollectionSlots, publicMethod, satisfies, seq, vectors, views
  function reduce(rf, init, collection) {
    const arity = arguments.length;
    if (arity !== 2 && arity !== 3) return invalidArity(arity);
    const source = arity === 3 ? collection : init;
    const method = publicMethod(source, "IReduce/-reduce", arity, true);
    if (method) return arity === 3 ? method.call(source, source, rf, init) : method.call(source, source, rf);
    if (!builtinCoreImplementation(IReduce, source) && !Array.isArray(source) && typeof source !== "string" && satisfies(IIterable, source)) {
      const iter = iteratorProtocol(source);
      if (arity === 2) { if (!iter.hasNext()) return rf.call(null); init = iter.next(); }
      while (iter.hasNext()) {
        init = rf.call(null, init, iter.next());
        if (isReduced(init)) return deref(init);
      }
      return init;
    }
    const chunkedSlots = publicCollectionSlots.get(source);
    if (chunkedSlots?.arr) {
      const iter = iterator(source);
      if (arity === 2) { if (!iter.hasNext()) return rf.call(null); init = iter.next(); }
      while (iter.hasNext()) { init = rf.call(null, init, iter.next()); if (isReduced(init)) return deref(init); }
      return init;
    }
    if (chunkedSlots?.cnt != null && vectors.has(source) && arity === 2) return reduce(rf, vectors.get(source));
    if (chunkedSlots?.vec) {
      const values = vectors.get(chunkedSlots.vec).slice(chunkedSlots.i + chunkedSlots.off);
      return arity === 3 ? reduce(rf, init, values) : reduce(rf, values);
    }
    const producer = cells.get(source);
    if (producer?.integer && arity === 2) {
      if (producer.count === 0) return rf.call(null);
      init = producer.head;
      for (let i = 1; i < producer.count; i++) {
        init = rf.call(null, init, producer.head + i * producer.step);
        if (isReduced(init)) return deref(init);
      }
      return init;
    }
    if (producer?.kind === "repeat") {
      const n = producer.remaining, item = producer.head;
      let i = arity === 2 ? 1 : 0;
      if (arity === 2) init = item;
      while (n === -1 || i++ < n) { init = rf.call(null, init, item); if (isReduced(init)) return deref(init); }
      return init;
    }
    if (producer?.kind === "iterate") {
      let value = first(source);
      if (arity === 2) { init = value; value = producer.fn.call(null, value); }
      while (true) { init = rf.call(null, init, value); if (isReduced(init)) return deref(init); value = producer.fn.call(null, value); }
    }
    if (producer?.kind === "cycle") {
      let current = producer.currentValue();
      if (arity === 2) { init = first(current); current = next(current) ?? producer.all; }
      while (true) { init = rf.call(null, init, first(current)); if (isReduced(init)) return deref(init); current = next(current) ?? producer.all; }
    }
    if (chunks.has(source) && !chunks.get(source).integer) {
      const c = chunks.get(source); let i = c.offset;
      if (arity === 2) init = c.array[i++];
      for (; i < c.array.length; i++) { init = rf.call(null, init, c.array[i]); if (isReduced(init)) return deref(init); }
      return init;
    }
    const sequenceReduction = cells.has(source) || lazyValues.has(source) || (!builtinCoreImplementation(IReduce, source) && !Array.isArray(source) && typeof source !== "string");
    if (arity === 2 && sequenceReduction) {
      const current = seq(source);
      return current == null ? rf.call(null) : reduce(rf, first(current), next(current));
    }
    if (sequenceReduction) {
      for (let current = seq(source); current != null; current = next(current)) {
        init = rf.call(null, init, first(current));
        if (isReduced(init)) return deref(init);
      }
      return init;
    }
    let initialized = arity === 3, result = init;
    let current = seq(initialized ? collection : init);
    let slice = views.get(current), offset = slice?.offset;
    // Traverse immutable storage directly. No recursive calls, per-step seq
    // wrappers, captured closures or persistent traversal caches are needed.
    while (current !== null) {
      const cell = cells.get(current);
      const item = cell ? cell.head : slice ? slice.storage[offset] : first(current);
      if (!initialized) {
        result = item;
        initialized = true;
      } else {
        // Read .call on every invocation, after the source/arguments have been
        // evaluated. Callable data and user functions share this ordinary ABI.
        result = rf.call(null, result, item);
        if (isReduced(result)) return deref(result);
      }
      if (cell || !slice) {
        current = cell ? seq(cell.tail) : next(current);
        slice = views.get(current);
        offset = slice?.offset;
      } else if (++offset >= slice.storage.length) {
        current = seq(slice.tail);
        slice = views.get(current); offset = slice?.offset;
      }
    }
    return initialized ? result : rf.call(null);
  }

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

  //@unit reduceKv requires: deref, isReduced, keyProtocol, mapEntries, mapTraversal, publicMethod,
  //@ reduce, valProtocol, vectors
  function reduceKv(rf, init, collection) {
    const method = publicMethod(collection, "IKVReduce/-kv-reduce", 3);
    if (method) return method.call(collection, collection, rf, init);
    if (collection == null) return init;
    // MapEntry has vector storage, but no IKVReduce implementation. Its
    // elements must go through the public IMapEntry sequence fallback.
    const entries = mapTraversal(collection)?.map(entry => [entry.key, entry.value]) ?? (mapEntries.has(collection) ? undefined : vectors.get(collection)?.map((value, key) => [key, value]));
    if (!entries) return reduce((result, entry) => rf.call(null, result, keyProtocol(entry), valProtocol(entry)), init, collection);
    for (const [key, value] of entries) {
      init = rf.call(null, init, key, value);
      if (isReduced(init)) return deref(init);
    }
    return init;
  }
  //@unit subvecSources
  const subvecSources = new WeakMap();
  //@unit subvec requires: count, isVector, nth, subvecSources, SubvecValue, vectorFromArray
  function subvec(value, start, end = count(value)) {
    if (start == null || end == null) throw new Error("Assert failed: subvec indices cannot be nil");
    start = start | 0; end = end | 0;
    if (start < 0 || end < start || end > count(value)) throw new Error("Index out of bounds");
    if (!isVector(value)) throw new Error("subvec requires a vector");
    const result = vectorFromArray(Array.from({length: Math.ceil(end - start)}, (_, i) => nth(value, start + i)));
    Object.setPrototypeOf(result, SubvecValue.prototype);
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
  //@unit RangedIterator requires: vectors
  class RangedIterator {
    constructor(i, base, arr, v, start, end) {
      Object.assign(this, {i, base, arr, v, start, end});
    }
    hasNext() { return this.i < this.end; }
    next() {
      if (this.i - this.base === 32) {
        this.arr = vectors.get(this.v).slice(this.i, this.i + 32);
        this.base += 32;
      }
      const value = this.arr[this.i & 31]; this.i++; return value;
    }
  }
  //@unit rangedIterator requires: count, RangedIterator, vectors
  function rangedIterator(value, start, end) {
    const base = start - start % 32;
    return new RangedIterator(start, base, start < count(value) ? vectors.get(value).slice(base, base + 32) : null, value, start, end);
  }
  //@unit seqIter requires: iteratorInit, SeqIter
  function seqIter(value) { return new SeqIter(iteratorInit, value); }
  //@unit iterator requires: builtin, count, iteratorInit, mapEntry, publicCollectionSlots,
  //@ rangedIterator, SeqIter, subvecSources, vectors
  function iterator(value) {
    const method = builtin(value, "IIterable/-iterator", 1);
    if (method) return method.call(value, value);
    const slots = publicCollectionSlots.get(value);
    if (slots?.arr) {
      let i = 0;
      return {hasNext: () => i < slots.cnt * 2, next() {
        if (i >= slots.cnt * 2) throw new Error("No such element");
        const entry = mapEntry(slots.arr[i], slots.arr[i + 1]); i += 2; return entry;
      }, remove() { return new Error("Unsupported operation"); }};
    }
    const source = subvecSources.get(value) ?? value;
    return vectors.has(source) ? rangedIterator(value, 0, count(value)) : new SeqIter(iteratorInit, value);
  }
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

  //@unit conj requires: assoc, builtin, cells, cons, copyEntries, emptyList, lazyValues, makeSet,
  //@ makeVector, mapConj, maps, meta, PersistentHashSetValue, PersistentQueueValue, preserveMeta,
  //@ publicCollectionSlots, putCollectionEntry, queueFromArray, queues, setKinds, sets,
  //@ sortedComparators, subvecSources, SubvecValue, treeSetConstructor, vector, vectors, views
  function conj(...args) {
    if (args.length === 0) return vector();
    let result = args[0];
    for (let i = 1; i < args.length; i++) {
      const previous = result;
      const custom = result == null ? null : builtin(result, "ICollection/-conj", 2);
      if (custom) { result = custom.call(result, result, args[i]); continue; }
      const items = vectors.get(result);
      if (queues.has(result)) {
        const q = publicCollectionSlots.get(result);
        result = q ? PersistentQueueValue(meta(result), q.count + 1, q.front != null ? q.front : conj(null, args[i]), q.front != null ? conj(q.rear ?? vector(), args[i]) : vector(), null)
          : queueFromArray([...queues.get(result), args[i]]);
      }
      else if (items) {
        const slots = publicCollectionSlots.get(result);
        result = slots && subvecSources.has(result) ? SubvecValue(meta(result), assoc(subvecSources.get(result), slots.end, args[i]), slots.start, slots.end + 1, null) : makeVector([...items, args[i]]);
      }
      else if (maps.has(result)) result = mapConj(result, args[i]);
      else if (sets.has(result) && publicCollectionSlots.has(result)) {
        const slots = publicCollectionSlots.get(result), source = slots["tree-map"] ?? slots["hash-map"], updated = assoc(source, args[i], null);
        result = updated === source ? result : slots["tree-map"] ? treeSetConstructor(meta(result), updated, null) : PersistentHashSetValue(meta(result), updated, null);
      }
      else if (sets.has(result)) {
        const entries = copyEntries(sets.get(result));
        putCollectionEntry(result, entries, args[i], undefined, true);
        if (entries.length === sets.get(result).length) continue;
        result = sortedComparators.has(previous) ? makeSet(entries, "sorted-set", sortedComparators.get(previous)) : makeSet(entries, setKinds.get(previous) === "hash-map" || entries.length > 8 ? "hash-map" : "array-map");
      }
      else if (result == null) result = cons(args[i], emptyList, "list");
      else if (cells.has(result) || views.has(result) || lazyValues.has(result))
        result = cons(args[i], result, cells.get(result)?.kind === "list" ? "list" : "cons");
      else throw new Error("conj not supported on this type");
      if (vectors.has(previous) || maps.has(previous) || sets.has(previous) || queues.has(previous) || cells.get(previous)?.kind === "list") result = preserveMeta(previous, result);
    }
    return result;
  }

  //@unit assoc requires: builtin, contains, copyEntries, makeVector, mapFromArray, maps, meta,
  //@ outOfBounds, preserveMeta, publicCollectionSlots, putCollectionEntry, subvecSources,
  //@ SubvecValue, updatedMap, vectors, withMeta
  function assoc(value, key, item, ...pairs) {
    const method = value == null ? null : builtin(value, "IAssociative/-assoc", 3);
    if (method) {
      let result = method.call(value, value, key, item);
      return pairs.length ? assoc(result, ...pairs) : result;
    }
    if (vectors.has(value) && subvecSources.has(value) && publicCollectionSlots.has(value)) {
      const slots = publicCollectionSlots.get(value);
      if (key < 0 || slots.start + key > slots.end) return outOfBounds();
      let result = SubvecValue(meta(value), assoc(subvecSources.get(value), slots.start + key, item), slots.start, Math.max(slots.end, slots.start + key + 1), null);
      return pairs.length ? assoc(result, ...pairs) : result;
    }
    const mapSlots = publicCollectionSlots.get(value);
    if (maps.has(value) && mapSlots?.arr && mapSlots.cnt >= 8 && !contains(value, key)) {
      const normalized = mapFromArray(Array.from({length: mapSlots.cnt * 2}, (_, i) => mapSlots.arr[i]), "hash-map");
      return withMeta(assoc(normalized, key, item, ...pairs), meta(value));
    }
    if (value == null || maps.has(value)) {
      const entries = value == null ? [] : copyEntries(maps.get(value));
      putCollectionEntry(value, entries, key, item);
      let result = updatedMap(value, entries);
      for (let i = 0; i < pairs.length; i += 2)
        result = assoc(result, pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null);
      return result;
    }
    const original = vectors.get(value);
    if (!original) throw new Error("assoc not supported on this type");
    const items = original.slice();
    const assign = (index, value) => {
      if (typeof index !== "number") throw new Error("Vector key must be a number");
      if (index >= 0 && index < items.length) items[Math.trunc(index)] = value;
      else if (index === items.length) items.push(value);
      else outOfBounds();
    };
    // The fixed arity follows JS argument binding, including missing item.
    assign(key, item);
    for (let i = 0; i < pairs.length; i += 2)
      assign(pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null);
    return preserveMeta(value, makeVector(items));
  }

  //@unit mapPrototype requires: contains, es6EntriesIterator, es6Iterator, get, invalidArity, keys,
  //@ lookup, maps, objectEquiv, print, vals
  const mapPrototype = Object.freeze({
    equiv(other) { return objectEquiv(this, other); },
    has(key) { return contains(this, key); },
    get(key, notFound = null) { return get(this, key, notFound); },
    keys() { return es6Iterator(keys(this)); },
    values() { return es6Iterator(vals(this)); },
    entries() { return es6EntriesIterator(this); },
    [Symbol.iterator]() { return es6Iterator(this); },
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    toString() { return print(this, true); },
    call(_self, key, notFound) {
      if (!maps.has(this)) throw new Error("Expected map");
      switch (arguments.length) {
        case 2: return lookup(this, key, null);
        case 3: return lookup(this, key, notFound);
        default: return invalidArity(arguments.length - 1);
      }
    }
  });

  //@unit PersistentArrayMapValue.type effect: PersistentArrayMapValue requires: builtinType,
  //@ mapPrototype, PersistentArrayMapValue
  builtinType(PersistentArrayMapValue, mapPrototype);
  //@unit PersistentHashMapValue.type effect: PersistentHashMapValue requires: builtinType,
  //@ mapPrototype, PersistentHashMapValue
  builtinType(PersistentHashMapValue, mapPrototype);
  //@unit treeMapConstructor.type effect: treeMapConstructor requires: builtinType, mapPrototype,
  //@ treeMapConstructor
  builtinType(treeMapConstructor, mapPrototype);
  //@unit hashRoots
  // Routing nodes preserve observable key tests and rehashes at trie splits.
  // Immutable nodes can be shared by old collections and transient copies.
  // Transient capacity affects expansion timing.
  const hashRoots = new WeakMap();
  //@unit hashTraversals
  const hashTraversals = new WeakMap();
  //@unit copyEntries requires: hashRoots
  function copyEntries(entries) {
    const copy = entries.slice();
    if (hashRoots.has(entries)) hashRoots.set(copy, hashRoots.get(entries));
    return copy;
  }
  //@unit bitmapNode
  function bitmapNode(slots = [], edit = null, capacity = slots.filter(Boolean).length) {
    return {kind: "bitmap", slots, edit, capacity};
  }
  //@unit hashNodeAssoc requires: bitmapNode, equalPair, hash
  function hashNodeAssoc(node, shift, keyHash, entry, edit, result, effects = true) {
    node ??= bitmapNode();
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
          entries[index] = Object.freeze({...entry, key: result.old.key, hash: result.old.hash});
          result.entry = entries[index];
        }
        return {...node, entries};
      }
      const slots = []; slots[(node.hash >>> shift) & 31] = {node};
      return hashNodeAssoc(bitmapNode(slots, edit, edit ? 2 : 1), shift, keyHash, entry, edit, result, effects);
    }
    const index = (keyHash >>> shift) & 31, slot = node.slots[index];
    const slots = node.slots.slice(), n = slots.filter(Boolean).length;
    if (node.kind === "array") {
      slots[index] = {node: hashNodeAssoc(slot?.node, shift + 5, keyHash, entry, edit, result, effects)};
      return {...node, slots};
    }
    if (!slot) {
      if (n >= 16 && (!edit || n >= node.capacity)) {
        // Canonical expansion visits retained leaves in ascending branch order.
        slots[index] = {node: hashNodeAssoc(null, shift + 5, keyHash, entry, edit, result, effects)};
        for (let i = 0; i < node.slots.length; ++i) {
          const old = node.slots[i];
          if (old?.entry) slots[i] = {node: hashNodeAssoc(null, shift + 5, effects ? hash(old.entry.key) : old.entry.hash, old.entry, edit, {}, effects)};
        }
        return {kind: "array", slots};
      }
      slots[index] = {entry};
      const capacity = !edit ? n + 1 : n < node.capacity ? node.edit === edit ? node.capacity : n + 1 : n + 4;
      return bitmapNode(slots, edit, capacity);
    }
    if (slot.node) slots[index] = {node: hashNodeAssoc(slot.node, shift + 5, keyHash, entry, edit, result, effects)};
    else if (effects ? equalPair(entry.key, slot.entry.key) : entry.key === slot.entry.key) {
      result.old = slot.entry;
      result.entry = Object.freeze({...entry, key: slot.entry.key, hash: slot.entry.hash});
      slots[index] = {entry: result.entry};
    } else {
      const oldHash = effects ? hash(slot.entry.key) : slot.entry.hash;
      let child;
      if (oldHash === keyHash) child = {kind: "collision", hash: keyHash, entries: [slot.entry, entry]};
      else {
        child = hashNodeAssoc(null, shift + 5, oldHash, slot.entry, edit, {}, effects);
        child = hashNodeAssoc(child, shift + 5, keyHash, entry, edit, {}, effects);
      }
      slots[index] = {node: child};
    }
    return bitmapNode(slots, edit, edit && node.edit !== edit ? n + 1 : node.capacity);
  }
  //@unit hashNodeFind requires: equalPair
  function hashNodeFind(node, shift, keyHash, key) {
    if (!node) return null;
    if (node.kind === "collision") return node.entries.find(entry => equalPair(key, entry.key)) ?? null;
    const slot = node.slots[(keyHash >>> shift) & 31];
    return !slot ? null : slot.node ? hashNodeFind(slot.node, shift + 5, keyHash, key) : equalPair(key, slot.entry.key) ? slot.entry : null;
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
    const slots = node.slots.slice(), n = slots.filter(Boolean).length;
    if (slot.node) {
      const child = hashNodeRemove(slot.node, shift + 5, keyHash, removed, edit);
      if (child === slot.node) return node;
      slots[index] = child ? {node: child} : undefined;
    } else {
      if (slot.entry !== removed) return node;
      slots[index] = undefined;
    }
    if (!slots.some(Boolean)) return null;
    if (node.kind === "array") return n <= 8 && !slots[index] ? bitmapNode(slots, edit, edit ? 2 * (n - 1) : n - 1) : {...node, slots};
    return bitmapNode(slots, edit, edit && node.edit !== edit ? n + 1 : node.capacity);
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
  //@unit mapTraversal requires: collectionTraversal, mapKinds, maps
  function mapTraversal(value) {
    return collectionTraversal(maps.get(value), mapKinds.get(value));
  }
  //@unit setTraversal requires: collectionTraversal, setKinds, sets
  function setTraversal(value) {
    return collectionTraversal(sets.get(value), setKinds.get(value));
  }
  //@unit collectionTraversal requires: ensureHashRoot, hashTraversals
  function collectionTraversal(entries, kind) {
    // Array maps, positional constants, and sorted collections retain their
    // storage order. Hash-map promotion stays observable after removals.
    if (kind !== "hash-map") return entries;
    if (!hashTraversals.has(entries)) {
      const ordered = entries.filter(entry => entry.key == null);
      const visit = node => {
        if (!node) return;
        if (node.kind === "collision") { ordered.push(...node.entries); return; }
        for (const slot of node.slots) {
          if (slot?.node) visit(slot.node);
          else if (slot?.entry) ordered.push(slot.entry);
        }
      };
      // Walk retained routing nodes without hashing or comparing keys again.
      visit(ensureHashRoot(entries));
      hashTraversals.set(entries, Object.freeze(ordered));
    }
    return hashTraversals.get(entries);
  }
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

  //@unit makeMap requires: compareKeys, mapKinds, maps, PersistentArrayMapValue,
  //@ PersistentHashMapValue, sortedComparators, treeMapConstructor
  function makeMap(entries, kind = entries.length <= 8 ? "array-map" : "hash-map", comparator = compareKeys) {
    const value = Object.create((kind === "hash-map" ? PersistentHashMapValue : kind === "sorted-map" ? treeMapConstructor : PersistentArrayMapValue).prototype);
    maps.set(value, Object.freeze(entries));
    mapKinds.set(value, kind);
    if (kind === "sorted-map") sortedComparators.set(value, comparator);
    return value;
  }

  //@unit entryIndex requires: equalPair
  function entryIndex(entries, key, keyHash, kind) {
    // Canonical array maps compare keys without a hash guard. Keyword equality uses
    // fqn, while hashing uses namespace/name; unusual equal keywords can have
    // different hashes and therefore remain distinct in hash maps.
    if (kind === "array-map") return entries.findIndex(entry =>
      key == null ? entry.key == null : typeof key === "string" || typeof key === "number"
        ? key === entry.key : equalPair(key, entry.key));
    return entries.findIndex(entry => entry.hash === keyHash && equalPair(key, entry.key));
  }

  //@unit putEntry requires: ensureHashRoot, entryIndex, hash, hashNodeAssoc, hashRoots
  // Builders mutate only fresh unpublished arrays. Entry cells and retained
  // equivalent keys are shared by subsequent immutable versions.
  function putEntry(entries, key, value, kind, edit = null) {
    if (kind === "array-map") {
      const index = entryIndex(entries, key, undefined, kind), retained = index < 0 ? key : entries[index].key;
      let cachedHash;
      const entry = Object.freeze({key: retained, value, get hash() {
        return cachedHash === undefined ? cachedHash = hash(retained) : cachedHash;
      }});
      if (index < 0) entries.push(entry); else entries[index] = entry;
      return;
    }
    const keyHash = key == null ? 0 : hash(key), entry = Object.freeze({key, value, hash: keyHash});
    if (key == null) {
      const index = entries.findIndex(old => old.key == null);
      if (index < 0) entries.push(entry); else entries[index] = Object.freeze({...entry, key: entries[index].key});
      return;
    }
    const result = {}, root = hashNodeAssoc(ensureHashRoot(entries), 0, keyHash, entry, edit, result);
    if (result.old) entries[entries.indexOf(result.old)] = result.entry;
    else entries.push(entry);
    hashRoots.set(entries, root);
  }

  //@unit mapFromArray requires: makeMap, putEntry, str
  function mapFromArray(pairs, kind = pairs.length <= 16 ? "array-map" : "hash-map") {
    if (pairs.length % 2) throw new Error("No value supplied for key: " + str(pairs[pairs.length - 1]));
    const entries = [];
    const edit = kind === "hash-map" ? {} : null;
    for (let i = 0; i < pairs.length; i += 2) putEntry(entries, pairs[i], pairs[i + 1], kind, edit);
    return makeMap(entries, kind);
  }

  //@unit hashMap requires: mapFromArray
  function hashMap(...pairs) { return mapFromArray(pairs, "hash-map"); }
  //@unit arrayMap requires: mapFromArray
  function arrayMap(...pairs) { return mapFromArray(pairs, "array-map"); }

  //@unit mapFromArrays requires: mapFromArray
  // Source hash-map literals evaluate all reader-ordered keys before values.
  // This emitter helper preserves the ordinary runtime constructor semantics.
  function mapFromArrays(keys, values) {
    const pairs = [];
    for (let i = 0; i < keys.length; i++) pairs.push(keys[i], values[i]);
    return mapFromArray(pairs, "hash-map");
  }

  //@unit constantMapFromArray requires: PersistentArrayMapValue
  // The analyzer has checked distinct source constants. Character/string and
  // numeric reader categories can lower to equal JS values, so literal storage
  // retains positional entries and the array map's first-match lookup.
  function constantMapFromArray(pairs) {
    return PersistentArrayMapValue(null, pairs.length / 2, pairs, null);
  }

  //@unit constantSetFromArray requires: constantMapFromArray, PersistentHashSetValue
  function constantSetFromArray(items) {
    const pairs = [];
    for (const item of items) pairs.push(item, null);
    return PersistentHashSetValue(null, constantMapFromArray(pairs), null);
  }

  //@unit dissoc requires: builtin, collectionEntryIndex, copyEntries, equalPair, hashRoots,
  //@ keyword, mapKinds, maps, recordTypes, removeHashEntry, updatedMap
  function dissoc(value, ...keys) {
    if (value != null && builtin(value, "IMap/-dissoc", 2)) return keys.reduce((result, key) => {
      const method = result == null ? null : builtin(result, "IMap/-dissoc", 2);
      return method ? method.call(result, result, key) : dissoc(result, key);
    }, value);
    if (arguments.length === 0) return null;
    if (keys.length === 0 || value == null) return value;
    const original = maps.get(value);
    if (!original) throw new Error("dissoc not supported on this type");
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

  //@unit mapConj requires: assoc, first, isVector, next, nthProtocol, seq
  function mapConj(value, member) {
    if (member == null) return value;
    if (isVector(member)) return assoc(value, nthProtocol(member, 0), nthProtocol(member, 1));
    let result = value;
    for (let current = seq(member); current !== null; current = next(current)) {
      const pair = first(current);
      if (!isVector(pair)) throw new Error("Map conj requires vector entries");
      result = assoc(result, nthProtocol(pair, 0), nthProtocol(pair, 1));
    }
    return result;
  }

  //@unit setPrototype requires: contains, es6EntriesIterator, es6Iterator, invalidArity, lookup,
  //@ objectEquiv, print, sets
  const setPrototype = Object.freeze({
    equiv(other) { return objectEquiv(this, other); },
    has(key) { return contains(this, key); },
    keys() { return es6Iterator(this); },
    values() { return es6Iterator(this); },
    entries() { return es6EntriesIterator(this, true); },
    [Symbol.iterator]() { return es6Iterator(this); },
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    toString() { return print(this, true); },
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
  //@ PersistentHashSetValue, setPrototype
  builtinType(PersistentHashSetValue, setPrototype);
  //@unit treeSetConstructor.type effect: treeSetConstructor requires: builtinType, setPrototype,
  //@ treeSetConstructor
  builtinType(treeSetConstructor, setPrototype);
  //@unit makeSet requires: compareKeys, PersistentHashSetValue, setKinds, sets, sortedComparators,
  //@ treeSetConstructor
  function makeSet(entries, kind = entries.length > 8 ? "hash-map" : "array-map", comparator = compareKeys) {
    const value = Object.create((kind === "sorted-set" ? treeSetConstructor : PersistentHashSetValue).prototype);
    sets.set(value, Object.freeze(entries));
    setKinds.set(value, kind);
    if (kind === "sorted-set") sortedComparators.set(value, comparator);
    return value;
  }

  //@unit putMember requires: entryIndex, hashRoots, promoteEntries, putEntry
  function putMember(entries, key, edit = null) {
    if (!hashRoots.has(entries)) {
      if (entries.length < 8 || entryIndex(entries, key, undefined, "array-map") >= 0) {
        putEntry(entries, key, null, "array-map"); return;
      }
      const promoted = promoteEntries(entries, edit ?? {});
      entries.splice(0, entries.length, ...promoted);
      hashRoots.set(entries, hashRoots.get(promoted));
    }
    putEntry(entries, key, null, "hash-map", edit);
  }

  //@unit setFromArray requires: makeSet, putMember
  function setFromArray(items) {
    const entries = [];
    const edit = {};
    for (const item of items) putMember(entries, item, edit);
    return makeSet(entries);
  }

  //@unit literalSetFromArray requires: makeSet, putMember, str
  function literalSetFromArray(items) {
    const entries = [], edit = {};
    for (const item of items) {
      const before = entries.length;
      putMember(entries, item, edit);
      if (entries.length === before) throw new Error("Duplicate key: " + str(item));
    }
    return makeSet(entries);
  }

  //@unit hashSet requires: setFromArray
  function hashSet(...items) { return setFromArray(items); }

  //@unit disj requires: builtin, collectionEntryIndex, copyEntries, dissoc, hashRoots, makeSet,
  //@ meta, PersistentHashSetValue, preserveMeta, publicCollectionSlots, removeHashEntry, setKinds,
  //@ sets, sortedComparators, treeSetConstructor
  function disj(value, ...keys) {
    if (value != null && builtin(value, "ISet/-disjoin", 2)) return keys.reduce((result, key) => builtin(result, "ISet/-disjoin", 2).call(result, result, key), value);
    if (arguments.length === 0) return null;
    if (keys.length === 0) return value;
    if (value == null) return null;
    const original = sets.get(value);
    if (!original) throw new Error("disj not supported on this type");
    const treeMap = publicCollectionSlots.get(value)?.["tree-map"];
    if (treeMap) return treeSetConstructor(meta(value), dissoc(treeMap, ...keys), null);
    const hashMap = publicCollectionSlots.get(value)?.["hash-map"];
    if (hashMap) return PersistentHashSetValue(meta(value), dissoc(hashMap, ...keys), null);
    const entries = copyEntries(original);
    for (const key of keys) {
      const index = collectionEntryIndex(value, entries, key);
      if (index >= 0) {
        if (hashRoots.has(entries)) removeHashEntry(entries, index); else entries.splice(index, 1);
      }
    }
    if (entries.length === original.length) return value;
    return preserveMeta(value, makeSet(entries, setKinds.get(value), sortedComparators.get(value)));
  }

  //@unit transientPrototype requires: invalidArity, print, transientLookup
  // Transient payloads own a private mutable copy of canonical collection
  // storage. Persistence freezes that storage; no persistent alias can mutate it.
  const transientPrototype = Object.freeze({
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    toString() { return print(this, true); },
    call(_self, key, notFound) {
      if (arguments.length !== 2 && arguments.length !== 3) return invalidArity(arguments.length - 1);
      return transientLookup(this, key, arguments.length === 2 ? null : notFound);
    }
  });
  //@unit TransientVectorType effect requires: builtinType, transientPrototype
  const TransientVectorType = builtinType(function () {}, transientPrototype);
  //@unit TransientArrayMapType effect requires: builtinType, transientPrototype
  const TransientArrayMapType = builtinType(function () {}, transientPrototype);
  //@unit TransientHashMapType effect requires: builtinType, transientPrototype
  const TransientHashMapType = builtinType(function () {}, transientPrototype);
  //@unit TransientHashSetType effect requires: builtinType, transientPrototype
  const TransientHashSetType = builtinType(function () {}, transientPrototype);
  //@unit makeTransient requires: TransientArrayMapType, TransientHashMapType, TransientHashSetType,
  //@ transients, TransientVectorType
  function makeTransient(kind, entries, mapKind) {
    const Type = kind === "vector" ? TransientVectorType : kind === "set" ? TransientHashSetType : mapKind === "array-map" ? TransientArrayMapType : TransientHashMapType;
    const result = Object.create(Type.prototype);
    transients.set(result, {kind, entries, mapKind, editable: true, edit: {}});
    return result;
  }
  //@unit editableTransient requires: transients
  function editableTransient(value, operation) {
    const state = transients.get(value);
    if (!state?.editable) throw new Error(operation === "persistent!" ? "persistent! called twice" : operation + " after persistent!");
    return state;
  }
  //@unit transient requires: asTransientProtocol
  function transient(value) { return asTransientProtocol(value); }
  //@unit asTransient requires: copyEntries, makeTransient, mapKinds, maps, setKinds, sets, vectors
  function asTransient(value) {
    if (vectors.has(value)) return makeTransient("vector", vectors.get(value).slice());
    if (maps.has(value)) return makeTransient("map", copyEntries(maps.get(value)), mapKinds.get(value));
    return makeTransient("set", copyEntries(sets.get(value)), setKinds.get(value));
  }
  //@unit persistentBang requires: persistentBangProtocol
  function persistentBang(value) { return persistentBangProtocol(value); }
  //@unit persistTransient requires: editableTransient, makeMap, makeSet, makeVector
  function persistTransient(value) {
    const state = editableTransient(value, "persistent!");
    state.editable = false;
    if (state.kind === "vector") return makeVector(state.entries);
    if (state.kind === "map") return makeMap(state.entries, state.mapKind);
    return makeSet(state.entries, state.mapKind);
  }
  //@unit transientNth requires: editableTransient, missing, transients, vectorNth
  function transientNth(value, index, fallback) {
    const state = transients.get(value);
    // Pinned CLJS's three-argument -nth returns an out-of-range fallback
    // even after persistence; in-range access still checks ownership.
    if (fallback !== missing && !(index >= 0 && index < state.entries.length)) return fallback;
    editableTransient(value, "nth");
    return vectorNth(state.entries, index, fallback);
  }
  //@unit transientLookup requires: editableTransient, entryIndex, hashEntryIndex, transientNth,
  //@ transients
  function transientLookup(value, key, notFound) {
    const state = transients.get(value);
    // TransientHashMap (and its set wrapper) retains readable lookup after
    // persistence in both pinned versions. Array maps and vectors reject it.
    if (state.mapKind !== "hash-map") editableTransient(value, "lookup");
    if (state.kind === "vector") return typeof key === "number" ? transientNth(value, key, notFound) : notFound;
    const index = state.mapKind === "array-map" ? entryIndex(state.entries, key, undefined, "array-map") : hashEntryIndex(state.entries, key);
    return index < 0 ? notFound : state.kind === "set" ? key : state.entries[index].value;
  }
  //@unit assocTransient requires: editableTransient, entryIndex, makeTransient, outOfBounds,
  //@ putEntry, transients
  function assocTransient(value, key, item, indexed = false) {
    const state = transients.get(value);
    if (state.kind === "vector") {
      if (!indexed && typeof key !== "number") throw new Error("TransientVector's key for assoc! must be a number.");
      editableTransient(value, "assoc!");
      if (key >= 0 && key < state.entries.length) state.entries[key & 0x7fffffff] = item;
      else if (key === state.entries.length) state.entries.push(item);
      else outOfBounds();
    } else {
      editableTransient(value, "assoc!");
      if (state.mapKind === "array-map" && state.entries.length >= 8 && entryIndex(state.entries, key, undefined, "array-map") < 0) {
        const promoted = makeTransient("map", [], "hash-map"), target = transients.get(promoted);
        for (const entry of state.entries) putEntry(target.entries, entry.key, entry.value, "hash-map", target.edit);
        return assocTransient(promoted, key, item);
      }
      putEntry(state.entries, key, item, state.mapKind, state.edit);
    }
    return value;
  }
  //@unit conjTransient requires: assocBangProtocol, editableTransient, first, isMapEntry, isVector,
  //@ keyProtocol, next, putMember, seq, truth, valProtocol
  function conjTransient(value, member) {
    const state = editableTransient(value, "conj!");
    if (state.kind === "vector") { state.entries.push(member); return value; }
    if (state.kind === "set") {
      putMember(state.entries, member, state.edit);
      if (state.entries.length > 8) state.mapKind = "hash-map";
      return value;
    }
    if (isMapEntry(member)) return assocBangProtocol(value, keyProtocol(member), valProtocol(member));
    if (isVector(member)) return assocBangProtocol(value, member.call(null, 0), member.call(null, 1));
    for (let s = seq(member); s !== null && truth(first(s)); s = next(s)) {
      const entry = first(s);
      value = assocBangProtocol(value, keyProtocol(entry), valProtocol(entry));
    }
    return value;
  }
  //@unit removeTransient requires: editableTransient, entryIndex, hashEntryIndex, hashRoots,
  //@ removeHashEntry
  function removeTransient(value, key, operation) {
    const state = editableTransient(value, operation), index = state.mapKind === "array-map" ? entryIndex(state.entries, key, undefined, "array-map") : hashEntryIndex(state.entries, key);
    if (index >= 0) {
      if (state.mapKind === "array-map" && !hashRoots.has(state.entries)) state.entries[index] = state.entries[state.entries.length - 1];
      if (hashRoots.has(state.entries)) removeHashEntry(state.entries, index, state.edit);
      else state.entries.splice(state.mapKind === "array-map" ? state.entries.length - 1 : index, 1);
    }
    return value;
  }
  //@unit popTransient requires: editableTransient
  function popTransient(value) {
    const state = editableTransient(value, "pop!");
    if (!state.entries.length) throw new Error("Can't pop empty vector");
    state.entries.pop(); return value;
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

  //@unit vectorPrototype requires: count, es6Iterator, invalidArity, mapEntries, missing,
  //@ objectEquiv, outOfBounds, print, sequentialIndex, vectorNth, vectors
  const vectorPrototype = Object.freeze({
    equiv(other) { return objectEquiv(this, other); },
    [Symbol.iterator]() { return es6Iterator(this); },
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    toString() { return print(this, true); },
    indexOf(value, start = 0) { return sequentialIndex(this, value, start, false); },
    lastIndexOf(value, start = count(this)) { return sequentialIndex(this, value, start, true); },
    // The pinned fixed method accepts one index and ignores extra arguments.
    call(_self, index) {
      const items = vectors.get(this);
      if (!items) throw new Error("Expected vector");
      if (mapEntries.has(this)) {
        if (arguments.length !== 2 && arguments.length !== 3) return invalidArity(arguments.length - 1);
        if (index === 0 || index === 1) return items[index];
        return arguments.length === 3 ? arguments[2] : outOfBounds();
      }
      if (typeof index !== "number") throw new Error("Key must be integer");
      return vectorNth(items, index, missing);
    }
  });

  //@unit PersistentVectorValue.type effect: PersistentVectorValue requires: builtinType,
  //@ PersistentVectorValue, vectorPrototype
  builtinType(PersistentVectorValue, vectorPrototype);
  //@unit SubvecValue.type effect: SubvecValue requires: builtinType, SubvecValue, vectorPrototype
  builtinType(SubvecValue, vectorPrototype);
  //@unit MapEntryType effect requires: builtinType, mapEntry, vectorPrototype
  const MapEntryType = builtinType(function (key, value) { return mapEntry(key, value); }, vectorPrototype);
  //@unit makeKeyword requires: keywords, KeywordValue
  function makeKeyword(namespace, name, spelling) {
    const value = Object.create(KeywordValue.prototype);
    keywords.set(value, Object.freeze({namespace, name, spelling}));
    return value;
  }

  //@unit freshKeyword requires: makeKeyword
  // Keywords built from data are fresh, as upstream.
  function freshKeyword(namespace, name) {
    return makeKeyword(namespace, name,
      namespace == null ? name : namespace + "/" + name);
  }

  //@unit keyword requires: freshKeyword, internedKeywords
  // Literal construction receives separately owned namespace/name strings
  // and is interned: one keyword per namespace and name.
  function keyword(namespace, name) {
    const key = namespace == null ? "/" + name : namespace.length + "/" + namespace + name;
    let value = internedKeywords.get(key);
    if (!value) internedKeywords.set(key, value = freshKeyword(namespace, name));
    return value;
  }

  //@unit keywordFqn requires: keywords
  // What keyword case compares, or null.
  const keywordFqn = value => keywords.get(value)?.spelling ?? null;

  //@unit makeVector requires: PersistentVectorValue, vectors
  function makeVector(items) {
    const value = Object.create(PersistentVectorValue.prototype);
    vectors.set(value, Object.freeze(items));
    return value;
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

  //@unit keywordCore requires: freshKeyword, invalidArity, keywords, makeKeyword, symbols
  function keywordCore(namespace, name) {
    switch (arguments.length) {
      case 1: {
        if (keywords.has(namespace)) return namespace;
        const sym = symbols.get(namespace);
        if (sym) return makeKeyword(sym.namespace, sym.name, sym.spelling);
        if (typeof namespace !== "string") return null;
        if (namespace === "/") return freshKeyword(null, namespace);
        const parts = namespace.split("/");
        return makeKeyword(parts.length === 2 ? parts[0] : null,
          parts.length === 2 ? parts[1] : parts[0], namespace);
      }
      case 2: {
        const nsKeyword = keywords.get(namespace) || symbols.get(namespace);
        const nameKeyword = keywords.get(name) || symbols.get(name);
        if (nsKeyword) namespace = nsKeyword.name;
        if (nameKeyword) name = nameKeyword.name;
        if ((namespace != null && typeof namespace !== "string") ||
            typeof name !== "string") {
          throw new Error("keyword expects strings or keywords and an optional nil namespace");
        }
        return freshKeyword(namespace, name);
      }
      default: return invalidArity(arguments.length);
    }
  }

  //@unit name requires: builtin, implementsProtocol, INamed, keywords, multiFns, symbols
  function name(value) {
    if (multiFns.has(value)) return name(value.name);
    const method = implementsProtocol(INamed, value) ? builtin(value, "INamed/-name", 1) : null;
    if (method) return method.call(value, value);
    const named = keywords.get(value) || symbols.get(value);
    if (named) return named.name;
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

  //@unit namespace requires: builtin, implementsProtocol, INamed, keywords, multiFns, symbols
  function namespace(value) {
    if (multiFns.has(value)) return namespace(value.name);
    const method = implementsProtocol(INamed, value) ? builtin(value, "INamed/-namespace", 1) : null;
    if (method) return method.call(value, value);
    const named = keywords.get(value) || symbols.get(value);
    if (named) return named.namespace;
    throw new Error("Doesn't support namespace on this type");
  }

  //@unit count requires: accumulatingCount, builtin, cells, chunkBuffers, chunks,
  //@ editableTransient, implementsProtocol, ISeqable, lazyValues, maps, next,
  //@ publicCollectionSlots, publicMethod, queues, sets, subvecSources, transients, vectors, views
  function count(value) {
    const method = value == null ? null : publicMethod(value, "ICounted/-count", 1, true, true);
    if (method) return method.call(value, value);
    const slots = publicCollectionSlots.get(value);
    if (cells.get(value)?.kind === "tree-seq" && slots.cnt < 0) return 1 + count(next(value));
    if (slots && "tree-map" in slots) return count(slots["tree-map"]);
    if (slots && ("cnt" in slots || "count" in slots)) return "cnt" in slots ? slots.cnt : slots.count;
    if (slots?.vec) return count(slots.vec) - slots.i - slots.off;
    if (slots && subvecSources.has(value)) return slots.end - slots.start;
    if (slots && sets.has(value)) return count(slots["hash-map"]);
    if (transients.has(value)) return editableTransient(value, "count").entries.length;
    if (lazyValues.has(value)) return accumulatingCount(value);
    if (chunkBuffers.has(value)) return chunkBuffers.get(value).end;
    if (chunks.has(value)) { const c = chunks.get(value); return c.end - c.offset; }
    if (queues.has(value)) return queues.get(value).length;
    if (Array.isArray(value)) return value.length;
    if (value == null) return 0;
    const items = vectors.get(value);
    if (items) return items.length;
    if (typeof value === "string") return value.length;
    const entries = maps.get(value);
    if (entries) return entries.length;
    const members = sets.get(value);
    if (members) return members.length;
    const cell = cells.get(value);
    if (cell) return "count" in cell ? cell.count : accumulatingCount(value);
    const slice = views.get(value);
    if (slice) return Math.max(0, slice.storage.length - slice.offset) + count(slice.tail);
    if (implementsProtocol(ISeqable, value)) return accumulatingCount(value);
    const nativeCount = builtin(value, "ICounted/-count", 1);
    if (nativeCount) return nativeCount.call(value, value);
    throw new Error("count not supported on this type");
  }

  //@unit accumulatingCount requires: countProtocol, ICounted, next, satisfies, seq
  function accumulatingCount(value) {
    let result = 0;
    for (let current = seq(value); current != null; current = next(current), result++) {
      if (satisfies(ICounted, current)) return result + countProtocol(current);
    }
    return result;
  }

  //@unit vectorNth requires: missing, outOfBounds
  function vectorNth(items, index, notFound) {
    if (index >= 0 && index < items.length) return items[Math.trunc(index)];
    if (notFound === missing) return outOfBounds();
    return notFound;
  }

  //@unit nth requires: cells, chunkNth, chunks, first, IIndexed, implementsProtocol, invalidArity,
  //@ ISeq, ISequential, mapEntries, missing, next, nthProtocol, outOfBounds, publicMethod,
  //@ satisfies, seq, transientNth, transients, vectorNth, vectors, views
  function nth(value, index, notFound) {
    const arity = arguments.length;
    if (arity !== 2 && arity !== 3) return invalidArity(arity);
    if (typeof index !== "number") throw new Error("Index argument to nth must be a number");
    if (value == null) return arity === 2 ? value : notFound;
    const fallback = arity === 2 ? missing : notFound;
    const method = publicMethod(value, "IIndexed/-nth", arity, true, true);
    if (method) return method.call(value, ...arguments);
    if (transients.has(value)) return transientNth(value, index, fallback);
    if (chunks.has(value)) return chunkNth(value, index, fallback);
    if (cells.get(value)?.integer) { const c = cells.get(value); return index >= 0 && index < c.count ? c.head + Math.floor(index) * c.step : fallback === missing ? outOfBounds() : fallback; }
    if (Array.isArray(value)) return index > -1 && index < value.length ? value[Math.trunc(index)] : fallback === missing ? outOfBounds() : fallback;
    const items = vectors.get(value);
    if (items) return mapEntries.has(value) && index !== 0 && index !== 1 ? fallback === missing ? outOfBounds() : fallback : vectorNth(items, index, fallback);
    if (typeof value === "string") {
      if (index > -1 && index < value.length) return value.charAt(index | 0);
      if (fallback === missing) return outOfBounds();
      return fallback;
    }
    if (implementsProtocol(ISeq, value) || implementsProtocol(ISequential, value)) {
      // Lists traverse to an exact zero. Indexed finite views retain the
      // pinned absolute-offset lookup, including fractional/missing slots.
      let current = value, remaining = index;
      if (!views.get(current)?.indexed && remaining < 0)
        return fallback === missing ? outOfBounds() : fallback;
      while (current !== null) {
        const slice = views.get(current);
        if (slice?.indexed) {
          const absolute = slice.offset + remaining;
          if (absolute >= 0 && absolute < slice.storage.length) return slice.storage[absolute];
          break;
        }
        if (remaining === 0) {
          if (seq(current) != null) return first(current);
          break;
        }
        if (satisfies(IIndexed, current)) return fallback === missing ? nthProtocol(current, remaining) : nthProtocol(current, remaining, fallback);
        if (seq(current) == null) break;
        current = next(current);
        remaining--;
      }
      return fallback === missing ? outOfBounds() : fallback;
    }
    const nativeNth = publicMethod(value, "IIndexed/-nth", arity);
    if (nativeNth) return nativeNth.call(value, ...arguments);
    throw new Error("nth not supported on this type");
  }

  //@unit lookup requires: collectionEntryIndex, mapEntries, maps, publicCollectionSlots, sets,
  //@ sortedComparators, transientLookup, transients, vectors
  function lookup(value, key, notFound) {
    const slots = publicCollectionSlots.get(value);
    if (maps.has(value) && slots && "tree" in slots) {
      const comparator = sortedComparators.get(value);
      for (let node = slots.tree; node != null;) {
        const order = comparator.call(null, key, node.key);
        if (order === 0) return node.val;
        node = order < 0 ? node.left : node.right;
      }
      return notFound;
    }
    if (transients.has(value)) return transientLookup(value, key, notFound);
    const members = sets.get(value);
    if (members) {
      const index = collectionEntryIndex(value, members, key);
      return index < 0 ? notFound : members[index].key;
    }
    const entries = maps.get(value);
    if (entries) {
      const index = collectionEntryIndex(value, entries, key);
      return index < 0 ? notFound : entries[index].value;
    }
    const items = vectors.get(value);
    if (items) return mapEntries.has(value) ? key === 0 || key === 1 ? items[key] : notFound : typeof key === "number" && key >= 0 && key < items.length ? items[Math.trunc(key)] : notFound;
    if (typeof value === "string" && key != null && key > -1 && key < value.length) {
      return value.charAt(key | 0);
    }
    // Raw object properties are not lookup entries. Public get handles array
    // indexing separately without giving host objects owned collection storage.
    return notFound;
  }

  //@unit get requires: invalidArity, lookup, publicMethod
  function get(value, key, notFound) {
    const method = value == null ? null : publicMethod(value, "ILookup/-lookup", arguments.length, true);
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

  //@unit contains requires: get, missing, publicMethod, vectors
  function contains(value, key) {
    const method = publicMethod(value, "IAssociative/-contains-key?", 2);
    if (method) return method.call(value, value, key);
    const items = vectors.get(value);
    if (items) return Number.isInteger(key) && key >= 0 && key < items.length;
    return get(value, key, missing) !== missing;
  }

  //@unit every requires: first, next, seq, truth
  function every(predicate, value) {
    for (let current = seq(value); current !== null; current = next(current)) if (!truth(predicate.call(null, first(current)))) return false;
    return true;
  }
  //@unit find requires: collectionEntryIndex, contains, get, isAssociative, mapEntry, maps,
  //@ publicMethod, publicTree, recordTypes, sortedComparators
  function find(value, key) {
    const method = publicMethod(value, "IFind/-find", 2);
    if (method) return method.call(value, value, key);
    const entries = !recordTypes.has(value) && maps.get(value);
    if (entries) {
      const comparator = sortedComparators.get(value);
      if (comparator) {
        for (let node = publicTree(value); node != null;) {
          const order = comparator.call(null, key, node.key);
          if (order === 0) return node;
          node = order < 0 ? node.left : node.right;
        }
        return null;
      }
      // IFind returns the retained entry from one search. Repeating contains?
      // or get would repeat observable IHash/IEquiv calls on the query key.
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
  //@unit selectKeys requires: assoc, first, get, makeMap, meta, missing, next, seq, withMeta
  function selectKeys(value, keyseq) {
    let result = makeMap([]);
    for (let current = seq(keyseq); current != null; current = next(current)) {
      const key = first(current), item = get(value, key, missing);
      if (item !== missing) result = assoc(result, key, item);
    }
    return withMeta(result, meta(value));
  }
  //@unit mergeMaps requires: assoc, conj, contains, first, get, keyProtocol, makeMap, missing,
  //@ next, reduce, seq, truth, valProtocol
  function mergeMaps(values, combine) {
    // `some identity` precedes reduction, including its sequence effects.
    let present = false;
    for (let current = seq(values); current != null; current = next(current)) {
      if (truth(first(current))) { present = true; break; }
    }
    if (!present) return null;
    return reduce((left, right) => {
      const target = truth(left) ? left : makeMap([]);
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
  //@unit requiredGet requires: get, missing, str
  function requiredGet(value, key) {
    const result = get(value, key, missing);
    if (result === missing) throw new Error("Missing required key: " + str(key));
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
  //@ reducingFunction, rest, seq, transform
  function distinct(coll) {
    if (arguments.length === 0) return transform(rf => {
      let seen = makeSet([]);
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
    return step(coll, makeSet([]));
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
  //@unit sort requires: compareKeys, emptyList, fnComparator, invalidArity, meta, seq, toArray,
  //@ withMeta
  function sort(fn, coll) {
    if (arguments.length === 1) { coll = fn; fn = compareKeys; }
    else if (arguments.length !== 2) return invalidArity(arguments.length);
    if (seq(coll) === null) return emptyList;
    const values = toArray(coll);
    values.sort(fnComparator(fn));
    return withMeta(seq(values), meta(coll));
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
  //@unit isSpecialSymbol requires: specialSymbols, symbols
  function isSpecialSymbol(value) {
    const data = symbols.get(value);
    return !!data && data.namespace === null && specialSymbols.has(data.name);
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
  //@unit updateVals requires: assoc, mapFromArray, mapKinds, meta, reduceKv, withMeta
  function updateVals(coll, fn) {
    const kind = mapKinds.get(coll);
    const initial = kind === "array-map" || kind === "hash-map" ? coll : mapFromArray([]);
    return withMeta(reduceKv((result, key, value) => assoc(result, key, fn.call(null, value)), initial, coll), meta(coll));
  }
  //@unit updateKeys requires: assoc, mapFromArray, meta, reduceKv, withMeta
  function updateKeys(coll, fn) {
    return withMeta(reduceKv((result, key, value) => assoc(result, fn.call(null, key), value), mapFromArray([]), coll), meta(coll));
  }
  //@unit merge requires: fnRest, mergeMaps, missing
  function merge(...values) { return mergeMaps(fnRest(values, 0), missing); }
  //@unit mergeWith requires: fnRest, mergeMaps
  function mergeWith(fn, ...values) { return mergeMaps(fnRest(values, 0), fn); }
  //@unit zipmap requires: assoc, first, makeMap, next, seq
  function zipmap(keys, values) {
    let result = makeMap([]);
    let ks = seq(keys), vs = seq(values);
    while (ks != null && vs != null) {
      result = assoc(result, first(ks), first(vs));
      ks = next(ks);
      vs = next(vs);
    }
    return result;
  }
  //@unit set requires: first, isSet, makeSet, next, putMember, seq, withMeta
  function set(value) {
    if (isSet(value)) return withMeta(value, null);
    const entries = [];
    const edit = {};
    for (let current = seq(value); current != null; current = next(current)) putMember(entries, first(current), edit);
    return makeSet(entries);
  }
  //@unit objectEquiv requires: builtin, cells, equalPair, keywords, maps, sequential, sets,
  //@ structuralEqualPair, symbols
  // Object.equiv calls the receiver's IEquiv implementation directly, even
  // when an extension rejects an identical value. Language = keeps its own
  // identity shortcut. Saved Object methods dispatch on the borrowed receiver.
  function objectEquiv(value, other) {
    const equiv = builtin(value, "IEquiv/-equiv", 2);
    if (equiv) return equiv.call(value, value, other);
    if (maps.has(value)) return structuralEqualPair(value, other);
    // Sequence/collection protocols compare contents even for the same receiver
    // (for example, a list containing NaN is not equivalent to itself). Cycle
    // and Iterate retain their default identity equality.
    return sets.has(value) || keywords.has(value) || symbols.has(value) ||
      (sequential(value) && !["cycle", "iterate"].includes(cells.get(value)?.kind))
      ? structuralEqualPair(value, other) : equalPair(value, other);
  }
  //@unit equalPair requires: builtin, structuralEqualPair
  function equalPair(left, right) {
    if (left == null) return right == null;
    if (left === right) return true;
    const equiv = builtin(left, "IEquiv/-equiv", 2);
    if (equiv) return equiv.call(left, left, right);
    return structuralEqualPair(left, right);
  }
  //@unit structuralEqualPair requires: cells, contains, count, equalPair, first, get, ICounted,
  //@ isMap, isRecord, isSequential, isSet, keywords, mapKinds, maps, mapTraversal, missing, next,
  //@ recordTypes, satisfies, seq, sequential, sets, symbols, uuids, vars
  function structuralEqualPair(left, right) {
    if (left instanceof Date) return right instanceof Date && left.valueOf() === right.valueOf();
    if (uuids.has(left)) return uuids.has(right) && uuids.get(left) === uuids.get(right);
    if (vars.has(left)) return vars.has(right) && equalPair(vars.get(left).symbol, vars.get(right).symbol);
    const leftKeyword = keywords.get(left);
    if (leftKeyword) {
      const rightKeyword = keywords.get(right);
      return !!rightKeyword && leftKeyword.spelling === rightKeyword.spelling;
    }
    const leftSymbol = symbols.get(left);
    if (leftSymbol) {
      const rightSymbol = symbols.get(right);
      return !!rightSymbol && leftSymbol.spelling === rightSymbol.spelling;
    }
    if (recordTypes.get(left) !== recordTypes.get(right)) return false;
    const leftMap = maps.get(left);
    if (leftMap) {
      if (!isMap(right) || isRecord(right) || count(left) !== count(right)) return false;
      return mapTraversal(left).every(entry => {
        const found = get(right, entry.key, missing);
        // Hash/tree maps use equiv-map's right-first value dispatch. Array
        // maps retain their distinct left-first IEquiv implementation.
        return found !== missing && (mapKinds.get(left) === "array-map"
          ? equalPair(entry.value, found) : equalPair(found, entry.value));
      });
    }
    const leftSet = sets.get(left);
    if (leftSet) {
      if (!isSet(right) || leftSet.length !== count(right)) return false;
      try { return leftSet.every(entry => contains(right, entry.key)); }
      catch (error) { if (error instanceof Error) return false; throw error; }
    }
    if (sequential(left) && !["cycle", "iterate"].includes(cells.get(left)?.kind)) {
      if (!isSequential(right)) return false;
      if (satisfies(ICounted, left) && satisfies(ICounted, right) && count(left) !== count(right)) return false;
      let a = seq(left), b = seq(right);
      while (a !== null) {
        if (b === null || !equalPair(first(a), first(b))) return false;
        a = next(a); b = next(b);
      }
      return b === null;
    }
    // Primitive NaN stays unequal to itself; foreign objects use identity only.
    return false;
  }

  //@unit equal requires: equalPair
  function equal(...values) {
    for (let i = 1; i < values.length; i++) {
      if (!equalPair(values[i - 1], values[i])) return false;
    }
    return true;
  }

  //@unit sequential requires: cells, lazyValues, queues, vectors, views
  function sequential(value) {
    return vectors.has(value) || cells.has(value) || views.has(value) || queues.has(value) || lazyValues.has(value);
  }

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
  function hashNamed(value) {
    const seed = hashChars(value.name);
    return seed ^ (stringHash(value.namespace) + 0x9e3779b9 + (seed << 6) + (seed >> 2));
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
  //@unit hash requires: builtin, cells, constructorHashes, hashDouble, hashInt, hashNamed,
  //@ hashOrderedColl, identityCounter, identityHashes, IHash, implementsProtocol, keywords,
  //@ lazyValues, maps, mapTraversal, mixCollectionHash, publicCollectionSlots, sequential, sets,
  //@ stringHash, symbols, uuidHashes, uuids, vars
  // Weak identity bookkeeping never reads or mutates foreign object fields.
  function hash(value) {
    if (value == null) return 0;
    const method = implementsProtocol(IHash, value) ? builtin(value, "IHash/-hash", 1) : null;
    if (method) return method.call(value, value) | 0;
    if (typeof value === "boolean") return value ? 1231 : 1237;
    if (typeof value === "number") {
      if (!Number.isFinite(value)) return value === Infinity ? 2146435072 : value === -Infinity ? -1048576 : 2146959360;
      return Number.isSafeInteger(value) ? (Math.floor(value) % 2147483647) | 0 : hashDouble(value);
    }
    if (typeof value === "string") return hashInt(stringHash(value));
    if (value instanceof Date) return value.valueOf() | 0;
    if (uuids.has(value)) {
      if (uuidHashes.get(value) == null) uuidHashes.set(value, hash(uuids.get(value)));
      return uuidHashes.get(value);
    }
    if (vars.has(value)) return hash(vars.get(value).symbol);
    if (constructorHashes.has(value) && constructorHashes.get(value) != null) return constructorHashes.get(value) | 0;
    const named = keywords.get(value) || symbols.get(value);
    if (named) {
      if (symbols.has(value) || keywords.has(value)) {
        const cached = constructorHashes.get(value);
        if (cached != null) return cached | 0;
        const result = (hashNamed(named) + (keywords.has(value) ? 0x9e3779b9 : 0)) | 0;
        constructorHashes.set(value, result);
        return result;
      }
      return (hashNamed(named) + (keywords.has(value) ? 0x9e3779b9 : 0)) | 0;
    }
    const entries = maps.get(value);
    if (entries) {
      let result = 0;
      for (const entry of mapTraversal(value)) result = (result + mixCollectionHash((Math.imul(31, 31 + hash(entry.key)) + hash(entry.value)) | 0, 2)) | 0;
      const resultHash = mixCollectionHash(result, entries.length);
      constructorHashes.set(value, resultHash);
      return resultHash;
    }
    const members = sets.get(value);
    if (members) {
      let result = 0;
      for (const entry of members) result = (result + entry.hash) | 0;
      const resultHash = mixCollectionHash(result, members.length);
      if (publicCollectionSlots.has(value)) constructorHashes.set(value, resultHash);
      return resultHash;
    }
    if (sequential(value) && !["cycle", "iterate"].includes(cells.get(value)?.kind)) {
      if (["cons", "list", "repeat"].includes(cells.get(value)?.kind) || publicCollectionSlots.has(value)) {
        const cached = constructorHashes.get(value);
        if (cached != null) return cached | 0;
        const result = hashOrderedColl(value);
        constructorHashes.set(value, result);
        return result;
      }
      const cached = lazyValues.get(value) ?? cells.get(value)?.chunkedState;
      if (cached?.hash != null) return cached.hash;
      const result = hashOrderedColl(value);
      if (cached) cached.hash = result;
      return result;
    }
    const nativeHash = builtin(value, "IHash/-hash", 1);
    if (nativeHash) return nativeHash.call(value, value) | 0;
    if (typeof value === "symbol" || typeof value === "bigint") return hash(String(value));
    if (!identityHashes.has(value)) identityHashes.set(value, ++identityCounter);
    return identityHashes.get(value);
  }

  //@unit compilerFormBridge requires: atoms, cells, deferredValue, emptyList, EmptyListValue,
  //@ generatedNames, ifnValues, jsObject, keyword, keywords, lazyValues, listFromArray,
  //@ mapFromArray, maps, mapTraversal, metadataValues, name, queueFromArray, queues, recordTypes,
  //@ reifyValues, rePattern, setFromArray, sets, setTraversal, symbol, symbols, toArray,
  //@ typeFields, uuid, uuids, vars, vectorFromArray, vectors, view, views, volatiles
  // Versioned in-process compiler bridge. Descriptors are private arrays,
  // never printed text or JSON. Only brands above authorize language values.
  function compilerFormBridge() {
    const borrowed = new WeakMap(), literals = new WeakMap(), primitives = new Map();
    const negativeZero = Object.freeze({});
    const primitiveKey = v => typeof v === "number" && Object.is(v, -0) ? negativeZero : v;
    const dateTime = Function.prototype.call.bind(Date.prototype.getTime);
    const regexSource = Function.prototype.call.bind(Object.getOwnPropertyDescriptor(RegExp.prototype, "source").get);
    const regexFlags = [["hasIndices","d"],["global","g"],["ignoreCase","i"],["multiline","m"],["dotAll","s"],["unicode","u"],["sticky","y"]]
      .map(([property, letter]) => [Object.getOwnPropertyDescriptor(RegExp.prototype, property)?.get, letter]);
    const fromUnits = units => {
      let result = "";
      for (let i = 0; i < units.length; i += 4096)
        result += String.fromCharCode(...units.slice(i, i + 4096));
      return result;
    };
    const units = text => {
      const result = [];
      for (let i = 0; i < text.length; i++) result.push(text.charCodeAt(i));
      return result;
    };
    let terminal = null;
    const fail = (category, path) => {
      const message = "Unsupported macro result " + category + " at " + path;
      if (category.includes("limit")) terminal ??= message;
      throw new Error(message);
    };
    function decode(root) {
      // Bootstrap syntax quotations can be deep. Traverse descriptors explicitly
      // so the documented form depth budget does not consume the engine stack.
      const values = new Map(), pending = [[root, false]];
      while (pending.length) {
        const [d, ready] = pending.pop();
        if (ready) { values.set(d, decodeNode(d, values)); continue; }
        pending.push([d, true]);
        const [kind, , metadata, a] = d;
        if (metadata != null) pending.push([metadata, false]);
        if (["list", "vector", "map", "set"].includes(kind)) {
          for (let i = a.length - 1; i >= 0; --i) pending.push([a[i], false]);
        } else if (["regex", "instant", "uuid", "queue", "js-array", "js-object"].includes(kind)) {
          pending.push([a, false]);
        }
      }
      return values.get(root);
    }
    function decodeNode(d, values) {
      const decode = child => values.get(child);
      const [kind, id, metadata, a, b] = d;
      let value;
      switch (kind) {
        case "nil": value = null; break;
        case "bool": case "number": value = a; break;
        case "string": case "character": value = fromUnits(a); break;
        case "symbol": case "keyword": value = (kind === "symbol" ? symbol : keyword)(a == null ? null : fromUnits(a), fromUnits(b)); if (kind === "symbol" && a == null) generatedNames.add(fromUnits(b)); break;
        case "list": value = listFromArray(a.map(decode));
          if (a.length === 0) { value = Object.create(EmptyListValue.prototype); cells.set(value, cells.get(emptyList)); }
          break;
        case "vector": value = vectorFromArray(a.map(decode)); break;
        case "map": value = mapFromArray(a.map(decode)); break;
        case "set": value = setFromArray(a.map(decode)); break;
        case "regex": value = rePattern(decode(a)); break;
        case "instant": value = new Date(decode(a)); break;
        case "uuid": value = uuid(decode(a)); break;
        case "queue": value = queueFromArray(vectors.get(decode(a))); break;
        case "js-array": value = vectors.get(decode(a)).slice(); literals.set(value, "js-array"); break;
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
        metadataValues.set(value, m);
      }
      if (value != null && typeof value === "object") borrowed.set(value, id);
      else primitives.set(primitiveKey(value), id);
      return value;
    }
    function encode(value, limit, depthLimit) {
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
          else if (kind === "string") { tag = "string"; a = units(v); }
          else if (kind !== "object") fail(kind, path);
          else if (vars.has(v)) fail("Var", path);
          else if (atoms.has(v)) fail("atom", path);
          else if (volatiles.has(v)) fail("volatile", path);
          else if (typeFields.has(v) || recordTypes.has(v) || reifyValues.has(v) || ifnValues.has(v)) fail("protocol/type instance", path);
          else if (symbols.has(v) || keywords.has(v)) {
            const n = symbols.get(v) || keywords.get(v); tag = symbols.has(v) ? "symbol" : "keyword";
            a = n.namespace == null ? null : units(n.namespace); b = units(n.name);
          } else if (vectors.has(v)) { tag = "vector"; a = vectors.get(v).map((x, i) => visit(x, path + "[" + i + "]", depth + 1)); }
          else if (maps.has(v)) { tag = "map"; a = []; let i = 0;
            for (const e of mapTraversal(v)) { a.push(visit(e.key, path + ".key[" + i + "]", depth + 1), visit(e.value, path + ".value[" + i++ + "]", depth + 1)); }
          } else if (sets.has(v)) { tag = "set"; a = setTraversal(v).map((e, i) => visit(e.key, path + "[" + i + "]", depth + 1)); }
          else if (queues.has(v)) { tag = "queue"; a = visit(vectorFromArray(toArray(v)), path + ".payload", depth + 1); }
          else if (uuids.has(v)) { tag = "uuid"; a = visit(uuids.get(v), path + ".payload", depth + 1); }
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
            a = [tag === "js-array" ? "vector" : "map", 0, null, items];
          } else if (cells.has(v) || views.has(v) || lazyValues.has(v)) {
            tag = "list"; a = [];
            const seen = new WeakSet(); let current = v;
            while (true) {
              if (--remaining < 0) fail("sequence limit", path);
              if (current == null) break;
              if (!cells.has(current) && !views.has(current) && !lazyValues.has(current)) fail("noncanonical sequence tail", path);
              if (seen.has(current)) fail("cyclic sequence", path); seen.add(current);
              // Lazy realization is intentionally executable but shares limits.
              // Work only with private payloads. User protocol objects are not
              // expansion trees; realization cannot call their getters to diagnose.
              try {
                while (lazyValues.has(current)) {
                  if (--remaining < 0) fail("sequence limit", path);
                  current = deferredValue(lazyValues.get(current), true);
                }
              } catch (_) { fail("lazy realization failure", path); }
              if (current == null) break;
              if (vectors.has(current)) current = view(vectors.get(current), 0, true);
              if (cells.has(current)) {
                const cell = cells.get(current);
                if (cell.kind === "list" && cell.count === 0) break;
                let head, tail;
                try { head = cell.head; tail = cell.tail; } catch (_) { fail("sequence realization failure", path); }
                a.push(visit(head, path + "[" + a.length + "]", depth + 1));
                current = tail;
              } else if (views.has(current)) {
                const slice = views.get(current);
                if (slice.offset >= slice.storage.length) break;
                a.push(visit(slice.storage[slice.offset], path + "[" + a.length + "]", depth + 1));
                current = slice.offset + 1 < slice.storage.length ? view(slice.storage, slice.offset + 1, slice.indexed, slice.kind, slice.tail) : slice.tail;
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
          const metadata = object && metadataValues.has(v) ? visit(metadataValues.get(v), path + ".meta", depth + 1) : null;
          return [tag, object ? borrowed.get(v) || 0 : primitives.get(primitiveKey(v)) || 0, metadata, a, b];
        } finally { if (object) active.delete(v); }
      }
      return visit(value, "$", 0);
    }
    return Object.freeze({version: 1, decode, encode, check: () => { if (terminal != null) throw new Error(terminal); },
      // Explicit descriptors authorize host literal data; ordinary objects remain invalid.
      jsArray: items => { const v = Array.from(items); literals.set(v, "js-array"); return v; },
      jsObject: pairs => { const v = jsObject(pairs); literals.set(v, "js-object"); return v; },
      withMeta: (value, metadata) => {
        if (!literals.has(value)) throw new Error("Invalid compiler literal metadata target");
        metadataValues.set(value, metadata); return value;
      }});
  }

  //@unit inspect requires: atoms, cells, first, isReduced, keywords, mapTraversal, next, nth,
  //@ recordEntries, recordTypes, seq, sets, setTraversal, symbols, uuids, vars, vectors, views,
  //@ volatiles
  // Private test adapter. Production emission keeps this factory local; only
  // the observation harness registers inspect. Exposed storage remains frozen.
  function inspect(value) {
    if (isReduced(value) || uuids.has(value) || vars.has(value) || atoms.has(value) || volatiles.has(value)) return Object.freeze({kind: "opaque"});
    const named = keywords.get(value) || symbols.get(value);
    if (named) return Object.freeze({kind: keywords.has(value) ? "keyword" : "symbol",
      namespace: named.namespace, name: named.name, spelling: named.spelling});
    const items = vectors.get(value);
    if (items) return Object.freeze({kind: "vector", items});
    const entries = recordTypes.has(value)
      ? recordEntries(value, recordTypes.get(value)).map(pair => ({key: nth(pair, 0), value: nth(pair, 1)}))
      : mapTraversal(value);
    if (entries) return Object.freeze({kind: "map", entries: Object.freeze(entries.map(
      entry => Object.freeze([entry.key, entry.value]))), count: entries.length});
    const members = sets.get(value);
    if (members) return Object.freeze({kind: "set",
      items: Object.freeze(setTraversal(value).map(entry => entry.key)), count: members.length});
    if (cells.has(value) || views.has(value)) {
      const items = [];
      for (let current = seq(value); current !== null; current = next(current)) items.push(first(current));
      return Object.freeze({kind: cells.get(value)?.kind === "list" ? "list" : "seq", items: Object.freeze(items)});
    }
    return null;
  }

  //@unit queueFromArray requires: PersistentQueueValue, queues
  function queueFromArray(items) {
    const value = Object.create(PersistentQueueValue.prototype);
    queues.set(value, Object.freeze(items.slice()));
    return value;
  }

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
  const IChunk = function () {}, IChunkedNext = function () {}, IDrop = function () {};
  //@unit IMapEntry
  const IMapEntry = function () {}, IFind = function () {}, ISorted = function () {}, IChunkedSeq = function () {};
  //@unit IEncodeJS
  const IEncodeJS = function () {}, IEncodeClojure = function () {};
  //@unit IRecord
  const IRecord = function () {}, ICloneable = function () {}, IPrintWithWriter = function () {}, IWriter = function () {};
  //@unit INamed
  const INamed = function () {};
  //@unit Inst
  const Inst = function () {}, IUUID = function () {}, ASeq = function () {}, IDerefWithTimeout = function () {};
  //@unit IMeta
  const IMeta = function () {}, IWithMeta = function () {}, IHash = function () {}, IEquiv = function () {};
  //@unit IPending
  const IPending = function () {}, IDeref = function () {}, IAtom = function () {}, IReset = function () {}, ISwap = function () {}, IVolatile = function () {}, IWatchable = function () {};
  //@unit Fn
  const Fn = function () {}, IList = function () {}, IFn = function () {}, ISeqable = function () {}, ISeq = function () {}, INext = function () {}, ICounted = function () {}, IIndexed = function () {}, ILookup = function () {}, ISequential = function () {}, IAssociative = function () {};
  //@unit IReduce
  const IReduce = function () {}, IKVReduce = function () {}, IVector = function () {}, IMap = function () {}, ICollection = function () {}, IEmptyableCollection = function () {}, ISet = function () {}, IStack = function () {}, IReversible = function () {}, IComparable = function () {}, IIterable = function () {};
  //@unit IMultiFn
  const IMultiFn = function () {};
  //@unit IEditableCollection
  const IEditableCollection = function () {}, ITransientCollection = function () {}, ITransientAssociative = function () {}, ITransientMap = function () {}, ITransientVector = function () {}, ITransientSet = function () {};
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
  //@unit protocolDefaults effect: IMeta, Fn, ICounted, IHash, IEquiv requires: Fn, ICounted,
  //@ IEquiv, IHash, IMeta
  IMeta.function = true; Fn.function = true; ICounted.null = true; IHash._ = true; IEquiv._ = true;
  //@unit meta requires: assoc, atoms, deferredVarMetadata, keyword, metadataValues, publicMethod
  // Metadata belongs to values, never structural equality or hash payloads.
  function meta(value) {
    const method = value == null ? null : publicMethod(value, "IMeta/-meta", 1);
    if (method) return method.call(value, value);
    const deferred = deferredVarMetadata.get(value);
    if (deferred) {
      const metadata = assoc(metadataValues.get(value), keyword(null, "test"), deferred());
      metadataValues.set(value, metadata);
      deferredVarMetadata.delete(value);
    }
    return atoms.has(value) ? value.meta : metadataValues.has(value) ? metadataValues.get(value) : null;
  }
  //@unit empty requires: cells, emptyList, lazyValues, makeMap, makeSet, mapEntries, mapKinds,
  //@ maps, meta, preserveMeta, publicCollectionSlots, publicMethod, queueFromArray, queues,
  //@ recordTypes, setKinds, sets, sortedComparators, vector, vectors, views, withMeta
  function empty(value) {
    const method = value == null ? null : publicMethod(value, "IEmptyableCollection/-empty", 1);
    if (method) return method.call(value, value);
    if (value == null) return null;
    if (recordTypes.has(value) || mapEntries.has(value)) return null;
    if (vectors.has(value)) return preserveMeta(value, vector());
    if (maps.has(value)) return preserveMeta(value, makeMap([], mapKinds.get(value), sortedComparators.get(value)));
    if (sets.has(value)) return preserveMeta(value, makeSet([], setKinds.get(value), sortedComparators.get(value)));
    if (queues.has(value)) return preserveMeta(value, queueFromArray([]));
    if (cells.get(value)?.kind === "list" && cells.get(value).count === 0 && !publicCollectionSlots.has(value)) return value;
    if (cells.has(value) || views.has(value) || lazyValues.has(value))
      return cells.get(value)?.kind === "list" || views.get(value)?.kind === "queue" || lazyValues.has(value) ? withMeta(emptyList, meta(value)) : emptyList;
    return null;
  }
  //@unit peek requires: builtin, cells, first, publicCollectionSlots, queues, vectors
  function peek(value) {
    const method = value == null ? null : builtin(value, "IStack/-peek", 1);
    if (method) return method.call(value, value);
    if (value == null) return null;
    if (vectors.has(value)) { const items = vectors.get(value); return items.length ? items[items.length - 1] : null; }
    if (queues.has(value) && publicCollectionSlots.has(value)) return first(publicCollectionSlots.get(value).front);
    if (queues.has(value) || cells.get(value)?.kind === "list") return first(value);
    throw new Error("No protocol method IStack.-peek defined for this value");
  }
  //@unit pop requires: builtin, cells, count, makeVector, meta, next, PersistentQueueValue,
  //@ preserveMeta, publicCollectionSlots, queueFromArray, queues, rest, seq, subvecSources,
  //@ SubvecValue, vector, vectors
  function pop(value) {
    const method = value == null ? null : builtin(value, "IStack/-pop", 1);
    if (method) return method.call(value, value);
    if (value == null) return null;
    if (vectors.has(value)) {
      const items = vectors.get(value);
      if (!items.length) throw new Error("Can't pop empty vector");
      const slots = publicCollectionSlots.get(value);
      if (slots && subvecSources.has(value)) return SubvecValue(meta(value), subvecSources.get(value), slots.start, slots.end - 1, null);
      return preserveMeta(value, makeVector(items.slice(0, -1)));
    }
    if (queues.has(value)) {
      const q = publicCollectionSlots.get(value);
      if (q) { if (q.front == null) return value; const front = next(q.front);
        return PersistentQueueValue(meta(value), q.count - 1, front ?? seq(q.rear), front == null ? vector() : q.rear, null); }
      return preserveMeta(value, queueFromArray(queues.get(value).slice(1)));
    }
    if (cells.get(value)?.kind === "list") {
      if (!count(value)) throw new Error("Can't pop empty list");
      return rest(value);
    }
    throw new Error("No protocol method IStack.-pop defined for this value");
  }
  //@unit rseq requires: builtin, maps, sortedComparators, toArray, vectors, view, views
  function rseq(value) {
    const method = builtin(value, "IReversible/-rseq", 1);
    if (method) return method.call(value, value);
    if (!vectors.has(value) && views.get(value)?.kind !== "indexed" && !sortedComparators.has(value))
      throw new Error("No protocol method IReversible.-rseq defined for this value");
    const items = Object.freeze(toArray(value).reverse());
    return items.length ? view(items, 0, false, maps.has(value) ? "sorted-map" : "reverse") : null;
  }
  //@unit mapProjection requires: cells, emptyList, firstProtocol, INext, keyProtocol, next,
  //@ nextProtocol, satisfies, seq, sequencePrototype, valProtocol
  function mapProjection(value, index, alreadySeq = false) {
    const entries = alreadySeq ? value : seq(value);
    if (entries === null) return null;
    const result = Object.create(sequencePrototype);
    cells.set(result, Object.freeze({kind: index === 0 ? "keys" : "vals",
      get head() { const entry = firstProtocol(entries); return index === 0 ? keyProtocol(entry) : valProtocol(entry); },
      get tail() {
        const following = satisfies(INext, entries) ? nextProtocol(entries) : next(entries);
        return following == null ? emptyList : mapProjection(following, index, true);
      }}));
    return result;
  }
  //@unit keys requires: mapProjection
  function keys(value) { return mapProjection(value, 0); }
  //@unit vals requires: mapProjection
  function vals(value) { return mapProjection(value, 1); }
  //@unit repeat requires: emptyList, invalidArity, RepeatValue
  function repeat(n, item) {
    if (arguments.length === 1) { item = n; n = -1; }
    if (arguments.length < 1 || arguments.length > 2) return invalidArity(arguments.length);
    if (arguments.length === 2 && !(n > 0)) return emptyList;
    return RepeatValue(null, n, item, null, null);
  }
  //@unit cycleNode requires: cells, CycleValue, first, next, truth
  function cycleNode(all, previous, current, following = null) {
    const value = Object.create(CycleValue.prototype), state = {current, following};
    const currentValue = () => truth(state.current) ? state.current : (state.current = next(previous) ?? all);
    cells.set(value, Object.freeze({kind: "cycle", all, previous, state, currentValue,
      get realized() { return state.current != null; },
      get head() { return first(currentValue()); },
      get tail() { return state.following ?? (state.following = cycleNode(all, currentValue(), null)); }}));
    return value;
  }
  //@unit cycle requires: cycleNode, emptyList, seq
  function cycle(collection) {
    const start = seq(collection);
    return start === null ? emptyList : cycleNode(start, null, start);
  }
  //@unit compareKeys requires: builtin, keywords, str, symbols, uuids, vectors
  function compareKeys(a, b) {
    if (a === b) return 0;
    if (a == null) return -1;
    if (b == null) return 1;
    const cannotCompare = () => { throw new Error("Cannot compare " + str(a) + " to " + str(b)); };
    if (typeof a === "number") return typeof b === "number" ? (a < b ? -1 : a > b ? 1 : 0) : cannotCompare();
    const method = builtin(a, "IComparable/-compare", 2);
    if (method) return method.call(a, a, b);
    if (uuids.has(a)) return uuids.has(b) ? compareKeys(uuids.get(a), uuids.get(b)) : cannotCompare();
    if (a instanceof Date) return b instanceof Date ? compareKeys(a.valueOf(), b.valueOf()) : cannotCompare();
    const left = keywords.get(a) || symbols.get(a), right = keywords.get(b) || symbols.get(b);
    if (left || right) {
      if (!left || !right || keywords.has(a) !== keywords.has(b)) throw new Error("Cannot compare different types");
      return compareKeys(left.namespace, right.namespace) || compareKeys(left.name, right.name);
    }
    if (vectors.has(a) && vectors.has(b)) {
      const xs = vectors.get(a), ys = vectors.get(b);
      if (xs.length !== ys.length) return xs.length < ys.length ? -1 : 1;
      for (let i = 0; i < xs.length; i++) { const order = compareKeys(xs[i], ys[i]); if (order) return order; }
      return 0;
    }
    if (!(Array.isArray(a) && Array.isArray(b)) && (typeof a !== typeof b || !["string", "boolean"].includes(typeof a))) return cannotCompare();
    return a < b ? -1 : a > b ? 1 : 0;
  }
  //@unit sequentialIndex requires: count, equalPair, nth
  function sequentialIndex(coll, value, start, reverse) {
    const length = count(coll);
    if (reverse) {
      if (length === 0) return -1;
      for (let i = start > 0 ? Math.min(length - 1, start) : start < 0 ? length + start : start; i >= 0; i--)
        if (equalPair(nth(coll, i), value)) return i;
    } else {
      for (let i = start < 0 ? Math.max(0, length + start) : start; i < length; i++)
        if (equalPair(nth(coll, i), value)) return i;
    }
    return -1;
  }
  //@unit treeRoots
  // Comparator equality controls sorted membership; structural hashes remain
  // attached to the retained key for unordered collection equality/hash.
  const treeRoots = new WeakMap(), treeSetMaps = new WeakMap();
  //@unit publicTree requires: mapEntry, maps, publicCollectionSlots, treeRoots
  function publicTree(value) {
    const slots = publicCollectionSlots.get(value);
    if (slots && "tree" in slots) return slots.tree;
    if (!treeRoots.has(value)) {
      const entries = maps.get(value);
      function build(start, end) {
        if (start >= end) return null;
        const mid = (start + end) >>> 1, entry = entries[mid];
        const node = mapEntry(entry.key, entry.value);
        Object.assign(node, {key: entry.key, val: entry.value, left: build(start, mid), right: build(mid + 1, end)});
        return node;
      }
      treeRoots.set(value, build(0, entries.length));
    }
    return treeRoots.get(value);
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
  //@unit treeMapConstructor requires: constructorSlots, deferredEntries, hash, makeMap, maps, nth,
  //@ sortedComparators
  function treeMapConstructor(comparator, tree, suppliedCount, metadata, cachedHash) {
    const value = makeMap([], "sorted-map", comparator);
    sortedComparators.set(value, comparator);
    maps.set(value, deferredEntries(() => {
      const entries = [];
      function visit(node) {
        if (node == null) return;
        visit(node.left);
        entries.push(Object.freeze({key: "key" in node ? node.key : nth(node, 0), value: "val" in node ? node.val : nth(node, 1), get hash() { return hash(this.key); }}));
        visit(node.right);
      }
      visit(tree); return entries;
    }));
    return constructorSlots(value, {tree, cnt: suppliedCount}, metadata, cachedHash);
  }
  //@unit treeSetConstructor requires: constructorSlots, deferredEntries, makeSet, maps, sets,
  //@ sortedComparators
  function treeSetConstructor(metadata, treeMap, cachedHash) {
    const value = makeSet([], "sorted-set", sortedComparators.get(treeMap));
    sets.set(value, deferredEntries(() => treeMap == null ? [] : maps.get(treeMap).map(entry => Object.freeze({key: entry.key, get hash() { return entry.hash; }}))));
    return constructorSlots(value, {"tree-map": treeMap}, metadata, cachedHash);
  }
  //@unit treeMapPush requires: conj, truth
  function treeMapPush(tree, stack, ascending) {
    while (tree != null) { stack = conj(stack, tree); tree = truth(ascending) ? tree.left : tree.right; }
    return stack;
  }
  //@unit treeMapSeqConstructor requires: cells, constructorSlots, emptyList, first, next, peek,
  //@ treeMapPush, truth
  function treeMapSeqConstructor(metadata, stack, ascending, suppliedCount, cachedHash) {
    const value = Object.create(treeMapSeqConstructor.prototype);
    cells.set(value, {kind: "tree-seq", get head() { return peek(stack); }, get tail() {
      const node = first(stack), pending = treeMapPush(truth(ascending) ? node?.right : node?.left, next(stack), ascending);
      return pending == null ? emptyList : treeMapSeqConstructor(null, pending, ascending, suppliedCount - 1, null);
    }});
    return constructorSlots(value, {stack, "ascending?": ascending, cnt: suppliedCount}, metadata, cachedHash);
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
  //@unit collectionEntryIndex requires: entryIndex, hashEntryIndex, mapKinds, setKinds,
  //@ sortedComparators, sortedPosition
  function collectionEntryIndex(value, entries, key) {
    const comparator = sortedComparators.get(value);
    if (!comparator) {
      const kind = mapKinds.get(value) ?? setKinds.get(value);
      if (kind === "array-map") return entryIndex(entries, key, undefined, kind);
      // Hash-map nil slots are separate from the trie and never hash a query.
      if (kind === "hash-map") {
        if (key == null) return entries.findIndex(entry => entry.key == null);
        if (!entries.some(entry => entry.key != null)) return -1;
      }
      return hashEntryIndex(entries, key);
    }
    const i = sortedPosition(entries, key, comparator);
    return i < entries.length && comparator.call(null, entries[i].key, key) === 0 ? i : -1;
  }
  //@unit putSortedEntry requires: equalPair, hash, sortedPosition
  function putSortedEntry(entries, key, value, comparator, member = false) {
    const i = sortedPosition(entries, key, comparator);
    const exists = i < entries.length && comparator.call(null, entries[i].key, key) === 0;
    if (exists && (member || equalPair(entries[i].value, value))) return;
    const retained = exists ? entries[i].key : key;
    const entry = Object.freeze({key: retained, value, get hash() { return hash(retained); }});
    if (exists) entries[i] = entry;
    else entries.splice(i, 0, entry);
  }
  //@unit putCollectionEntry requires: entryIndex, hashRoots, mapKinds, promoteEntries, putEntry,
  //@ putMember, putSortedEntry, sortedComparators
  function putCollectionEntry(coll, entries, key, value, member = false) {
    const comparator = sortedComparators.get(coll);
    if (comparator) putSortedEntry(entries, key, value, comparator, member);
    else if (member) putMember(entries, key);
    else {
      const kind = mapKinds.get(coll);
      // A fresh nil map starts as an array map; the ninth distinct key promotes
      // it. Check existing keys before promotion, as canonical assoc does.
      const array = (coll == null || kind === "array-map") &&
        (entries.length < 8 || entryIndex(entries, key, undefined, "array-map") >= 0);
      if (!array && kind === "array-map") {
        const promoted = promoteEntries(entries, {});
        entries.splice(0, entries.length, ...promoted);
        hashRoots.set(entries, hashRoots.get(promoted));
      }
      putEntry(entries, key, value, array ? "array-map" : undefined);
    }
  }
  //@unit buildSortedMap requires: makeMap, putSortedEntry
  function buildSortedMap(comparator, pairs) {
    const entries = [];
    for (let i = 0; i < pairs.length; i += 2) putSortedEntry(entries, pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null, comparator);
    return makeMap(entries, "sorted-map", comparator);
  }
  //@unit sortedMap requires: buildSortedMap, compareKeys
  function sortedMap(...pairs) { return buildSortedMap(compareKeys, pairs); }
  //@unit sortedMapBy requires: buildSortedMap, fnComparator
  function sortedMapBy(comparator, ...pairs) {
    return buildSortedMap(fnComparator(comparator), pairs);
  }
  //@unit buildSortedSet requires: makeSet, putSortedEntry
  function buildSortedSet(comparator, items) {
    const entries = [];
    for (const item of items) putSortedEntry(entries, item, undefined, comparator, true);
    return makeSet(entries, "sorted-set", comparator);
  }
  //@unit sortedSet requires: buildSortedSet, compareKeys
  function sortedSet(...items) { return buildSortedSet(compareKeys, items); }
  //@unit sortedSetBy requires: buildSortedSet, fnComparator
  function sortedSetBy(comparator, ...items) {
    return buildSortedSet(fnComparator(comparator), items);
  }
  //@unit sortedSeqFrom requires: identity, mapEntry, maps, mapSeq, sets, sortedComparators,
  //@ sortedPosition, truth, view
  function sortedSeqFrom(value, key, ascending) {
    const entries = maps.get(value) ?? sets.get(value), comparator = sortedComparators.get(value);
    let i = sortedPosition(entries, key, comparator);
    const absent = () => sets.has(value) ? mapSeq(identity, null) : null;
    if (!truth(ascending)) {
      if (i === entries.length || comparator.call(null, entries[i].key, key) !== 0) i--;
      if (i < 0) return absent();
    } else if (i === entries.length) return absent();
    const selected = truth(ascending) ? entries.slice(i) : entries.slice(0, i + 1).reverse();
    return sets.has(value) ? mapSeq(entry => entry.key, selected) : view(Object.freeze(selected.map(entry => mapEntry(entry.key, entry.value))), 0, false, "sorted-map");
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
  //@unit metaSupported requires: atoms, builtin, cells, lazyValues, maps, metaFunctions, queues,
  //@ reifyValues, sets, symbols, vars, vectors, views
  function metaSupported(value) {
    return typeof value === "function" || !!builtin(value, "IMeta/-meta", 1) || [symbols, vectors, maps, sets, cells, views, queues, lazyValues, vars, atoms, metaFunctions].some(store => store.has(value)) || reifyValues.has(value);
  }
  //@unit withMetaSupported requires: builtin, cells, lazyValues, maps, metaFunctions, queues,
  //@ reifyValues, sets, symbols, vars, vectors, views
  function withMetaSupported(value) {
    return !!builtin(value, "IWithMeta/-with-meta", 2) || [symbols, vectors, maps, sets, cells, views, queues, lazyValues, vars, metaFunctions].some(store => store.has(value)) || reifyValues.has(value);
  }
  //@unit declarationDemand
  let declarationDemand = null;
  //@unit declarationTransactions
  const declarationTransactions = [];
  //@unit setDeclarationDemand requires: declarationDemand
  function setDeclarationDemand(demand) { declarationDemand = demand; }
  //@unit demandProtocol requires: declarationDemand, typeNames
  function demandProtocol(key, value) {
    if (!declarationDemand) return;
    const name = typeNames.get(value?.constructor);
    declarationDemand(key, value, name);
  }
  //@unit declarationReceiver requires: demandProtocol
  function declarationReceiver(value) { demandProtocol("Object", value); return value; }
  //@unit patchDeclaration mutator requires: declarationTransactions
  function patchDeclaration(value, key, descriptor) {
    const transaction = declarationTransactions.at(-1);
    if (transaction) {
      const old = Object.getOwnPropertyDescriptor(value, key);
      transaction.push({undo: () => old ? Object.defineProperty(value, key, old) : Reflect.deleteProperty(value, key), redo: () => Object.defineProperty(value, key, descriptor), snapshot: () => { const saved = Object.getOwnPropertyDescriptor(value, key); return () => saved ? Object.defineProperty(value, key, saved) : Reflect.deleteProperty(value, key); }});
    }
    Object.defineProperty(value, key, descriptor);
  }
  //@unit setDeclaration mutator requires: declarationTransactions
  function setDeclaration(store, key, value) {
    const transaction = declarationTransactions.at(-1), present = store.has(key), previous = store.get(key);
    if (transaction) transaction.push({undo: () => present ? store.set(key, previous) : store.delete(key), redo: () => store.set(key, value), snapshot: () => { const saved = store.get(key), has = store.has(key); return () => has ? store.set(key, saved) : store.delete(key); }});
    store.set(key, value);
  }
  //@unit declarationTransaction requires: declarationTransactions
  function declarationTransaction(run, capture = false) {
    const undo = [];
    declarationTransactions.push(undo);
    try { const value = run(); return capture ? {value, patches: undo} : value; }
    catch (error) {
      for (let i = undo.length - 1; i >= 0; i--) undo[i].undo();
      throw error;
    } finally { declarationTransactions.pop(); }
  }
  //@unit applyDeclarationPatches
  function applyDeclarationPatches(patches, forward) {
    if (forward) for (const patch of patches) patch.redo();
    else for (let i = patches.length - 1; i >= 0; i--) patches[i].undo();
  }
  //@unit snapshotDeclarationPatches
  function snapshotDeclarationPatches(patches) { return patches.map(patch => patch.snapshot()); }
  //@unit restoreDeclarationPatches
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
  //@ nativeType, ownedProtocolValue, protocolMaskNames, protocolNames
  function satisfies(protocol, value) {
    demandProtocol(protocolNames.get(protocol) ?? coreProtocolNames.get(protocol), value);
    if (implementsProtocol(protocol, value)) return true;
    const key = protocolNames.get(protocol);
    if (key) return !!(protocol[nativeType(value)] || protocol._);
    if (ownedProtocolValue(value)) return false;
    const coreName = coreProtocolNames.get(protocol);
    const index = protocolMaskNames.lastIndexOf(coreName);
    if (index >= 0 && value?.["cljs$lang$protocol_mask$partition" + (index >> 5) + "$"]) return false;
    return coreName ? !!(protocol[nativeType(value)] || protocol._ || nativeMethods.get(coreName)?.get(nativeType(value)) || nativeMethods.get(coreName)?.get("_")) : false;
  }
  //@unit protocolMaskNames
  const protocolMaskNames = "IFn ICounted IEmptyableCollection ICollection IIndexed ASeq ISeq INext ILookup IAssociative IMap IMapEntry ISet IStack IVector IDeref IDerefWithTimeout IMeta IWithMeta IReduce IKVReduce IEquiv IHash ISeqable ISequential IList IRecord IReversible ISorted IPrintWithWriter IWriter IPrintWithWriter IPending IWatchable IEditableCollection ITransientCollection ITransientAssociative ITransientMap ITransientVector ITransientSet IMultiFn IChunkedSeq IChunkedNext IComparable INamed ICloneable IAtom IReset ISwap IIterable IDrop".split(" ");
  //@unit cloneablePayload requires: cells, mapEntries, maps, queues, recordTypes, sets, vectors,
  //@ views
  function cloneablePayload(value) { return (vectors.has(value) && !mapEntries.has(value)) || (maps.has(value) && !recordTypes.has(value)) || sets.has(value) || queues.has(value) || ["list", "cons", "range"].includes(cells.get(value)?.kind) || ["indexed", "reverse"].includes(views.get(value)?.kind); }
  //@unit coreProtocolImplementation requires: builtinCoreImplementation, coreProtocolNames,
  //@ directMethod, protocolMaskNames
  function coreProtocolImplementation(protocol, value) {
    const name = coreProtocolNames.get(protocol);
    const maskIndex = protocolMaskNames.lastIndexOf(name);
    if (maskIndex >= 0 && (value?.["cljs$lang$protocol_mask$partition" + (maskIndex >> 5) + "$"] & (1 << (maskIndex % 32)))) return true;
    if (name && directMethod(value, name, 0)) return true;
    return builtinCoreImplementation(protocol, value);
  }
  //@unit builtinCoreImplementation requires: ASeq, atoms, cells, chunkBuffers, chunks,
  //@ cloneablePayload, delays, directMethod, Fn, IAssociative, IAtom, IChunk, IChunkedNext,
  //@ IChunkedSeq, ICloneable, ICollection, IComparable, ICounted, IDeref, IDrop,
  //@ IEditableCollection, IEmptyableCollection, IEquiv, IFind, IFn, ifnMethods, IHash, IIndexed,
  //@ IIterable, IKVReduce, IList, ILookup, IMap, IMapEntry, IMeta, IMultiFn, INamed, INext, Inst,
  //@ IPending, IPrintWithWriter, IReduce, IReset, IReversible, ISeq, ISeqable, ISequential, ISet,
  //@ isIdent, ISorted, IStack, ISwap, ITransientAssociative, ITransientCollection, ITransientMap,
  //@ ITransientSet, ITransientVector, IUUID, IVector, IVolatile, IWatchable, IWithMeta, keywords,
  //@ lazyValues, mapEntries, mapKinds, maps, metaFunctions, multiFns, ownedProtocolValue,
  //@ publicCollectionSlots, queues, recordTypes, reducedValues, reifyValues, sets,
  //@ sortedComparators, subvecSources, symbols, transients, uuids, vars, vectors, views, volatiles
  function builtinCoreImplementation(protocol, value) {
    if (protocol === Inst) return value instanceof Date;
    if (protocol === IUUID) return uuids.has(value);
    if (protocol === ASeq && publicCollectionSlots.has(value) && cells.get(value)?.kind === "list") return true;
    if (protocol === ASeq && views.get(value)?.kind === "chunked") return true;
    if (["cycle", "iterate"].includes(cells.get(value)?.kind) && [IHash, IEquiv, IStack, IIterable].includes(protocol)) return false;
    if (["range", "tree-seq"].includes(cells.get(value)?.kind) && protocol === IStack) return false;
    if (cells.get(value)?.kind === "repeat" && [IStack, IIterable].includes(protocol)) return false;
    if (protocol === ASeq) return cells.get(value)?.kind === "cons" || (cells.get(value)?.kind === "list" && cells.get(value).count > 0) || cells.get(value)?.kind === "chunked-cons" || ["indexed", "hash-map"].includes(views.get(value)?.kind);
    if (cells.get(value)?.kind === "chunked-cons" && [IReduce, IIterable, IStack].includes(protocol)) return false;
    if (protocol === INamed) return keywords.has(value) || symbols.has(value) || multiFns.has(value);
    if (protocol === IMultiFn) return multiFns.has(value);
    if (protocol === IFn && multiFns.has(value)) return true;
    const transient = transients.get(value);
    if (protocol === IEditableCollection) return vectors.has(value) && !mapEntries.has(value) && !subvecSources.has(value) || maps.has(value) && !recordTypes.has(value) && !sortedComparators.has(value) || sets.has(value) && !sortedComparators.has(value);
    if (protocol === ITransientCollection) return !!transient;
    if (protocol === ITransientAssociative) return !!transient && transient.kind !== "set";
    if (protocol === ITransientVector || protocol === IIndexed && transient) return transient?.kind === "vector";
    if (protocol === ITransientMap) return transient?.kind === "map";
    if (protocol === ITransientSet) return transient?.kind === "set";
    if (transient && (protocol === ICounted || protocol === ILookup || protocol === IFn)) return true;
    if (transient) return false;
    if (protocol === Fn) return metaFunctions.has(value) || vars.has(value);
    if (protocol === IList) return ["list", "cons"].includes(cells.get(value)?.kind);
    if (protocol === IFn) return !!ifnMethods(value) || isIdent(value) || vectors.has(value) || (maps.has(value) && !recordTypes.has(value)) || sets.has(value) || vars.has(value);
    if (protocol === IChunk) return chunks.has(value);
    if (protocol === IChunkedNext) return ["range", "chunked-cons"].includes(cells.get(value)?.kind) || views.get(value)?.kind === "chunked";
    if (protocol === IDrop) return mapKinds.get(value) === "array-map" || cells.get(value)?.integer === true || cells.get(value)?.kind === "repeat" || vectors.has(value) || ["indexed", "chunked", "array-map"].includes(views.get(value)?.kind);
    if (protocol === IReduce) return chunks.has(value) && !chunks.get(value).integer || vectors.has(value) || (maps.has(value) && !recordTypes.has(value)) || cells.has(value) || lazyValues.has(value) || (views.has(value) && views.get(value).kind !== "queue");
    if (protocol === IKVReduce) return vectors.has(value) && !mapEntries.has(value) || maps.has(value);
    if (protocol === IPrintWithWriter || protocol === IHash || protocol === IEquiv) return ownedProtocolValue(value);
    if (protocol === IMapEntry) return mapEntries.has(value);
    if (protocol === IFind) return vectors.has(value) || (maps.has(value) && !recordTypes.has(value));
    if (protocol === ISorted) return sortedComparators.has(value);
    if (protocol === IChunkedSeq) return ["range", "chunked-cons"].includes(cells.get(value)?.kind) || views.get(value)?.kind === "chunked";
    if (protocol === ICloneable) return cloneablePayload(value);
    if (protocol === IVector) return vectors.has(value);
    if (protocol === IMap) return maps.has(value);
    if (protocol === ISet) return sets.has(value);
    if (protocol === ICollection) return [vectors, maps, sets, cells, views, queues, lazyValues].some(store => store.has(value));
    if (protocol === IEmptyableCollection) return !recordTypes.has(value) && builtinCoreImplementation(ICollection, value);
    if (protocol === IStack) return vectors.has(value) || cells.has(value) || queues.has(value);
    if (protocol === IReversible) return vectors.has(value) || views.get(value)?.kind === "indexed" || sortedComparators.has(value);
    if (protocol === IIterable) return builtinCoreImplementation(ICollection, value);
    if (protocol === IComparable) return vectors.has(value) || uuids.has(value) || symbols.has(value) || keywords.has(value) || value instanceof Date;
    if (protocol === ISeqable) return [vectors, maps, sets, cells, views, queues, lazyValues].some(store => store.has(value));
    if (protocol === ISeq || protocol === INext) return protocol === ISeq && chunks.get(value)?.integer === true || cells.has(value) || views.has(value) || queues.has(value) || lazyValues.has(value);
    if (protocol === ISequential) return vectors.has(value) || cells.has(value) || views.has(value) || queues.has(value) || lazyValues.has(value);
    if (protocol === ICounted) return cells.get(value)?.kind === "tree-seq" || chunkBuffers.has(value) || chunks.has(value) || cells.get(value)?.integer === true || vectors.has(value) || maps.has(value) || sets.has(value) || queues.has(value) || cells.get(value)?.kind === "list" || (views.has(value) && ["indexed", "array-map", "sorted-map", "chunked"].includes(views.get(value).kind));
    if (protocol === IIndexed) return chunks.has(value) || cells.get(value)?.integer === true || vectors.has(value) || views.get(value)?.indexed === true;
    if (protocol === ILookup) return vectors.has(value) || maps.has(value) || sets.has(value);
    if (protocol === IAssociative) return vectors.has(value) || maps.has(value);
    if (protocol === IMeta) return [symbols, vectors, maps, sets, cells, views, queues, lazyValues, vars, atoms, metaFunctions].some(store => store.has(value)) || reifyValues.has(value);
    if (protocol === IWithMeta) return [symbols, vectors, maps, sets, cells, views, queues, lazyValues, vars, metaFunctions].some(store => store.has(value)) || reifyValues.has(value);
    if (protocol === IAtom) return atoms.has(value) || !!directMethod(value, "IAtom", 0);
    if (protocol === IPending) return ["repeat", "cycle", "iterate"].includes(cells.get(value)?.kind) || delays.has(value) || lazyValues.has(value);
    if (protocol === IDeref) return delays.has(value) || vars.has(value) || atoms.has(value) || volatiles.has(value) || reducedValues.has(value);
    if (protocol === IReset) return false;
    if (protocol === ISwap) return false;
    if (protocol === IVolatile) return volatiles.has(value);
    if (protocol === IWatchable) return atoms.has(value);
    return false;
  }
  //@unit metaProtocol requires: meta, metaSupported
  function metaProtocol(value) {
    if (!metaSupported(value)) throw new Error("No protocol method IMeta.-meta defined for this value");
    return meta(value);
  }
  //@unit withMetaProtocol requires: withMeta, withMetaSupported
  function withMetaProtocol(value, metadata) {
    if (!withMetaSupported(value)) throw new Error("No protocol method IWithMeta.-with-meta defined for this value");
    return withMeta(value, metadata);
  }
  //@unit preserveMeta requires: meta, metadataValues
  function preserveMeta(original, result) {
    const metadata = meta(original);
    if (metadata != null || metadataValues.has(original)) metadataValues.set(result, metadata);
    return result;
  }
  //@unit varyMeta requires: apply, meta, withMeta
  function varyMeta(value, fn, ...args) { return withMeta(value, apply(fn, meta(value), args)); }
  //@unit resetMeta
  // CLJS reference metadata operations assign .meta; Var uses a separate
  // immutable _meta, so reset-meta! does not change what (meta #'x) observes.
  function resetMeta(value, metadata) { value.meta = metadata; return metadata; }
  //@unit alterMeta requires: apply, resetMeta
  function alterMeta(value, fn, ...args) { return resetMeta(value, apply(fn, value.meta, args)); }
  //@unit Atom requires: atoms, objectEquiv
  class Atom {
    equiv(other) { return objectEquiv(this, other); }
    constructor(state, meta, validator, watches) {
      this.state = state; this.meta = meta; this.validator = validator; this.watches = watches;
      atoms.set(this, this);
    }
  }
  //@unit Volatile requires: volatiles
  class Volatile {
    constructor(state) { this.state = state; volatiles.set(this, this); }
  }
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
  const multiFns = new WeakSet();
  //@unit makeHierarchy requires: invalidArity, keyword, mapFromArray
  function makeHierarchy() {
    if (arguments.length) return invalidArity(arguments.length);
    return mapFromArray([keyword(null, "parents"), mapFromArray([]), keyword(null, "ancestors"), mapFromArray([]), keyword(null, "descendants"), mapFromArray([])]);
  }
  //@unit getGlobalHierarchy requires: atom, globalHierarchy, invalidArity, makeHierarchy
  function getGlobalHierarchy() {
    if (arguments.length) return invalidArity(arguments.length);
    return globalHierarchy ??= atom(makeHierarchy());
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
  //@unit isa requires: contains, deref, equalPair, get, getGlobalHierarchy, hierarchyPart,
  //@ invalidArity, supers, toArray, vectors
  function isa(h, child, parent) {
    if (arguments.length === 2) return isa(deref(getGlobalHierarchy()), h, child);
    if (arguments.length !== 3) return invalidArity(arguments.length);
    if (equalPair(child, parent)) return true;
    if (typeof parent === "function" && typeof child === "function" && child instanceof parent) return true;
    const ancestors = hierarchyPart(h, "ancestors");
    if (contains(get(ancestors, child), parent)) return true;
    if (typeof child === "function") for (const base of toArray(supers(child))) if (contains(get(ancestors, base), parent)) return true;
    const cv = vectors.get(child), pv = vectors.get(parent);
    return !!(cv && pv && cv.length === pv.length && cv.every((v, i) => isa(h, v, pv[i])));
  }
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
  //@ hierarchyPart, invalidArity, isIdent, keyword, mapFromArray, namespace, print, reset,
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
    if (contains(get(a, tag), parent)) throw new Error(print(tag, false) + "already has" + print(parent, false) + "as ancestor");
    if (contains(get(a, parent), tag)) throw new Error("Cyclic derivation:" + print(parent, false) + "has" + print(tag, false) + "as ancestor");
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
  //@unit MultiFn requires: getMethodProtocol, multiFns, print, truth
  class MultiFn {
    constructor(name, dispatchFn, defaultDispatchVal, hierarchy, methodTable, preferTable, methodCache, cachedHierarchy) {
      this.name = name; this.dispatch_fn = dispatchFn; this.default_dispatch_val = defaultDispatchVal;
      this.hierarchy = hierarchy; this.method_table = methodTable; this.prefer_table = preferTable;
      this.method_cache = methodCache; this.cached_hierarchy = cachedHierarchy;
      multiFns.add(this);
    }
    call(_self, ...args) {
      const dispatch = this.dispatch_fn.call(null, ...args), target = getMethodProtocol(this, dispatch);
      if (!truth(target)) throw new Error("No method in multimethod '" + print(this.name, false) + "' for dispatch value: " + print(dispatch, false));
      return target.call(null, ...args);
    }
    apply(_self, args) { return this.call(null, ...Array.from(args)); }
  }
  //@unit demandMulti requires: declarationDemand, print
  function demandMulti(mf) { if (declarationDemand) declarationDemand("multifn:" + print(mf.name, false), mf); }
  //@unit multiState requires: atoms, declarationTransactions, reset
  function multiState(ref, value) {
    // Journal compiler declaration table/cache writes for failed bundles and
    // source-frontier rewinds; ordinary runtime mutations retain source order.
    const transaction = declarationTransactions.at(-1);
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
  //@unit addMethod requires: assoc, declarationTransactions, demandMulti, deref, multiState,
  //@ resetMultiCache
  function addMethod(mf, dispatch, method) {
    // Bundle installation must not demand itself or later method declarations.
    if (!declarationTransactions.length) demandMulti(mf);
    multiState(mf.method_table, assoc(deref(mf.method_table), dispatch, method)); resetMultiCache(mf); return mf;
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
  //@ print, resetMultiCache, setFromArray
  function preferMethod(mf, x, y) {
    demandMulti(mf);
    if (multiPrefers(mf, y, x)) throw new Error("Preference conflict in multimethod '" + print(mf.name, false) + "': " + print(y, false) + " is already preferred to " + print(x, false));
    const table = deref(mf.prefer_table);
    multiState(mf.prefer_table, assoc(table, x, conj(get(table, x, setFromArray([])), y)));
    return resetMultiCache(mf);
  }
  //@unit getMethod requires: assoc, demandMulti, deref, equalPair, get, isa, multiPrefers,
  //@ multiState, nth, print, resetMultiCache, toArray, truth
  function getMethod(mf, dispatch) {
    demandMulti(mf);
    for (;;) {
      const h = deref(mf.hierarchy);
      if (!equalPair(deref(mf.cached_hierarchy), h)) resetMultiCache(mf);
      const cached = get(deref(mf.method_cache), dispatch);
      if (truth(cached)) return cached;
      let best = null;
      const dominates = (x, y) => multiPrefers(mf, x, y) || isa(h, x, y);
      for (const entry of toArray(deref(mf.method_table))) {
        const key = nth(entry, 0);
        if (!isa(h, dispatch, key)) continue;
        const candidate = best === null || dominates(key, nth(best, 0)) ? entry : best;
        if (!dominates(nth(candidate, 0), key)) throw new Error("Multiple methods in multimethod '" + print(mf.name, false) + "' match dispatch value: " + print(dispatch, false) + " -> " + print(key, false) + " and " + print(nth(candidate, 0), false) + ", and neither is preferred");
        best = candidate;
      }
      const target = best === null ? get(deref(mf.method_table), mf.default_dispatch_val) : nth(best, 1);
      if (!truth(target)) return null;
      if (!equalPair(deref(mf.cached_hierarchy), deref(mf.hierarchy))) { resetMultiCache(mf); continue; }
      multiState(mf.method_cache, assoc(deref(mf.method_cache), dispatch, target)); return target;
    }
  }

  //@unit stateProtocol requires: builtin
  function stateProtocol(value, key, args) {
    const method = builtin(value, key, args.length + 1);
    if (!method) throw new Error("No protocol method " + key + " defined for this value");
    return method.call(value, value, ...args);
  }
  //@unit resetProtocol requires: stateProtocol
  function resetProtocol(value, state) { return stateProtocol(value, "IReset/-reset!", [state]); }
  //@unit swapProtocol requires: invalidArity, stateProtocol
  function swapProtocol(value, fn, ...args) {
    if (arguments.length < 2 || arguments.length > 5) return invalidArity(arguments.length);
    return stateProtocol(value, "ISwap/-swap!", [fn, ...args]);
  }
  //@unit notifyWatches requires: atoms, builtin, first, next, nth, seq, stateProtocol
  function notifyWatches(value, oldState, state) {
    const method = builtin(value, "IWatchable/-notify-watches", 3);
    if (method) return method.call(value, value, oldState, state);
    if (!atoms.has(value)) return stateProtocol(value, "IWatchable/-notify-watches", [oldState, state]);
    // Hold the original persistent watch map through all synchronous callbacks.
    for (let s = seq(value.watches); s !== null; s = next(s)) {
      const entry = first(s);
      nth(entry, 1).call(null, nth(entry, 0), value, oldState, state);
    }
    return null;
  }
  //@unit addWatchProtocol requires: assoc, atoms, builtin, stateProtocol
  function addWatchProtocol(value, key, fn) {
    const method = builtin(value, "IWatchable/-add-watch", 3);
    if (method) return method.call(value, value, key, fn);
    if (!atoms.has(value)) return stateProtocol(value, "IWatchable/-add-watch", [key, fn]);
    value.watches = assoc(value.watches, key, fn);
    return value;
  }
  //@unit removeWatchProtocol requires: atoms, builtin, dissoc, stateProtocol
  function removeWatchProtocol(value, key) {
    const method = builtin(value, "IWatchable/-remove-watch", 2);
    if (method) return method.call(value, value, key);
    if (!atoms.has(value)) return stateProtocol(value, "IWatchable/-remove-watch", [key]);
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
  //@unit swapState requires: apply, atoms, deref, first, next, resetAtom, rest, stateProtocol,
  //@ toArray, vector
  function swapState(value, fn, args, pair, variadic = false) {
    if (atoms.has(value)) {
      const state = variadic ? apply(fn, value.state, first(args), first(next(args)), rest(next(args))) : fn.call(null, value.state, ...toArray(args));
      return resetAtom(value, state, pair);
    }
    const oldState = pair ? deref(value) : null;
    const inputs = variadic ? [fn, first(args), first(next(args)), rest(next(args))] : [fn, ...toArray(args)];
    const result = stateProtocol(value, "ISwap/-swap!", inputs);
    return pair ? vector(oldState, result) : result;
  }
  //@unit swap requires: cons, fnRest, swapState
  function swap(value, fn, ...args) {
    if (arguments.length < 2) return swapState(value, fn, cons(undefined, cons(undefined, null)), false, true);
    return swapState(value, fn, fnRest(args, 0), false, args.length > 2);
  }
  //@unit swapVals requires: cons, fnRest, swapState
  function swapVals(value, fn, ...args) {
    if (arguments.length < 2) return swapState(value, fn, cons(undefined, cons(undefined, null)), true, true);
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
  //@unit vresetProtocol requires: builtin, stateProtocol, volatiles
  function vresetProtocol(value, state) {
    const method = builtin(value, "IVolatile/-vreset!", 2);
    if (method) return method.call(value, value, state);
    if (volatiles.has(value)) return value.state = state;
    return stateProtocol(value, "IVolatile/-vreset!", [state]);
  }
  //@unit update requires: apply, assoc, fnRest, get
  function update(value, key, fn, ...args) {
    const old = get(value, key);
    return assoc(value, key, arguments.length >= 3 && arguments.length <= 6
      ? fn.call(null, old, ...args)
      : apply(fn, old, args[0], args[1], args[2], fnRest(args, 3)));
  }
  //@unit updatedMap requires: constructorSlots, equalPair, keyword, makeMap, mapKinds, maps, meta,
  //@ preserveMeta, publicCollectionSlots, recordTypes, sortedComparators
  function updatedMap(original, entries, removeRecord = false) {
    const Type = !removeRecord && recordTypes.get(original);
    if (mapKinds.get(original) === "sorted-map") {
      const prior = maps.get(original);
      if (entries.length === prior.length && entries.every((entry, i) => entry === prior[i])) return original;
      const result = makeMap(entries, "sorted-map", sortedComparators.get(original)), slots = publicCollectionSlots.get(original);
      if (slots && "cnt" in slots) constructorSlots(result, {cnt: slots.cnt + entries.length - prior.length}, meta(original), null);
      return preserveMeta(original, result);
    }
    const result = Type ? new Type(...Type.fieldNames.map(name => {
      const entry = entries.find(e => equalPair(e.key, keyword(null, name)));
      return entry?.value;
    })) : makeMap(entries, mapKinds.get(original) === "hash-map" || (entries.length > 8 && entries.length > (maps.get(original)?.length ?? 0)) ? "hash-map" : "array-map");
    if (Type) { maps.set(result, Object.freeze(entries)); mapKinds.set(result, mapKinds.get(original)); }
    const slots = publicCollectionSlots.get(original);
    if (slots?.cnt != null) {
      const cnt = slots.cnt + entries.length - maps.get(original).length;
      constructorSlots(result, mapKinds.get(result) === "array-map" ? {cnt, arr: entries.flatMap(e => [e.key, e.value])}
        : {cnt, root: result, "has-nil?": entries.some(e => e.key == null), "nil-val": entries.find(e => e.key == null)?.value}, meta(original), null);
    }
    return preserveMeta(original, result);
  }
  //@unit withMeta requires: apply, builtin, canonicalVectorRoots, cells, ChunkedCons,
  //@ ChunkedSeqValue, constructorHashes, cycleNode, field, ifnValues, IntegerRangeValue,
  //@ iterateNode, LazySeq, lazyValues, makeIFn, mapEntries, mapKinds, maps, meta, metadataValues,
  //@ metaFunctions, protocolImplementations, publicCollectionSlots, queues, rangeChunks,
  //@ RangeValue, recordTypes, reifyValues, RepeatValue, seq, setKinds, sets, sortedComparators,
  //@ subvecSources, symbols, treeRoots, treeSetMaps, typeFields, vars, vectorFromArray, vectors,
  //@ views
  function withMeta(value, metadata) {
    if (value == null) return null;
    const method = typeof value === "function" ? null : builtin(value, "IWithMeta/-with-meta", 2);
    if (method) return method.call(value, value, metadata);
    if (!recordTypes.has(value) && !mapEntries.has(value) &&
        [vectors, maps, sets, cells, views, queues, lazyValues].some(store => store.has(value)) && meta(value) === metadata) return value;
    let result;
    if (mapEntries.has(value)) return withMeta(vectorFromArray(vectors.get(value)), metadata);
    if (typeof value === "function" || metaFunctions.has(value)) {
      const original = metaFunctions.get(value) || value;
      const methods = {};
      for (let n = 0; n <= 20; n++) methods[n] = function (_self, ...args) { return original.call(null, ...args); };
      methods[21] = function (_self, ...args) { return apply(original, ...args); };
      result = makeIFn(methods);
      metaFunctions.set(result, original);
    }
    else if (lazyValues.has(value)) {
      result = new LazySeq(metadata, () => seq(value), null, lazyValues.get(value).hash);
    }
    else if (cells.get(value)?.kind === "range") {
      const c = cells.get(value), state = rangeChunks.get(c);
      result = c.integer ? IntegerRangeValue(metadata, c.head, c.end, c.step, c.count, constructorHashes.get(value))
        : RangeValue(metadata, c.head, c.end, c.step, state?.chunk, state?.tail, constructorHashes.get(value));
    }
    else if (cells.get(value)?.kind === "chunked-cons") {
      const c = cells.get(value);
      result = new ChunkedCons(c.chunk, c.more, metadata, c.hash);
    }
    else if (cells.get(value)?.kind === "repeat") {
      const c = cells.get(value); result = RepeatValue(metadata, c.remaining, c.head, c.state?.following ?? null, null);
    }
    else if (cells.get(value)?.kind === "iterate") {
      const c = cells.get(value), state = c.state;
      result = iterateNode(c.fn, c.previous, state.value, state.realized, state.following);
    }
    else if (publicCollectionSlots.get(value)?.vec) {
      const c = publicCollectionSlots.get(value); result = ChunkedSeqValue(c.vec, c.node, c.i, c.off, metadata, null);
    }
    else if (cells.get(value)?.kind === "cycle") {
      const c = cells.get(value), state = c.state;
      result = cycleNode(c.all, c.previous, state.current, state.following);
    }
    else if (recordTypes.has(value)) {
      // A record copy owns fresh mutable host fields. Copying accessor
      // descriptors would leave their closures pointing at the old instance.
      const Type = recordTypes.get(value);
      result = new Type(...Type.fieldNames.map(name => field(value, name)));
      maps.set(result, maps.get(value)); mapKinds.set(result, mapKinds.get(value));
    }
    else {
      const stores = [treeRoots, treeSetMaps, canonicalVectorRoots, publicCollectionSlots, constructorHashes, symbols, vectors, maps, mapKinds, setKinds, sortedComparators, sets, cells, views, queues, lazyValues, vars, subvecSources];
      if (!stores.some(store => store.has(value)) && !reifyValues.has(value)) throw new Error("value does not support metadata");
      result = Object.create(Object.getPrototypeOf(value));
      for (const store of [...stores, protocolImplementations, typeFields, recordTypes]) if (store.has(value)) store.set(result, store.get(value));
      if (reifyValues.has(value)) reifyValues.add(result);
      if (ifnValues.has(value)) ifnValues.set(result, ifnValues.get(value));
      if (ifnValues.has(value) || reifyValues.has(value) || recordTypes.has(value)) Object.defineProperties(result, Object.getOwnPropertyDescriptors(value));
    }
    metadataValues.set(result, metadata);
    return result;
  }
  //@unit uuidPrototype requires: uuids
  const uuidPrototype = Object.freeze({toString() { return uuids.get(this); }});
  //@unit UUIDType effect requires: builtinType, uuidPrototype, uuidValue
  const UUIDType = builtinType(function (text, cachedHash) { return uuidValue(text, cachedHash); }, uuidPrototype);
  //@unit uuid requires: uuidValue
  function uuid(text) {
    if (typeof text !== "string") throw new Error("UUID expects a string");
    return uuidValue(text.toLowerCase(), null);
  }
  //@unit uuidValue requires: uuidHashes, uuids, UUIDType
  function uuidValue(text, cachedHash) {
    const value = Object.create(UUIDType.prototype);
    uuids.set(value, text);
    if (cachedHash != null) uuidHashes.set(value, cachedHash);
    Object.defineProperties(value, {uuid: {get() { return uuids.get(this); }},
      __hash: {get() { return uuidHashes.has(this) ? uuidHashes.get(this) : cachedHash; }, set(hash) { uuidHashes.set(this, hash); }}});
    return value;
  }
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
  //@unit makeVar requires: apply, makeIFn, metadataValues, varPrototype, vars
  function makeVar(get, symbol, metadata = null) {
    const methods = {};
    for (let n = 0; n <= 20; n++) methods[n] = function (_self, ...args) { return get.call(null).call(null, ...args); };
    methods[21] = function (_self, ...args) { return apply(get.call(null), ...args); };
    const value = makeIFn(methods);
    Object.setPrototypeOf(value, varPrototype);
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
  //@unit Var.prototype effect: Var requires: Var, varPrototype
  Var.prototype = varPrototype;
  //@unit namespaces
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
  //@unit createNs mutator requires: invalidArity, Namespace, namespaces, str, symbols
  function createNs(name, obj = null) {
    if (arguments.length !== 1 && arguments.length !== 2) return invalidArity(arguments.length);
    if (!symbols.has(name)) throw new TypeError("create-ns requires a symbol");
    const key = str(name);
    if (!namespaces.has(key)) namespaces.set(key, new Namespace(obj, name));
    else if (obj != null) namespaces.get(key).obj = obj;
    return namespaces.get(key);
  }
  //@unit readerCoreNames
  // Pinned source names from core_public_bindings.tsv and core_var_metadata.tsv.
  // Qualification describes source identity and does not demand a core value.
  const readerCoreNames = new Set("* *1 *2 *3 *assert* *clojurescript-version* *command-line-args* *e *eval* *exec-tap-fn* *flush-on-newline* *loaded-libs* *main-cli-fn* *ns* *out* *print-dup* *print-err-fn* *print-fn* *print-fn-bodies* *print-length* *print-level* *print-meta* *print-namespace-maps* *print-newline* *print-readably* *unchecked-arrays* *unchecked-if* *warn-on-infer* + - --destructure-map -> ->> ->ArrayChunk ->ArrayIter ->Atom ->ChunkBuffer ->ChunkedCons ->ChunkedSeq ->Cons ->Cycle ->Delay ->ES6EntriesIterator ->ES6Iterator ->ES6IteratorSeq ->ES6SetEntriesIterator ->Eduction ->Empty ->EmptyList ->IndexedSeq ->IndexedSeqIterator ->IntegerRange ->IntegerRangeChunk ->Iterate ->KeySeq ->Keyword ->LazySeq ->List ->Many ->MapEntry ->MetaFn ->MultiFn ->MultiIterator ->Namespace ->PersistentArrayMap ->PersistentArrayMapIterator ->PersistentArrayMapSeq ->PersistentHashMap ->PersistentHashSet ->PersistentQueue ->PersistentQueueIter ->PersistentQueueSeq ->PersistentTreeMap ->PersistentTreeMapSeq ->PersistentTreeSet ->PersistentVector ->RSeq ->Range ->RangeIterator ->RangedIterator ->RecordIter ->Reduced ->Repeat ->SeqIter ->Single ->StringBufferWriter ->StringIter ->Subvec ->Symbol ->TaggedLiteral ->TransformerIterator ->UUID ->ValSeq ->Var ->Volatile -add-method -add-watch -as-transient -assoc -assoc! -assoc-n -assoc-n! -chunked-first -chunked-next -chunked-rest -clj->js -clone -comparator -compare -conj -conj! -contains-key? -count -default-dispatch-val -deref -deref-with-timeout -disjoin -disjoin! -dispatch-fn -dissoc -dissoc! -drop -drop-first -empty -entry-key -equiv -find -first -flush -get-method -hash -invoke -iterator -js->clj -key -key->js -kv-reduce -lookup -meta -methods -name -namespace -next -notify-watches -nth -peek -persistent! -pop -pop! -pr-writer -prefer-method -prefers -realized? -reduce -remove-method -remove-watch -reset -reset! -rest -rseq -seq -sorted-seq -sorted-seq-from -swap! -val -vreset! -with-meta -write .. / < <= = == > >= APersistentVector ASeq ArrayChunk ArrayIter Atom CHAR_MAP ChunkBuffer ChunkedCons ChunkedSeq Cons Cycle DEMUNGE_MAP DEMUNGE_PATTERN Delay ES6EntriesIterator ES6Iterator ES6IteratorSeq ES6SetEntriesIterator Eduction Empty EmptyList ExceptionInfo Fn IAssociative IAtom IChunk IChunkedNext IChunkedSeq ICloneable ICollection IComparable ICounted IDeref IDerefWithTimeout IDrop IEditableCollection IEmptyableCollection IEncodeClojure IEncodeJS IEquiv IFind IFn IHash IIndexed IIterable IKVReduce IList ILookup IMap IMapEntry IMeta IMultiFn INIT INamed INext IPending IPrintWithWriter IRecord IReduce IReset IReversible ISeq ISeqable ISequential ISet ISorted IStack ISwap ITransientAssociative ITransientCollection ITransientMap ITransientSet ITransientVector IUUID IVector IVolatile IWatchable IWithMeta IWriter IndexedSeq IndexedSeqIterator Inst IntegerRange IntegerRangeChunk Iterate KeySeq Keyword LazySeq List LongImpl MODULE_INFOS MODULE_URIS Many MapEntry MetaFn MultiFn MultiIterator NS_CACHE NaN? Namespace NodeSeq PROTOCOL_SENTINEL PersistentArrayMap PersistentArrayMapIterator PersistentArrayMapSeq PersistentHashMap PersistentHashSet PersistentQueue PersistentQueueIter PersistentQueueSeq PersistentTreeMap PersistentTreeMapSeq PersistentTreeSet PersistentVector RSeq Range RangeIterator RangedIterator RecordIter Reduced Repeat START SeqIter Single StringBufferWriter StringIter Subvec Symbol TaggedLiteral Throwable->map TransformerIterator UUID ValSeq Var Volatile abs aclone add-tap add-to-string-hash-cache add-watch aget alength alter-meta! amap ancestors and any? apply apply-to areduce array array-chunk array-index-of array-iter array-list array-map array-seq array? as-> aset assert assoc assoc! assoc-in associative? atom await bases binding bit-and bit-and-not bit-clear bit-count bit-flip bit-not bit-or bit-set bit-shift-left bit-shift-right bit-shift-right-zero-fill bit-test bit-xor boolean boolean? booleans bounded-count butlast byte bytes case cat char char? chars checked-aget checked-aget' checked-aset checked-aset' chunk chunk-append chunk-buffer chunk-cons chunk-first chunk-next chunk-rest chunked-seq chunked-seq? clj->js clone cloneable? coercive-= coercive-boolean coercive-not coercive-not= coll? comment comp comparator compare compare-and-set! complement completing concat cond cond-> cond->> condp conj conj! cons constantly contains? copy-arguments count counted? create-ns cycle dec declare dedupe default-dispatch-val defmacro defmethod defmulti defn defn- defonce defprotocol defrecord deftype delay delay? demunge deref derive descendants destructure disj disj! dispatch-fn dissoc dissoc! distinct distinct? divide doall dorun doseq dotimes doto double double-array double? doubles drop drop-last drop-while dt->et eduction empty empty? enable-console-print! ensure-reduced equiv-map es6-entries-iterator es6-iterable es6-iterator es6-iterator-seq es6-set-entries-iterator eval even? every-pred every? ex-cause ex-data ex-info ex-message exists? extend-protocol extend-type false? ffirst filter filterv find find-macros-ns find-ns find-ns-obj first flatten float float? floats flush fn fn? fnext fnil for force frequencies gen-apply-to gen-apply-to-simple gensym gensym_counter get get-global-hierarchy get-in get-method get-validator goog-define group-by halt-when hash hash-combine hash-double hash-keyword hash-long hash-map hash-ordered-coll hash-set hash-string hash-string* hash-unordered-coll ident? identical? identity if-let if-not if-some ifind? ifn? implements? import imul inc indexed? infinite? inst-ms inst-ms* inst? instance? int int-array int-rotate-left int? integer? interleave interpose into into-array ints is_proto_ isa? iter iterable? iterate iteration js->clj js-arguments js-comment js-debugger js-delete js-fn? js-in js-inline-comment js-invoke js-iterable? js-keys js-mod js-obj js-reserved js-str js-symbol? juxt keep keep-indexed key key->js key-test keys keyword keyword-identical? keyword? last lazy-cat lazy-seq let letfn list list* list? load-file load-file* locking long long-array longs loop m3-C1 m3-C2 m3-fmix m3-hash-int m3-hash-unencoded-chars m3-mix-H1 m3-mix-K1 m3-seed macroexpand macroexpand-1 make-array make-hierarchy map map-entry? map-indexed map? mapcat mapv max max-key memfn memoize merge merge-with meta methods min min-key missing-protocol mix-collection-hash mk-bound-fn mod munge munge-str name namespace nat-int? native-satisfies? neg-int? neg? newline next nfirst nil-iter nil? nnext not not-any? not-empty not-every? not-native not= ns-imports ns-interns ns-interns* ns-name ns-publics ns-unmap nth nthnext nthrest number? object-array object? odd? or parents parse-boolean parse-double parse-long parse-uuid partial partition partition-all partition-by partitionv partitionv-all peek persistent! persistent-array-map-seq pop pop! pos-int? pos? pr pr-seq-writer pr-sequential-writer pr-str pr-str* pr-str-with-opts prefer-method prefers prim-seq print print-map print-meta? print-prefix-map print-str println println-str prn prn-str prn-str-with-opts qualified-ident? qualified-keyword? qualified-symbol? quot rand rand-int rand-nth random-sample random-uuid range ranged-iterator re-find re-matches re-pattern re-seq realized? record? reduce reduce-kv reduceable? reduced reduced? reductions refer-clojure refer-global regexp? reify rem remove remove-all-methods remove-method remove-tap remove-watch repeat repeatedly replace replicate req! require require-global require-macros reset! reset-meta! reset-vals! resolve rest reverse reversible? rseq rsubseq run! satisfies? second select-keys selector seq seq-iter seq-to-map-for-destructuring seq? seqable? sequence sequential? set set-from-indexed-seq set-lite-check set-print-err-fn! set-print-fn! set-validator! set? short shorts shuffle simple-benchmark simple-ident? simple-keyword? simple-symbol? some some-> some->> some-fn some-vals some? sort sort-by sorted-map sorted-map-by sorted-set sorted-set-by sorted? special-symbol? specify specify! split-at split-with splitv-at spread str str_ string-hash-cache string-hash-cache-count string-iter string-print string? subs subseq subvec supers swap! swap-vals! symbol symbol-identical? symbol? system-time tagged-literal tagged-literal? take take-last take-nth take-while tap> test this-as time to-array to-array-2d trampoline transduce transformer-iterator transient tree-seq true? truth_ type type->str unchecked-add unchecked-add-int unchecked-byte unchecked-char unchecked-dec unchecked-dec-int unchecked-divide-int unchecked-double unchecked-float unchecked-get unchecked-inc unchecked-inc-int unchecked-int unchecked-long unchecked-max unchecked-min unchecked-multiply unchecked-multiply-int unchecked-negate unchecked-negate-int unchecked-remainder-int unchecked-set unchecked-short unchecked-subtract unchecked-subtract-int undefined? underive unreduced unsafe-bit-and unsafe-cast unsigned-bit-shift-right update update-in update-keys update-vals uri? use use-macros uuid uuid? val vals var? vary-meta vec vec-lite vector vector-lite vector? volatile! volatile? vreset! vswap! when when-first when-let when-not when-some while with-meta with-out-str with-redefs write-all zero? zipmap".split(" "));
  //@unit registerReaderNamespace requires: createNs, symbol
  function registerReaderNamespace(name, context) {
    const ns = createNs(symbol(null, name));
    ns.readerContext = {aliases: new Map(context.aliases ?? []),
      refers: new Map((context.refers ?? []).map(([local, ns, name]) => [local, [ns, name]])),
      excludes: new Set(context.excludes ?? []), bindings: new Set(context.bindings ?? [])};
    return ns;
  }
  //@unit macroReaderFunction requires: withMacroReaderContext
  function macroReaderFunction(fn) {
    const wrapped = function (...args) { return withMacroReaderContext(fn, args); };
    return Object.assign(wrapped, fn);
  }
  //@unit withMacroReaderContext requires: coreCells, createNs, get, keyword, nth, str, toArray
  function withMacroReaderContext(fn, args) {
    const saved = coreCells.currentNs, env = get(args[1], keyword(null, "ns"));
    const name = get(env, keyword(null, "name"));
    if (name == null) return Reflect.apply(fn, null, args);
    const ns = createNs(name);
    const context = ns.readerContext ?? {aliases: new Map(), refers: new Map(), excludes: new Set(), bindings: new Set()};
    const aliases = new Map(context.aliases);
    for (const pair of toArray(get(env, keyword(null, "requires")))) aliases.set(str(nth(pair, 0)), str(nth(pair, 1)));
    const previous = ns.readerContext;
    const view = get(args[1], keyword("light-cljs", "reader-context"));
    const refers = new Map(context.refers), excludes = new Set(context.excludes), bindings = new Set(context.bindings);
    if (view != null) {
      for (const entry of toArray(get(view, keyword(null, "refers")))) refers.set(nth(entry, 0), [nth(entry, 1), nth(entry, 2)]);
      for (const name of toArray(get(view, keyword(null, "excludes")))) excludes.add(name);
      for (const name of toArray(get(view, keyword(null, "bindings")))) bindings.add(name);
    }
    ns.readerContext = {aliases, refers, excludes, bindings}; coreCells.currentNs = ns;
    try { return Reflect.apply(fn, null, args); }
    finally { ns.readerContext = previous; coreCells.currentNs = saved; }
  }
  //@unit findNs requires: invalidArity, Namespace, namespaces, str, symbols
  function findNs(name) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    if (name == null) return null;
    if (name instanceof Namespace) return name;
    if (!symbols.has(name)) throw new TypeError("find-ns requires a symbol or Namespace");
    return namespaces.get(str(name)) ?? null;
  }
  //@unit nsName requires: invalidArity, symbols
  function nsName(value) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    // Pinned ns-name reads .name: metadata symbols expose their string name,
    // while explicitly constructed Namespaces preserve their supplied slot.
    if (symbols.has(value)) return symbols.get(value).name;
    return value.name;
  }
  //@unit theNs requires: findNs, invalidArity, str
  function theNs(name) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    const value = findNs(name);
    if (!value) throw new Error("No namespace: " + str(name));
    return value;
  }
  //@unit allNs requires: invalidArity, listFromArray, namespaces
  function allNs() { if (arguments.length) return invalidArity(arguments.length); return listFromArray([...namespaces.values()]); }
  //@unit compilerMacroVar requires: into, invalidArity, keyword, makeVar, mapFromArray, name,
  //@ namespace, str, symbolCore
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
  //@unit Delay requires: delays
  function Delay(thunk, result) {
    const value = Object.create(Delay.prototype);
    delays.set(value, {thunk, value: result});
    return value;
  }
  //@unit LazySeq requires: lazyValues, metadataValues
  function LazySeq(metadata, thunk, result, cachedHash) {
    const value = Object.create(LazySeq.prototype);
    lazyValues.set(value, {thunk, value: result, hash: cachedHash});
    metadataValues.set(value, metadata);
    return value;
  }
  //@unit lazySequence requires: LazySeq
  function lazySequence(thunk) { return new LazySeq(null, thunk, null, null); }
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
  //@unit reSeq requires: cells, lazySequence, sequencePrototype, vectorFromArray
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
    const result = Object.create(sequencePrototype);
    cells.set(result, Object.freeze({kind: "cons", head: value, tail}));
    return result;
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
  //@unit concatTwo requires: chunkCons, chunkFirst, chunkRest, cons, first, IChunkedSeq,
  //@ implementsProtocol, lazySequence, rest, seq
  function concatTwo(x, y) {
    return lazySequence(() => {
      const s = seq(x);
      if (s === null) return y;
      return implementsProtocol(IChunkedSeq, s) ? chunkCons(chunkFirst(s), concatTwo(chunkRest(s), y)) : cons(first(s), concatTwo(rest(s), y));
    });
  }
  //@unit concatMany requires: chunkCons, chunkFirst, chunkRest, cons, first, IChunkedSeq,
  //@ implementsProtocol, lazySequence, next, rest, seq
  function concatMany(current, remaining) {
    return lazySequence(() => {
      const s = seq(current);
      if (s !== null) return implementsProtocol(IChunkedSeq, s) ? chunkCons(chunkFirst(s), concatMany(chunkRest(s), remaining)) : cons(first(s), concatMany(rest(s), remaining));
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
  //@unit es6EntriesIterator requires: es6Iterator, nth
  function es6EntriesIterator(value, setEntries = false) {
    const iterator = es6Iterator(value);
    return {next() {
      const item = iterator.next();
      if (!item.done) item.value = setEntries ? [item.value, item.value] : [nth(item.value, 0), nth(item.value, 1)];
      return item;
    }};
  }
  //@unit sequenceIterator requires: IIterable, isSeqable, iteratorInit, iteratorProtocol,
  //@ satisfies, SeqIter, str
  function sequenceIterator(value) {
    if (satisfies(IIterable, value)) return iteratorProtocol(value);
    if (value == null) return {hasNext() { return false; }, next() { return new Error("No such element"); }, remove() { return new Error("Unsupported operation"); }};
    if (typeof value === "string" || Array.isArray(value)) {
      let index = 0;
      return {hasNext() { return index < value.length; }, next() { return typeof value === "string" ? value.charAt(index++) : value[index++]; }, remove() { return new Error("Unsupported operation"); }};
    }
    if (isSeqable(value)) return new SeqIter(iteratorInit, value);
    throw new Error("Cannot create iterator from " + str(value));
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
  //@unit Eduction requires: eductions
  function Eduction(xform, coll) {
    // Retain only the view's inputs; each protocol call owns its traversal state.
    // This is not a collection payload with structural equality or metadata.
    const value = Object.create(Eduction.prototype);
    eductions.set(value, {xform, coll});
    Object.defineProperties(value, {xform: {value: xform}, coll: {value: coll}});
    return value;
  }
  //@unit eduction requires: callable, comp, Eduction, toArray
  const eduction = callable(0, {}, args => {
    const values = toArray(args);
    return new Eduction(comp(...values.slice(0, -1)), values.length ? values.at(-1) : null);
  });
  //@unit fnRest requires: view
  function fnRest(args, start) {
    return args.length > start ? view(Object.freeze(Array.prototype.slice.call(args, start)), 0, true) : null;
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
  //@unit ifnInvokeEntry requires: fnApplyTo
  // This is the callable object ABI used by IFn and metadata wrappers.
  // The last method receives twenty values followed by a sequence tail.
  function ifnInvokeEntry(method) {
    const entry = function (...args) { return method.call(this, this, ...args); };
    const delegate = method.cljs$core$IFn$_invoke$arity$variadic;
    if (delegate) {
      entry.cljs$core$IFn$_invoke$arity$variadic = function (...args) { return delegate.call(this, this, ...args); };
      entry.cljs$lang$maxFixedArity = method.cljs$lang$maxFixedArity - 1;
      entry.cljs$lang$applyTo = args => fnApplyTo(entry.cljs$core$IFn$_invoke$arity$variadic, entry.cljs$lang$maxFixedArity, args);
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
  const nativeMethods = new Map(), coreDispatchers = new Map();
  //@unit protocol mutator requires: protocolNames, protocolValues, setDeclaration
  function protocol(name) {
    const value = function () {};
    protocolNames.set(value, name); setDeclaration(protocolValues, name, value);
    return value;
  }
  //@unit nativeType
  function nativeType(value) { return value == null ? "null" : Array.isArray(value) ? "array" : typeof value; }
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
    const slot = arity === 0 ? prefix : prefix + (method === "/" ? "_SLASH_" : mungeMember(method)) + "$arity$" + arity;
    slots.set(arity, slot); return slot;
  }
  //@unit directMethod requires: demandProtocol, protocolSentinel, protocolSlot
  function directMethod(value, key, arity) {
    demandProtocol(arity === 0 ? key : key.slice(0, key.lastIndexOf("/")), value);
    if (value == null) return undefined;
    const method = value[protocolSlot(key, arity)];
    return arity === 0 ? (method === protocolSentinel ? method : undefined) : method;
  }
  //@unit protocolStores requires: atoms, cells, delays, keywords, lazyValues, maps, metaFunctions,
  //@ multiFns, queues, reducedValues, sets, symbols, transients, vars, vectors, views, volatiles
  const protocolStores = [multiFns, transients, keywords, symbols, vectors, maps, sets, cells, views, queues, lazyValues, delays, vars, atoms, volatiles, reducedValues, metaFunctions];
  //@unit ownedProtocolValue requires: protocolStores
  function ownedProtocolValue(value) { return protocolStores.some(store => store.has(value)); }
  //@unit builtin requires: builtinCoreImplementation, coreDispatchers, coreProtocolsByName,
  //@ directMethod, nativeMethods, nativeType
  function builtin(value, key, arity) {
    const method = directMethod(value, key, arity);
    if (method != null) return method;
    const external = coreDispatchers.get(key);
    const dispatch = external?.[nativeType(value)] ?? external?._;
    if (!nativeMethods.size && !dispatch) return undefined;
    const name = key.split("/")[0];
    const protocol = coreProtocolsByName.get(name);
    // Native-object fallback cannot override an owned collection's direct
    // core implementation. Its public operation handles the private payload.
    if (protocol && builtinCoreImplementation(protocol, value)) return undefined;
    return dispatch ?? nativeMethods.get(key)?.get(nativeType(value))?.get(arity) ?? nativeMethods.get(key)?.get("_")?.get(arity);
  }
  //@unit publicMethod requires: builtin, builtinCoreImplementation, coreProtocolsByName, IMeta,
  //@ implementsProtocol, satisfies
  // Public consumers test protocol membership before selecting an arity. A
  // declared but omitted method must not silently fall back to another API.
  function publicMethod(value, key, arity, fast = false, directOnly = false) {
    const protocol = coreProtocolsByName.get(key.split("/")[0]);
    const direct = implementsProtocol(protocol, value);
    if (!direct && (directOnly || (fast && (Array.isArray(value) || typeof value === "string")))) return undefined;
    if (!direct && !satisfies(protocol, value)) return undefined;
    const method = builtin(value, key, arity);
    if (method) return method;
    if (protocol === IMeta && typeof value === "function") return undefined;
    if (satisfies(protocol, value) && !builtinCoreImplementation(protocol, value))
      throw new Error("No protocol method " + key + " defined for this value");
    return undefined;
  }
  //@unit protocolMethod mutator requires: directMethod, fnRest, get, invalidArity, meta,
  //@ nativeType, protocolFunctions, protocolGetters, protocolKeys, protocolVariables,
  //@ setDeclaration, symbolCore, truth
  function protocolMethod(key, metadataName, arities, viaMetadata = false, variadic = null, getter = null) {
    const dispatch = function (receiver, ...args) {
      const fixed = arities?.filter(n => n !== (variadic == null ? -1 : variadic + 2));
      const variadicCall = variadic != null && !fixed.includes(arguments.length);
      const arity = variadicCall ? variadic + 2 : variadic == null && arities?.length === 1 ? arities[0] : arguments.length;
      if (!variadicCall && fixed?.length > 1 && !fixed.includes(arity)) return invalidArity(arguments.length);
      if (variadicCall) {
        // CLJS accepts these declarations with a warning. Its macro's literal
        // signature expansion forwards the unbound ampersand and packed tail.
        const actual = [receiver, ...args];
        args = [...Array.from({length: variadic - 1}, (_, i) => actual[i + 1]), undefined, fnRest(actual, variadic)];
      } else if (variadic == null && arities?.length === 1) args = Array.from({length: arity - 1}, (_, i) => args[i]);
      if (viaMetadata) {
        const implementation = get(meta(receiver), symbolCore(metadataName));
        if (truth(implementation)) return implementation.call(null, receiver, ...args);
      }
      const method = directMethod(receiver, key, arity);
      if (method != null) return method.call(receiver, receiver, ...args);
      // Saved method values retain their direct slot while dynamic dispatch
      // observes the current method Var, as the CLJS macro expansion does.
      const current = getter ? getter() : protocolVariables.get(metadataName) ?? dispatch;
      const external = current[nativeType(receiver)] ?? current._;
      if (external != null) return external.call(null, receiver, ...args);
      throw new Error("No protocol method " + metadataName + " defined for this value");
    };
    protocolKeys.set(dispatch, key); setDeclaration(protocolFunctions, key, metadataName); setDeclaration(protocolVariables, metadataName, dispatch);
    setDeclaration(protocolGetters, key, getter ?? (() => protocolVariables.get(metadataName)));
    return dispatch;
  }
  //@unit extendType mutator requires: coreDispatchers, coreProtocolsByName, installMethods,
  //@ invalidArity, nativeMethods, patchDeclaration, protocolFunctions, protocolGetters,
  //@ protocolSentinel, protocolValues, setDeclaration
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
        const dispatch = protocolGetters.get(key)?.() ?? coreDispatchers.get(key);
        const implementation = function (...args) {
          const method = implementations.size === 1 ? implementations.values().next().value : implementations.get(args.length);
          return method ? method.call(null, ...args) : invalidArity(args.length);
        };
        result = implementation;
        if (dispatch) patchDeclaration(dispatch, target, {value: implementation, writable: true, configurable: true, enumerable: true});
        if (!protocolFunctions.has(key)) {
          if (!nativeMethods.has(key)) setDeclaration(nativeMethods, key, new Map());
          setDeclaration(nativeMethods.get(key), target, implementations);
        }
      }
    } else {
      if (typeof target !== "function" || target.prototype == null) throw new TypeError("extend-type requires a type constructor");
      installMethods(target.prototype, methods);
      result = methods.length ? (methods.at(-1)[1] === 0 ? protocolSentinel : methods.at(-1)[2]) : null;
    }
    return result;
  }
  //@unit extendInstance mutator requires: installMethods, protocolSentinel
  function extendInstance(target, methods, returnTarget) {
    installMethods(target, methods);
    return returnTarget ? target : methods.length ? (methods.at(-1)[1] === 0 ? protocolSentinel : methods.at(-1)[2]) : null;
  }
  //@unit installMethods mutator requires: declarationTransactions, ifnInvokeEntry, ifnPrototype,
  //@ ifnValues, ifnVariadics, invalidArity, mungeMember, patchDeclaration, protocolImplementations,
  //@ protocolKeys, protocolSentinel, protocolSlot
  function installMethods(value, methods) {
    if (value != null && typeof value !== "object" && typeof value !== "function") return value;
    const oldImplementations = protocolImplementations.get(value), oldCalls = ifnValues.get(value);
    const transaction = declarationTransactions.at(-1);
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
  //@unit reify requires: field, makeType, metadataValues, reifySites, reifyValues
  function reify(site, fields, values, methods, metadata = null) {
    let Type = reifySites.get(site);
    if (!Type) {
      Type = makeType(fields, [
        ["IMeta", 0, null], ["IWithMeta", 0, null],
        ["IMeta/-meta", 1, function () { return metadataValues.has(this) ? metadataValues.get(this) : null; }],
        ["IWithMeta/-with-meta", 2, function (_self, metadata) {
          const result = new Type(...fields.map(name => field(this, name)));
          reifyValues.add(result);
          metadataValues.set(result, metadata);
          return result;
        }], ...methods()], false);
      reifySites.set(site, Type);
    }
    const value = new Type(...values);
    metadataValues.set(value, metadata);
    reifyValues.add(value);
    return value;
  }
  //@unit field requires: mungeMember
  function field(value, name) { return value[mungeMember(name)]; }
  //@unit memberMunging
  const memberMunging = {"-":"_",":":"_COLON_","+":"_PLUS_",">":"_GT_","<":"_LT_","=":"_EQ_","~":"_TILDE_","!":"_BANG_","@":"_CIRCA_","#":"_SHARP_","'":"_SINGLEQUOTE_","\"":"_DOUBLEQUOTE_","%":"_PERCENT_","^":"_CARET_","&":"_AMPERSAND_","*":"_STAR_","|":"_BAR_","{":"_LBRACE_","}":"_RBRACE_","[":"_LBRACK_","]":"_RBRACK_","/":"$","\\":"_BSLASH_","?":"_QMARK_"};
  //@unit mungeMember requires: memberMunging
  function mungeMember(name) { return Array.from(name, c => memberMunging[c] ?? c).join(""); }
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
  //@unit munge requires: mungeStr, reservedNames, str, symbolCore, symbols
  function munge(value) { let result = mungeStr(str(value)); result = result === ".." ? "_DOT__DOT_" : reservedNames.has(result) ? result + "$" : result; return symbols.has(value) ? symbolCore(result) : result; }
  //@unit demunge requires: demungeCharacters, demungePattern, str, symbolCore, symbols
  function demunge(value) { let result = str(value); result = result === "_DOT__DOT_" ? ".." : result.replace(/\$$/, "").replace(demungePattern, key => key === "$" ? "/" : demungeCharacters[key]); return symbols.has(value) ? symbolCore(result) : result; }
  //@unit mapEntry requires: mapEntries, MapEntryType, vector
  // Surface records supply protocol methods just as the upstream macro does.
  // Raw defrecord* only emits a constructor; its explicit body installs methods.
  function mapEntry(key, value) { const pair = vector(key, value); Object.setPrototypeOf(pair, MapEntryType.prototype); mapEntries.add(pair); return pair; }
  //@unit recordEntries requires: field, keyword, mapEntry, toArray
  function recordEntries(value, Type) {
    return [...Type.fieldNames.map(name => mapEntry(keyword(null, name), field(value, name))), ...toArray(value.__extmap)];
  }
  //@unit recordCopy requires: field
  function recordCopy(value, Type, metadata, extmap, cachedHash) {
    const Constructor = Type.currentConstructor?.() ?? Type;
    return new Constructor(...Type.fieldNames.map(name => field(value, name)), metadata, extmap, cachedHash);
  }
  //@unit recordMethods requires: assoc, conj, contains, count, dissoc, equalPair, field, get, hash,
  //@ into, isVector, iteratorInit, keyword, mapFromArray, nth, printWriter, prSequentialWriter,
  //@ recordCopy, recordEntries, reduce, seq, SeqIter, stringHash, typeNames, vectorFromArray,
  //@ withMeta
  function recordMethods(Type) {
    const copy = (value, metadata, extmap, cachedHash) => recordCopy(value, Type, metadata, extmap, cachedHash);
    const fieldIndex = key => Type.fieldNames.findIndex(name => equalPair(key, keyword(null, name)));
    const methods = [
      ["ICloneable/-clone", 1, function () { return recordCopy(this, Type, this.__meta, this.__extmap, this.__hash); }],
      ["IHash/-hash", 1, function () {
        if (this.__hash != null) return this.__hash;
        let result = 0;
        for (const entry of recordEntries(this, Type)) result = (result + (hash(nth(entry, 0)) ^ hash(nth(entry, 1)))) | 0;
        return this.__hash = result ^ stringHash(typeNames.get(Type) ?? "");
      }],
      ["IEquiv/-equiv", 2, function (_self, other) {
        return other != null && this.constructor === other.constructor &&
          Type.fieldNames.every(name => equalPair(field(this, name), field(other, name))) && equalPair(this.__extmap, other.__extmap);
      }],
      ["IMeta/-meta", 1, function () { return this.__meta; }],
      ["IWithMeta/-with-meta", 2, function (_self, metadata) { return copy(this, metadata, this.__extmap, this.__hash); }],
      ["ILookup/-lookup", 2, function (_self, key) { return get(this, key, null); }],
      ["ILookup/-lookup", 3, function (_self, key, fallback) {
        const index = fieldIndex(key); return index < 0 ? get(this.__extmap, key, fallback) : field(this, Type.fieldNames[index]);
      }],
      ["ICounted/-count", 1, function () { return Type.fieldNames.length + count(this.__extmap); }],
      ["ICollection/-conj", 2, function (_self, entry) {
        if (isVector(entry)) return assoc(this, nth(entry, 0), nth(entry, 1));
        return reduce((result, pair) => conj(result, pair), this, entry);
      }],
      ["IAssociative/-contains-key?", 2, function (_self, key) { return fieldIndex(key) >= 0 || contains(this.__extmap, key); }],
      ["IAssociative/-assoc", 3, function (_self, key, value) {
        const index = fieldIndex(key);
        if (index < 0) return copy(this, this.__meta, assoc(this.__extmap, key, value), null);
        const Constructor = Type.currentConstructor?.() ?? Type;
        return new Constructor(...Type.fieldNames.map((name, i) => i === index ? value : field(this, name)), this.__meta, this.__extmap, null);
      }],
      ["IMap/-dissoc", 2, function (_self, key) {
        if (fieldIndex(key) >= 0) return dissoc(withMeta(into(mapFromArray([]), this), this.__meta), key);
        const extmap = dissoc(this.__extmap, key);
        return copy(this, this.__meta, seq(extmap) === null ? null : extmap, null);
      }],
      ["ISeqable/-seq", 1, function () { return seq(vectorFromArray(recordEntries(this, Type))); }],
      ["IIterable/-iterator", 1, function () { return new SeqIter(iteratorInit, this); }],
      ["IPrintWithWriter/-pr-writer", 3, function (_self, writer, options) {
        return prSequentialWriter(writer, (entry, target, opts) => prSequentialWriter(target, printWriter, "", " ", "", opts, entry),
          "#" + (typeNames.get(Type) ?? "").replaceAll("/", ".") + "{", ", ", "}", options, recordEntries(this, Type));
      }],
      ["IKVReduce/-kv-reduce", 3, function (_self, fn, init) {
        return reduce((result, entry) => fn.call(null, result, nth(entry, 0), nth(entry, 1)), init, this);
      }]
    ];
    return [["IRecord", 0, null], ...Array.from(new Set(methods.map(([key]) => key.split("/")[0])), key => [key, 0, null]), ...methods];
  }
  //@unit makeType requires: ifnValues, installMethods, mapKinds, maps, mungeMember,
  //@ protocolMaskNames, recordMethods, recordTypes, typeFields, typeNames
  function makeType(fields, methods, record, masks = null, name = null, currentConstructor = null) {
    let implicitMasks;
    const allFields = record ? [...fields, "__meta", "__extmap", "__hash"] : fields;
    const Type = function (...args) {
      const value = this, data = Object.create(null);
      for (let i = 0; i < allFields.length; i++) {
        const name = allFields[i]; data[name] = args[i];
        Object.defineProperty(value, mungeMember(name), {enumerable: true, configurable: true,
          get() { return data[name]; },
          set(state) { data[name] = state; }});
      }
      if (masks != null) for (const entry of maps.get(masks) ?? []) value["cljs$lang$protocol_mask$partition" + entry.key + "$"] = entry.value;
      else for (let i = 0; i < implicitMasks.length; i++) value["cljs$lang$protocol_mask$partition" + i + "$"] = implicitMasks[i];
      typeFields.set(value, data);
      if (ifnValues.has(Type.prototype)) ifnValues.set(value, ifnValues.get(Type.prototype));
      if (record === true) { recordTypes.set(value, Type); mapKinds.set(value, "record"); maps.set(value, Object.freeze([])); }
    };
    Type.fieldNames = fields;
    if (currentConstructor) Type.currentConstructor = currentConstructor;
    if (name != null) typeNames.set(Type, name);
    const implementations = record === true ? [...methods, ...recordMethods(Type)] : methods;
    implicitMasks = [0, 0];
    for (const [key, arity] of implementations) {
      const index = arity === 0 ? protocolMaskNames.lastIndexOf(key) : -1;
      if (index >= 0) implicitMasks[index >> 5] |= 1 << (index % 32);
    }
    installMethods(Type.prototype, implementations);
    return Type;
  }
  //@unit recordFromMap requires: dissoc, get, into, isRecord, keyword, mapFromArray, seq
  function recordFromMap(Type, map) {
    let extmap = dissoc(map, ...Type.fieldNames.map(name => keyword(null, name)));
    if (isRecord(map)) extmap = into(mapFromArray([]), extmap);
    return new Type(...Type.fieldNames.map(name => get(map, keyword(null, name))), null, seq(extmap) === null ? null : extmap, null);
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
  //@unit boundedCount requires: count, isCounted, next, seq
  function boundedCount(n, value) {
    if (isCounted(value)) return count(value);
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
  //@unit iterateNode requires: cells, first, iterateUnrealizedSeed, IterateValue
  function iterateNode(fn, previous, value, realized, following = null) {
    const result = Object.create(IterateValue.prototype), state = {value, realized, following};
    cells.set(result, Object.freeze({kind: "iterate", fn, previous, state,
      get realized() { return state.realized; },
      get head() { if (!state.realized) { state.value = fn.call(null, previous); state.realized = true; } return state.value; },
      get tail() { return state.following ?? (state.following = iterateNode(fn, first(result), iterateUnrealizedSeed, false)); }}));
    return result;
  }
  //@unit iterate requires: iterateNode
  function iterate(fn, seed) { return iterateNode(fn, null, seed, true); }
  //@unit chunks
  // Chunk values expose indexed/count/reduce protocols, without being seqs.
  const chunks = new WeakMap(), chunkBuffers = new WeakMap();
  //@unit ChunkBuffer requires: chunkBuffers
  function ChunkBuffer(array, end) {
    const value = Object.create(ChunkBuffer.prototype);
    chunkBuffers.set(value, {array, end});
    return value;
  }
  //@unit chunkBuffer requires: ChunkBuffer
  function chunkBuffer(capacity) { return new ChunkBuffer(new Array(capacity), 0); }
  //@unit chunkAppend requires: chunkBuffers
  function chunkAppend(buffer, item) { const state = chunkBuffers.get(buffer); state.array[state.end] = item; return ++state.end; }
  //@unit chunk requires: arrayChunk, chunkBuffers
  function chunk(buffer) { const state = chunkBuffers.get(buffer), value = arrayChunk(state.array, 0, state.end); state.array = null; return value; }
  //@unit arrayChunk requires: chunks
  function arrayChunk(array, offset = 0, end = array.length) {
    const result = Object.create(arrayChunk.prototype);
    chunks.set(result, {array, offset, end});
    return result;
  }
  //@unit chunkNth requires: chunks, missing
  function chunkNth(chunk, i, fallback = missing) {
    const state = chunks.get(chunk), size = state.end - state.offset;
    if (state.integer && fallback === missing) return state.start + i * state.step;
    if (i >= 0 && i < size) return state.integer ? state.start + i * state.step : state.array[state.offset + i];
    if (fallback !== missing) return fallback;
    // ArrayChunk's two-argument -nth reads the backing array directly.
    return state.array?.[state.offset + i];
  }
  //@unit chunkFirst requires: chunkedFirstProtocol
  function chunkFirst(value) { return chunkedFirstProtocol(value); }
  //@unit chunkRest requires: chunkedRestProtocol
  function chunkRest(value) { return chunkedRestProtocol(value); }
  //@unit chunkNext requires: chunkedNextProtocol, chunkRest, IChunkedNext, implementsProtocol, seq
  function chunkNext(value) {
    return implementsProtocol(IChunkedNext, value) ? chunkedNextProtocol(value) : seq(chunkRest(value));
  }
  //@unit ownedChunkFirst requires: arrayChunk, cells, chunks, forceRangeChunk,
  //@ publicCollectionSlots, views
  function ownedChunkFirst(value) {
    const cell = cells.get(value);
    if (cell?.kind === "chunked-cons") return cell.chunk;
    if (cell?.kind === "range") {
      if (cell.integer) {
        const chunk = arrayChunk([], 0, Math.min(32, cell.count));
        chunks.set(chunk, {...chunks.get(chunk), integer: true, start: cell.head, step: cell.step});
        return chunk;
      }
      return forceRangeChunk(cell).chunk;
    }
    const slice = views.get(value);
    if (publicCollectionSlots.has(value)) return arrayChunk(slice.storage, slice.offset);
    const end = Math.min(slice.storage.length, slice.offset + 32 - slice.offset % 32);
    return arrayChunk(slice.storage, slice.offset, end);
  }
  //@unit rangeChunks
  const rangeChunks = new WeakMap();
  //@unit forceRangeChunk requires: arrayChunk, emptyList, rangeChunks, rangeNode
  function forceRangeChunk(cell) {
    if (!rangeChunks.has(cell) || rangeChunks.get(cell).chunk == null) {
      const values = []; let current = cell.head;
      do { values.push(current); current += cell.step; }
      while (values.length < 32 && (cell.step > 0 ? current < cell.end : current > cell.end));
      const tail = (cell.step > 0 ? current < cell.end : current > cell.end) ? rangeNode(current, cell.end, cell.step, false) : emptyList;
      const state = rangeChunks.get(cell) ?? {};
      state.chunk = arrayChunk(values);
      if (values.length === 32 && tail !== emptyList) state.tail = tail;
      rangeChunks.set(cell, state);
    }
    return rangeChunks.get(cell);
  }
  //@unit ownedChunkRest requires: cells, ChunkedSeqValue, count, emptyList, forceRangeChunk,
  //@ publicCollectionSlots, rangeNode, vectors, view, views
  function ownedChunkRest(value) {
    const cell = cells.get(value);
    if (cell?.kind === "chunked-cons") return cell.more ?? emptyList;
    if (cell?.kind === "range") return cell.integer ? (cell.count <= 32 ? emptyList : rangeNode(cell.head + 32 * cell.step, cell.end, cell.step, true)) : forceRangeChunk(cell).tail ?? emptyList;
    const slice = views.get(value), slots = publicCollectionSlots.get(value);
    if (slots?.vec) {
      const end = slots.i + slots.node.length;
      return end < count(slots.vec) ? ChunkedSeqValue(slots.vec, vectors.get(slots.vec).slice((end >>> 5) << 5, ((end >>> 5) << 5) + 32), end, 0, null, null) : emptyList;
    }
    const end = Math.min(slice.storage.length, slice.offset + 32 - slice.offset % 32);
    return end < slice.storage.length ? view(slice.storage, end, slice.indexed, slice.kind, slice.tail) : slice.tail ?? emptyList;
  }
  //@unit ChunkedCons requires: cells, count, dropFirstProtocol, emptyList, metadataValues, nth
  function ChunkedCons(chunk, more, metadata, cachedHash) {
    const value = Object.create(ChunkedCons.prototype);
    const chunkedState = {hash: cachedHash};
    cells.set(value, Object.freeze({kind: "chunked-cons", chunk, more, chunkedState,
      get hash() { return chunkedState.hash; },
      get head() { return nth(chunk, 0); },
      get tail() { return count(chunk) > 1 ? new ChunkedCons(dropFirstProtocol(chunk), more, null, null) : more ?? emptyList; }}));
    metadataValues.set(value, metadata);
    return value;
  }
  //@unit chunkCons requires: ChunkedCons, count
  function chunkCons(chunk, tail) { return count(chunk) === 0 ? tail : new ChunkedCons(chunk, tail, null, null); }
  //@unit dropOwned requires: cells, ChunkedSeqValue, count, maps, publicCollectionSlots, rangeNode,
  //@ repeat, seq, vectors, view, views
  function dropOwned(value, n) {
    if (value == null) return null;
    if (maps.has(value)) return dropOwned(seq(value), n);
    const cell = cells.get(value);
    if (cell?.kind === "repeat") return cell.remaining === -1 ? value : repeat(cell.remaining - n, cell.head);
    if (cell?.kind === "range") return n > 0 ? n < cell.count ? rangeNode(cell.head + cell.step * n, cell.end, cell.step, true) : null : value;
    const items = vectors.get(value), slice = views.get(value), slots = publicCollectionSlots.get(value);
    if (slots?.vec) {
      const offset = slots.off + n;
      if (offset < slots.node.length) return ChunkedSeqValue(slots.vec, slots.node, slots.i, offset, null, null);
      const i = slots.i + offset, newOffset = i % 32;
      return i < count(slots.vec) ? ChunkedSeqValue(slots.vec, vectors.get(slots.vec).slice(i - newOffset, i - newOffset + 32), i - newOffset, newOffset, null, null) : null;
    }
    const storage = items ?? slice.storage, offset = (items ? 0 : slice.offset) + n;
    return offset < storage.length ? view(storage, offset, items ? items.length <= 32 : slice.indexed, items ? items.length <= 32 ? "indexed" : "chunked" : slice.kind) : null;
  }
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
  //@unit drop requires: collectionArity, dropProtocol, emptyList, IDrop, implementsProtocol,
  //@ invalidArity, lazySequence, numberArgument, reducingFunction, rest, seq, transform
  function drop(n, coll) {
    if (arguments.length !== 1 && arguments.length !== 2) return invalidArity(arguments.length);
    numberArgument(n);
    if (arguments.length === 1) return transform(rf => {
      let remaining = n;
      return reducingFunction(rf, (result, input) => { const prior = remaining; remaining--; return prior > 0 ? result : rf.call(null, result, input); });
    });
    collectionArity("drop", arguments.length);
    if (implementsProtocol(IDrop, coll)) return (n > 0 ? dropProtocol(coll, Math.ceil(n)) : seq(coll)) ?? emptyList;
    return lazySequence(() => { let s = seq(coll), remaining = n; while (remaining > 0 && s !== null) { s = seq(rest(s)); remaining--; } return s; });
  }
  //@unit mapSeq requires: apply, arrayChunk, callable, chunkCons, chunkFirst, chunkRest, cons,
  //@ count, first, IChunkedSeq, implementsProtocol, invalidArity, lazySequence, listFromArray,
  //@ mapManySteps, nth, rest, seq, transform
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
      if (sources.length === 1 && implementsProtocol(IChunkedSeq, sources[0])) {
        const chunk = chunkFirst(sources[0]), size = count(chunk), items = [];
        for (let i = 0; i < size; i++) items.push(fn.call(null, nth(chunk, i)));
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
  //@unit indexedSequence requires: arrayChunk, chunkCons, chunkFirst, chunkRest, cons, count,
  //@ first, IChunkedSeq, implementsProtocol, lazySequence, nth, rest, seq
  function indexedSequence(fn, coll, index, keepOnly, indexed = true) {
    return lazySequence(() => {
      const s = seq(coll), at = index;
      if (s !== null) {
        if (implementsProtocol(IChunkedSeq, s)) {
          const chunk = chunkFirst(s), size = count(chunk), values = [];
          for (let i = 0; i < size; i++) { const value = indexed ? fn.call(null, at + i, nth(chunk, i)) : fn.call(null, nth(chunk, i)); if (!keepOnly || value != null) values.push(value); }
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
  //@unit filter requires: arrayChunk, chunkCons, chunkFirst, chunkRest, collectionArity, cons,
  //@ count, first, IChunkedSeq, implementsProtocol, lazySequence, nth, reducingFunction, rest, seq,
  //@ transform, truth
  function filter(pred, coll) {
    if (arguments.length === 1) return transform(rf => reducingFunction(rf, (result, input) => truth(pred.call(null, input)) ? rf.call(null, result, input) : result));
    collectionArity("filter", arguments.length);
    return lazySequence(() => {
      const s = seq(coll);
      if (s !== null) {
        if (implementsProtocol(IChunkedSeq, s)) {
          const chunk = chunkFirst(s), size = count(chunk), values = [];
          for (let i = 0; i < size; i++) { const value = nth(chunk, i); if (truth(pred.call(null, value))) values.push(value); }
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
  //@unit mapv requires: into, mapSeq, vector
  function mapv(fn, ...colls) { return into(vector(), mapSeq(fn, ...colls)); }
  //@unit filterv requires: conj, reduce, truth, vector
  function filterv(fn, coll) { return reduce((v, x) => truth(fn.call(null, x)) ? conj(v, x) : v, vector(), coll); }
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
  //@unit rangeNode requires: cells, emptyList, IntegerRangeValue, RangeValue
  function rangeNode(start, end, step, integer, suppliedCount) {
    const rangeCount = arguments.length >= 5 ? suppliedCount : Math.ceil((end - start) / step);
    const value = Object.create((integer ? IntegerRangeValue : RangeValue).prototype);
    cells.set(value, Object.freeze({kind: "range", head: start, end, step, integer,
      ...(integer ? {count: rangeCount} : {}),
      get tail() { const following = start + step; return (step > 0 ? following < end : following > end) ? rangeNode(following, end, step, integer) : emptyList; }}));
    return value;
  }
  //@unit IndexedSeqValue requires: metadataValues, views
  function IndexedSeqValue(arr, i, metadata) {
    const value = Object.create(IndexedSeqValue.prototype);
    metadataValues.set(value, metadata);
    // Ordinary views snapshot inputs; positional construction retains
    // the supplied public array and observes subsequent host-array writes.
    views.set(value, {storage: arr, offset: i, indexed: true, kind: "indexed", tail: null});
    return value;
  }
  //@unit primSeq requires: IndexedSeqValue, invalidArity
  function primSeq(prim, start) {
    if (arguments.length === 1) start = 0;
    else if (arguments.length !== 2) return invalidArity(arguments.length);
    return start < prim.length ? IndexedSeqValue(prim, start, null) : null;
  }
  //@unit RangeValue requires: cells, constructorSlots, rangeChunks, rangeNode
  function RangeValue(metadata, start, end, step, chunk, chunkNext, cachedHash) {
    const value = rangeNode(start, end, step, false);
    const state = {chunk, tail: chunkNext};
    rangeChunks.set(cells.get(value), state);
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
  //@unit into requires: conj, conjBang, conjBangProtocol, conjProtocol, IEditableCollection,
  //@ implementsProtocol, invalidArity, meta, persistentBang, reduce, transduce, transient, vector,
  //@ withMetaProtocol
  function into(to, from, source) {
    switch (arguments.length) {
      case 0: return vector();
      case 1: return to;
      case 2: {
        if (implementsProtocol(IEditableCollection, to))
          return withMetaProtocol(persistentBang(reduce(conjBangProtocol, transient(to), from)), meta(to));
        return reduce(to == null ? conj : conjProtocol, to, from);
      }
      case 3: {
        const editable = implementsProtocol(IEditableCollection, to), metadata = editable ? meta(to) : null;
        const rf = !editable ? conj : function (...args) {
          if (args.length === 1) return withMetaProtocol(persistentBang(args[0]), metadata);
          if (args.length === 2) return conjBang(args[0], args[1]);
          return invalidArity(args.length);
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
  //@unit compilerExceptionMessage requires: exceptionMessages
  const compilerExceptionMessage = value => exceptionMessages.get(value);
  //@unit ExceptionInfo effect requires: exceptionMessages, print, truth
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
  Object.setPrototypeOf(ExceptionInfo.prototype, Error.prototype);
  ExceptionInfo.prototype.toString = function () {
    return "#error {:message " + print(this.message, true)
      + (truth(this.data) ? ", :data " + print(this.data, true) : "")
      + (truth(this.cause) ? ", :cause " + print(this.cause, true) : "") + "}";
  };
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
  //@unit enableConsolePrint requires: coreCells, invalidArity, setPrintErrFn, setPrintFn
  function enableConsolePrint() {
    if (arguments.length !== 0) return invalidArity(arguments.length);
    coreCells.printNewline = false;
    setPrintFn((...args) => globalThis.console.log(...args));
    setPrintErrFn((...args) => globalThis.console.error(...args));
    return null;
  }
  //@unit stringPrint requires: coreCells, invalidArity
  function stringPrint(text) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    if (coreCells.printFn == null) throw new Error("No *print-fn* fn set for evaluation environment");
    coreCells.printFn.call(null, text); return null;
  }
  //@unit printOption requires: coreCells, get, keyword
  function printOption(options, name, root) { return options == null ? coreCells[root] : get(options, keyword(null, name)); }
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
  //@unit taggedLiteral requires: symbols, TaggedLiteral
  function taggedLiteral(tag, form) { if (!symbols.has(tag)) throw new Error("Assert failed: (symbol? tag)"); return new TaggedLiteral(tag, form); }
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
  //@unit StringBufferWriter
  function StringBufferWriter(sb) {
    const writer = Object.create(StringBufferWriter.prototype);
    writer.sb = sb;
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
  //@unit printMetaQ requires: IMeta, implementsProtocol, invalidArity, meta, printOption, truth
  function printMetaQ(options, value) {
    if (arguments.length !== 2) return invalidArity(arguments.length);
    return truth(printOption(options, "meta", "printMeta")) && implementsProtocol(IMeta, value) && meta(value) != null;
  }
  //@unit prSequentialWriter requires: coreCells, first, get, invalidArity, keyword, next,
  //@ printOption, seq, truth, write
  function prSequentialWriter(writer, printOne, begin, separator, end, options, coll) {
    if (arguments.length !== 7) return invalidArity(arguments.length);
    const saved = coreCells.printLevel;
    coreCells.printLevel = saved == null ? null : saved - 1;
    try {
      if (coreCells.printLevel != null && coreCells.printLevel < 0) return write(writer, "#");
      write(writer, begin);
      let current = seq(coll), remaining = printOption(options, "print-length", "printLength"), printed = false;
      while (current != null && (remaining == null || remaining !== 0)) {
        if (printed) write(writer, separator);
        printOne.call(null, first(current), writer, options); printed = true;
        current = next(current); if (remaining != null) remaining--;
      }
      if (current != null) {
        if (printed) write(writer, separator);
        const marker = get(options, keyword(null, "more-marker")); write(writer, truth(marker) ? marker : "...");
      }
      return write(writer, end);
    } finally { coreCells.printLevel = saved; }
  }
  //@unit printMapEntries requires: coreCells, freshKeyword, keywords, name, printWriter,
  //@ prSequentialWriter, symbol, symbols, truth, write
  function printMapEntries(entries, writer, options, prefix = "") {
    const names = entries.map(entry => keywords.get(entry.key) || symbols.get(entry.key));
    const ns = truth(coreCells.printNamespaceMaps) && names.length && names[0]?.namespace;
    const lifted = !prefix && ns && names.every(name => name?.namespace === ns);
    // JavaScript object tags precede the map printer's depth suppression.
    if (prefix) write(writer, prefix);
    return prSequentialWriter(writer, (entry, target, opts) => {
      const key = lifted ? keywords.has(entry.key) ? freshKeyword(null, name(entry.key)) : symbol(null, name(entry.key)) : entry.key;
      printWriter(key, target, opts); write(target, " "); return printWriter(entry.value, target, opts);
    }, (lifted ? "#:" + ns : "") + "{", ", ", "}", options, entries);
  }
  //@unit printWriter requires: assoc, get, keyword, printWriterImpl, truth
  function printWriter(value, writer, options) {
    const alt = get(options, keyword(null, "alt-impl"));
    if (truth(alt)) return alt.call(null, value, writer, assoc(options, keyword(null, "fallback-impl"), printWriterImpl));
    return printWriterImpl(value, writer, options);
  }
  //@unit printWriterImpl requires: atoms, coreCells, delays, ExceptionInfo, freshKeyword, keywords,
  //@ maps, mapTraversal, meta, printMapEntries, printMetaQ, printOption, printWriter,
  //@ prSequentialWriter, publicMethod, queues, sequential, sets, setTraversal, symbols, transients,
  //@ truth, typeNames, uuids, vars, vectors, volatiles, write
  function printWriterImpl(value, writer, options, skipMetadata = false) {
    if (value == null) return write(writer, "nil");
    if (!skipMetadata && printMetaQ(options, value)) { write(writer, "^"); printWriter(meta(value), writer, options); write(writer, " "); }
    const writerMethod = publicMethod(value, "IPrintWithWriter/-pr-writer", 3);
    if (writerMethod) return writerMethod.call(value, value, writer, options);
    const readable = truth(printOption(options, "readably", "printReadably"));
    if (typeof value === "string") {
      // CLJS escapes its seven named characters and preserves other UTF-16 units.
      const escapes = {'"': '\\"', '\\': '\\\\', '\b': '\\b', '\f': '\\f', '\n': '\\n', '\r': '\\r', '\t': '\\t'};
      return write(writer, readable ? '"' + value.replace(/[\\"\b\f\n\r\t]/g, ch => escapes[ch]) + '"' : value);
    }
    if (typeof value === "number") return write(writer, Number.isNaN(value) ? "##NaN" : value === Infinity ? "##Inf" : value === -Infinity ? "##-Inf" : Object.is(value, -0) ? "-0.0" : String(value));
    if (typeof value === "boolean") return write(writer, String(value));
    if (typeof value === "function") return write(writer, value.cljs$lang$type && typeNames.has(value) ? typeNames.get(value) : "#object[" + (value.name || "Function") + (truth(coreCells.printFnBodies) ? ' "' + String(value) + '"' : "") + "]");
    if (value instanceof ExceptionInfo) return write(writer, value.toString());
    if (transients.has(value)) {
      const state = transients.get(value);
      return write(writer, "#object[cljs.core." + (state.kind === "vector" ? "TransientVector" : state.kind === "set" ? "TransientHashSet" : state.mapKind === "array-map" ? "TransientArrayMap" : "TransientHashMap") + "]");
    }
    if (atoms.has(value) || volatiles.has(value)) {
      write(writer, "#object[cljs.core." + (atoms.has(value) ? "Atom" : "Volatile") + " {:val "); printWriter(value.state, writer, options); return write(writer, "}]");
    }
    if (delays.has(value)) {
      const state = delays.get(value);
      write(writer, "#object[cljs.core.Delay {:status " + (state.thunk == null ? ":ready" : ":pending") + ", :val "); printWriter(state.value, writer, options); return write(writer, "}]");
    }
    if (value instanceof Error) return write(writer, "#object[" + (value.constructor?.name || "Object") + " " + String(value) + "]");
    if (keywords.has(value) || symbols.has(value) || vars.has(value)) return write(writer, String(value));
    if (uuids.has(value)) return write(writer, '#uuid "' + uuids.get(value) + '"');
    if (value instanceof Date) return write(writer, '#inst "' + value.toISOString().replace(/Z$/, "-00:00") + '"');
    if (value instanceof RegExp) return write(writer, '#"' + value.source + '"');
    if (sequential(value) || queues.has(value)) {
      const [start, end] = queues.has(value) ? ["#queue [", "]"] : vectors.has(value) ? ["[", "]"] : ["(", ")"];
      return prSequentialWriter(writer, printWriter, start, " ", end, options, value);
    }
    const entries = maps.get(value);
    if (entries) return printMapEntries(mapTraversal(value), writer, options);
    const members = sets.get(value);
    if (members) return prSequentialWriter(writer, (e, target, opts) => printWriter(e.key, target, opts), "#{", " ", "}", options, setTraversal(value));
    if (Array.isArray(value)) return prSequentialWriter(writer, printWriter, "#js [", " ", "]", options, value);
    if (typeof value === "object" && Object.getPrototypeOf(value) === null) return write(writer, "#object[Object]");
    if (typeof value === "object" && value.constructor === Object) return printMapEntries(Object.keys(value).map(k => ({key: /^[A-Za-z_*+?!\-'][\w*+?!\-']*$/.test(k) ? freshKeyword(null, k) : k, value: value[k]})), writer, options, "#js ");
    if (typeof value === "symbol") return write(writer, "#object[" + String(value) + "]");
    return write(writer, "#object[" + (value.constructor?.cljs$lang$ctorStr?.replaceAll("/", ".") || value.constructor?.name || "Object") + " " + String(value) + "]");
  }
  //@unit prSeqWriter requires: first, invalidArity, next, printWriter, write
  function prSeqWriter(values, writer, options) {
    if (arguments.length !== 3) return invalidArity(arguments.length);
    printWriter(first(values), writer, options);
    for (let s = next(values); s != null; s = next(s)) { write(writer, " "); printWriter(first(s), writer, options); }
    return null;
  }
  //@unit prStrWithOpts requires: flushWriter, invalidArity, prSeqWriter, seq, stringWriter
  function prStrWithOpts(values, options) {
    if (arguments.length !== 2) return invalidArity(arguments.length);
    if (seq(values) == null) return "";
    const {buffer, writer} = stringWriter(); prSeqWriter(values, writer, options); flushWriter(writer); return buffer.text;
  }
  //@unit prnStrWithOpts requires: invalidArity, prStrWithOpts
  function prnStrWithOpts(values, options) { if (arguments.length !== 2) return invalidArity(arguments.length); return prStrWithOpts(values, options) + "\n"; }
  //@unit print requires: coreCells, flushWriter, printWriterImpl, stringWriter
  function print(value, readable) {
    const saved = coreCells.printReadably; coreCells.printReadably = readable;
    try { const {buffer, writer} = stringWriter(); printWriterImpl(value, writer, null, true); flushWriter(writer); return buffer.text; }
    finally { coreCells.printReadably = saved; }
  }
  //@unit prStr requires: prStrWithOpts
  function prStr(...values) { return prStrWithOpts(values, null); }
  //@unit printForHost requires: prStr
  // The Rust display API returns UTF-8. Keep canonical JS printing lossless,
  // but represent isolated surrogate units as escapes at this text boundary.
  function printForHost(value) {
    return prStr(value).replace(/[\uD800-\uDBFF][\uDC00-\uDFFF]|[\uD800-\uDFFF]/g,
      units => units.length === 2 ? units : "\\u" + units.charCodeAt(0).toString(16).padStart(4, "0"));
  }
  //@unit str requires: maps, print, recordTypes, sequential, sets
  function str(...values) { return values.map(value => value == null ? "" : typeof value === "string" ? value : sequential(value) || (maps.has(value) && !recordTypes.has(value)) || sets.has(value) ? print(value, true) : String(value)).join(""); }

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
  //@unit arrayMap.variadic install: arrayMap requires: arrayMap, mapFromArray, toArray,
  //@ variadicCore
  variadicCore(arrayMap, 0, args => mapFromArray(toArray(args), "array-map"));
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
  //@unit keyToJs requires: cljToJs, encodeJsProtocol, IEncodeJS, keywords, prStr, satisfies,
  //@ symbols
  function keyToJs(key, primitive = cljToJs) {
    if (satisfies(IEncodeJS, key)) return encodeJsProtocol(key);
    return typeof key === "string" || typeof key === "number" || keywords.has(key) || symbols.has(key) ? primitive.call(null, key) : prStr(key);
  }
  //@unit cljToJs requires: encodeJsProtocol, first, get, IEncodeJS, isColl, isMap, keyToJs,
  //@ keyword, keywords, listFromArray, mapSeq, name, next, nth, satisfies, seq, seqToMap, str,
  //@ symbols, toArray
  function cljToJs(value, ...options) {
    const opts = seqToMap(listFromArray(options)), keywordFn = get(opts, keyword(null, "keyword-fn"), name);
    const convert = value => {
      if (value == null) return null;
      if (satisfies(IEncodeJS, value)) return encodeJsProtocol(value);
      if (keywords.has(value)) return keywordFn.call(null, value);
      if (symbols.has(value)) return str(value);
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
  //@unit jsToClj requires: arrayMap, assoc, doall, empty, encodeClojureProtocol, get,
  //@ IEncodeClojure, into, isColl, isMapEntry, isSeq, keyword, keywordCore, listFromArray,
  //@ mapEntry, mapFromArray, mapSeq, nth, satisfies, seqToMap, truth, vectorFromArray
  function jsToClj(value, ...options) {
    if (arguments.length === 1) options = [keyword(null, "keywordize-keys"), false];
    const opts = seqToMap(listFromArray(options)), keywordize = truth(get(opts, keyword(null, "keywordize-keys")));
    const convert = value => {
      if (satisfies(IEncodeClojure, value)) return encodeClojureProtocol(value, arrayMap(...options));
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

  //@unit coreMethod mutator requires: builtin, builtinCoreImplementation, coreDispatchers,
  //@ coreProtocolsByName, ICounted, IEquiv, IHash, invalidArity
  function coreMethod(key, arities, fallback) {
    const dispatch = function (value, ...args) {
      const arity = arities.length === 1 ? arities[0] : arguments.length;
      if (!arities.includes(arity)) return invalidArity(arguments.length);
      if (arities.length === 1) args = args.slice(0, arity - 1);
      const method = builtin(value, key, arity);
      if (method != null) return method.call(value, value, ...args);
      const protocol = coreProtocolsByName.get(key.split("/")[0]);
      if (builtinCoreImplementation(protocol, value) || protocol === IHash || protocol === IEquiv || (protocol === ICounted && value == null)) return fallback(value, ...args);
      throw new Error("No protocol method " + key + " defined for this value");
    };
    coreDispatchers.set(key, dispatch);
    return dispatch;
  }
  //@unit write effect requires: coreMethod
  const write = coreMethod("IWriter/-write", [2], () => null);
  //@unit flushWriter effect requires: coreMethod
  const flushWriter = coreMethod("IWriter/-flush", [1], () => null);
  //@unit StringBufferWriter.members effect: StringBufferWriter requires: installMethods,
  //@ StringBufferWriter
  installMethods(StringBufferWriter.prototype, [
    ["IWriter", 0, null],
    ["IWriter/-write", 2, (writer, text) => writer.sb.append(text)],
    ["IWriter/-flush", 1, () => null]
  ]);
  //@unit prWriter effect requires: coreMethod, printWriterImpl
  const prWriter = coreMethod("IPrintWithWriter/-pr-writer", [3], (value, writer, options) => printWriterImpl(value, writer, options, true));
  //@unit clone effect requires: canonicalVectorRoots, cells, cloneablePayload, constructorHashes,
  //@ coreMethod, IntegerRangeValue, mapEntries, mapKinds, meta, metadataValues, protocolStores,
  //@ rangeChunks, RangeValue, setKinds, sortedComparators, subvecSources, treeRoots, treeSetMaps
  const clone = coreMethod("ICloneable/-clone", [1], value => {
    if (cells.get(value)?.kind === "range") {
      const c = cells.get(value), state = rangeChunks.get(c);
      return c.integer ? IntegerRangeValue(meta(value), c.head, c.end, c.step, c.count, constructorHashes.get(value))
        : RangeValue(meta(value), c.head, c.end, c.step, state?.chunk, state?.tail, constructorHashes.get(value));
    }
    if (!cloneablePayload(value)) throw new Error("No protocol method ICloneable.-clone defined for this value");
    const result = Object.create(Object.getPrototypeOf(value));
    for (const store of [...protocolStores, treeRoots, treeSetMaps, canonicalVectorRoots, constructorHashes, metadataValues, mapKinds, setKinds, sortedComparators, subvecSources]) if (store.has(value)) store.set(result, store.get(value));
    if (mapEntries.has(value)) mapEntries.add(result);
    return result;
  });
  //@unit seqProtocol effect requires: contains, coreMethod, count, first, get, missing, next, nth,
  //@ rest, seq, transientNth, transients
  const seqProtocol = coreMethod("ISeqable/-seq", [1], seq), firstProtocol = coreMethod("ISeq/-first", [1], first), restProtocol = coreMethod("ISeq/-rest", [1], rest), nextProtocol = coreMethod("INext/-next", [1], next), countProtocol = coreMethod("ICounted/-count", [1], count), nthProtocol = coreMethod("IIndexed/-nth", [2, 3], function (value, index, notFound) { return transients.has(value) ? transientNth(value, index, arguments.length === 2 ? missing : notFound) : nth(...arguments); }), lookupProtocol = coreMethod("ILookup/-lookup", [2, 3], get), containsProtocol = coreMethod("IAssociative/-contains-key?", [2], contains);
  //@unit publicFields mutator requires: cells, constructorHashes, iterateUnrealizedSeed, meta,
  //@ mungeMember, ownedProtocolValue, rangeChunks
  // The upstream deftype fields; `read` answers all but `meta` and the hash.
  function publicFields(Type, fields, read) {
    for (const field of fields) Object.defineProperty(Type.prototype, mungeMember(field), {
      get() {
        if (field === "meta") return meta(this);
        if (field === "__hash" || field === "_hash") return constructorHashes.has(this) ? constructorHashes.get(this) : null;
        return read(this, field);
      },
      set: ["__hash", "_hash", "next", "_next", "current", "seed", "chunk", "chunk-next"].includes(field) ? function (value) {
        if (!ownedProtocolValue(this) || !(this instanceof Type)) throw new Error("Expected public core value");
        if (field === "__hash" || field === "_hash") constructorHashes.set(this, value);
        else if (field === "chunk" || field === "chunk-next") { const c = cells.get(this), state = rangeChunks.get(c) ?? {}; state[field === "chunk" ? "chunk" : "tail"] = value; rangeChunks.set(c, state); }
        else { const state = cells.get(this).state; state[field === "current" ? "current" : field === "seed" ? "value" : "following"] = value; if (field === "seed") state.realized = value !== iterateUnrealizedSeed; }
      } : function (value) {
        // A host field shadow cannot mutate an immutable branded payload.
        Object.defineProperty(this, mungeMember(field), {value, writable: true, configurable: true, enumerable: true});
      }
    });
  }
  //@unit slotField requires: count, missing, publicCollectionSlots
  // A field a positional constructor supplied, else the count, else `missing`.
  function slotField(value, field) {
    const slots = publicCollectionSlots.get(value);
    if (slots && field in slots) return slots[field];
    return field === "cnt" || field === "count" ? count(value) : missing;
  }
  //@unit rangeField requires: cells, rangeChunks
  function rangeField(value, field) {
    const c = cells.get(value);
    if (["start", "end", "step", "cnt"].includes(field)) return c[field === "start" ? "head" : field === "cnt" ? "count" : field];
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
  //@unit SymbolValue.members effect: SymbolValue requires: constructorHashes, metadataValues,
  //@ symbols, SymbolValue
  for (const [field, key] of Object.entries({ns: "namespace", name: "name", str: "spelling"})) Object.defineProperty(SymbolValue.prototype, field, {
    get() { return symbols.get(this)[key]; },
    set(value) {
      if (!symbols.has(this)) throw new Error("Expected public core value");
      symbols.set(this, Object.freeze({...symbols.get(this), [key]: value}));
    }
  });
  Object.defineProperty(SymbolValue.prototype, "_hash", {
    get() { return constructorHashes.has(this) ? constructorHashes.get(this) : null; },
    set(value) {
      if (!symbols.has(this)) throw new Error("Expected public core value");
      constructorHashes.set(this, value);
    }
  });
  Object.defineProperty(SymbolValue.prototype, "_meta", {
    get() { return metadataValues.has(this) ? metadataValues.get(this) : null; }, set(value) {
      if (!symbols.has(this)) throw new Error("Expected public core value");
      metadataValues.set(this, value);
    }
  });
  //@unit Cons.members effect: Cons requires: cells, Cons, constructorHashes, metadataValues,
  //@ seqIndexes, seqMethods
  for (const [field, key] of Object.entries({first: "head", rest: "tail"})) Object.defineProperty(Cons.prototype, field, {
    get() { return cells.get(this)[key]; },
    set(value) {
      if (!cells.has(this)) throw new Error("Expected public core value");
      cells.set(this, Object.freeze({...cells.get(this), [key]: value}));
    }
  });
  Object.defineProperty(Cons.prototype, "__hash", {
    get() { return constructorHashes.has(this) ? constructorHashes.get(this) : null; },
    set(value) {
      if (!(cells.get(this)?.kind === "cons")) throw new Error("Expected public core value");
      constructorHashes.set(this, value);
    }
  });
  Object.defineProperty(Cons.prototype, "meta", {
    get() { return metadataValues.has(this) ? metadataValues.get(this) : null; }, set(value) {
      if (!(cells.get(this)?.kind === "cons")) throw new Error("Expected public core value");
      metadataValues.set(this, value);
    }
  });
  seqIndexes(Cons);
  Cons.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit Reduced.members effect: Reduced requires: Reduced, reducedValues
  Object.defineProperty(Reduced.prototype, "val", {
    get() { return reducedValues.get(this); }, set(value) {
      if (!reducedValues.has(this)) throw new Error("Expected Reduced");
      reducedValues.set(this, value);
    }
  });
  //@unit ListValue.members effect: ListValue requires: cells, ListValue, publicFields, seqIndexes,
  //@ seqMethods
  publicFields(ListValue, ["meta", "first", "rest", "count", "__hash"], (value, field) => cells.get(value)[{first: "head", rest: "tail", count: "count"}[field]]);
  seqIndexes(ListValue);
  ListValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit EmptyListValue.members effect: EmptyListValue requires: EmptyListValue, publicFields,
  //@ seqIndexes, seqMethods
  publicFields(EmptyListValue, ["meta"], null);
  seqIndexes(EmptyListValue);
  EmptyListValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit KeywordValue.members effect: KeywordValue requires: keywords, KeywordValue, publicFields
  publicFields(KeywordValue, ["ns", "name", "fqn", "_hash"], (value, field) => keywords.get(value)[{ns: "namespace", name: "name", fqn: "spelling"}[field]]);
  //@unit RepeatValue.members effect: RepeatValue requires: cells, publicFields, RepeatValue,
  //@ seqIndexes
  publicFields(RepeatValue, ["meta", "count", "val", "next", "__hash"], (value, field) => {
    const c = cells.get(value);
    return field === "count" ? c.remaining : field === "val" ? c.head : c.state?.following ?? null;
  });
  seqIndexes(RepeatValue);
  //@unit CycleValue.members effect: CycleValue requires: cells, CycleValue, publicFields
  publicFields(CycleValue, ["meta", "all", "prev", "current", "_next"], (value, field) => {
    const c = cells.get(value);
    return field === "all" ? c.all : field === "prev" ? c.previous : c.state[field === "current" ? "current" : "following"];
  });
  CycleValue.prototype.currentval = function () { return cells.get(this).currentValue(); };
  //@unit IterateValue.members effect: IterateValue requires: cells, IterateValue, publicFields
  publicFields(IterateValue, ["meta", "f", "prev-seed", "seed", "next"], (value, field) => {
    const c = cells.get(value);
    return field === "f" ? c.fn : field === "prev-seed" ? c.previous : c.state[field === "seed" ? "value" : "following"];
  });
  //@unit PersistentVectorValue.members effect: PersistentVectorValue requires: canonicalVectorRoots,
  //@ missing, PersistentVectorValue, publicFields, slotField, vectorRoots, vectors
  publicFields(PersistentVectorValue, ["meta", "cnt", "shift", "root", "tail", "__hash"], (value, field) => {
    const slot = slotField(value, field);
    if (slot !== missing) return slot;
    const items = vectors.get(value), tailOffset = items.length < 32 ? 0 : ((items.length - 1) >>> 5) << 5;
    if (field === "tail") return items.slice(tailOffset);
    if (field === "shift") return 5;
    if (field === "root") {
      if (!canonicalVectorRoots.has(value)) { const root = {}; vectorRoots.set(root, items.slice(0, tailOffset)); canonicalVectorRoots.set(value, root); }
      return canonicalVectorRoots.get(value);
    }
  });
  //@unit SubvecValue.members effect: SubvecValue requires: count, missing, publicFields, slotField,
  //@ subvecSources, SubvecValue
  publicFields(SubvecValue, ["meta", "start", "end", "__hash"], (value, field) => {
    const slot = slotField(value, field);
    return slot !== missing ? slot : field === "start" ? 0 : count(value);
  });
  Object.defineProperty(SubvecValue.prototype, "v", {get() { return subvecSources.get(this); }});
  //@unit ChunkedSeqValue.members effect: ChunkedSeqValue requires: ChunkedSeqValue, missing,
  //@ publicFields, seqIndexes, seqMethods, slotField, vectorFromArray, views
  publicFields(ChunkedSeqValue, ["vec", "node", "i", "off", "meta", "__hash"], (value, field) => {
    const slot = slotField(value, field);
    if (slot !== missing) return slot;
    const v = views.get(value);
    return field === "node" ? v.storage : field === "off" ? v.offset : field === "i" ? 0 : vectorFromArray(v.storage);
  });
  seqIndexes(ChunkedSeqValue);
  ChunkedSeqValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit PersistentQueueValue.members effect: PersistentQueueValue requires: missing,
  //@ PersistentQueueValue, publicFields, queues, seq, seqMethods, slotField, vector
  publicFields(PersistentQueueValue, ["meta", "count", "front", "rear", "__hash"], (value, field) => {
    const slot = slotField(value, field);
    return slot !== missing ? slot : field === "front" ? seq(queues.get(value)) : vector();
  });
  PersistentQueueValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit PersistentArrayMapValue.members effect: PersistentArrayMapValue requires: maps, missing,
  //@ PersistentArrayMapValue, publicFields, slotField
  publicFields(PersistentArrayMapValue, ["meta", "cnt", "arr", "__hash"], (value, field) => {
    const slot = slotField(value, field);
    return slot !== missing ? slot : maps.get(value).flatMap(e => [e.key, e.value]);
  });
  //@unit PersistentHashMapValue.members effect: PersistentHashMapValue requires: contains, get,
  //@ missing, PersistentHashMapValue, publicFields, slotField
  publicFields(PersistentHashMapValue, ["meta", "cnt", "root", "has-nil?", "nil-val", "__hash"], (value, field) => {
    const slot = slotField(value, field);
    return slot !== missing ? slot : field === "root" ? value : field === "has-nil?" ? contains(value, null) : get(value, null);
  });
  //@unit PersistentHashSetValue.members effect: PersistentHashSetValue requires: mapFromArray,
  //@ missing, PersistentHashSetValue, publicFields, setKinds, sets, slotField
  publicFields(PersistentHashSetValue, ["meta", "hash-map", "__hash"], (value, field) => {
    const slot = slotField(value, field);
    return slot !== missing ? slot : mapFromArray(sets.get(value).flatMap(e => [e.key, null]), setKinds.get(value));
  });
  //@unit IndexedSeqValue.members effect: IndexedSeqValue requires: IndexedSeqValue, publicFields,
  //@ seqIndexes, seqMethods, views
  publicFields(IndexedSeqValue, ["arr", "i", "meta"], (value, field) => {
    const v = views.get(value);
    return field === "arr" ? v.storage : v.offset;
  });
  seqIndexes(IndexedSeqValue);
  IndexedSeqValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit RangeValue.members effect: RangeValue requires: cells, forceRangeChunk, publicFields,
  //@ rangeField, RangeValue, seqIndexes, seqMethods
  publicFields(RangeValue, ["meta", "start", "end", "step", "chunk", "chunk-next", "__hash"], rangeField);
  RangeValue.prototype.forceChunk = function () { forceRangeChunk(cells.get(this)); return null; };
  seqIndexes(RangeValue);
  RangeValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit IntegerRangeValue.members effect: IntegerRangeValue requires: IntegerRangeValue,
  //@ publicFields, rangeField, seqIndexes, seqMethods
  publicFields(IntegerRangeValue, ["meta", "start", "end", "step", "cnt", "__hash"], rangeField);
  seqIndexes(IntegerRangeValue);
  IntegerRangeValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit treeMapConstructor.members effect: treeMapConstructor requires: publicFields, publicTree,
  //@ slotField, sortedComparators, treeMapConstructor
  publicFields(treeMapConstructor, ["tree", "cnt", "meta", "__hash"], (value, field) => field === "tree" ? publicTree(value) : slotField(value, field));
  Object.defineProperty(treeMapConstructor.prototype, "comp", {get() { return sortedComparators.get(this); }});
  //@unit treeSetConstructor.members effect: treeSetConstructor requires: makeMap,
  //@ publicCollectionSlots, publicFields, sets, sortedComparators, treeSetConstructor, treeSetMaps
  publicFields(treeSetConstructor, ["meta", "tree-map", "__hash"], (value, field) => {
    const slots = publicCollectionSlots.get(value);
    if (slots && field in slots) return slots[field];
    if (!treeSetMaps.has(value)) treeSetMaps.set(value, makeMap(sets.get(value).map(e => ({key: e.key, value: null, get hash() { return e.hash; }})), "sorted-map", sortedComparators.get(value)));
    return treeSetMaps.get(value);
  });
  //@unit treeMapSeqConstructor.members effect: treeMapSeqConstructor requires: missing,
  //@ publicFields, seqMethods, slotField, treeMapSeqConstructor
  publicFields(treeMapSeqConstructor, ["meta", "stack", "ascending?", "cnt", "__hash"], (value, field) => {
    const slot = slotField(value, field);
    return slot !== missing ? slot : undefined;
  });
  treeMapSeqConstructor.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit Eduction.members effect: Eduction requires: completing, count, Eduction, eductions,
  //@ es6Iterator, installMethods, printWriter, prSequentialWriter, seq, sequence, sequenceIterator,
  //@ sequentialIndex, transduce, transformedIterator
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
      return prSequentialWriter(writer, printWriter, "(", " ", ")", options, value);
    }]
  ]);
  Object.defineProperties(Eduction.prototype, Object.getOwnPropertyDescriptors({
    get xform() { return eductions.get(this).xform; },
    set xform(value) { eductions.get(this).xform = value; },
    get coll() { return eductions.get(this).coll; },
    set coll(value) { eductions.get(this).coll = value; },
    indexOf(value, start = 0) { return sequentialIndex(this, value, start, false); },
    lastIndexOf(value, start = count(this)) { return sequentialIndex(this, value, start, true); },
    [Symbol.iterator]() { return es6Iterator(this); }
  }));
  //@unit Delay.members effect: Delay requires: Delay, delays
  for (const [field, key] of Object.entries({f: "thunk", value: "value"})) Object.defineProperty(Delay.prototype, field, {
    configurable: true, get() { return delays.get(this)[key]; }, set(value) { delays.get(this)[key] = value; }
  });
  //@unit LazySeq.members effect: LazySeq requires: deferredValue, LazySeq, lazyValues,
  //@ metadataValues, objectEquiv, seqMethods
  for (const [field, key] of Object.entries({fn: "thunk", s: "value", __hash: "hash"})) Object.defineProperty(LazySeq.prototype, field, {
    configurable: true, get() { return lazyValues.get(this)[key]; }, set(value) { lazyValues.get(this)[key] = value; }
  });
  Object.defineProperty(LazySeq.prototype, "meta", {
    configurable: true, get() { return metadataValues.get(this); }, set(value) { metadataValues.set(this, value); }
  });
  Object.defineProperties(LazySeq.prototype, Object.getOwnPropertyDescriptors({
    sval() { return deferredValue(lazyValues.get(this), true); },
    equiv(other) { return objectEquiv(this, other); },
    indexOf: seqMethods.indexOf,
    lastIndexOf: seqMethods.lastIndexOf,
    [Symbol.iterator]: seqMethods[Symbol.iterator]
  }));
  //@unit NodeSeqType.members effect: NodeSeqType requires: NodeSeqType, seqMethods
  NodeSeqType.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  //@unit ChunkedCons.members effect: ChunkedCons requires: cells, ChunkedCons, meta, seqMethods
  ChunkedCons.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  for (const [field, key] of Object.entries({chunk: "chunk", more: "more", __hash: "hash"})) Object.defineProperty(ChunkedCons.prototype, field, {
    configurable: true, get() { return cells.get(this)[key]; },
    set: field === "__hash" ? function (value) { cells.get(this).chunkedState.hash = value; } : undefined
  });
  Object.defineProperty(ChunkedCons.prototype, "meta", {get() { return meta(this); }});
  //@unit ChunkBuffer.members effect: ChunkBuffer requires: chunk, chunkAppend, ChunkBuffer,
  //@ chunkBuffers
  for (const [field, key] of Object.entries({buf: "array", end: "end"})) Object.defineProperty(ChunkBuffer.prototype, field, {
    configurable: true, get() { return chunkBuffers.get(this)[key]; }, set(value) { chunkBuffers.get(this)[key] = value; }
  });
  Object.defineProperties(ChunkBuffer.prototype, Object.getOwnPropertyDescriptors({
    add(value) { return chunkAppend(this, value); },
    chunk() { return chunk(this); }
  }));
  //@unit arrayChunk.members effect: arrayChunk requires: arrayChunk, chunks
  for (const [field, key] of Object.entries({arr: "array", off: "offset", end: "end"})) Object.defineProperty(arrayChunk.prototype, field, {
    configurable: true, get() { return chunks.get(this)[key]; }, set(value) { chunks.get(this)[key] = value; }
  });
  //@unit realized effect requires: cells, coreMethod, delays, lazyValues, truth
  const realized = coreMethod("IPending/-realized?", [1], value => cells.has(value) ? cells.get(value).realized === true : !truth((delays.get(value) || lazyValues.get(value)).thunk));
  //@unit ListValue.statics effect: ListValue requires: emptyList, ListValue
  ListValue.EMPTY = emptyList;
  //@unit PersistentHashSetValue.statics effect: PersistentHashSetValue requires: makeSet,
  //@ PersistentHashSetValue
  PersistentHashSetValue.EMPTY = makeSet([], "array-map");
  //@unit PersistentQueueValue.statics effect: PersistentQueueValue requires: PersistentQueueValue,
  //@ queueFromArray
  PersistentQueueValue.EMPTY = queueFromArray([]);
  //@unit PersistentVectorValue.statics effect: PersistentVectorValue requires: PersistentVectorValue,
  //@ vector
  PersistentVectorValue.EMPTY = vector();
  //@unit PersistentArrayMapValue.statics effect: PersistentArrayMapValue requires: makeMap,
  //@ PersistentArrayMapValue
  PersistentArrayMapValue.EMPTY = makeMap([], "array-map");
  PersistentArrayMapValue.HASHMAP_THRESHOLD = 8;
  //@unit PersistentHashMapValue.statics effect: PersistentHashMapValue requires: aclone, makeMap,
  //@ PersistentHashMapValue, putEntry, str
  PersistentHashMapValue.EMPTY = makeMap([], "hash-map");
  PersistentHashMapValue.fromArray = (array, noClone) => {
    const items = noClone ? array : aclone(array), entries = [], length = items.length;
    const edit = {};
    for (let i = 0; i < length; i += 2) putEntry(entries, items[i], items[i + 1], "hash-map", edit);
    return makeMap(entries, "hash-map");
  };
  PersistentHashMapValue.fromArrays = (keys, values) => {
    const length = keys.length, entries = [], edit = {};
    for (let i = 0; i < length; i++) {
      if (values.length <= i) throw new Error(str("No value supplied for key: ", keys[i]));
      putEntry(entries, keys[i], values[i], "hash-map", edit);
    }
    return makeMap(entries, "hash-map");
  };
  //@unit positionalFactory requires: invalidArity
  function positionalFactory(Type, arity) {
    return function (...args) {
      if (args.length !== arity) return invalidArity(args.length);
      return Reflect.construct(Type, args);
    };
  }
  //@unit coreCells requires: defaultHostClock, defaultHostRandom, defaultTapSchedule, flush,
  //@ hostRand
  // Mutable core cells: dynamic Vars, host hooks, and operations the runtime reads
  // through a cell so `set!` reaches it. The `core` hub extends this object.
  const coreCells = {
    replOne: null, replTwo: null, replThree: null, replError: null, sourceFile: null,
    uncheckedArrays: false, commandLineArgs: null, printFn: null, printErrFn: null, printNewline: true,
    printReadably: true, printLength: null, printLevel: null, printNamespaceMaps: false,
    printMeta: false, printDup: false, flushOnNewline: true, printFnBodies: false,
    readerCurrentInputValue: null, currentNs: null, assertions: true,
    hostClock: defaultHostClock, hostRandom: defaultHostRandom, execTapFn: defaultTapSchedule,
    rand: hostRand, flush};
  //@unit removeAllMethods effect requires: coreMethod, resetMulti
  // Core operations without a function of their own, one unit each.
  const removeAllMethods = coreMethod("IMultiFn/-reset", [1], resetMulti);
  //@unit addMethodProtocol effect requires: addMethod, coreMethod
  const addMethodProtocol = coreMethod("IMultiFn/-add-method", [3], addMethod);
  //@unit removeMethodProtocol effect requires: coreMethod, removeMethod
  const removeMethodProtocol = coreMethod("IMultiFn/-remove-method", [2], removeMethod);
  //@unit preferMethodProtocol effect requires: coreMethod, preferMethod
  const preferMethodProtocol = coreMethod("IMultiFn/-prefer-method", [3], preferMethod);
  //@unit getMethodProtocol effect requires: coreMethod, getMethod
  const getMethodProtocol = coreMethod("IMultiFn/-get-method", [2], getMethod);
  //@unit methods effect requires: coreMethod, demandMulti, deref
  const methods = coreMethod("IMultiFn/-methods", [1], mf => { demandMulti(mf); return deref(mf.method_table); });
  //@unit prefers effect requires: coreMethod, demandMulti, deref
  const prefers = coreMethod("IMultiFn/-prefers", [1], mf => { demandMulti(mf); return deref(mf.prefer_table); });
  //@unit defaultDispatchVal effect requires: coreMethod
  const defaultDispatchVal = coreMethod("IMultiFn/-default-dispatch-val", [1], mf => mf.default_dispatch_val);
  //@unit dispatchFn effect requires: coreMethod
  const dispatchFn = coreMethod("IMultiFn/-dispatch-fn", [1], mf => mf.dispatch_fn);
  //@unit asTransientProtocol effect requires: asTransient, coreMethod
  const asTransientProtocol = coreMethod("IEditableCollection/-as-transient", [1], asTransient);
  //@unit persistentBangProtocol effect requires: coreMethod, persistTransient
  const persistentBangProtocol = coreMethod("ITransientCollection/-persistent!", [1], persistTransient);
  //@unit conjBangProtocol effect requires: conjTransient, coreMethod
  const conjBangProtocol = coreMethod("ITransientCollection/-conj!", [2], conjTransient);
  //@unit assocBangProtocol effect requires: assocTransient, coreMethod
  const assocBangProtocol = coreMethod("ITransientAssociative/-assoc!", [3], assocTransient);
  //@unit assocNBangProtocol effect requires: assocTransient, coreMethod
  const assocNBangProtocol = coreMethod("ITransientVector/-assoc-n!", [3], (value, index, item) => assocTransient(value, index, item, true));
  //@unit dissocBangProtocol effect requires: coreMethod, removeTransient
  const dissocBangProtocol = coreMethod("ITransientMap/-dissoc!", [2], (value, key) => removeTransient(value, key, "dissoc!"));
  //@unit disjoinBangProtocol effect requires: coreMethod, removeTransient
  const disjoinBangProtocol = coreMethod("ITransientSet/-disjoin!", [2], (value, key) => removeTransient(value, key, "disj!"));
  //@unit popBangProtocol effect requires: coreMethod, popTransient
  const popBangProtocol = coreMethod("ITransientVector/-pop!", [1], popTransient);
  //@unit chunkedNextProtocol effect requires: cells, coreMethod, ownedChunkRest, seq
  const chunkedNextProtocol = coreMethod("IChunkedNext/-chunked-next", [1], value => cells.get(value)?.kind === "chunked-cons" ? cells.get(value).more ?? null : seq(ownedChunkRest(value)));
  //@unit dropProtocol effect requires: coreMethod, dropOwned
  const dropProtocol = coreMethod("IDrop/-drop", [2], dropOwned);
  //@unit dropFirstProtocol effect requires: arrayChunk, chunks, coreMethod, count
  const dropFirstProtocol = coreMethod("IChunk/-drop-first", [1], value => { const c = chunks.get(value); if (c.integer ? count(value) <= 1 : count(value) === 0) throw new Error("-drop-first of empty chunk"); const result = arrayChunk(c.array, c.offset + 1, c.end); if (c.integer) chunks.set(result, {...c, start: c.start + c.step, offset: c.offset + 1}); return result; });
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
  //@unit isIterable requires: IIterable, satisfies
  const isIterable = value => satisfies(IIterable, value);
  //@unit writeAll requires: write
  const writeAll = (writer, ...texts) => { for (const text of texts) write(writer, text); return null; };
  //@unit keyProtocol effect requires: coreMethod, nth
  const keyProtocol = coreMethod("IMapEntry/-key", [1], value => nth(value, 0));
  //@unit valProtocol effect requires: coreMethod, nth
  const valProtocol = coreMethod("IMapEntry/-val", [1], value => nth(value, 1));
  //@unit findProtocol effect requires: coreMethod, find
  const findProtocol = coreMethod("IFind/-find", [2], find);
  //@unit sortedSeqProtocol effect requires: coreMethod, identity, mapSeq, rseq, seq, sets, truth
  const sortedSeqProtocol = coreMethod("ISorted/-sorted-seq", [2], (value, ascending) => { const s = truth(ascending) ? seq(value) : rseq(value); return sets.has(value) ? mapSeq(identity, s) : s; });
  //@unit sortedSeqFromProtocol effect requires: coreMethod, sortedSeqFrom
  const sortedSeqFromProtocol = coreMethod("ISorted/-sorted-seq-from", [3], sortedSeqFrom);
  //@unit entryKeyProtocol effect requires: coreMethod, keyProtocol, sets
  const entryKeyProtocol = coreMethod("ISorted/-entry-key", [2], (value, entry) => sets.has(value) ? entry : keyProtocol(entry));
  //@unit comparatorProtocol effect requires: coreMethod, sortedComparators
  const comparatorProtocol = coreMethod("ISorted/-comparator", [1], value => sortedComparators.get(value));
  //@unit chunkedFirstProtocol effect requires: coreMethod, ownedChunkFirst
  const chunkedFirstProtocol = coreMethod("IChunkedSeq/-chunked-first", [1], ownedChunkFirst);
  //@unit chunkedRestProtocol effect requires: coreMethod, ownedChunkRest
  const chunkedRestProtocol = coreMethod("IChunkedSeq/-chunked-rest", [1], ownedChunkRest);
  //@unit invokeProtocol effect requires: coreMethod
  const invokeProtocol = coreMethod("IFn/-invoke", Array.from({length: 22}, (_, i) => i + 1), (value, ...args) => value.call(null, ...args));
  //@unit evalCore
  const evalCore = Object.defineProperty(() => { throw new Error("compiler eval service is not installed"); }, "name", {value: "eval"});
  //@unit isCloneable requires: ICloneable, satisfies
  const isCloneable = value => satisfies(ICloneable, value);
  //@unit isDelay requires: delays
  const isDelay = value => delays.has(value);
  //@unit derefWithTimeoutProtocol effect requires: coreMethod
  const derefWithTimeoutProtocol = coreMethod("IDerefWithTimeout/-deref-with-timeout", [3], () => null);
  //@unit reduceProtocol effect requires: coreMethod, reduce
  const reduceProtocol = coreMethod("IReduce/-reduce", [2, 3], function (value, rf, init) { return arguments.length === 2 ? reduce(rf, value) : reduce(rf, init, value); });
  //@unit kvReduceProtocol effect requires: coreMethod, reduceKv
  const kvReduceProtocol = coreMethod("IKVReduce/-kv-reduce", [3], (value, rf, init) => reduceKv(rf, init, value));
  //@unit assocProtocol effect requires: assoc, coreMethod
  const assocProtocol = coreMethod("IAssociative/-assoc", [3], assoc);
  //@unit assocNProtocol effect requires: assoc, coreMethod
  const assocNProtocol = coreMethod("IVector/-assoc-n", [3], assoc);
  //@unit dissocProtocol effect requires: coreMethod, dissoc
  const dissocProtocol = coreMethod("IMap/-dissoc", [2], dissoc);
  //@unit conjProtocol effect requires: conj, coreMethod
  const conjProtocol = coreMethod("ICollection/-conj", [2], conj);
  //@unit emptyProtocol effect requires: coreMethod, empty
  const emptyProtocol = coreMethod("IEmptyableCollection/-empty", [1], empty);
  //@unit disjoinProtocol effect requires: coreMethod, disj
  const disjoinProtocol = coreMethod("ISet/-disjoin", [2], disj);
  //@unit peekProtocol effect requires: coreMethod, peek
  const peekProtocol = coreMethod("IStack/-peek", [1], peek);
  //@unit popProtocol effect requires: coreMethod, pop
  const popProtocol = coreMethod("IStack/-pop", [1], pop);
  //@unit rseqProtocol effect requires: coreMethod, rseq
  const rseqProtocol = coreMethod("IReversible/-rseq", [1], rseq);
  //@unit compareProtocol effect requires: compareKeys, coreMethod
  const compareProtocol = coreMethod("IComparable/-compare", [2], compareKeys);
  //@unit iteratorProtocol effect requires: coreMethod, iterator
  const iteratorProtocol = coreMethod("IIterable/-iterator", [1], iterator);
  //@unit imul
  const imul = Math.imul;
  //@unit nameProtocol effect requires: builtinCoreImplementation, coreMethod, INamed, name
  const nameProtocol = coreMethod("INamed/-name", [1], value => { if (!builtinCoreImplementation(INamed, value)) throw new Error("No protocol method INamed.-name defined for this value"); return name(value); });
  //@unit namespaceProtocol effect requires: coreMethod, namespace
  const namespaceProtocol = coreMethod("INamed/-namespace", [1], namespace);
  //@unit hashProtocol effect requires: coreMethod, hash
  const hashProtocol = coreMethod("IHash/-hash", [1], hash);
  //@unit equivProtocol effect requires: coreMethod, equalPair
  const equivProtocol = coreMethod("IEquiv/-equiv", [2], equalPair);
  //@unit type
  const type = value => value == null ? null : value.constructor;
  //@unit identical
  const identical = (a, b) => a === b;
  //@unit isUuid requires: implementsProtocol, IUUID
  const isUuid = value => implementsProtocol(IUUID, value);
  //@unit isInst requires: Inst, satisfies
  const isInst = value => satisfies(Inst, value);
  //@unit isTaggedLiteral requires: TaggedLiteral
  const isTaggedLiteral = value => value instanceof TaggedLiteral;
  //@unit encodeJsProtocol effect requires: coreMethod
  const encodeJsProtocol = coreMethod("IEncodeJS/-clj->js", [1], value => value);
  //@unit encodeKeyProtocol effect requires: coreMethod
  const encodeKeyProtocol = coreMethod("IEncodeJS/-key->js", [1], value => value);
  //@unit encodeClojureProtocol effect requires: coreMethod
  const encodeClojureProtocol = coreMethod("IEncodeClojure/-js->clj", [2], value => value);
  //@unit instMs requires: instMsProtocol
  const instMs = value => instMsProtocol(value);
  //@unit instMsProtocol effect requires: coreMethod
  const instMsProtocol = coreMethod("Inst/inst-ms*", [1], value => value.getTime());
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
  //@unit arraySeq requires: invalidArity, view
  const arraySeq = function (array, start = 0) { if (arguments.length > 2) return invalidArity(arguments.length); return array != null && start < array.length ? view(array, start, true) : null; };
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

  //@unit compilerForms requires: compilerFormBridge
  const compilerForms = compilerFormBridge();
  //@unit reader requires: cells, compilerForms, conj, contains, coreCells, first, freshKeyword,
  //@ gensym, hashMap, hashSet, inspect, into, isRecord, keywords, listFromArray, mapFromArray,
  //@ maps, merge, meta, next, nsName, PersistentQueueValue, prStr, readerCoreNames, rePattern, seq,
  //@ setFromArray, sets, str, symbol, symbols, toArray, uuid, uuids, vectorFromArray, vectors,
  //@ views, withMeta
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
        if (data?.kind === 'vector') return rt.compilerForms.jsArray(data.items);
        if (data?.kind === 'map') {
          const pairs = [];
          for (const [key, value] of data.entries) {
            const name = inspect(key);
            if (typeof key !== 'string' && !(name?.kind === 'keyword' && name.namespace === null)) fail(s, 'Invalid JS object literal key');
            pairs.push(typeof key === 'string' ? key : name.name, value);
          }
          return rt.compilerForms.jsObject(pairs);
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
        if (data.namespace === null) {
          if (data.name.endsWith('#')) { if (!gensyms.has(data.name)) gensyms.set(data.name, gensym(data.name.slice(0,-1) + '__')); resolved = gensyms.get(data.name); }
          else if (!['if','do','let*','fn*','loop*','recur','throw','try','new','set!','quote','var','def'].includes(data.name)) {
            resolved = rt.symbol(readerCoreNames.has(data.name) ? 'cljs.core' : currentName, data.name);
          }
        }
        if (data.namespace !== null && context?.aliases.has(data.namespace)) resolved = rt.symbol(context.aliases.get(data.namespace), data.name);
        if (data.namespace === null && !data.name.endsWith('#')) {
          if (context?.bindings.has(data.name) || context?.excludes.has(data.name)) resolved = rt.symbol(currentName, data.name);
          const referred = context?.refers.get(data.name);
          if (referred && !context?.bindings.has(data.name)) resolved = rt.symbol(...referred);
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
  function readWith(profile, args) {
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
    const opts = options(option, profile), state = stateOf(stream);
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
  function readStringWith(profile, args) {
    if (args.length !== 1 && args.length !== 2) arity(args.length);
    const text = args[args.length - 1];
    if (profile !== 'code' && (text == null || text === false)) return null;
    const opts = options(args.length === 2 ? args[0] : undefined, profile, true);
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
    readString: (...args) => readStringWith('code', args), read: (...args) => readWith('code', args),
    readLine: function () { if (arguments.length) arity(arguments.length); return readLine(); },
    compilerInputWindow, compilerInputConsume, compilerInputRecover, compilerInputDiscard, compilerInputCompact,
    readerStringReader: stringReader, readerInputProvider: inputProvider, readerReadLine: readLine,
    readerParseTimestampFields: function (value) { if (arguments.length !== 1) arity(arguments.length); return rt.vectorFromArray(timestampFields(value)); },
    readerParseTimestamp: function (value) { if (arguments.length !== 1) arity(arguments.length); return parseTimestamp(value); },
    readerInitialTagTable: function () { if (arguments.length) arity(arguments.length); return rt.mapFromArray([...builtinTags].flatMap(([tag,f]) => [rt.symbol(null,tag),f])); },
    readerRegisterTagParser: register, readerDeregisterTagParser: deregister,
    readerRegisterDefaultTagParser: registerDefault, readerDeregisterDefaultTagParser: deregisterDefault,
    readerWithInput: withInput, readerCurrentInput: () => coreCells.readerCurrentInputValue, readerClose: close};
  })({keyword: freshKeyword, symbol, vectorFromArray, listFromArray, mapFromArray, setFromArray, inspect, compilerForms, ednKind: value => symbols.has(value) ? 'symbol' : keywords.has(value) ? 'keyword' : vectors.has(value) ? 'vector' : maps.has(value) ? 'map' : sets.has(value) ? 'set' : cells.has(value) || views.has(value) ? 'seq' : uuids.has(value) ? 'uuid' : value instanceof Date ? 'instant' : null});

  //@unit loadFile requires: invalidArity
  const loadFile = function () { if (arguments.length !== 1) return invalidArity(arguments.length); throw new Error("load-file requires a compiler session and an explicit file provider"); };
  //@unit core requires: abs, aclone, add, addMethodProtocol, addTap, addWatch, addWatchProtocol,
  //@ aget, alength, allNs, alterMeta, ancestors, apply, applyTo, array, arrayChunk, arrayMap,
  //@ arraySeq, ASeq, aset, assoc, assocBang, assocBangProtocol, assocIn, assocNBangProtocol,
  //@ assocNProtocol, assocProtocol, asTransientProtocol, Atom, atom, atomFactory, bases, bitAnd,
  //@ bitAndNot, bitClear, bitCount, bitFlip, bitNot, bitOr, bitSet, bitShiftLeft, bitShiftRight,
  //@ bitShiftRightZeroFill, bitTest, bitXor, boolean, booleans, boundedCount, butlast, byte, bytes,
  //@ cat, char, chars, checkedAget, checkedAgetStrict, checkedAset, checkedAsetStrict, chunk,
  //@ chunkAppend, ChunkBuffer, chunkBuffer, chunkCons, ChunkedCons, chunkedFirstProtocol,
  //@ chunkedNextProtocol, chunkedRestProtocol, ChunkedSeqValue, chunkFirst, chunkNext, chunkRest,
  //@ cljToJs, clone, comp, comparator, comparatorProtocol, compareAndSet, compareKeys,
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
  //@ greaterEqual, groupBy, haltWhen, hash, hashDouble, hashMap, hashOrderedColl, hashProtocol,
  //@ hashSet, hashUnorderedColl, hostRandInt, hostRandNth, hostRandomUuid, hostShuffle,
  //@ IAssociative, IAtom, IChunk, IChunkedNext, IChunkedSeq, ICloneable, ICollection, IComparable,
  //@ ICounted, identical, identity, IDeref, IDerefWithTimeout, IDrop, IEditableCollection,
  //@ IEmptyableCollection, IEncodeClojure, IEncodeJS, IEquiv, IFind, IFn, IHash, IIndexed,
  //@ IIterable, IKVReduce, IList, ILookup, IMap, IMapEntry, IMeta, implementsProtocol, imul,
  //@ IMultiFn, INamed, inc, IndexedSeqFactory, IndexedSeqValue, INext, Inst, instMs,
  //@ instMsProtocol, int, intArray, IntegerRangeFactory, IntegerRangeValue, interleave, interpose,
  //@ into, intoArray, ints, invokeProtocol, IPending, IPrintWithWriter, IRecord, IReduce, IReset,
  //@ IReversible, isa, isAny, isArray, isAssociative, isBoolean, isChar, isChunkedSeq, isCloneable,
  //@ isColl, isCounted, isDelay, isDistinct, isDouble, isEmpty, ISeq, ISeqable, ISequential, ISet,
  //@ isEven, isFalse, isFloat, isFn, isIdent, isIFind, isIFn, isIndexed, isInfinite, isInst,
  //@ isInstance, isInt, isInteger, isIterable, isJsSymbol, isKeyword, isList, isMap, isMapEntry,
  //@ isNaNCore, isNaturalInt, isNegative, isNegativeInt, isNil, isNumber, isObject, isOdd, ISorted,
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
  //@ namespaceProtocol, newline, next, nextProtocol, nfirst, nnext, NodeSeqType, not, notAny,
  //@ notEmpty, notEqual, notEvery, notifyWatches, nsName, nth, nthnext, nthProtocol, nthrest,
  //@ numericEqual, objectArray, parents, parseBoolean, parseDouble, parseLong, parseUuid, partial,
  //@ partition, partitionAll, partitionBy, partitionV, partitionVAll, peek, peekProtocol,
  //@ PersistentArrayMapValue, persistentBang, persistentBangProtocol, PersistentHashMapValue,
  //@ PersistentHashSetValue, PersistentQueueValue, PersistentTreeMapFactory,
  //@ PersistentTreeMapSeqFactory, PersistentTreeSetFactory, PersistentVectorValue, pop, popBang,
  //@ popBangProtocol, popProtocol, pr, preferMethodProtocol, prefers, primSeq, printCore,
  //@ printForHost, println, printlnStr, printMetaQ, printStr, prn, prnStr, prnStrWithOpts,
  //@ protocolSentinel, prSequentialWriter, prSeqWriter, prStr, prStrStar, prStrWithOpts, prWriter,
  //@ quot, randomSample, range, RangedIterator, rangedIterator, RangedIteratorFactory,
  //@ RangeFactory, RangeValue, reader, realized, reduce, Reduced, reduced, reduceKv,
  //@ reduceProtocol, reductions, reFind, registerReaderNamespace, rem, reMatches, remove,
  //@ removeAllMethods, removeMethodProtocol, removeTap, removeWatch, removeWatchProtocol,
  //@ rePattern, repeat, repeatedly, RepeatValue, replace, replicate, requiredGet, reSeq, reset,
  //@ resetMeta, resetProtocol, resetVals, rest, restProtocol, reverse, rotateLeft, rseq,
  //@ rseqProtocol, rsubseq, run, satisfies, second, selectKeys, seq, SeqIter, seqIter,
  //@ SeqIterFactory, seqProtocol, seqToMap, sequence, sequenceIterator, set, setPrintErrFn,
  //@ setPrintFn, setValidator, short, shorts, some, someFn, someVals, sort, sortBy, sortedMap,
  //@ sortedMapBy, sortedSeqFromProtocol, sortedSeqProtocol, sortedSet, sortedSetBy, splitAt,
  //@ splitVAt, splitWith, str, StringBufferWriter, stringHash, stringPrint, subs, subseq, subtract,
  //@ subvec, SubvecValue, supers, swap, swapProtocol, swapVals, symbolCore, symbolIdentical,
  //@ SymbolValue, systemTime, TaggedLiteral, taggedLiteral, TaggedLiteralFactory, take, takeLast,
  //@ takeNth, takeWhile, tapGreater, test, theNs, throwableToMap, toArray, toArray2d, trampoline,
  //@ transduce, transient, TransientArrayMapType, TransientHashMapType, TransientHashSetType,
  //@ TransientVectorType, treeMapConstructor, treeMapSeqConstructor, treeSeq, treeSetConstructor,
  //@ type, uncheckedAdd, uncheckedAddInt, uncheckedByte, uncheckedChar, uncheckedDec,
  //@ uncheckedDecInt, uncheckedDivideInt, uncheckedDouble, uncheckedFloat, uncheckedGet,
  //@ uncheckedInc, uncheckedIncInt, uncheckedInt, uncheckedLong, uncheckedMultiply,
  //@ uncheckedMultiplyInt, uncheckedNegate, uncheckedNegateInt, uncheckedRemainderInt,
  //@ uncheckedShort, uncheckedSubtract, uncheckedSubtractInt, underive, unreduced,
  //@ unsignedBitShiftRight, update, updateIn, updateKeys, updateVals, uuid, UUIDFactory, UUIDType,
  //@ valProtocol, vals, Var, varyMeta, vec, vector, Volatile, volatile, volatileFactory, vreset,
  //@ vresetProtocol, withMeta, withMetaProtocol, write, writeAll, zipmap
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
    prStrWithOpts, prnStrWithOpts, prSequentialWriter, prSeqWriter, printMetaQ, printForHost,
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
    readString: reader.readString, read: reader.read, readLine: reader.readLine,
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
  //@unit namespaces.cljsUser effect: namespaces requires: coreCells, createNs, symbol
  coreCells.currentNs = createNs(symbol(null, "cljs.user"));
  //@unit namespaces.cljsCore effect: namespaces requires: createNs, symbol
  createNs(symbol(null, "cljs.core"));
  //@unit core.seal effect: core requires: core
  Object.seal(core);
  //@exports
  return Object.freeze({setDeclarationDemand, declarationReceiver, declarationTransaction, applyDeclarationPatches, snapshotDeclarationPatches, restoreDeclarationPatches, compilerExceptionMessage, compilerForms, keyword, keywordFqn, symbol, vector, vectorFromArray, listFromArray, mapFromArray, mapFromArrays, setFromArray, constantMapFromArray, constantSetFromArray, literalSetFromArray, queueFromArray, jsObject, makeVar, definitionVar, makeIFn, protocol, protocolPredicate, protocolMethod, extendType, extendInstance, makeReify, reify, makeType, recordFromMap, field, fnRest, fnApplyTo, setMacroexpandServices, compilerFormBridge, core, inspect, hash});
})()
