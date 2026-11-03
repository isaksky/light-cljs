// Original bounded runtime for emitted JavaScript. This file is a factory
// expression, embedded once in each compilation unit that needs owned values.
(() => {
  "use strict";

  // Brands and payloads stay private: copying fields or inheriting a value's
  // prototype cannot turn a host object into a language value.
  const keywords = new WeakMap();
  const internedKeywords = new Map();
  // A vector's brand and store (see `vectorTailOffset`) is instead a private
  // field, with a WeakMap's `has`, `get` and `set`: no host object can have
  // or copy one either, and adding it to a new value is a property store,
  // not a WeakMap entry, which a vector built by every `conj` cannot afford.
  // `new vectors` adds the field `#s` to the value its base constructor
  // returns, which `set` requires to be a new object.
  const vectors = class extends (function (value) { return value; }) {
    #s;
    static has(value) { return (typeof value === "object" ? value !== null : typeof value === "function") && #s in value; }
    static get(value) { if (vectors.has(value)) return value.#s; }
    static set(value, store) { new vectors(value).#s = store; }
  };
  const symbols = new WeakMap();
  // A host whose WeakMap entries cost less than private fields, as the
  // evaluator's reference-counting QuickJS, states so before the runtime
  // loads; a `privateStore` or `kindedStore` is then a WeakMap, as
  // unforgeable a brand. The engine is never sniffed (light-cljs-292).
  // Where it is not, a store tests that a value can hold a private field
  // with `canHoldField`, which V8 inlines.
  const weakMapStores = globalThis.lightCljsWeakMapStores === true,
    canHoldField = value => typeof value === "object" ? value !== null : typeof value === "function";
  // A store like a vector's, with a private field of its own: the seq kinds'
  // stores (`cells`, `views`, `lazyValues`, `chunks`), and those of the other
  // values that operations build one per call (`reducedValues`, `transients`,
  // `queues`, `iteratorSeqs`; and `kindedStore`). Each step of a seq builds a
  // value, and a WeakMap entry per value, which the collector traces as an
  // ephemeron, cost more than the step (light-cljs-259, light-cljs-291). As
  // `vectors.set`, `set` requires a new value; `reset` replaces the payload
  // of a value that has one (light-cljs-237).
  // The stores share these methods' code, and so V8's feedback for them,
  // which the seq operations do not feel; `vectors` keeps its own, since
  // sharing slowed `conj` and `nth` on a vector by a third.
  function privateStore() {
    if (weakMapStores) { const store = new WeakMap(); store.reset = store.set; return store; }
    const Store = class extends (function (value) { return value; }) {
      #s;
      static has(value) { return canHoldField(value) && #s in value; }
      static get(value) { if (canHoldField(value) && #s in value) return value.#s; }
      static set(value, payload) { new Store(value).#s = payload; }
      static reset(value, payload) { value.#s = payload; }
    };
    return Store;
  }
  // The store of the maps and sets outside the hash tries (`maps`, `sets`):
  // a `privateStore` that also keeps a value's kind (`mapKinds`, `setKinds`)
  // in a second private field, so that a map or set, which each `assoc` or
  // `conj` builds, gets both with one construction (light-cljs-291). A
  // value's kind is set after its payload.
  function kindedStore() {
    if (weakMapStores) { const store = new WeakMap(); store.reset = store.set; store.kinds = new WeakMap(); return store; }
    const Store = class extends (function (value) { return value; }) {
      #s; #k;
      static has(value) { return canHoldField(value) && #s in value; }
      static get(value) { if (canHoldField(value) && #s in value) return value.#s; }
      static set(value, payload) { new Store(value).#s = payload; }
      static reset(value, payload) { value.#s = payload; }
      static kinds = {
        // Not `Store.has`, which the hash tries and records replace.
        has: value => canHoldField(value) && #s in value && value.#k !== undefined,
        get(value) { if (canHoldField(value) && #s in value) return value.#k; },
        set(value, kind) { value.#k = kind; return this; }
      };
    };
    return Store;
  }
  const maps = kindedStore();
  const mapKinds = maps.kinds;
  const sortedComparators = new WeakMap();
  const mapEntries = new WeakSet();
  const sets = kindedStore();
  const setKinds = sets.kinds;
  // The type of each map kind that a kept map type builds, for building a map
  // of an existing map's kind; `setTypes` likewise for sorted sets.
  const mapTypes = {}, setTypes = {};
  const transients = privateStore();
  const cells = privateStore();
  const views = privateStore();
  const iteratorSeqs = privateStore();
  const reducedValues = privateStore();
  const stringBufferWriters = new WeakSet();
  const constructorHashes = new WeakMap();
  const publicCollectionSlots = new WeakMap();
  const vectorRoots = new WeakMap();
  const canonicalVectorRoots = new WeakMap();
  const metadataValues = new WeakMap();
  const deferredVarMetadata = new WeakMap();
  const uuids = new WeakMap();
  const uuidHashes = new WeakMap();
  const vars = new WeakMap();
  const atoms = new WeakMap(), volatiles = new WeakMap();
  const queues = privateStore();
  const lazyValues = privateStore(), delays = new WeakMap();
  const ifnValues = new WeakMap(), ifnVariadics = new WeakMap();
  const metaFunctions = new WeakMap();
  const typeNames = new WeakMap();
  const recordTypes = new WeakMap();
  const reifyValues = new WeakSet();
  // Generic operations' protocol dispatch, empty unless `installMethods` is kept.
  const protocolHooks = {};
  // Generic operations' branches for one kind, installed by the kind's builders.
  const kindHooks = {};
  // The key under which a runtime type's prototype holds its kind's number
  // (set where the type is), which picks the kind's branches of the generic
  // operations (`kindTables`, light-cljs-237).
  const kindOps = Symbol("kind");
  // What a branch of a generic operation gives for a value it leaves to the
  // operation (`kindTables`): a marker of its own, which no operation gives
  // and no method receives, so that every other answer, `missing` among
  // them, is the branch's (light-cljs-387).
  const noBranch = Object.freeze({});
  // The generic operations' branches by kind number, each kind's by
  // operation: 0 `seq`, 1 `first`, 2 `rest`, 3 `next`, 4 `count`, 5 `nth`,
  // 6 `get`, 7 `conj`, 8 `assoc`. The kinds are 1 lazy seqs (`LazySeq`),
  // 2 cells (`ChunkedCons`, `Cons`, `ListValue`, `IntegerRangeValue`,
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
  const protocolMaskNames = "IFn ICounted IEmptyableCollection ICollection IIndexed ASeq ISeq INext ILookup IAssociative IMap IMapEntry ISet IStack IVector IDeref IDerefWithTimeout IMeta IWithMeta IReduce IKVReduce IEquiv IHash ISeqable ISequential IList IRecord IReversible ISorted IPrintWithWriter IWriter IPrintWithWriter IPending IWatchable IEditableCollection ITransientCollection ITransientAssociative ITransientMap ITransientVector ITransientSet IMultiFn IChunkedSeq IChunkedNext IComparable INamed ICloneable IAtom IReset ISwap IIterable IDrop".split(" ");
  // Membership checks are on every generic collection operation. Resolve the
  // fixed ABI once as [property, bit], including the last occurrence of
  // duplicate protocol names.
  const protocolMasks = new Map(protocolMaskNames.map((name, index) => [name,
    ["cljs$lang$protocol_mask$partition" + (index >> 5) + "$", 1 << (index % 32)]]));
  const memberMunging = {"-":"_",":":"_COLON_","+":"_PLUS_",">":"_GT_","<":"_LT_","=":"_EQ_","~":"_TILDE_","!":"_BANG_","@":"_CIRCA_","#":"_SHARP_","'":"_SINGLEQUOTE_","\"":"_DOUBLEQUOTE_","%":"_PERCENT_","^":"_CARET_","&":"_AMPERSAND_","*":"_STAR_","|":"_BAR_","{":"_LBRACE_","}":"_RBRACE_","[":"_LBRACK_","]":"_RBRACK_","/":"$","\\":"_BSLASH_","?":"_QMARK_"};
  function mungeMember(name) { return Array.from(name, c => memberMunging[c] ?? c).join(""); }
  const protocolSentinel = Object.freeze({}), protocolSlots = new Map();
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
  // The resolved keys (`methodKey`) the generic operations dispatch, by a
  // number each: the key of `ILookup/-lookup` is number 30, which the unit
  // `installMethods.ILookupLookupKey` installs.
  const methodKeys = [];
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
  // The slot name of a resolved key's method for `arity`, named once.
  function keySlot(key, arity) { return key.slots[arity] ?? (key.slots[arity] = protocolSlot(key.key, arity)); }
  protocolHooks.methodKey = methodKey;
  methodKeys[1] = methodKey("IAssociative/-assoc");
  methodKeys[2] = methodKey("IAssociative/-contains-key?");
  methodKeys[5] = methodKey("IChunkedSeq");
  methodKeys[7] = methodKey("ICollection/-conj");
  methodKeys[10] = methodKey("ICounted/-count");
  methodKeys[11] = methodKey("ICounted");
  methodKeys[12] = methodKey("IDeref/-deref");
  methodKeys[14] = methodKey("IEditableCollection");
  methodKeys[18] = methodKey("IEquiv/-equiv");
  methodKeys[22] = methodKey("IHash/-hash");
  methodKeys[23] = methodKey("IHash");
  methodKeys[24] = methodKey("IIndexed");
  methodKeys[25] = methodKey("IIndexed/-nth");
  methodKeys[26] = methodKey("IIterable/-iterator");
  methodKeys[27] = methodKey("IIterable");
  methodKeys[30] = methodKey("ILookup/-lookup");
  methodKeys[32] = methodKey("IMapEntry");
  methodKeys[33] = methodKey("IMap");
  methodKeys[34] = methodKey("IMeta");
  methodKeys[35] = methodKey("IMeta/-meta");
  methodKeys[36] = methodKey("INamed");
  methodKeys[37] = methodKey("INamed/-name");
  methodKeys[40] = methodKey("INext/-next");
  methodKeys[41] = methodKey("IPrintWithWriter/-pr-writer");
  methodKeys[42] = methodKey("IRecord");
  methodKeys[44] = methodKey("IReduce/-reduce");
  methodKeys[48] = methodKey("ISeq/-first");
  methodKeys[49] = methodKey("ISeq");
  methodKeys[50] = methodKey("ISeq/-rest");
  methodKeys[51] = methodKey("ISeqable");
  methodKeys[52] = methodKey("ISeqable/-seq");
  methodKeys[53] = methodKey("ISequential");
  methodKeys[55] = methodKey("ISet");
  methodKeys[61] = methodKey("IVector");
  methodKeys[66] = methodKey("IWithMeta/-with-meta");
  // Called once per type, before any of its values exist; a type whose base
  // is `Object.prototype` has that prototype already. Extension prototypes
  // are public and mutable; immutable collection payloads and their base
  // callability remain private and frozen.
  function builtinType(Type, base) {
    Type.prototype = Object.create(base);
    Object.defineProperty(Type.prototype, "constructor", {value: Type, writable: true, configurable: true});
    return Type;
  }
  const sequencePrototype = Object.freeze({
    equiv(other) { return objectEquiv(this, other); },
    toString() { return printHooks.collection(this); },
    indexOf(value, start = 0) { return sequentialIndex(this, value, start, false); },
    lastIndexOf(value, start = count(this)) { return sequentialIndex(this, value, start, true); }
  });
  builtinType(ListValue, sequencePrototype).prototype[kindOps] = 2;
  builtinType(EmptyListValue, sequencePrototype);
  builtinType(Cons, sequencePrototype).prototype[kindOps] = 2;
  builtinType(IndexedSeqValue, sequencePrototype);
  builtinType(ChunkedSeqValue, sequencePrototype);
  builtinType(ChunkedCons, sequencePrototype).prototype[kindOps] = 2;
  builtinType(LazySeq, sequencePrototype).prototype[kindOps] = 1;
  builtinType(RangeValue, sequencePrototype).prototype[kindOps] = 2;
  builtinType(IntegerRangeValue, sequencePrototype).prototype[kindOps] = 2;
  const NodeSeqType = builtinType(function () {}, sequencePrototype);
  const emptyList = Object.create(EmptyListValue.prototype);
  cells.set(emptyList, Object.freeze({kind: "list", count: 0}));
  const missing = Object.freeze({});
  const truth = value => value != null && value !== false;
  const invalidArity = n => { throw new Error("Invalid arity: " + n); };
  const outOfBounds = () => { throw new Error("Index out of bounds"); };

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
  builtinType(KeywordValue, keywordPrototype);

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
  builtinType(SymbolValue, symbolPrototype);

  // Positional constructors retain the supplied slots; unlike cons/symbol they
  // do not normalize a tail or derive the printed spelling from name/namespace.
  function SymbolValue(namespace, name, spelling, cachedHash, metadata) {
    if (arguments.length !== 5) return invalidArity(arguments.length);
    const value = Object.create(SymbolValue.prototype);
    symbols.set(value, Object.freeze({namespace, name, spelling}));
    // A null hash reads as none: `hash` computes it, the `_hash` field gives
    // null (an undefined one is stored, as the field gives it back).
    if (cachedHash !== null) constructorHashes.set(value, cachedHash);
    metadataValues.set(value, metadata);
    return value;
  }

  function Cons(metadata, head, tail, cachedHash) {
    if (arguments.length !== 4) return invalidArity(arguments.length);
    const value = Object.create(Cons.prototype);
    cells.set(value, Object.freeze({kind: "cons", head, tail}));
    if (cachedHash !== null) constructorHashes.set(value, cachedHash);
    metadataValues.set(value, metadata);
    return value;
  }

  function constructorSlots(value, slots, metadata, cachedHash) {
    publicCollectionSlots.set(value, slots);
    metadataValues.set(value, metadata);
    if (cachedHash !== null) constructorHashes.set(value, cachedHash);
    return value;
  }
  function ListValue(metadata, head, tail, suppliedCount, cachedHash) {
    const value = Object.create(ListValue.prototype);
    cells.set(value, Object.freeze({kind: "list", head, tail, count: suppliedCount}));
    return constructorSlots(value, {}, metadata, cachedHash);
  }
  function EmptyListValue(metadata) {
    const value = Object.create(EmptyListValue.prototype);
    cells.set(value, Object.freeze({kind: "list", count: 0}));
    metadataValues.set(value, metadata);
    return value;
  }
  function KeywordValue(namespace, name, spelling, cachedHash) {
    const value = makeKeyword(namespace, name, spelling);
    constructorHashes.set(value, cachedHash);
    return value;
  }
  // Root slots are opaque canonical tokens, never an upstream trie-node ABI:
  // a token names the private root of the vector whose `root` field gave it.
  // The supplied tail is shared, as upstream.
  function PersistentVectorValue(metadata, cnt, shift, root, tail, cachedHash) {
    // `callerTail` marks the tail, which stores that keep it inherit, as the
    // caller's: no seq freezes it (`vectorChunk`).
    const value = makeVector(null, {cnt, shift, root: vectorRoots.get(root) ?? [], tail, callerTail: true});
    return constructorSlots(value, {cnt, shift, root, tail}, metadata, cachedHash);
  }
  function ChunkedSeqValue(source, node, index, offset, metadata, cachedHash) {
    const value = view(ChunkedSeqValue, node, offset, false, "chunked"), slots = {vec: source, node, i: index, off: offset};
    // Without metadata or a hash, which a vector's seq has at every step,
    // no entry reads differently from a null one.
    if (metadata == null && cachedHash == null) { publicCollectionSlots.set(value, slots); return value; }
    return constructorSlots(value, slots, metadata, cachedHash);
  }
  // The array map type, which builders instantiate with `makeMap`. Called, it
  // is `publicArrayMap`, which a program naming `PersistentArrayMap` keeps.
  function PersistentArrayMapValue(metadata, cnt, array, cachedHash) {
    return typeHooks.arrayMap(metadata, cnt, array, cachedHash);
  }
  function PersistentHashMapValue(metadata, cnt, root, hasNil, nilValue, cachedHash) {
    const source = root == null ? null : maps.get(root);
    let entries;
    // A hash map root shares its trie; other maps' entries are hashed again.
    if (source && !Array.isArray(source)) entries = {root: source.root, nil: null, length: source.length - (source.nil ? 1 : 0)};
    else {
      entries = (source ?? []).filter(e => e.key != null);
      if (hashRoots.has(source)) hashRoots.set(entries, hashRoots.get(source));
      entries = hashTrie(entries);
    }
    if (hasNil) { entries.nil = Object.freeze({key: null, value: nilValue, hash: 0}); entries.length++; }
    return constructorSlots(makeMap(PersistentHashMapValue, entries, "hash-map"), {cnt, root, "has-nil?": hasNil, "nil-val": nilValue}, metadata, cachedHash);
  }
  function symbol(namespace, name) {
    const value = Object.create(SymbolValue.prototype);
    symbols.set(value, Object.freeze({namespace, name,
      spelling: namespace == null ? name : namespace + "/" + name}));
    return value;
  }

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

  let gensymCounter = 0;
  const generatedNames = new Set();
  function gensym(prefix) {
    if (arguments.length > 1) return invalidArity(arguments.length);
    const base = str(arguments.length === 0 ? "G__" : prefix);
    let name; do { name = base + ++gensymCounter; } while (generatedNames.has(name));
    generatedNames.add(name);
    return symbolCore(name);
  }

  function cons(head, tail, kind = "cons") {
    if (kind === "cons") {
      if (tail == null) kind = "list";
      else if (!(protocolHooks.implements?.(methodKeys[49], tail) ?? seqValue(tail))) tail = seq(tail);
    }
    const value = Object.create((kind === "list" ? ListValue : Cons).prototype);
    // The payload is private and never written, so a step does not freeze it.
    cells.set(value, kind === "list" ? {kind, head, tail, count: count(tail) + 1} : {kind, head, tail});
    return value;
  }

  // Each tail was just built here. Its stored count is intrinsic, independent
  // of public ICounted overrides, as in List.-conj; no protocol dispatch is
  // needed to construct it. Later public count calls still honor overrides.
  function listFromArray(items) {
    let result = emptyList, count = 0;
    for (let i = items.length - 1; i >= 0; i--) {
      const value = Object.create(ListValue.prototype);
      cells.set(value, Object.freeze({kind: "list", head: items[i], tail: result, count: ++count}));
      result = value;
    }
    return result;
  }

  function list(...items) { return listFromArray(items); }

  // A seq over `storage` from `offset`, of the type the caller builds
  // (IndexedSeq, ChunkedSeq, NodeSeq or PersistentTreeMapSeq for `kind`), so
  // that a caller keeps only the types it can build. A view rebuilt further
  // on reads its type from the view.
  function view(Type, storage, offset, indexed, kind = "indexed", tail = null) {
    const value = Object.create(Type.prototype);
    views.set(value, Object.freeze({storage, offset, indexed, kind, tail, Type}));
    return value;
  }

  function seq(value) {
    if (value == null || (cells.get(value)?.kind === "list" && cells.get(value).count === 0 && !publicCollectionSlots.has(value))) return null;
    const lazy = lazyValues.get(value);
    if (lazy) return kindHooks.seqLazy(lazy);
    const queue = queues.get(value);
    if (queue) return kindHooks.seqQueue(value, queue);
    if (Array.isArray(value)) return value.length ? view(IndexedSeqValue, value, 0, true) : null;
    if (cells.has(value)) return value;
    if (views.has(value)) { const v = views.get(value); return v.offset < v.storage.length ? value : null; }
    const items = vectors.get(value);
    if (items) return kindHooks.seqVector(value, items);
    if (typeof value === "string") return value.length ? view(IndexedSeqValue, value, 0, true) : null;
    if (maps.has(value)) return kindHooks.seqMap(value);
    const members = sets.get(value);
    if (members) return kindHooks.seqSet(value, members);
    if (iteratorSeqs.has(value)) return value;
    if (value[Symbol.iterator] != null) return iteratorSeq(value[Symbol.iterator]());
    throw new Error(String(value) + " is not ISeqable");
  }
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
  // A generic operation of one value behind the protocol implementation
  // `method` finds for it: the operations call protocol implementations only
  // where `installMethods` is kept, which wraps them (`seq.protocol`).
  function behindProtocol(operation, method) {
    return Object.defineProperty(function (value) {
      const implementation = value == null ? null : method(value);
      return implementation ? implementation.call(value, value) : operation(value);
    }, "name", {value: operation.name});
  }
  seq = behindProtocol(seq, value => protocolHooks.publicMethod?.(value, methodKeys[52], 1, true));
  // The seq of a cell, itself, where the operations' paths give it: the
  // branch of the kinds whose payload is a cell.
  function cellSeq(value) {
    const cell = cells.get(value);
    return cell === undefined || cell.kind === "list" && cell.count === 0 ? noBranch : value;
  }
  kindTables[2][0] = cellSeq;
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
      const lazy = lazyValues.get(value), method = protocolHooks.publicMethod;
      if (lazy === undefined) return noBranch;
      if (!method) return kindTables[2][index](kindHooks.realizeLazy(lazy));
      const own = method(value, methodKeys[52], 1, true);
      if (own) own.call(value, value); else kindHooks.seqLazy(lazy);
      return operation(lazy.value);
    };
  }
  kindTables[1][0] = value => {
    const lazy = lazyValues.get(value);
    return lazy === undefined ? noBranch : kindHooks.seqLazy(lazy);
  };
  // The seq of what a lazy seq realizes: a cell is its own, which `seq`,
  // looking for an `ISeqable` implementation that JavaScript gave it, also
  // finds where protocol implementations exist.
  kindHooks.seqLazy = lazy => {
    const current = kindHooks.realizeLazy(lazy);
    if (!protocolHooks.publicMethod) { const result = cellSeq(current); if (result !== noBranch) return result; }
    return seq(current);
  };
  // The raw value a lazy seq realizes, without the seq of it, which can take
  // an item of a host iterator.
  kindHooks.realizeLazy = lazy => {
    let current = deferredValue(lazy, true);
    // Flatten nested LazySeq results iteratively, retaining the raw final
    // value. CLJS calls seq on that raw value again on each traversal.
    while (lazyValues.has(current)) {
      current = deferredValue(lazyValues.get(current), true);
      // Compress as we advance, so discarded wrappers and their captured
      // source heads can be collected even through a long empty prefix.
      lazy.value = current;
    }
    return lazy.value = current;
  };
  // A vector of at most 32 items walks its tail; a longer one is upstream's
  // chunked seq over its trie's leaves.
  kindHooks.seqVector = (value, store) => !store.cnt ? null : store.cnt <= 32 ? view(IndexedSeqValue, vectorChunk(value, 0), 0, true)
    : ChunkedSeqValue(value, vectorChunk(value, 0), 0, 0, null, null);
  kindHooks.seqMap = value => {
    const treeSlots = publicCollectionSlots.get(value);
    if (treeSlots && "tree" in treeSlots) return kindHooks.seqTree(treeSlots);
    const entries = mapTraversal(value), kind = mapKinds.get(value);
    return entries.length ? view(kind === "hash-map" ? NodeSeqType : kind === "sorted-map" ? kindHooks.sortedMapSeq : IndexedSeqValue,
      Object.freeze(entries.map(entry => mapEntry(entry.key, entry.value))), 0, false, kind) : null;
  };
  // The seqs of the collections, kinds 4 to 7 (light-cljs-358), as the
  // operation gives them from their stores.
  kindTables[4][0] = value => { const store = vectors.get(value); return store ? kindHooks.seqVector(value, store) : noBranch; };
  kindTables[5][0] = value => maps.has(value) ? kindHooks.seqMap(value) : noBranch;
  // Map entries, kind 8 (light-cljs-373), whose store is a vector's.
  kindTables[8][0] = value => { const store = vectors.get(value); return store ? kindHooks.seqVector(value, store) : noBranch; };

  // A host iterator's seq (upstream's ES6IteratorSeq: ISeqable and ISeq, not
  // ISequential) is a plain object that `seq`, `first`, `rest` and
  // `builtinCoreImplementation` know from `iteratorSeqs`, without protocol
  // methods. CLJS reprobes a nil tail; only successful nonempty tails are
  // memoized.
  function iteratorSeq(iterator) {
    const item = iterator.next();
    if (item.done) return null;
    const value = {};
    let tail = null;
    iteratorSeqs.set(value, {first: item.value, rest() {
      if (tail === null) tail = iteratorSeq(iterator);
      return tail;
    }});
    return value;
  }
  function first(value) {
    if (value == null) return null;
    if (queues.has(value) && publicCollectionSlots.has(value)) return first(publicCollectionSlots.get(value).front);
    if (chunks.get(value)?.integer) return chunks.get(value).start;
    if (lazyValues.has(value)) { seq(value); return first(lazyValues.get(value).value); }
    // Existing ISeq values do not redispatch ISeqable.
    return firstSeq(cells.has(value) || views.has(value) || iteratorSeqs.has(value) ? value : seq(value));
  }
  // `first`'s steps once it has the seq `current` of its value, which the
  // collections' branches take too (`seqBranch`, light-cljs-375).
  function firstSeq(current) {
    if (current === null) return null;
    const cell = cells.get(current);
    if (cell) return cell.kind === "list" && cell.count === 0 && !publicCollectionSlots.has(current) ? null : cell.head;
    const slice = views.get(current);
    if (slice) return slice.offset < slice.storage.length ? slice.storage[slice.offset] : null;
    const own = iteratorSeqs.get(current);
    if (own) return own.first;
    const firstMethod = protocolHooks.builtin?.(current, methodKeys[48], 1);
    if (firstMethod) return firstMethod.call(current, current);
    throw new Error("ISeq.-first not supported on this type");
  }

  // As `seq.branches`.
  first = (operation => Object.defineProperty(function (value) {
    const kind = value?.[kindOps], result = typeof kind === "number" ? kindTables[kind & 15][1](value) : noBranch;
    return result !== noBranch ? result : operation(value);
  }, "name", {value: operation.name}))(first);
  first = behindProtocol(first, value => protocolHooks.publicMethod?.(value, methodKeys[48], 1, false, true));
  // `first` and `rest` of a cell, which is already a seq, take no seq of it
  // and dispatch no protocol method but the one their wrappers look for
  // (`first.protocol`); `next` takes the seq of its rest (`next.cells`).
  kindTables[2][1] = value => {
    const cell = cells.get(value);
    return cell === undefined || cell.kind === "list" && cell.count === 0 ? noBranch : cell.head;
  };
  kindTables[1][1] = lazyBranch(1, value => first(value));

  function rest(value) {
    if (value == null) return emptyList;
    if (lazyValues.has(value)) { seq(value); return rest(lazyValues.get(value).value); }
    return restSeq(cells.has(value) || views.has(value) || iteratorSeqs.has(value) ? value : seq(value));
  }
  // `rest`'s steps once it has the seq `current` of its value, as `firstSeq`.
  function restSeq(current) {
    if (current === null) return emptyList;
    const cell = cells.get(current);
    if (cell) return cell.kind === "list" ? cell.count === 1 || cell.count === 0 && !publicCollectionSlots.has(current) ? emptyList : cell.tail : cell.tail ?? emptyList;
    const slice = views.get(current);
    if (slice && !(slice.offset < slice.storage.length)) return emptyList;
    const slots = publicCollectionSlots.get(current);
    if (slice && slots?.vec) return kindHooks.restChunkedSeq(current, slice, slots);
    if (!slice) {
      const own = iteratorSeqs.get(current);
      if (own) return own.rest();
      const restMethod = protocolHooks.builtin?.(current, methodKeys[50], 1);
      if (restMethod) return restMethod.call(current, current);
      throw new Error("ISeq.-rest not supported on this type");
    }
    const tail = slice.offset + 1 < slice.storage.length
      ? view(slice.Type, slice.storage, slice.offset + 1, slice.indexed, slice.kind, slice.tail) : slice.tail ?? emptyList;
    return slice.kind === "queue" ? kindHooks.restQueue(tail, current) : tail;
  }
  // As `first.branches`.
  rest = (operation => Object.defineProperty(function (value) {
    const kind = value?.[kindOps], result = typeof kind === "number" ? kindTables[kind & 15][2](value) : noBranch;
    return result !== noBranch ? result : operation(value);
  }, "name", {value: operation.name}))(rest);
  rest = behindProtocol(rest, value => protocolHooks.publicMethod?.(value, methodKeys[50], 1, false, true));
  kindTables[2][2] = value => {
    const cell = cells.get(value);
    if (cell === undefined || cell.kind === "list" && cell.count === 0) return noBranch;
    return cell.kind === "list" ? cell.count === 1 ? emptyList : cell.tail : cell.tail ?? emptyList;
  };
  kindTables[1][2] = lazyBranch(2, value => rest(value));
  // A view with the `vec` slot is a ChunkedSeq its public constructor built.
  kindHooks.restChunkedSeq = (current, slice, slots) => slice.offset + 1 < slice.storage.length
    ? ChunkedSeqValue(slots.vec, slots.node, slots.i, slice.offset + 1, null, null) : chunkedVectorRest(slots);
  function next(value) {
    if (value == null) return null;
    if (lazyValues.has(value)) { seq(value); return next(lazyValues.get(value).value); }
    const cell = cells.get(value);
    if (cell?.kind === "list") return cell.count === 1 || cell.count === 0 && !publicCollectionSlots.has(value) ? null : cell.tail;
    if (cell?.kind === "repeat") return cell.state?.following ?? (cell.remaining === -1 ? value : cell.remaining > 1 ? cell.tail : null);
    // Flat views advance directly; other kinds keep their rest/seq behavior.
    const slice = views.get(value);
    if (slice && slice.tail == null && (slice.kind === "indexed" || slice.kind === "hash-map" || slice.kind === "array-map" || slice.kind === "set"))
      return slice.offset + 1 < slice.storage.length ? view(slice.Type, slice.storage, slice.offset + 1, slice.indexed, slice.kind) : null;
    return seq(rest(value));
  }

  // As `first.branches`.
  next = (operation => Object.defineProperty(function (value) {
    const kind = value?.[kindOps], result = typeof kind === "number" ? kindTables[kind & 15][3](value) : noBranch;
    return result !== noBranch ? result : operation(value);
  }, "name", {value: operation.name}))(next);
  next = behindProtocol(next, value => protocolHooks.publicMethod?.(value, methodKeys[40], 1, false, true));
  // A list's next is its tail; another cell's the seq of its rest, which the
  // operation takes with `rest` and `seq`: where protocol implementations
  // exist, the seq of what a `-rest` that JavaScript gave the cell gives,
  // and the rest's seq is `seq`'s, which looks for `-seq`.
  kindTables[2][3] = value => {
    const cell = cells.get(value);
    if (cell === undefined || cell.kind === "repeat") return noBranch;
    if (cell.kind === "list") return cell.count === 0 ? noBranch : cell.count === 1 ? null : cell.tail;
    const method = protocolHooks.publicMethod;
    const own = method ? method(value, methodKeys[50], 1, false, true) : undefined;
    if (own) return seq(own.call(value, value));
    // The rest, most often a lazy seq, whose seq this is.
    const tail = cell.tail ?? emptyList, lazy = lazyValues.get(tail);
    return lazy === undefined || method ? seq(tail) : kindHooks.seqLazy(lazy);
  };
  kindTables[1][3] = lazyBranch(3, value => next(value));
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
  // `first` of a vector or a map entry, kinds 4 and 8, as `seqBranch`: the
  // seq the operation takes starts with the item at 0, which the branch
  // reads without the seq, unless an `ISeqable` implementation gives the
  // seq, or the value has a public constructor's slots or its store reads a
  // caller's array (`callerTail`, also of the store a subvector views),
  // which can be shorter than its count or hold getters.
  function vectorFirst(value) {
    const store = vectors.get(value);
    if (store === undefined) return noBranch;
    const own = protocolHooks.publicMethod?.(value, methodKeys[52], 1, true);
    return own ? firstSeq(own.call(value, value))
      : publicCollectionSlots.has(value) || (store.sub ?? store).callerTail ? firstSeq(kindHooks.seqVector(value, store))
      : store.cnt ? vectorNth(store, 0) : null;
  }
  // `rest` of a vector or a map entry, and with `empty` null its `next`, as
  // `vectorFirst`: the rest of its seq, built from index 1 (`kindHooks.seqVector`
  // chunks a vector of more than 32 items), which is its own seq. Where
  // protocol implementations exist or the value's seq is its kind's, `next`
  // is the seq of `rest`, as the operation takes it.
  function vectorRest(value, empty) {
    const store = vectors.get(value);
    if (store === undefined) return noBranch;
    const method = protocolHooks.publicMethod, shared = publicCollectionSlots.has(value) || (store.sub ?? store).callerTail;
    const own = empty === null ? undefined : method?.(value, methodKeys[52], 1, true);
    if (!shared && !own && (empty !== null || !method)) return store.cnt < 2 ? empty : store.cnt <= 32
      ? view(IndexedSeqValue, vectorChunk(value, 0), 1, true) : ChunkedSeqValue(value, vectorChunk(value, 0), 0, 1, null, null);
    return empty === null ? seq(rest(value)) : restSeq(own ? own.call(value, value) : kindHooks.seqVector(value, store));
  }
  kindTables[4][1] = vectorFirst;
  kindTables[4][2] = value => vectorRest(value, emptyList);
  kindTables[4][3] = value => vectorRest(value, null);
  kindTables[8][1] = vectorFirst;
  kindTables[8][2] = value => vectorRest(value, emptyList);
  kindTables[8][3] = value => vectorRest(value, null);
  kindTables[5][1] = seqBranch(maps, value => kindHooks.seqMap(value), firstSeq);
  kindTables[5][2] = seqBranch(maps, value => kindHooks.seqMap(value), restSeq);
  function isReduced(value) { return typeof value === "object" && value !== null && reducedValues.has(value); }
  // The value of a `reduced`, as `deref` gives it: only a protocol
  // implementation (`protocolHooks.deref`) can give another.
  function reducedValue(value) { return protocolHooks.deref ? protocolHooks.deref(value) : reducedValues.get(value); }
  function deref(value) { return derefProtocol(value); }
  function derefProtocol(value) {
    if (kindHooks.isLoadable?.(value)) return value.deref();
    const method = protocolHooks.builtin?.(value, methodKeys[12], 1);
    if (method) return method.call(value, value);
    if (delays.has(value)) return deferredValue(delays.get(value));
    if (vars.has(value)) return vars.get(value).get.call(null);
    if (atoms.has(value) || volatiles.has(value)) return value.state;
    if (!isReduced(value)) throw new Error("value does not support deref");
    return reducedValues.get(value);
  }
  function add(x, y) {
    if (arguments.length === 0) return 0;
    if (arguments.length === 1) return x;
    let result = x + y;
    for (let i = 2; i < arguments.length; i++) result = result + arguments[i];
    return result;
  }

  // Numeric core functions use JavaScript Number/coercion rules. In particular,
  // single-arity CLJS functions accept missing/extra JS arguments, and the
  // variadic dispatcher falls through to its two leading slots at arity zero.
  function numericFold(operation, x, y, more) {
    let value = operation(x, y);
    for (const item of more) value = operation(value, item);
    return value;
  }
  function multiply(x, y, ...more) {
    if (arguments.length === 0) return 1;
    return arguments.length === 1 ? x : numericFold((a, b) => a * b, x, y, more);
  }
  // Numeric core operations, one unit each.
  const inc = x => x + 1;
  const isSymbol = x => symbols.has(x);
  const isVector = x => protocolHooks.satisfies?.(methodKeys[61], x) ?? vectors.has(x);
  const isMapEntry = x => protocolHooks.implements?.(methodKeys[32], x) ?? mapEntryValue(x);
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
    const chunkedSlots = publicCollectionSlots.get(source);
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
    const store = vectors.get(chunkedSlots?.vec ?? source);
    if (store && (chunkedSlots?.vec || chunkedSlots?.cnt != null && arity === 2
      || (store.cnt > 32 || store.sub) && !protocolHooks.publicMethod?.(source, methodKeys[52], 1, true)))
      return vectorReduce(store, rf, init, chunkedSlots?.vec ? chunkedSlots.i + chunkedSlots.off : 0, arity);
    const producer = cells.get(source);
    if (producer?.integer && arity === 2) {
      if (producer.count === 0) return rf.call(null);
      init = producer.head;
      for (let i = 1; i < producer.count; i++) {
        init = rf.call(null, init, producer.head + i * producer.step);
        if (isReduced(init)) return reducedValue(init);
      }
      return init;
    }
    // With an initial value, as upstream's IntegerRange: from start to end.
    if (producer?.integer) {
      for (let i = producer.head; producer.step > 0 ? i < producer.end : i > producer.end; i += producer.step) {
        init = rf.call(null, init, i);
        if (isReduced(init)) return reducedValue(init);
      }
      return init;
    }
    if (producer?.kind === "repeat") {
      const n = producer.remaining, item = producer.head;
      let i = arity === 2 ? 1 : 0;
      if (arity === 2) init = item;
      while (n === -1 || i++ < n) { init = rf.call(null, init, item); if (isReduced(init)) return reducedValue(init); }
      return init;
    }
    if (producer?.kind === "iterate") return kindHooks.reduceIterate(rf, init, source, producer, arity);
    if (producer?.kind === "cycle") return kindHooks.reduceCycle(rf, init, producer, arity);
    if (chunks.has(source) && !chunks.get(source).integer) {
      const c = chunks.get(source); let i = c.offset;
      if (arity === 2) init = c.array[i++];
      for (; i < c.array.length; i++) { init = rf.call(null, init, c.array[i]); if (isReduced(init)) return reducedValue(init); }
      return init;
    }
    const sequenceReduction = cells.has(source) || lazyValues.has(source) || (!reduceValue(source) && !Array.isArray(source) && typeof source !== "string");
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
    const walked = value => publicCollectionSlots.get(value)?.vec ? undefined : views.get(value);
    let slice = walked(current), offset = slice?.offset;
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
        if (isReduced(result)) return reducedValue(result);
      }
      if (cell || !slice) {
        current = cell ? seq(cell.tail) : next(current);
        slice = walked(current);
        offset = slice?.offset;
      } else if (++offset >= slice.storage.length) {
        current = seq(slice.tail);
        slice = walked(current); offset = slice?.offset;
      }
    }
    return initialized ? result : rf.call(null);
  }

  reduce = (operation => Object.defineProperty(function (rf, init, collection) {
    const arity = arguments.length, source = arity === 3 ? collection : init;
    const method = arity === 2 || arity === 3 ? protocolHooks.publicMethod?.(source, methodKeys[44], arity, true) : null;
    if (method) return arity === 3 ? method.call(source, source, rf, init) : method.call(source, source, rf);
    return operation.apply(null, arguments);
  }, "name", {value: operation.name}))(reduce);
  function transform(factory) {
    return function (rf) {
      return factory(rf);
    };
  }
  function transduce(xf, rf, init, coll) {
    if (arguments.length === 3) { coll = init; init = rf.call(null); }
    else if (arguments.length !== 4) return invalidArity(arguments.length);
    const transformed = xf.call(null, rf);
    return transformed.call(null, reduce(transformed, init, coll));
  }
  const subvecSources = new WeakMap();
  const iteratorInit = {}, iteratorStart = {};
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
  class RangedIterator {
    constructor(i, base, arr, v, start, end) {
      Object.assign(this, {i, base, arr, v, start, end});
    }
    hasNext() { return this.i < this.end; }
    next() {
      if (this.i - this.base === 32) {
        this.arr = vectorLeaf(vectors.get(this.v), this.i).slice();
        this.base += 32;
      }
      const value = this.arr[this.i & 31]; this.i++; return value;
    }
  }
  function rangedIterator(value, start, end) {
    const base = start - start % 32;
    // Its public `arr` is a copy of a leaf, which the caller may change.
    return new RangedIterator(start, base, start < count(value) ? vectorLeaf(vectors.get(value), base).slice() : null, value, start, end);
  }
  function iterator(value) {
    const method = protocolHooks.builtin?.(value, methodKeys[26], 1);
    if (method) return method.call(value, value);
    const slots = publicCollectionSlots.get(value);
    if (slots?.arr) return kindHooks.iteratorArrayMap(slots);
    return vectors.has(subvecSources.get(value) ?? value) ? kindHooks.iteratorVector(value) : new SeqIter(iteratorInit, value);
  }
  // Maps with an `arr` slot, which only the public array map constructor builds.
  kindHooks.iteratorArrayMap = slots => {
    let i = 0;
    return {hasNext: () => i < slots.cnt * 2, next() {
      if (i >= slots.cnt * 2) throw new Error("No such element");
      const entry = mapEntry(slots.arr[i], slots.arr[i + 1]); i += 2; return entry;
    }, remove() { return new Error("Unsupported operation"); }};
  };
  kindHooks.iteratorVector = value => rangedIterator(value, 0, count(value));
  function conj(coll, item) {
    // One item onto a vector, where no protocol implementation or public
    // constructor can intervene: the hot path of building one.
    const store = arguments.length === 2 && !protocolHooks.builtin ? vectors.get(coll) : undefined;
    if (store && !subvecSources.has(coll)) {
      const built = kindHooks.conjVector(store, item);
      if (metadataValues.has(coll)) metadataValues.set(built, metadataValues.get(coll));
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
      const items = vectors.get(result);
      if (queues.has(result)) result = kindHooks.conjQueue(result, args[i]);
      else if (items) result = publicCollectionSlots.has(result) && subvecSources.has(result) ? kindHooks.conjSubvec(result, args[i]) : kindHooks.conjVector(items, args[i]);
      else if (maps.has(result)) result = kindHooks.conjMap(result, args[i]);
      else if (sets.has(result)) {
        const updated = kindHooks.conjSet(previous, result, args[i]);
        if (updated === missing) continue;
        result = updated;
      }
      else if (result == null) result = cons(args[i], emptyList, "list");
      else if (cells.has(result) || views.has(result) || lazyValues.has(result))
        result = cons(args[i], result, cells.get(result)?.kind === "list" ? "list" : "cons");
      else throw new Error("conj not supported on this type");
      if (vectors.has(previous) || maps.has(previous) || sets.has(previous) || queues.has(previous) || cells.get(previous)?.kind === "list") result = preserveMeta(previous, result);
    }
    return result;
  }
  kindHooks.conjVector = (store, item) => makeVector(null, vectorConj(store, item));
  kindHooks.conjMap = mapConj;
  // As `seq.branches`, for one item: a branch takes the collection and the
  // item, and makes the protocol lookup that `conj` makes first.
  conj = (operation => Object.defineProperty(function (coll, item) {
    const kind = coll?.[kindOps], result = typeof kind === "number" && arguments.length === 2 ? kindTables[kind & 15][7](coll, item) : noBranch;
    return result !== noBranch ? result : operation.apply(null, arguments);
  }, "name", {value: operation.name}))(conj);
  // What `conj` of a vector or a set gives `built` from `coll`: its metadata,
  // which its fast path copies and its other path reads with `meta`, which
  // protocol implementations can answer.
  function conjMeta(coll, built) {
    if (protocolHooks.builtin) return preserveMeta(coll, built);
    if (metadataValues.has(coll)) metadataValues.set(built, metadataValues.get(coll));
    return built;
  }
  // A subvector takes the operation (`SubvecValue.conj`). Each branch calls
  // what the protocol lookup finds, as the operation would, so that it looks
  // it up once.
  kindTables[4][7] = (coll, item) => {
    const store = vectors.get(coll);
    if (!store || subvecSources.has(coll)) return noBranch;
    const custom = protocolHooks.builtin?.(coll, methodKeys[7], 2);
    return custom ? custom.call(coll, coll, item) : conjMeta(coll, kindHooks.conjVector(store, item));
  };
  kindTables[5][7] = (coll, item) => {
    if (!maps.has(coll)) return noBranch;
    const custom = protocolHooks.builtin?.(coll, methodKeys[7], 2);
    return custom ? custom.call(coll, coll, item) : preserveMeta(coll, kindHooks.conjMap(coll, item));
  };
  function assoc(value, key, item, ...pairs) {
    const method = value == null ? null : protocolHooks.builtin?.(value, methodKeys[1], 3);
    if (method) {
      let result = method.call(value, value, key, item);
      return pairs.length ? assoc(result, ...pairs) : result;
    }
    if (vectors.has(value) && subvecSources.has(value) && publicCollectionSlots.has(value)) return kindHooks.assocSubvec(value, key, item, pairs);
    const mapSlots = publicCollectionSlots.get(value);
    if (maps.has(value) && mapSlots?.arr && mapSlots.cnt >= 8) {
      const promoted = kindHooks.assocArrayMapSlots(value, mapSlots, key, item, pairs);
      if (promoted !== missing) return promoted;
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
    return kindHooks.assocVector(value, original, key, item, pairs);
  }
  // As `seq.branches`, for one key and item: a branch takes the collection,
  // the key and the item, and makes the protocol lookup that `assoc` makes
  // first.
  assoc = (operation => Object.defineProperty(function (value, key, item) {
    const kind = value?.[kindOps], result = typeof kind === "number" && arguments.length === 3 ? kindTables[kind & 15][8](value, key, item) : noBranch;
    return result !== noBranch ? result : operation.apply(null, arguments);
  }, "name", {value: operation.name}))(assoc);
  // A public constructor's value, a subvector or an array map with slots,
  // takes the operation; a branch calls what the protocol lookup finds.
  kindTables[4][8] = (value, key, item) => {
    const store = vectors.get(value);
    if (!store || publicCollectionSlots.has(value)) return noBranch;
    const method = protocolHooks.builtin?.(value, methodKeys[1], 3);
    return method ? method.call(value, value, key, item) : kindHooks.assocVector(value, store, key, item, []);
  };
  kindTables[5][8] = (value, key, item) => {
    const entries = maps.get(value);
    if (!entries || publicCollectionSlots.has(value)) return noBranch;
    const method = protocolHooks.builtin?.(value, methodKeys[1], 3);
    if (method) return method.call(value, value, key, item);
    const updated = copyEntries(entries);
    putCollectionEntry(value, updated, key, item);
    return updatedMap(value, updated);
  };
  // A map with an `arr` slot is an array map its public constructor built, or
  // one updated from it; it becomes a hash map at its ninth key, or else
  // gives `missing`.
  kindHooks.assocArrayMapSlots = (value, mapSlots, key, item, pairs) => {
    if (contains(value, key)) return missing;
    const normalized = mapFromArray(Array.from({length: mapSlots.cnt * 2}, (_, i) => mapSlots.arr[i]), "hash-map");
    return collectionWithMeta(assoc(normalized, key, item, ...pairs), meta(value));
  };
  kindHooks.assocVector = (value, store, key, item, pairs) => {
    const assign = (index, value) => {
      if (typeof index !== "number") throw new Error("Vector key must be a number");
      if (index >= 0 && index < store.cnt) store = vectorAssoc(store, index, value);
      else if (index === store.cnt) store = vectorConj(store, value);
      else outOfBounds();
    };
    // The fixed arity follows JS argument binding, including missing item.
    assign(key, item);
    for (let i = 0; i < pairs.length; i += 2)
      assign(pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null);
    return preserveMeta(value, makeVector(null, store));
  };

  // Whether a map iterates its own entries (see `hostHooks`); a record, whose
  // entries are its fields, is another type to these methods.
  function ownMap(value) { return !protocolHooks.extend && maps.has(value) && !recordTypes.has(value) && !("tree" in (publicCollectionSlots.get(value) ?? {})); }
  // `count` of a map without protocol implementations.
  function mapCount(value) {
    const slots = publicCollectionSlots.get(value);
    if (slots && ("cnt" in slots || "count" in slots)) return "cnt" in slots ? slots.cnt : slots.count;
    return maps.get(value).length;
  }
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
  // Of a map's seq (`makeMap.seq`).
  function mapIterator(value, item) {
    const entries = mapTraversal(value);
    return itemIterator(entries.length ? entries : null, item);
  }
  const mapPrototype = Object.freeze({
    equiv(other) { return objectEquiv(this, other); },
    has(key) { return contains(this, key); },
    get(key, notFound = null) { return get(this, key, notFound); },
    keys() { return ownMap(this) ? mapIterator(this, entry => entry.key) : hostHooks.keys(this); },
    values() { return ownMap(this) ? mapIterator(this, entry => entry.value) : hostHooks.values(this); },
    entries() { return ownMap(this) ? mapIterator(this, entry => [entry.key, entry.value]) : hostHooks.entries(this); },
    [Symbol.iterator]() { return ownMap(this) ? mapIterator(this, entry => mapEntry(entry.key, entry.value)) : hostHooks.iterator(this); },
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

  builtinType(PersistentArrayMapValue, mapPrototype).prototype[kindOps] = 5;
  builtinType(PersistentHashMapValue, mapPrototype).prototype[kindOps] = 5;
  mapTypes["array-map"] = PersistentArrayMapValue;
  mapTypes["hash-map"] = PersistentHashMapValue;
  // A hash map keeps its trie (`hashTrie`) in a private field rather than in
  // the `maps` WeakMap: as unforgeable a brand, but each new version of a map
  // costs no WeakMap entry, which dominated `assoc`. The stores answer for
  // both, beneath the wrappers another type's effect adds (`maps.records`),
  // and a value with a trie has the hash-map kind. A hash set's trie has a
  // field of its own, of another class (light-cljs-236).
  function trieBrand(store, kinds) {
    // Constructing a Trie adds its private field to `value`, which the base
    // constructor returns. A payload is set once, when its value is built.
    const Trie = class extends (class { constructor(value) { return value; } }) {
      #trie;
      static of(value) { return Object(value) === value && #trie in value ? value.#trie : null; }
      static put(value, trie) { new Trie(value).#trie = trie; }
    }, trieOf = Trie.of;
    store.get = (get => value => trieOf(value) ?? get.call(store, value))(store.get);
    store.has = (has => value => !!trieOf(value) || has.call(store, value))(store.has);
    store.set = (set => (value, payload) => (Array.isArray(payload) ? set.call(store, value, payload) : Trie.put(value, payload), store))(store.set);
    kinds.get = (get => value => trieOf(value) ? "hash-map" : get.call(kinds, value))(kinds.get);
    kinds.set = (set => (value, kind) => (trieOf(value) || set.call(kinds, value, kind), kinds))(kinds.set);
  }
  trieBrand(maps, mapKinds);
  lookup = (operation => (value, key, notFound) => transients.has(value) ? kindHooks.lookupTransient(value, key, notFound) : operation(value, key, notFound))(lookup);
  // The item under `key` of a hash map's trie, which `get.maps` (light-cljs-358)
  // and `lookup` read, kept only where a program builds hash maps
  // (light-cljs-235; a hook rather than a wrapper of `lookup` since
  // light-cljs-302, so that a chunk places it with the hash maps).
  kindHooks.trieValue = (trie, key, notFound) => { const entry = trieEntry(trie, key); return entry ? entry.value : notFound; };
  // A trie is immutable; its copy is the fields an update replaces.
  kindHooks.copyTrie = entries => ({root: entries.root, nil: entries.nil, length: entries.length});
  // A hash or array map of `entries`.
  function mapFromEntries(entries, hash) {
    return hash ? makeMap(PersistentHashMapValue, hashTrie(entries), "hash-map") : makeMap(PersistentArrayMapValue, entries, "array-map");
  }
  // Routing nodes preserve observable key tests and rehashes at trie splits.
  // Immutable nodes can be shared by old collections and transient copies.
  // Transient capacity affects expansion timing.
  const hashRoots = new WeakMap();
  const hashTraversals = new WeakMap();
  function copyEntries(entries) {
    // A trie, which is no array, has no `slice` (`copyEntries.trie`).
    const copy = entries.slice?.() ?? kindHooks.copyTrie(entries);
    if (hashRoots.has(entries)) hashRoots.set(copy, hashRoots.get(entries));
    return copy;
  }
  // A bitmap node: its slots, the edit that made it, the pairs its upstream
  // array holds (`capacity`, which times a transient's expansion) and its
  // occupied slots (`count`). An array node has the same fields, capacity 0,
  // so that every node has one shape.
  function bitmapNode(slots, edit, capacity, count) {
    return {kind: "bitmap", slots, edit, capacity, count};
  }
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
          entries[index] = Object.freeze({...entry, key: result.old.key, hash: result.old.hash});
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
      result.entry = Object.freeze({...entry, key: slot.key, hash: slot.hash});
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
  function hashNodeFind(node, shift, keyHash, key) {
    for (; node; shift += 5) {
      if (node.kind === "collision") return node.entries.find(entry => equalPair(key, entry.key)) ?? null;
      const slot = node.slots[(keyHash >>> shift) & 31];
      if (!slot?.kind) return slot && equalPair(key, slot.key) ? slot : null;
      node = slot;
    }
    return null;
  }
  function ensureHashRoot(entries) {
    if (!hashRoots.has(entries)) {
      let root = null;
      for (const entry of entries) if (entry.key != null) root = hashNodeAssoc(root, 0, entry.hash, entry, null, {}, false);
      hashRoots.set(entries, root);
    }
    return hashRoots.get(entries);
  }
  function mapTraversal(value) {
    return collectionTraversal(maps.get(value), mapKinds.get(value));
  }
  function setTraversal(value) {
    return collectionTraversal(sets.get(value), setKinds.get(value));
  }
  function collectionTraversal(entries, kind) {
    // Array maps, positional constants, and sorted collections retain their
    // storage order. Hash-map promotion stays observable after removals.
    return kind !== "hash-map" ? entries : kindHooks.hashTraversal(entries);
  }
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
  function hashEntryIndex(entries, key, keyHash) {
    if (key == null) return entries.findIndex(entry => entry.key == null);
    const root = ensureHashRoot(entries);
    if (!root) return -1;
    return entries.indexOf(hashNodeFind(root, 0, keyHash ?? hash(key), key));
  }
  function promoteEntries(entries, edit) {
    const promoted = [];
    for (const entry of entries) putEntry(promoted, entry.key, entry.value, "hash-map", edit);
    return promoted;
  }
  // A hash map's payload (light-cljs-235): the trie of its non-nil entries,
  // its nil entry or null, and its count as `length`, so that `assoc` and
  // `dissoc` copy only a path. Built from a builder's `entries`, it also keeps
  // them at their indexes, where advanced constant lookups read an entry
  // (light-cljs-221).
  function hashTrie(entries) {
    return !Array.isArray(entries) ? entries : Object.assign({root: ensureHashRoot(entries), nil: entries.find(entry => entry.key == null) ?? null, length: entries.length}, entries);
  }
  // The entry of `key` in a hash map's trie, or null. As in `hashEntryIndex`,
  // a nil key or an empty trie hashes no query.
  function trieEntry(trie, key) {
    return key == null ? trie.nil : trie.root ? hashNodeFind(trie.root, 0, hash(key), key) : null;
  }
  // Only hash maps have a trie (see `mapTraversal`), transient maps, which
  // `assoc!` can promote to one without a persistent hash map, and transient
  // hash sets (light-cljs-236).
  kindHooks.trieEntry = trieEntry;
  // A map of the type the caller builds (so a caller keeps only the map types
  // it can build); `mapTypes` gives the type of an existing map's kind.
  function makeMap(Type, entries, kind, comparator) {
    const value = Object.create(Type.prototype);
    maps.set(value, Object.freeze(entries));
    mapKinds.set(value, kind);
    if (kind === "sorted-map") sortedComparators.set(value, comparator);
    return value;
  }

  // The index of `key` in the entries of an array map, or -1.
  function entryIndex(entries, key) {
    // Canonical array maps compare keys without a hash guard. Keyword equality uses
    // fqn, while hashing uses namespace/name; unusual equal keywords can have
    // different hashes and therefore remain distinct in hash maps.
    return entries.findIndex(entry =>
      key == null ? entry.key == null : typeof key === "string" || typeof key === "number"
        ? key === entry.key : equalPair(key, entry.key));
  }

  // Builders mutate only fresh unpublished arrays. Entry cells and retained
  // equivalent keys are shared by subsequent immutable versions.
  function putEntry(entries, key, value, kind, edit = null) {
    if (kind === "array-map") {
      const index = entryIndex(entries, key), retained = index < 0 ? key : entries[index].key;
      let cachedHash;
      const entry = Object.freeze({key: retained, value, get hash() {
        return cachedHash === undefined ? cachedHash = entryHashing.hash(retained) : cachedHash;
      }});
      if (index < 0) entries.push(entry); else entries[index] = entry;
      return;
    }
    kindHooks.putHashEntry(entries, key, value, edit);
  }
  // Only hash maps, hash sets and transients have hash-map entries.
  kindHooks.putHashEntry = (entries, key, value, edit) => {
    const keyHash = key == null ? 0 : hash(key), entry = Object.freeze({key, value, hash: keyHash});
    if (!Array.isArray(entries)) {
      // A hash map's trie, of which an update owns this copy (`copyEntries.trie`).
      const result = {}, nil = entries.nil;
      if (key == null) entries.nil = nil ? Object.freeze({...entry, key: nil.key}) : entry;
      else entries.root = hashNodeAssoc(entries.root, 0, keyHash, entry, edit, result);
      entries.length += key == null ? !nil : !result.old;
      return;
    }
    if (key == null) {
      const index = entries.findIndex(old => old.key == null);
      if (index < 0) entries.push(entry); else entries[index] = Object.freeze({...entry, key: entries[index].key});
      return;
    }
    const result = {}, root = hashNodeAssoc(ensureHashRoot(entries), 0, keyHash, entry, edit, result);
    if (result.old) entries[entries.indexOf(result.old)] = result.entry;
    else entries.push(entry);
    hashRoots.set(entries, root);
  };

  function mapFromArray(pairs, kind = pairs.length <= 16 ? "array-map" : "hash-map") {
    if (pairs.length % 2) throw new Error("No value supplied for key: " + displayStr(pairs[pairs.length - 1]));
    const entries = [];
    const edit = kind === "hash-map" ? {} : null;
    for (let i = 0; i < pairs.length; i += 2) putEntry(entries, pairs[i], pairs[i + 1], kind, edit);
    return kind === "hash-map" ? makeMap(PersistentHashMapValue, hashTrie(entries), kind) : makeMap(PersistentArrayMapValue, entries, kind);
  }
  function mapConj(value, member) {
    if (member == null) return value;
    // A vector's `-nth` is `nth`'s own (see `protocolHooks`).
    const vectorLike = item => protocolHooks.satisfies?.(methodKeys[61], item) ?? vectors.has(item), nthOf = protocolHooks.nthProtocol ?? nth;
    if (vectorLike(member)) return assoc(value, nthOf(member, 0), nthOf(member, 1));
    let result = value;
    for (let current = seq(member); current !== null; current = next(current)) {
      const pair = first(current);
      if (!vectorLike(pair)) throw new Error("Map conj requires vector entries");
      result = assoc(result, nthOf(pair, 0), nthOf(pair, 1));
    }
    return result;
  }

  // Transient payloads own a private mutable copy of canonical collection
  // storage. Persistence freezes that storage; no persistent alias can mutate it.
  const transientPrototype = Object.freeze({
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    toString() { return printHooks.collection(this); },
    call(_self, key, notFound) {
      if (arguments.length !== 2 && arguments.length !== 3) return invalidArity(arguments.length - 1);
      return transientLookup(this, key, arguments.length === 2 ? null : notFound);
    }
  });
  const TransientVectorType = builtinType(function () {}, transientPrototype);
  const TransientArrayMapType = builtinType(function () {}, transientPrototype);
  const TransientHashMapType = builtinType(function () {}, transientPrototype);
  const TransientHashSetType = builtinType(function () {}, transientPrototype);
  function makeTransient(kind, entries, mapKind) {
    const Type = kind === "vector" ? TransientVectorType : kind === "set" ? TransientHashSetType : mapKind === "array-map" ? TransientArrayMapType : TransientHashMapType;
    const result = Object.create(Type.prototype);
    transients.set(result, {kind, entries, mapKind, editable: true, edit: {}});
    return result;
  }
  function editableTransient(value, operation) {
    const state = transients.get(value);
    if (!state?.editable) throw new Error(operation === "persistent!" ? "persistent! called twice" : operation + " after persistent!");
    return state;
  }
  function transient(value) { return asTransientProtocol(value); }
  function asTransient(value) {
    if (vectors.has(value)) return makeTransient("vector", kindHooks.transientVector(vectors.get(value)));
    if (maps.has(value)) return makeTransient("map", copyEntries(maps.get(value)), mapKinds.get(value));
    return makeTransient("set", copyEntries(sets.get(value)), setKinds.get(value));
  }
  function persistentBang(value) { return persistentBangProtocol(value); }
  function persistTransient(value) {
    const state = editableTransient(value, "persistent!");
    state.editable = false;
    if (state.kind === "vector") return kindHooks.persistVector(state.entries);
    if (state.kind === "map") return kindHooks.persistMap(state.entries, state.mapKind);
    return kindHooks.persistSet(state.entries, state.mapKind);
  }
  // A transient vector, map or set comes from a persistent one of its kind.
  // The vector's tail is trimmed to its count; the nodes it owned no longer
  // change, as nothing can edit them, and a leaf freezes where a seq first
  // shares it (`vectorChunk`).
  kindHooks.persistVector = ({cnt, shift, root, tail}) => makeVector(null, {cnt, shift, root, tail: tail.slice(0, cnt - vectorTailOffset(cnt))});
  // A transient vector edits a copy of the tail, and copies of the trie's
  // nodes along the paths it changes, which it owns (`vectorEditor`): a weak
  // set, which keeps no node a `pop!` discarded.
  kindHooks.transientVector = ({cnt, shift, root, tail}) => {
    tail = tail.slice();
    return {cnt, shift, root, tail, owned: new WeakSet([tail])};
  };
  kindHooks.persistMap = (entries, kind) => kind === "hash-map" ? makeMap(PersistentHashMapValue, hashTrie(entries), kind) : makeMap(PersistentArrayMapValue, entries, kind);
  function transientNth(value, index, fallback) {
    const state = transients.get(value);
    // Pinned CLJS's three-argument -nth returns an out-of-range fallback
    // even after persistence; in-range access still checks ownership.
    if (fallback !== missing && !(index >= 0 && index < state.entries.cnt)) return fallback;
    editableTransient(value, "nth");
    return vectorNth(state.entries, index, fallback);
  }
  function transientLookup(value, key, notFound) {
    const state = transients.get(value);
    // TransientHashMap (and its set wrapper) retains readable lookup after
    // persistence in both pinned versions. Array maps and vectors reject it.
    if (state.mapKind !== "hash-map") editableTransient(value, "lookup");
    if (state.kind === "vector") return typeof key === "number" ? transientNth(value, key, notFound) : notFound;
    // A transient hash map or set edits a trie (see `asTransient`).
    if (!Array.isArray(state.entries)) {
      const entry = kindHooks.trieEntry(state.entries, key);
      return !entry ? notFound : state.kind === "set" ? key : entry.value;
    }
    const index = kindHooks.transientEntryIndex(state, key);
    return index < 0 ? notFound : state.kind === "set" ? key : state.entries[index].value;
  }
  kindHooks.transientEntryIndex = (state, key) => state.mapKind === "array-map" ? entryIndex(state.entries, key) : hashEntryIndex(state.entries, key);
  function assocTransient(value, key, item, indexed = false) {
    const state = transients.get(value);
    if (state.kind === "vector") {
      if (!indexed && typeof key !== "number") throw new Error("TransientVector's key for assoc! must be a number.");
      editableTransient(value, "assoc!");
      kindHooks.assocTransientVector(state.entries, key, item);
    } else {
      editableTransient(value, "assoc!");
      if (state.mapKind === "array-map" && state.entries.length >= 8 && entryIndex(state.entries, key) < 0) {
        const promoted = makeTransient("map", {root: null, nil: null, length: 0}, "hash-map"), target = transients.get(promoted);
        for (const entry of state.entries) putEntry(target.entries, entry.key, entry.value, "hash-map", target.edit);
        return assocTransient(promoted, key, item);
      }
      putEntry(state.entries, key, item, state.mapKind, state.edit);
    }
    return value;
  }
  function conjTransient(value, member) {
    const state = editableTransient(value, "conj!");
    if (state.kind === "vector") { kindHooks.conjTransientVector(state.entries, member); return value; }
    return state.kind === "set" ? kindHooks.conjTransientSet(value, state, member) : kindHooks.conjTransientMap(value, member);
  }
  // `conj!` onto the transient a core function's reduction holds, past the
  // protocol dispatch where no protocol implementation can answer for it.
  const conjBangOwn = (value, member) => !protocolHooks.builtin && transients.has(value) ? conjTransient(value, member) : conjBangProtocol(value, member);
  kindHooks.conjTransientVector = conjVectorBang;
  // `assoc!` of an index of a transient vector, or of its count.
  kindHooks.assocTransientVector = (store, key, item) => {
    if (key >= 0 && key < store.cnt) {
      if (key >= vectorTailOffset(store.cnt)) store.tail[key & 31] = item;
      else store.root = vectorAssocNode(store.shift, store.root, key, item, vectorEditor(store.owned));
    } else if (key === store.cnt) conjVectorBang(store, item);
    else outOfBounds();
  };
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
  // Generic host-facing methods, for receivers that cannot answer natively.
  const hostHooks = {};
  // The items of a vector in an array, or null where it has none: the tail of
  // one with at most 32 items, which `seq` walks (`makeVector.seq`). Where
  // the store reads a caller's array (`callerTail`), whose items can change
  // or be getters, each item past one tail, or of a view, reads the store
  // when it is read, so the iterator, the printer and equality read no more
  // than they use.
  function vectorStorage(value) {
    const store = vectors.get(value);
    if (!store.cnt) return null;
    if (!(store.sub ?? store).callerTail) return store.cnt <= 32 ? store.tail : vectorItems(value);
    if (store.cnt <= 32 && !store.sub) return store.tail;
    const items = [];
    for (let i = 0; i < store.cnt; i++) Object.defineProperty(items, i, {enumerable: true, get: () => vectorNth(store, i, null)});
    return items;
  }
  function vectorCount(value) {
    const slots = publicCollectionSlots.get(value);
    if (slots && "cnt" in slots) return slots.cnt;
    return slots && subvecSources.has(value) ? slots.end - slots.start : vectors.get(value).cnt;
  }
  function vectorIndexOf(coll, value, start, reverse) {
    if (protocolHooks.extend || !vectors.has(coll)) return hostHooks.index(coll, value, start, reverse);
    const store = vectors.get(coll), entry = mapEntries.has(coll);
    return indexIn(vectorCount(coll), i => entry && i !== 0 && i !== 1 ? outOfBounds() : vectorNth(store, i, missing), value, start, reverse);
  }
  const vectorPrototype = Object.freeze({
    equiv(other) { return objectEquiv(this, other); },
    [Symbol.iterator]() { return protocolHooks.extend || !vectors.has(this) ? hostHooks.iterator(this) : itemIterator(vectorStorage(this), item => item); },
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    toString() { return printHooks.collection(this); },
    indexOf(value, start = 0) { return vectorIndexOf(this, value, start, false); },
    lastIndexOf(value, start) {
      if (start === undefined) start = protocolHooks.extend || !vectors.has(this) ? hostHooks.count(this) : vectorCount(this);
      return vectorIndexOf(this, value, start, true);
    },
    // The pinned fixed method accepts one index and ignores extra arguments.
    call(_self, index) {
      const store = vectors.get(this);
      if (!store) throw new Error("Expected vector");
      if (mapEntries.has(this)) {
        if (arguments.length !== 2 && arguments.length !== 3) return invalidArity(arguments.length - 1);
        if (index === 0 || index === 1) return store.tail[index];
        return arguments.length === 3 ? arguments[2] : outOfBounds();
      }
      if (typeof index !== "number") throw new Error("Key must be integer");
      return vectorNth(store, index, missing);
    }
  });

  builtinType(PersistentVectorValue, vectorPrototype).prototype[kindOps] = 4;
  // The type of map entries, whose prototype has the vectors' members where
  // vectors can be built (`MapEntryType.vector`), and else the members of
  // map entries alone (`MapEntryType.lean`). The shared runtime defines it
  // twice, the second time with the vectors'.
  let MapEntryType;
  MapEntryType = builtinType(function (key, value) { return mapEntry(key, value); }, vectorPrototype);
  // Map entries are kind 8 (light-cljs-373), with branches of their own, so
  // that a program building entries and no vector keeps no vector branch.
  MapEntryType.prototype[kindOps] = 8;
  function makeKeyword(namespace, name, spelling) {
    const value = Object.create(KeywordValue.prototype);
    keywords.set(value, Object.freeze({namespace, name, spelling}));
    return value;
  }

  // Keywords built from data are fresh, as upstream.
  function freshKeyword(namespace, name) {
    return makeKeyword(namespace, name,
      namespace == null ? name : namespace + "/" + name);
  }

  // Literal construction receives separately owned namespace/name strings
  // and is interned: one keyword per namespace and name.
  function keyword(namespace, name) {
    const key = namespace == null ? "/" + name : namespace.length + "/" + namespace + name;
    let value = internedKeywords.get(key);
    if (!value) internedKeywords.set(key, value = freshKeyword(namespace, name));
    return value;
  }

  // A vector of the new array `items`, or with the store an operation built.
  function makeVector(items, store = vectorTrie(items)) {
    const value = Object.create(PersistentVectorValue.prototype);
    vectors.set(value, store);
    return value;
  }

  // A vector's store (`vectors`) is upstream's PersistentVector: `cnt` items,
  // the last 1 to 32 of them in the array `tail` and the others in the full
  // 32-item leaves of a 32-way trie, whose `root` is `shift` / 5 levels above
  // them. Nodes are arrays with no trailing holes. No operation changes a
  // persistent vector's nodes; only a transient changes those it owns
  // (`vectorEditor`). Seqs share leaves, which are frozen, and the tail,
  // which freezes when one first does (`vectorChunk`); the root and the
  // other nodes stay private.
  // The index of the first item in the tail:
  const vectorTailOffset = cnt => cnt < 32 ? 0 : (cnt - 1) & -32;
  // The store of the new array `items`, which it keeps as its tail where it
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
    return {cnt, shift, root: nodes, tail: end ? slice(end, cnt) : items};
  }
  // The leaf, or the tail, that holds index `i` of a store; of a view
  // (`VectorView`), a new array of its items from index `i & -32`.
  function vectorLeaf(store, i) {
    if (store.sub) return store.leaf(i);
    if (i >= vectorTailOffset(store.cnt)) return store.tail;
    let node = store.root;
    for (let level = store.shift; level > 0; level -= 5) node = node[(i >>> level) & 31];
    return node;
  }
  // The leaf or tail holding index `i` of a vector, for a seq to share,
  // which freezes then (a leaf a transient built, the tail and a view's new
  // array), unless it is a tail the public constructor received (and
  // `assoc` kept): that one stays its caller's, as upstream.
  function vectorChunk(value, i) {
    const store = vectors.get(value), leaf = vectorLeaf(store, i);
    return !store.sub && store.callerTail && leaf === store.tail ? leaf : Object.freeze(leaf);
  }
  // A new array of a store's items from index `from` to `to`.
  function vectorSlice(store, from, to) {
    const items = [];
    for (let i = from; i < to; i = (i | 31) + 1) items.push(...vectorLeaf(store, i).slice(i & 31, (i & 31) + to - i));
    return items;
  }
  // A new array of a vector's items.
  const vectorItems = value => vectorSlice(vectors.get(value), 0, vectors.get(value).cnt);
  // `reduce` of a store's items from index `i`: from `init` where `arity` is
  // 3, else from the first of them. A view's walks the store it views.
  function vectorReduce(store, rf, init, i, arity) {
    let end = store.cnt;
    if (store.sub) { i += store.start; end += store.start; store = store.sub; }
    if (arity === 2) { if (i >= end) return rf.call(null); init = vectorNth(store, i++, null); }
    for (let leaf = vectorLeaf(store, i); i < end;) {
      init = rf.call(null, init, leaf[i & 31]);
      if (isReduced(init)) return reducedValue(init);
      if (!(++i & 31)) leaf = vectorLeaf(store, i);
    }
    return init;
  }
  // What a transient vector, which owns the nodes in `owned`, edits for
  // `node`: the node itself where it owns it, else a copy it then owns.
  const vectorEditor = owned => node => owned.has(node) ? node : (owned.add(node = node.slice()), node);
  // `node` below `level` / 5 new nodes of one child each.
  const vectorPath = (level, node) => level ? [vectorPath(level - 5, node)] : node;
  // `parent`, `level` above the leaves of a store of `cnt` items, with the
  // full tail `leaf` as its next leaf. A persistent vector copies the nodes on
  // the path; a transient's `edit` gives the ones it changes.
  function vectorPushTail(cnt, level, parent, leaf, edit) {
    const node = edit ? edit(parent) : parent.slice(), i = ((cnt - 1) >>> level) & 31;
    node[i] = level === 5 ? leaf : node[i] ? vectorPushTail(cnt, level - 5, node[i], leaf, edit) : vectorPath(level - 5, leaf);
    return node;
  }
  // `store` with `item` added: to a copy of the tail, or else as the tail of
  // one item after a frozen copy of the full tail moves into the trie, which
  // grows a level where its root is full.
  function vectorConj({cnt, shift, root, tail}, item) {
    if (cnt - vectorTailOffset(cnt) < 32) {
      // A copying loop: spreading the tail costs more than the rest of `conj`.
      const length = tail.length, copy = new Array(length + 1);
      for (let i = 0; i < length; i++) copy[i] = tail[i];
      copy[length] = item;
      return {cnt: cnt + 1, shift, root, tail: copy};
    }
    const grow = (cnt >>> 5) > (1 << shift), leaf = Object.freeze(tail.slice());
    return {cnt: cnt + 1, shift: grow ? shift + 5 : shift, tail: [item],
      root: grow ? [root, vectorPath(shift, leaf)] : vectorPushTail(cnt, shift, root, leaf)};
  }
  // `conj!` onto a transient vector's store: `vectorConj` in place.
  function conjVectorBang(store, item) {
    const {cnt, shift, root, tail, owned} = store;
    if (cnt - vectorTailOffset(cnt) < 32) tail[cnt & 31] = item;
    else {
      const grow = (cnt >>> 5) > (1 << shift);
      store.root = grow ? [root, vectorPath(shift, tail)] : vectorPushTail(cnt, shift, root, tail, vectorEditor(owned));
      if (grow) { owned.add(store.root); store.shift += 5; }
      owned.add(store.tail = [item]);
    }
    store.cnt++;
    return store;
  }
  // `node`, `level` above the leaves, with `item` at index `i`: copied, a
  // leaf frozen, or given by a transient's `edit` (see `vectorPushTail`).
  function vectorAssocNode(level, node, i, item, edit) {
    const copy = edit ? edit(node) : node.slice(), at = (i >>> level) & 31;
    copy[at] = level ? vectorAssocNode(level - 5, node[at], i, item, edit) : item;
    return edit || level ? copy : Object.freeze(copy);
  }
  // `store` with `item` at its index `i`.
  function vectorAssoc(store, i, item) {
    if (store.sub) return store.assoc(i, item);
    if (i < vectorTailOffset(store.cnt)) return {...store, root: vectorAssocNode(store.shift, store.root, i, item)};
    const tail = store.tail.slice();
    tail[i & 31] = item;
    return {cnt: store.cnt, shift: store.shift, root: store.root, tail};
  }
  function vector(...items) {
    return makeVector(items);
  }

  // Literal emission uses an array to avoid the host's argument-count limit.
  // Copy even when a test/host caller supplies its own array.
  function vectorFromArray(items) {
    return makeVector(Array.from(items));
  }

  function name(value) {
    if (multiFns.has(value)) return name(value.name);
    const method = protocolHooks.implements?.(methodKeys[36], value) ? protocolHooks.builtin?.(value, methodKeys[37], 1) : null;
    if (method) return method.call(value, value);
    const named = keywords.get(value) || symbols.get(value);
    if (named) return named.name;
    if (typeof value === "string") return value;
    throw new Error("Doesn't support name on this type");
  }

  function count(value) {
    const slots = publicCollectionSlots.get(value), cell = cells.get(value);
    if (cell?.kind === "tree-seq" && slots.cnt < 0) return kindHooks.countTreeSeq(value);
    if (slots && "tree-map" in slots) return count(slots["tree-map"]);
    if (slots && ("cnt" in slots || "count" in slots)) return "cnt" in slots ? slots.cnt : slots.count;
    if (slots?.vec) return count(slots.vec) - slots.i - slots.off;
    if (slots && subvecSources.has(value)) return slots.end - slots.start;
    if (slots && sets.has(value)) return count(slots["hash-map"]);
    if (transients.has(value)) return kindHooks.countTransient(value);
    if (chunkBuffers.has(value)) return chunkBuffers.get(value).end;
    if (chunks.has(value)) { const c = chunks.get(value); return c.end - c.offset; }
    if (queues.has(value)) return queues.get(value).length;
    if (Array.isArray(value)) return value.length;
    if (value == null) return 0;
    const store = vectors.get(value);
    if (store) return store.cnt;
    if (typeof value === "string") return value.length;
    const entries = maps.get(value);
    if (entries) return entries.length;
    const members = sets.get(value);
    if (members) return members.length;
    if (cell && "count" in cell) return cell.count;
    const slice = views.get(value);
    if (slice) return Math.max(0, slice.storage.length - slice.offset) + count(slice.tail);
    // Lazy seqs, cells without a count and other seqables walk their seqs.
    if (protocolHooks.implements?.(methodKeys[51], value) ?? seqableValue(value)) return kindHooks.countSeq(value);
    const nativeCount = protocolHooks.builtin?.(value, methodKeys[10], 1);
    if (nativeCount) return nativeCount.call(value, value);
    throw new Error("count not supported on this type");
  }

  // As `seq.branches`.
  count = (operation => Object.defineProperty(function (value) {
    const kind = value?.[kindOps], result = typeof kind === "number" ? kindTables[kind & 15][4](value) : noBranch;
    return result !== noBranch ? result : operation(value);
  }, "name", {value: operation.name}))(count);
  count = behindProtocol(count, value => protocolHooks.publicMethod?.(value, methodKeys[10], 1, true, true));
  hostHooks.count = count;
  function accumulatingCount(value) {
    let result = 0;
    for (let current = seq(value); current != null; current = next(current), result++) {
      if (protocolHooks.satisfies?.(methodKeys[11], current) ?? countedValue(current)) return result + (protocolHooks.countProtocol ?? count)(current);
    }
    return result;
  }
  // Only these units, and a program type implementing `ISeqable`
  // (`PROTOCOL_ROOTS` in src/runtime_slice.rs), build values `count` walks.
  kindHooks.countSeq = accumulatingCount;
  // A list and an integer range hold their count (and a public constructor
  // gave it the same count it gave the range); other cells walk their seqs.
  kindTables[2][4] = value => {
    const cell = cells.get(value), kind = cell?.kind;
    if (kind === "list" || kind === "range" && cell.integer) return cell.count;
    return (kind === "cons" || kind === "chunked-cons" || kind === "range") && (protocolHooks.implements?.(methodKeys[51], value) ?? true) ? kindHooks.countSeq(value) : noBranch;
  };
  kindTables[1][4] = value => lazyValues.has(value) && (protocolHooks.implements?.(methodKeys[51], value) ?? true) ? kindHooks.countSeq(value) : noBranch;
  // The counts the stores of the collections, kinds 4 to 7, hold
  // (light-cljs-358). A public constructor's value, whose slots can give
  // another count (`cnt`), a subvector's bounds or a sorted set's map, takes
  // the operation.
  kindTables[4][4] = value => { const store = vectors.get(value); return store && !publicCollectionSlots.has(value) ? store.cnt : noBranch; };
  kindTables[5][4] = value => { const entries = maps.get(value); return entries && !publicCollectionSlots.has(value) ? entries.length : noBranch; };
  kindTables[8][4] = value => { const store = vectors.get(value); return store && !publicCollectionSlots.has(value) ? store.cnt : noBranch; };
  // Views, the seqs of arrays, strings and vectors, are kind 3 where a kind
  // with types of its own (lazy seqs, cells or the collections) wraps
  // `first`: a program that builds only views keeps the operations' own
  // paths, and no wrapper.
  IndexedSeqValue.prototype[kindOps] = 3;
  ChunkedSeqValue.prototype[kindOps] = 3;
  kindTables[3][1] = value => {
    const slice = views.get(value);
    return slice === undefined ? noBranch : slice.offset < slice.storage.length ? slice.storage[slice.offset] : null;
  };


  // The item of a vector's store at a number `index`, truncated; a view's is
  // that of the store it views.
  function vectorNth(store, index, notFound) {
    if (index >= 0 && index < store.cnt) return store.sub ? vectorNth(store.sub, store.start + index, notFound) : vectorLeaf(store, index)[index & 31];
    if (notFound === missing) return outOfBounds();
    return notFound;
  }

  function nth(value, index, notFound) {
    const arity = arguments.length;
    if (arity !== 2 && arity !== 3) return invalidArity(arity);
    if (typeof index !== "number") throw new Error("Index argument to nth must be a number");
    if (value == null) return arity === 2 ? value : notFound;
    const fallback = arity === 2 ? missing : notFound;
    // A vector first, the most common: only a map entry, of two items, has
    // no index but 0 and 1.
    const store = vectors.get(value);
    if (store) return store.cnt === 2 && index !== 0 && index !== 1 && mapEntries.has(value) ? fallback === missing ? outOfBounds() : fallback : vectorNth(store, index, fallback);
    if (transients.has(value)) return kindHooks.nthTransient(value, index, fallback);
    if (chunks.has(value)) return kindHooks.nthChunk(value, index, fallback);
    if (cells.get(value)?.integer) { const c = cells.get(value); return index >= 0 && index < c.count ? c.head + Math.floor(index) * c.step : fallback === missing ? outOfBounds() : fallback; }
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

  // As `seq.branches`, for a number `index` and two or three arguments; a
  // branch takes the value, the index and the item for an index out of
  // bounds, or `missing` to throw.
  nth = (operation => Object.defineProperty(function (value, index, notFound) {
    const kind = value?.[kindOps], arity = arguments.length;
    const result = typeof kind === "number" && typeof index === "number" && (arity === 2 || arity === 3)
      ? kindTables[kind & 15][5](value, index, arity === 2 ? missing : notFound) : noBranch;
    return result !== noBranch ? result : operation.apply(null, arguments);
  }, "name", {value: operation.name}))(nth);
  // A map entry, of two items, has no index but 0 and 1 (see `nth`).
  kindTables[4][5] = (value, index, notFound) => {
    const store = vectors.get(value);
    return store && !(store.cnt === 2 && mapEntries.has(value)) ? vectorNth(store, index, notFound) : noBranch;
  };
  // A map entry's items (light-cljs-373), which its store's tail holds, read
  // first, as `nth` tests for an entry only at another index; a vector with
  // the entries' prototype takes the operation at those.
  kindTables[8][5] = (value, index, notFound) => {
    const store = vectors.get(value);
    if (store && store.cnt === 2 && !store.sub && (index === 0 || index === 1)) return store.tail[index];
    return store && store.cnt === 2 && mapEntries.has(value) ? notFound === missing ? outOfBounds() : notFound : noBranch;
  };
  nth = (operation => Object.defineProperty(function (value, index, notFound) {
    const arity = arguments.length;
    const method = value == null || typeof index !== "number" || arity !== 2 && arity !== 3 ? null : protocolHooks.publicMethod?.(value, methodKeys[25], arity, true, true);
    return method ? method.call(value, ...arguments) : operation.apply(null, arguments);
  }, "name", {value: operation.name}))(nth);

  // The map, vector and string branches; the other kinds' wrap it.
  function lookup(value, key, notFound) {
    const entries = maps.get(value);
    if (entries) {
      if (!Array.isArray(entries)) return kindHooks.trieValue(entries, key, notFound);
      const index = kindHooks.entryIndex(value, entries, key);
      return index < 0 ? notFound : entries[index].value;
    }
    const store = vectors.get(value);
    if (store) return mapEntries.has(value) ? key === 0 || key === 1 ? store.tail[key] : notFound : typeof key === "number" && key >= 0 && key < store.cnt ? kindHooks.nthVector(store, key, notFound) : notFound;
    if (typeof value === "string" && key != null && key > -1 && key < value.length) {
      return value.charAt(key | 0);
    }
    // Raw object properties are not lookup entries. Public get handles array
    // indexing separately without giving host objects owned collection storage.
    return notFound;
  }
  kindHooks.lookupTransient = transientLookup;
  kindHooks.nthVector = vectorNth;
  // A record is a map too, but `get` finds its `-lookup` (`recordMethods.lookup`)
  // first.
  kindHooks.entryIndex = collectionEntryIndex;

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
  // `lookup` of a vector, after the protocol lookup that `get` makes first,
  // whose method it calls as `get` would.
  kindTables[4][6] = (value, key, notFound, arity) => {
    const store = vectors.get(value);
    if (!store || mapEntries.has(value)) return noBranch;
    const method = protocolHooks.publicMethod?.(value, methodKeys[30], arity, true);
    if (method) return arity === 2 ? method.call(value, value, key) : method.call(value, value, key, notFound);
    return typeof key === "number" && key >= 0 && key < store.cnt ? vectorNth(store, key, notFound) : notFound;
  };
  // Likewise of a map: a hash map's trie and entries without slots are
  // searched as `lookup` does, and others, as trees, take `lookup`.
  kindTables[5][6] = (value, key, notFound, arity) => {
    const entries = maps.get(value);
    if (!entries) return noBranch;
    const method = protocolHooks.publicMethod?.(value, methodKeys[30], arity, true);
    if (method) return arity === 2 ? method.call(value, value, key) : method.call(value, value, key, notFound);
    if (!Array.isArray(entries)) return kindHooks.trieValue(entries, key, notFound);
    if (publicCollectionSlots.has(value)) return lookup(value, key, notFound);
    const index = collectionEntryIndex(value, entries, key);
    return index < 0 ? notFound : entries[index].value;
  };
  // Likewise of a map entry (light-cljs-373), as `lookup` reads it; where
  // protocol implementations exist, the operation, which looks them up.
  kindTables[8][6] = (value, key, notFound) => {
    const store = vectors.get(value);
    return store && !protocolHooks.publicMethod && mapEntries.has(value) ? key === 0 || key === 1 ? store.tail[key] : notFound : noBranch;
  };
  // `get` of a value whose type declares `ILookup` by its mask bit, as the
  // program's types and records do: the method its slot holds, read here, so
  // that the engine's feedback for the read is this operation's own, not that
  // of the protocol lookup every operation shares (light-cljs-358). A slot
  // without a method finishes as the protocol lookup would
  // (`protocolHooks.declaredMethod`), from the slot as read, which a getter
  // could have produced after changing the value; any other value takes the
  // operation.
  get = (operation => Object.defineProperty(function (value, key, notFound) {
    const resolved = methodKeys[30], arity = arguments.length;
    if (value != null && (arity === 2 || arity === 3) && (value[resolved.mask] & resolved.bit) !== 0) {
      demandKey(resolved, arity, value);
      const slot = value[keySlot(resolved, arity)], own = slot ? slot : protocolHooks.declaredMethod(value, resolved, arity, slot);
      if (own) return arity === 2 ? own.call(value, value, key) : own.call(value, value, key, notFound);
    }
    return arity === 2 ? operation(value, key) : arity === 3 ? operation(value, key, notFound) : operation.apply(null, arguments);
  }, "name", {value: operation.name}))(get);
  function contains(value, key) {
    const method = protocolHooks.publicMethod?.(value, methodKeys[2], 2);
    if (method) return method.call(value, value, key);
    const store = vectors.get(value);
    if (store) return Number.isInteger(key) && key >= 0 && key < store.cnt;
    return get(value, key, missing) !== missing;
  }
  // As `get.own`.
  contains = (operation => Object.defineProperty(function (value, key) {
    const resolved = methodKeys[2];
    if (value != null && (value[resolved.mask] & resolved.bit) !== 0) {
      demandKey(resolved, 2, value);
      const slot = value[keySlot(resolved, 2)], own = slot ? slot : protocolHooks.declaredMethod(value, resolved, 2, slot);
      if (own) return own.call(value, value, key);
    }
    return operation(value, key);
  }, "name", {value: operation.name}))(contains);
  // A map or set compares with a value of a program type through the generic
  // operations.
  protocolHooks.count = count;
  protocolHooks.has = contains;

  function every(predicate, value) {
    for (let current = seq(value); current !== null; current = next(current)) if (!truth(predicate.call(null, first(current)))) return false;
    return true;
  }
  // Object.equiv calls the receiver's IEquiv implementation directly, even
  // when an extension rejects an identical value. Language = keeps its own
  // identity shortcut. Saved Object methods dispatch on the borrowed receiver.
  function objectEquiv(value, other) {
    const equiv = protocolHooks.builtin?.(value, methodKeys[18], 2);
    if (equiv) return equiv.call(value, value, other);
    if (maps.has(value)) return structuralEqualPair(value, other);
    // Sequence/collection protocols compare contents even for the same receiver
    // (for example, a list containing NaN is not equivalent to itself). Cycle
    // and Iterate retain their default identity equality.
    return sets.has(value) || keywords.has(value) || symbols.has(value) || comparedSequential(value)
      ? structuralEqualPair(value, other) : equalPair(value, other);
  }
  function equalPair(left, right) {
    if (left == null) return right == null;
    if (left === right) return true;
    const equiv = protocolHooks.builtin?.(left, methodKeys[18], 2);
    if (equiv) return equiv.call(left, left, right);
    return structuralEqualPair(left, right);
  }
  function structuralEqualPair(left, right) {
    if (left instanceof Date) return right instanceof Date && left.valueOf() === right.valueOf();
    const leftKeyword = keywords.get(left);
    if (leftKeyword) {
      const rightKeyword = keywords.get(right);
      return !!rightKeyword && leftKeyword.spelling === rightKeyword.spelling;
    }
    if (recordTypes.get(left) !== recordTypes.get(right)) return false;
    if (maps.has(left)) return kindHooks.equivMap(left, right);
    if (sets.has(left)) return kindHooks.equivSet(left, right);
    if (comparedSequential(left)) return (kindHooks.equivSequential ?? kindHooks.equivVectors)(left, right);
    // UUIDs and Vars, whose builders install their comparison; primitive
    // NaN stays unequal, and foreign objects use identity only.
    return kindHooks.equivNamed?.(left, right) ?? false;
  }
  // Symbols compare their spelling, kept only where a program builds them
  // (light-cljs-247). No other kind's branch takes a symbol, so it may come
  // first.
  structuralEqualPair = (operation => (left, right) => {
    const leftSymbol = symbols.get(left);
    if (!leftSymbol) return operation(left, right);
    const rightSymbol = symbols.get(right);
    return !!rightSymbol && leftSymbol.spelling === rightSymbol.spelling;
  })(structuralEqualPair);
  // A UUID or a Var, or undefined for another value.
  kindHooks.equivNamed = (left, right) => uuids.has(left) ? uuids.has(right) && uuids.get(left) === uuids.get(right)
    : vars.has(left) ? vars.has(right) && equalPair(vars.get(left).symbol, vars.get(right).symbol) : undefined;
  // A record is a map too, but compares through its `IEquiv` method
  // (`recordMethods.equiv`), which every reader of this hook keeps.
  kindHooks.equivMap = (left, right) => {
    if (!(right != null && (protocolHooks.satisfies?.(methodKeys[33], right) ?? maps.has(right))) || (protocolHooks.satisfies?.(methodKeys[42], right) ?? false)) return false;
    if (protocolHooks.count ? protocolHooks.count(left) !== protocolHooks.count(right) : mapCount(left) !== mapCount(right)) return false;
    return mapTraversal(left).every(entry => {
      const found = get(right, entry.key, missing);
      // Hash/tree maps use equiv-map's right-first value dispatch. Array
      // maps retain their distinct left-first IEquiv implementation.
      return found !== missing && (mapKinds.get(left) === "array-map"
        ? equalPair(entry.value, found) : equalPair(found, entry.value));
    });
  };
  // Where no other sequential value and no protocol implementation exists.
  kindHooks.equivVectors = (left, right) => {
    if (!vectors.has(right) || vectorCount(left) !== vectorCount(right)) return false;
    const a = vectorStorage(left), b = vectorStorage(right);
    let index = 0, more = a !== null, other = b !== null;
    while (more) {
      if (!other || !equalPair(a[index], b[index])) return false;
      more = ++index < a.length; other = index < b.length;
    }
    return !other;
  };
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

  function equal(...values) {
    for (let i = 1; i < values.length; i++) {
      if (!equalPair(values[i - 1], values[i])) return false;
    }
    return true;
  }

  function sequential(value) {
    return vectors.has(value) || cells.has(value) || views.has(value) || queues.has(value) || lazyValues.has(value);
  }
  // The sequential values that equality and hashing compare by their items:
  // all but cycle and iterate seqs, which keep identity. Values other than
  // vectors only where the program builds them (`cons.compared`).
  function comparedSequential(value) {
    return vectors.has(value) || !!kindHooks.comparedSeq?.(value);
  }
  kindHooks.comparedSeq = value => cells.has(value) ? !["cycle", "iterate"].includes(cells.get(value).kind)
    : views.has(value) || queues.has(value) || lazyValues.has(value);

  const identityHashes = new WeakMap();
  let identityCounter = 0;
  function stringHash(value) {
    if (value == null) return 0;
    let result = 0;
    for (let i = 0; i < value.length; i++) result = Math.imul(result, 31) + value.charCodeAt(i);
    return result;
  }

  // ClojureScript's public Murmur3 collection/name hashes. Number hashes
  // follow the JavaScript Number profile, including signed zero and nonfinite values.
  function rotateLeft(value, distance) { return (value << distance) | (value >>> (32 - distance)); }
  function mixK1(value) { return Math.imul(rotateLeft(Math.imul(value, 0xcc9e2d51), 15), 0x1b873593); }
  function mixH1(hash, value) { return (Math.imul(rotateLeft(hash ^ value, 13), 5) + 0xe6546b64) | 0; }
  function fmix(hash, length) {
    hash ^= length;
    hash = Math.imul(hash ^ (hash >>> 16), 0x85ebca6b);
    hash = Math.imul(hash ^ (hash >>> 13), 0xc2b2ae35);
    return hash ^ (hash >>> 16);
  }
  function hashInt(value) { return value === 0 ? 0 : fmix(mixH1(0, mixK1(value)), 4); }
  function hashChars(value) {
    let h = 0, i = 1;
    for (; i < value.length; i += 2) h = mixH1(h, mixK1(value.charCodeAt(i - 1) | (value.charCodeAt(i) << 16)));
    if (value.length & 1) h ^= mixK1(value.charCodeAt(value.length - 1));
    return fmix(h, value.length * 2);
  }
  function hashNamed(value) {
    const seed = hashChars(value.name);
    return seed ^ (stringHash(value.namespace) + 0x9e3779b9 + (seed << 6) + (seed >> 2));
  }
  function hashDouble(value) {
    const buffer = new ArrayBuffer(8), view = new DataView(buffer);
    // Upstream reads native little-endian Float64 bytes as big-endian int32s.
    view.setFloat64(0, value, true);
    return view.getInt32(0) ^ view.getInt32(4);
  }
  function mixCollectionHash(basis, count) { return fmix(mixH1(0, mixK1(basis)), count); }
  function hashOrderedColl(value) {
    let result = 1, n = 0;
    for (let current = seq(value); current !== null; current = next(current), n++)
      result = (Math.imul(result, 31) + hash(first(current))) | 0;
    return mixCollectionHash(result, n);
  }
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
      if (uuidHashes.get(value) == null) uuidHashes.set(value, hash(uuids.get(value)));
      return uuidHashes.get(value);
    }
    if (vars.has(value)) return hash(vars.get(value).symbol);
    const cached = constructorHashes.get(value);
    if (cached != null) return cached | 0;
    const named = keywords.get(value) || symbols.get(value);
    if (named) {
      if (symbols.has(value) || keywords.has(value)) {
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

  // Sequential values (see `makeVector.equiv`).
  kindHooks.hashSequential = value => {
    if (vectors.has(value) || queues.has(value) || ["cons", "list", "repeat"].includes(cells.get(value)?.kind) || publicCollectionSlots.has(value)) {
      const result = hashOrderedColl(value);
      constructorHashes.set(value, result);
      return result;
    }
    const cached = lazyValues.get(value) ?? cells.get(value)?.chunkedState;
    if (cached?.hash != null) return cached.hash;
    const result = hashOrderedColl(value);
    if (cached) cached.hash = result;
    return result;
  };
  // The hash of a key for map and set entries that compute it on demand. Only
  // code that hashes, and so keeps `hash`, reads an entry's hash.
  const entryHashing = {};
  entryHashing.hash = hash;
  // Metadata belongs to values, never structural equality or hash payloads.
  function meta(value) {
    const method = value == null ? null : protocolHooks.publicMethod?.(value, methodKeys[35], 1);
    if (method) return method.call(value, value);
    const deferred = deferredVarMetadata.get(value);
    if (deferred) kindHooks.metaDeferred(value, deferred);
    return atoms.has(value) ? value.meta : metadataValues.has(value) ? metadataValues.get(value) : null;
  }
  function sequentialIndex(coll, value, start, reverse) {
    return indexIn(count(coll), i => nth(coll, i), value, start, reverse);
  }
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
  hostHooks.index = sequentialIndex;
  // Comparator equality controls sorted membership; structural hashes remain
  // attached to the retained key for unordered collection equality/hash.
  const treeRoots = new WeakMap(), treeSetMaps = new WeakMap();
  function sortedPosition(entries, key, comparator) {
    let lo = 0, hi = entries.length;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      if (comparator.call(null, entries[mid].key, key) < 0) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }
  function collectionEntryIndex(value, entries, key) {
    const comparator = sortedComparators.get(value);
    if (comparator) return kindHooks.sortedEntryIndex(entries, key, comparator);
    const kind = mapKinds.get(value) ?? setKinds.get(value);
    return kind === "array-map" ? entryIndex(entries, key) : kindHooks.hashEntryIndex(entries, key, kind);
  }
  // Maps and sets of a kind other than array-map or sorted: hash maps, hash
  // sets (public hash sets keep their map's kind) and records. Hash-map nil
  // slots are separate from the trie, and a hash map without other keys
  // never hashes a query (`hashEntryIndex` finds the nil key).
  kindHooks.hashEntryIndex = (entries, key, kind) => kind === "hash-map" && key != null && !entries.some(entry => entry.key != null) ? -1 : hashEntryIndex(entries, key);
  function putSortedEntry(entries, key, value, comparator, member = false) {
    const i = sortedPosition(entries, key, comparator);
    const exists = i < entries.length && comparator.call(null, entries[i].key, key) === 0;
    if (exists && (member || equalPair(entries[i].value, value))) return;
    const retained = exists ? entries[i].key : key;
    const entry = Object.freeze({key: retained, value, get hash() { return entryHashing.hash(retained); }});
    if (exists) entries[i] = entry;
    else entries.splice(i, 0, entry);
  }
  function putCollectionEntry(coll, entries, key, value) {
    const comparator = sortedComparators.get(coll);
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
  function withMetaSupported(value) {
    return !!protocolHooks.builtin?.(value, methodKeys[66], 2) || withMetaValue(value);
  }
  // The open declarationTransaction's undo journal; applications open none.
  let declarationJournal = null;
  // No-ops until setDeclarationDemand installs a compiler session's demand.
  let demandProtocol = () => {}, demandMethod = () => {}, demandMulti = () => {};
  // An open declarationTransaction replaces both with journaling versions.
  let patchDeclaration = (value, key, descriptor) => { Object.defineProperty(value, key, descriptor); };
  // A core protocol's native test, without the protocol objects (one per
  // protocol, here ICounted; see `builtinCoreImplementation`).
  function countedValue(value) {
    // A value is in one payload store: a cell or a lazy seq is in no other.
    const cell = cells.get(value);
    if (cell !== undefined) return cell.kind === "tree-seq" || cell.integer === true || cell.kind === "list";
    return !lazyValues.has(value) && (transients.has(value) || chunkBuffers.has(value) || chunks.has(value) || vectors.has(value) || maps.has(value) || sets.has(value) || queues.has(value) || (views.has(value) && ["indexed", "array-map", "sorted-map", "chunked"].includes(views.get(value).kind)));
  }
  function seqValue(value) {
    return cells.has(value) || lazyValues.has(value) || views.has(value) || chunks.get(value)?.integer === true || queues.has(value) || iteratorSeqs.has(value);
  }
  // Brand inventories are fixed. Build one private inventory per predicate,
  // without allocating an array and invoking a callback for each probe.
  function storesPredicate(...stores) {
    return value => {
      for (let i = 0; i < stores.length; i++) if (stores[i].has(value)) return true;
      return false;
    };
  }
  const seqableValue = storesPredicate(vectors, maps, sets, cells, views, queues, lazyValues, iteratorSeqs);
  function indexedValue(value) {
    const transient = transients.get(value);
    if (transient) return transient.kind === "vector";
    return chunks.has(value) || cells.get(value)?.integer === true || vectors.has(value) || views.get(value)?.indexed === true;
  }
  const collectionValue = storesPredicate(vectors, maps, sets, cells, views, queues, lazyValues);
  const editableValue = value => vectors.has(value) && !mapEntries.has(value) && !subvecSources.has(value) || maps.has(value) && !recordTypes.has(value) && !sortedComparators.has(value) || sets.has(value) && !sortedComparators.has(value);
  const reduceValue = value => cells.get(value)?.kind !== "chunked-cons" && (chunks.has(value) && !chunks.get(value).integer || vectors.has(value) || (maps.has(value) && !recordTypes.has(value)) || cells.has(value) || lazyValues.has(value) || (views.has(value) && views.get(value).kind !== "queue"));
  const iterableValue = value => !["cycle", "iterate", "repeat", "chunked-cons"].includes(cells.get(value)?.kind) && collectionValue(value);
  const chunkedSeqValue = value => { const kind = cells.get(value)?.kind; return kind === "range" || kind === "chunked-cons" || views.get(value)?.kind === "chunked"; };
  const chunkValue = value => chunks.has(value);
  const transientValue = value => transients.has(value);
  const transientAssociativeValue = value => transients.has(value) && transients.get(value).kind !== "set";
  const mapEntryValue = value => mapEntries.has(value);
  const metaValue = storesPredicate(symbols, vectors, maps, sets, cells, views, queues, lazyValues, vars, atoms, metaFunctions, reifyValues);
  const withMetaValue = storesPredicate(symbols, vectors, maps, sets, cells, views, queues, lazyValues, vars, metaFunctions, reifyValues);
  const writerValue = value => stringBufferWriters.has(value);
  function withMetaProtocol(value, metadata) {
    if (!withMetaSupported(value)) throw new Error("No protocol method IWithMeta.-with-meta defined for this value");
    // A function here has metadata, or a protocol implementation.
    return typeof value === "function" ? (protocolHooks.fnWithMeta ?? kindHooks.withMetaFn)(value, metadata) : collectionWithMeta(value, metadata);
  }
  function preserveMeta(original, result) {
    const metadata = meta(original);
    if (metadata != null || metadataValues.has(original)) metadataValues.set(result, metadata);
    return result;
  }
  const multiFns = new WeakSet();
  function updatedMap(original, entries, removeRecord = false) {
    const Type = !removeRecord && recordTypes.get(original);
    if (mapKinds.get(original) === "sorted-map") {
      const prior = maps.get(original);
      if (entries.length === prior.length && entries.every((entry, i) => entry === prior[i])) return original;
      const result = makeMap(mapTypes["sorted-map"], entries, "sorted-map", sortedComparators.get(original)), slots = publicCollectionSlots.get(original);
      if (slots && "cnt" in slots) constructorSlots(result, {cnt: slots.cnt + entries.length - prior.length}, meta(original), null);
      return preserveMeta(original, result);
    }
    const result = Type ? kindHooks.updatedRecord(Type, entries, mapKinds.get(original))
      : mapFromEntries(entries, mapKinds.get(original) === "hash-map" || (entries.length > 8 && entries.length > (maps.get(original)?.length ?? 0)));
    const slots = publicCollectionSlots.get(original);
    if (slots?.cnt != null) {
      const cnt = slots.cnt + entries.length - maps.get(original).length;
      const nil = Array.isArray(entries) ? entries.find(e => e.key == null) : entries.nil;
      constructorSlots(result, mapKinds.get(result) === "array-map" ? {cnt, arr: entries.flatMap(e => [e.key, e.value])}
        : {cnt, root: result, "has-nil?": nil != null, "nil-val": nil?.value}, meta(original), null);
    }
    return preserveMeta(original, result);
  }
  function withMeta(value, metadata) {
    return typeof value === "function" ? fnWithMeta(value, metadata) : collectionWithMeta(value, metadata);
  }
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
  // `withMeta` of a value other than a function, for callers that pass no
  // function.
  function collectionWithMeta(value, metadata) {
    if (value == null) return null;
    const method = protocolHooks.builtin?.(value, methodKeys[66], 2);
    if (method) return method.call(value, value, metadata);
    if (!recordTypes.has(value) && !mapEntries.has(value) &&
        [vectors, maps, sets, cells, views, queues, lazyValues].some(store => store.has(value)) && meta(value) === metadata) return value;
    let result;
    if (mapEntries.has(value)) return collectionWithMeta(kindHooks.entryVector(value), metadata);
    const kind = cells.get(value)?.kind;
    if (metaFunctions.has(value)) return kindHooks.withMetaFn(value, metadata);
    else if (lazyValues.has(value)) result = kindHooks.withMetaLazy(value, metadata);
    else if (kind === "range") result = kindHooks.withMetaRange(value, metadata);
    else if (kind === "chunked-cons") result = kindHooks.withMetaChunkedCons(value, metadata);
    else if (kind === "repeat") result = kindHooks.withMetaRepeat(value, metadata);
    else if (kind === "iterate") result = kindHooks.withMetaIterate(value);
    else if (publicCollectionSlots.get(value)?.vec) result = kindHooks.withMetaChunkedSeq(value, metadata);
    else if (kind === "cycle") result = kindHooks.withMetaCycle(value);
    else if (recordTypes.has(value)) result = kindHooks.withMetaRecord(value);
    else {
      const stores = [treeRoots, treeSetMaps, canonicalVectorRoots, publicCollectionSlots, constructorHashes, symbols, vectors, maps, mapKinds, sets, setKinds, sortedComparators, cells, views, queues, lazyValues, vars, subvecSources];
      if (!stores.some(store => store.has(value)) && !reifyValues.has(value)) throw new Error("value does not support metadata");
      result = Object.create(Object.getPrototypeOf(value));
      // A copy of a reify value has no brand of its type: `reifyValues` holds it.
      for (const store of [...stores, protocolImplementations]) if (store.has(value)) store.set(result, store.get(value));
      if (reifyValues.has(value)) reifyValues.add(result);
      if (ifnValues.has(value)) ifnValues.set(result, ifnValues.get(value));
      if (ifnValues.has(value) || reifyValues.has(value) || recordTypes.has(value)) Object.defineProperties(result, Object.getOwnPropertyDescriptors(value));
    }
    metadataValues.set(result, metadata);
    return result;
  }
  kindHooks.withMetaFn = fnWithMeta;
  // A map entry's metadata copy is a vector.
  kindHooks.entryVector = value => makeVector(null, vectors.get(value));
  kindHooks.withMetaLazy = (value, metadata) => new LazySeq(metadata, () => seq(value), null, lazyValues.get(value).hash);
  kindHooks.withMetaRange = (value, metadata) => {
    const c = cells.get(value), state = rangeChunks.get(c);
    return c.integer ? IntegerRangeValue(metadata, c.head, c.end, c.step, c.count, constructorHashes.get(value))
      : RangeValue(metadata, c.head, c.end, c.step, state?.chunk, state?.tail, constructorHashes.get(value));
  };
  kindHooks.withMetaChunkedCons = (value, metadata) => {
    const c = cells.get(value);
    return new ChunkedCons(c.chunk, c.more, metadata, c.hash);
  };
  kindHooks.withMetaChunkedSeq = (value, metadata) => {
    const c = publicCollectionSlots.get(value);
    return ChunkedSeqValue(c.vec, c.node, c.i, c.off, metadata, null);
  };
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
  function Var(get, symbol, metadata) { const value = makeVar(get, symbol, metadata); metadataValues.set(value, metadata); return value; }
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
  Var.prototype = varPrototype;
  // Namespace identities are session-owned, with source registration from emitted units.
  const namespaces = new Map();
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
  function createNs(name, obj = null) {
    if (arguments.length !== 1 && arguments.length !== 2) return invalidArity(arguments.length);
    if (!symbols.has(name)) throw new TypeError("create-ns requires a symbol");
    const key = str(name);
    if (!namespaces.has(key)) namespaces.set(key, new Namespace(obj, name));
    else if (obj != null) namespaces.get(key).obj = obj;
    return namespaces.get(key);
  }
  function macroReaderFunction(fn) {
    const wrapped = function (...args) { return withMacroReaderContext(fn, args); };
    return Object.assign(wrapped, fn);
  }
  // Reader contexts are immutable views. Nested macro expansion extends the
  // current view without copying its namespace maps and sets.
  function readerOverlay(base, own) {
    if (own.size === 0) return base;
    return {has: key => own.has(key) || base.has(key),
      get: key => own.has(key) ? own.get(key) : base.get(key)};
  }
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
  function deferredValue(state, lazy = false) {
    // Assignment occurs only after success; failures leave the thunk installed.
    // Reentrant calls are ordinary calls, matching CLJS's observable behavior.
    if (lazy ? state.thunk != null : truth(state.thunk)) { state.value = state.thunk.call(null); state.thunk = null; }
    return state.value;
  }
  // Nil metadata is not stored: a lazy seq without an entry reads as one
  // with nil (`meta`, the `meta` field, the compiler bridge).
  function LazySeq(metadata, thunk, result, cachedHash) {
    const value = Object.create(LazySeq.prototype);
    lazyValues.set(value, {thunk, value: result, hash: cachedHash});
    if (metadata !== null) metadataValues.set(value, metadata);
    return value;
  }
  function lazySequence(thunk) { return LazySeq(null, thunk, null, null); }
  function toArray(value) {
    const result = [];
    for (let current = seq(value); current !== null; current = next(current)) result.push(first(current));
    return result;
  }
  function vec(value) {
    if (isMapEntry(value)) return vector(keyProtocol(value), valProtocol(value));
    if (isVector(value)) return withMeta(value, null);
    return into(vector(), value);
  }
  function es6Iterator(value) {
    let current = seq(value);
    return {next() {
      if (current === null) return {value: null, done: true};
      const value = first(current);
      current = next(current);
      return {value, done: false};
    }};
  }
  hostHooks.iterator = es6Iterator;
  function fnRest(args, start) {
    return args.length > start ? view(IndexedSeqValue, Object.freeze(Array.prototype.slice.call(args, start)), 0, true) : null;
  }
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
  const typeHooks = {};
  typeHooks.applyTo = fnApplyTo;
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
  function makeIFn(methods, invokeMethods = methods) {
    const value = Object.create(ifnPrototype);
    ifnValues.set(value, methods);
    for (const [arity, method] of Object.entries(invokeMethods)) {
      value["cljs$core$IFn$_invoke$arity$" + arity] = ifnInvokeEntry(method);
    }
    reifyValues.add(value);
    return value;
  }
  const protocolKeys = new WeakMap(), protocolImplementations = new WeakMap();
  // The implementations `extend-type` gives host types, `object` and
  // `default`, by a protocol method key's text and then the type's name, and
  // the dispatch cells by a key's text (`dispatchCell`).
  const nativeMethods = new Map(), coreDispatchers = new Map();
  // The dispatch cell of a protocol method key's text (light-cljs-375): the
  // core protocol dispatcher `coreMethod` made for it (`dispatcher`), the
  // implementations `extend-type` gave host types for it (`natives`, the
  // key's entry of `nativeMethods`, which the declaration journal sets and
  // undoes with it) and its core protocol (`protocol`). A resolved key
  // (`methodKey`) holds its cell once `externalMethod` has looked it up, so
  // that a call reads them without a lookup by the key's text.
  function dispatchCell(text) { return coreDispatchers.get(text) ?? coreDispatchers.set(text, {}).get(text); }
  // The declaration demand of a method's resolved key (`methodKey`), for an
  // operation that reads the method's slot itself (`get`).
  function demandKey(key, arity, value) { demandMethod(key.key, arity, value); }
  // `directMethod` of a method's resolved key (`methodKey`): its slot for
  // `arity`, read from the key.
  function keyMethod(value, key, arity) {
    demandMethod(key.key, arity, value);
    return value == null ? undefined : value[keySlot(key, arity)];
  }
  // `directMethod` of a resolved key's protocol (arity 0): whether the value
  // holds the protocol's marker.
  function keyMarker(value, key) {
    demandMethod(key.name, 0, value);
    return value != null && value[key.marker] === protocolSentinel;
  }
  // The value's own implementation of a resolved key (`methodKey`), else one
  // that `extend-type` of a host type, `object` or `default` gave
  // (`externalMethod`).
  function builtin(value, key, arity) {
    const method = keyMethod(value, key, arity);
    if (method != null) return method;
    return protocolHooks.externalMethod?.(value, key, arity);
  }
  // `true` where the value declares the core protocol of a resolved key
  // (`methodKey`: its mask bit or marker, as `implementsProtocol` reads
  // them), else nothing, for the caller's native test (`?? countedValue(value)`).
  function ownProtocol(key, value) {
    return key.bit !== 0 && value != null && value[key.mask] & key.bit || keyMarker(value, key) ? true : undefined;
  }
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
    const declared = key.bit !== 0 && value != null && (value[key.mask] & key.bit) !== 0 || keyMarker(value, key);
    return declared || meta ? ownDeclared(value, key, arity, keyMethod(value, key, arity)) : undefined;
  }
  // The rest of `ownMethod` once the value declares the key's protocol (or,
  // for `IMeta`, is a function), given the `method` its slot held: that, else
  // nothing for the natively implemented `IMeta` of a function or a `reify`,
  // else an error.
  function ownDeclared(value, key, arity, method) {
    if (method) return method;
    if (key.meta && (typeof value === "function" || reifyValues.has(value))) return undefined;
    throw new Error("No protocol method " + key.key + " defined for this value");
  }
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
  const mungeCharacters = {...memberMunging, "/": "_SLASH_"};
  // These two upstream CHAR_MAP entries are two-character strings, while the
  // public function iterates one UTF-16 character at a time.
  delete mungeCharacters['"']; delete mungeCharacters["\\"];
  mungeCharacters['\\"'] = "_DOUBLEQUOTE_"; mungeCharacters["\\\\"] = "_BSLASH_";
  function mungeStr(value) { let result = ""; for (let i = 0; i < value.length; i++) result += mungeCharacters[value.charAt(i)] ?? value.charAt(i); return result; }
  // A map entry is a vector of its key and value of type `MapEntryType`: one
  // `makeVector` builds where the program can build vectors
  // (`mapEntry.vector`), and else one with the store it would build, a tail
  // of two items (`mapEntry.lean`), so that it keeps no vector type. The
  // shared runtime defines it twice, the second time with `makeVector`.
  let mapEntry;
  mapEntry = (key, value) => { const pair = Object.setPrototypeOf(makeVector([key, value]), MapEntryType.prototype); mapEntries.add(pair); return pair; };
  function ifnMethods(value) {
    return ifnValues.get(value) ?? (value == null ? undefined : ifnValues.get(Object.getPrototypeOf(value)));
  }
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
  // Unlike Function.apply, an IFn apply must bundle the 21st and later values
  // exactly once. Keep that path independent of .call's already-expanded ABI.
  function ifnApply(value, args) {
    const values = Array.from(args);
    return value.call(null, ...values.slice(0, 20), ...(values.length > 20 ? [values.slice(20)] : []));
  }
  function boundedCount(n, value) {
    if (protocolHooks.satisfies?.(methodKeys[11], value) ?? (value == null || countedValue(value))) return count(value);
    let total = 0;
    for (let current = seq(value); n > 0 && current !== null; n--, current = next(current)) total++;
    return total;
  }
  function applyTo(fn, n, args) {
    args = seq(args);
    if (!Number.isInteger(n) || n < 0 || n > 20) throw new Error("Only up to 20 arguments supported on functions");
    const values = [];
    for (let i = 0; i < n; i++, args = rest(args)) values.push(first(args));
    return fn.call(null, ...values);
  }
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
  // Chunk values expose indexed/count/reduce protocols, without being seqs.
  const chunks = privateStore(), chunkBuffers = new WeakMap();
  function arrayChunk(array, offset = 0, end = array.length) {
    const result = Object.create(arrayChunk.prototype);
    chunks.set(result, {array, offset, end});
    return result;
  }
  function chunkNth(chunk, i, fallback = missing) {
    const state = chunks.get(chunk), size = state.end - state.offset;
    if (state.integer && fallback === missing) return state.start + i * state.step;
    if (i >= 0 && i < size) return state.integer ? state.start + i * state.step : state.array[state.offset + i];
    if (fallback !== missing) return fallback;
    // ArrayChunk's two-argument -nth reads the backing array directly.
    return state.array?.[state.offset + i];
  }
  kindHooks.nthChunk = chunkNth;
  kindHooks.nthTransient = transientNth;
  // A transient vector's entries are its store; a map's or set's an array.
  kindHooks.countTransient = value => { const entries = editableTransient(value, "count").entries; return entries.cnt ?? entries.length; };
  function chunkFirst(value) { return chunkedFirstProtocol(value); }
  function chunkRest(value) { return chunkedRestProtocol(value); }
  function ownedChunkFirst(value) {
    const cell = cells.get(value);
    if (cell?.kind === "chunked-cons") return cell.chunk;
    if (cell?.kind === "range") {
      if (cell.integer) {
        const chunk = arrayChunk([], 0, Math.min(32, cell.count));
        chunks.reset(chunk, {...chunks.get(chunk), integer: true, start: cell.head, step: cell.step});
        return chunk;
      }
      return forceRangeChunk(cell).chunk;
    }
    const slice = views.get(value);
    if (publicCollectionSlots.has(value)) return arrayChunk(slice.storage, slice.offset);
    const end = Math.min(slice.storage.length, slice.offset + 32 - slice.offset % 32);
    return arrayChunk(slice.storage, slice.offset, end);
  }
  const rangeChunks = new WeakMap();
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
  function ownedChunkRest(value) {
    const cell = cells.get(value);
    if (cell?.kind === "chunked-cons") return cell.more ?? emptyList;
    if (cell?.kind === "range") return cell.integer ? (cell.count <= 32 ? emptyList : rangeNode(cell.head + 32 * cell.step, cell.end, cell.step, true)) : forceRangeChunk(cell).tail ?? emptyList;
    const slice = views.get(value), slots = publicCollectionSlots.get(value);
    if (slots?.vec) return kindHooks.chunkedVectorRest(slots);
    const end = Math.min(slice.storage.length, slice.offset + 32 - slice.offset % 32);
    return end < slice.storage.length ? view(slice.Type, slice.storage, end, slice.indexed, slice.kind, slice.tail) : slice.tail ?? emptyList;
  }
  // A view with the `vec` slot is a ChunkedSeq its public constructor built.
  kindHooks.chunkedVectorRest = chunkedVectorRest;
  // The chunks after a public ChunkedSeq's node.
  function chunkedVectorRest(slots) {
    const end = slots.i + slots.node.length;
    return end < count(slots.vec) ? ChunkedSeqValue(slots.vec, vectorChunk(slots.vec, end), end, 0, null, null) : emptyList;
  }
  // A runtime chunk's store where no protocol implementation exists, which
  // then gives the items and count that `nth` and `count` give; else
  // undefined (light-cljs-237).
  function chunkStore(chunk) { return protocolHooks.publicMethod ? undefined : chunks.get(chunk); }
  // Item `i` of a chunk, below its count: from `store`, its `chunkStore`.
  function chunkItem(chunk, store, i) { return store === undefined ? nth(chunk, i) : store.integer ? store.start + i * store.step : store.array[store.offset + i]; }
  // A runtime chunk without its first item.
  function dropFirstChunk(value) {
    const c = chunks.get(value);
    const size = protocolHooks.publicMethod ? count(value) : c.end - c.offset;
    if (c.integer ? size <= 1 : size === 0) throw new Error("-drop-first of empty chunk");
    const result = arrayChunk(c.array, c.offset + 1, c.end);
    if (c.integer) chunks.reset(result, {...c, start: c.start + c.step, offset: c.offset + 1});
    return result;
  }
  // A ChunkedCons's cell. Its `head` and `tail` are getters of the class, so
  // that a step allocates no closures. It is its own `chunkedState`, the
  // holder of its cached hash, the only field written after construction,
  // so it is not frozen: a step allocates one object (light-cljs-237).
  // Declared fields are defined, as a literal's: no inherited setter sees one.
  const ChunkedCell = class {
    kind = "chunked-cons"; chunk; more; hash;
    constructor(chunk, more, hash) { this.chunk = chunk; this.more = more; this.hash = hash; }
    get chunkedState() { return this; }
    // Where no protocol implementation exists, a runtime chunk's items and
    // count are its store's, as `nth`, `count` and `-drop-first` give them.
    get head() {
      const c = chunkStore(this.chunk);
      return c === undefined ? nth(this.chunk, 0) : c.integer ? c.start + 0 * c.step : c.array?.[c.offset];
    }
    get tail() {
      const c = chunkStore(this.chunk);
      if (c === undefined) return count(this.chunk) > 1 ? ChunkedCons(dropFirstProtocol(this.chunk), this.more, null, null) : this.more ?? emptyList;
      return c.end - c.offset > 1 ? ChunkedCons(dropFirstChunk(this.chunk), this.more, null, null) : this.more ?? emptyList;
    }
  };
  // Nil metadata is not stored, as for LazySeq.
  function ChunkedCons(chunk, more, metadata, cachedHash) {
    const value = Object.create(ChunkedCons.prototype);
    cells.set(value, new ChunkedCell(chunk, more, cachedHash));
    if (metadata !== null) metadataValues.set(value, metadata);
    return value;
  }
  function chunkCons(chunk, tail) { return count(chunk) === 0 ? tail : ChunkedCons(chunk, tail, null, null); }
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
  function mapManySteps(collections) {
    return lazySequence(() => {
      const sources = mapSeq(seq, collections);
      return every(x => x, sources) ? cons(mapSeq(first, sources), mapManySteps(mapSeq(rest, sources))) : null;
    });
  }
  // As upstream: of one collection, a reduction into a transient vector.
  function mapv(fn, ...colls) {
    return colls.length === 1 ? persistentBang(reduce((v, x) => conjBangOwn(v, fn.call(null, x)), transient(vector()), colls[0]))
      : into(vector(), mapSeq(fn, ...colls));
  }
  function rangeNode(start, end, step, integer, suppliedCount) {
    const value = Object.create((integer ? IntegerRangeValue : RangeValue).prototype);
    cells.set(value, integer ? new IntegerRangeCell(start, end, step, integer, arguments.length >= 5 ? suppliedCount : Math.ceil((end - start) / step))
      : new RangeCell(start, end, step, integer));
    return value;
  }
  // A range's cell, whose `tail` is a getter of the class, so that a step
  // builds no object literal and no closure (light-cljs-237); only an
  // integer range's has a count. Declared fields are defined, as a
  // literal's: no inherited setter sees one.
  const RangeCell = class {
    kind = "range"; head; end; step; integer;
    constructor(head, end, step, integer) { this.head = head; this.end = end; this.step = step; this.integer = integer; }
    get tail() {
      const following = this.head + this.step;
      return (this.step > 0 ? following < this.end : following > this.end) ? rangeNode(following, this.end, this.step, this.integer) : emptyList;
    }
  };
  const IntegerRangeCell = class extends RangeCell {
    count;
    constructor(head, end, step, integer, count) { super(head, end, step, integer); this.count = count; }
  };
  function IndexedSeqValue(arr, i, metadata) {
    const value = Object.create(IndexedSeqValue.prototype);
    metadataValues.set(value, metadata);
    // Ordinary views snapshot inputs; positional construction retains
    // the supplied public array and observes subsequent host-array writes.
    views.set(value, {storage: arr, offset: i, indexed: true, kind: "indexed", tail: null, Type: IndexedSeqValue});
    return value;
  }
  function RangeValue(metadata, start, end, step, chunk, chunkNext, cachedHash) {
    const value = rangeNode(start, end, step, false);
    const state = {chunk, tail: chunkNext};
    rangeChunks.set(cells.get(value), state);
    return constructorSlots(value, {start, end, step}, metadata, cachedHash);
  }
  function IntegerRangeValue(metadata, start, end, step, cnt, cachedHash) {
    return constructorSlots(rangeNode(start, end, step, true, cnt), {start, end, step, cnt}, metadata, cachedHash);
  }
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
  function printOption(options, name, root) { return options == null ? coreCells[root] : printHooks.option(options, name); }
  function flush() { if (arguments.length !== 0) return invalidArity(arguments.length); return null; }
  function stringWriter() {
    const buffer = {text: "", append(text) { this.text += text; return this; }};
    return {buffer, writer: new StringBufferWriter(buffer)};
  }
  function StringBufferWriter(sb) {
    const writer = Object.create(StringBufferWriter.prototype);
    writer.sb = sb;
    stringBufferWriters.add(writer);
    return writer;
  }
  function printsMeta(options, value) {
    return truth(printOption(options, "meta", "printMeta")) && (protocolHooks.implements?.(methodKeys[34], value) ?? metaValue(value)) && meta(value) != null;
  }
  // The printer's own entries pass on the options they receive (see
  // `printOptions`). Without protocol implementations, which could see the
  // seq of an array, an array is walked by index, other collections through
  // `printHooks.seq`, which the units that can pass it install.
  function printSequential(writer, printOne, begin, separator, end, options, coll) {
    const saved = coreCells.printLevel;
    coreCells.printLevel = saved == null ? null : saved - 1;
    try {
      if (coreCells.printLevel != null && coreCells.printLevel < 0) return write(writer, "#");
      write(writer, begin);
      const items = Array.isArray(coll) && !protocolHooks.publicMethod;
      let current = items ? coll.length ? 0 : null : printHooks.seq(coll), remaining = printOption(options, "print-length", "printLength"), printed = false;
      while (current != null && (remaining == null || remaining !== 0)) {
        if (printed) write(writer, separator);
        printOne.call(null, items ? coll[current] : printHooks.first(current), writer, options); printed = true;
        current = items ? current + 1 < coll.length ? current + 1 : null : printHooks.next(current); if (remaining != null) remaining--;
      }
      if (current != null) {
        if (printed) write(writer, separator);
        const marker = options == null ? null : printHooks.option(options, "more-marker"); write(writer, truth(marker) ? marker : "...");
      }
      return write(writer, end);
    } finally { coreCells.printLevel = saved; }
  }
  // Only keys of the kinds that keyword and symbol builders print lift a
  // namespace (`kindHooks.namespaceOf`).
  function printMapEntries(entries, writer, options, prefix = "", printKey = printWriter) {
    const ns = !prefix && kindHooks.namespaceOf?.(entries);
    // JavaScript object tags precede the map printer's depth suppression.
    if (prefix) write(writer, prefix);
    return printSequential(writer, (entry, target, opts) => {
      printKey(ns ? kindHooks.unqualified(entry.key) : entry.key, target, opts); write(target, " "); return printWriter(entry.value, target, opts);
    }, (ns ? "#:" + ns : "") + "{", ", ", "}", options, entries);
  }
  // The namespace `*print-namespace-maps*` lifts out of keys that all have
  // it, else a falsy value.
  kindHooks.namespaceOf = entries => {
    const names = entries.map(entry => keywords.get(entry.key) || symbols.get(entry.key));
    const ns = truth(coreCells.printNamespaceMaps) && names.length && names[0]?.namespace;
    return ns && names.every(name => name?.namespace === ns) && ns;
  };
  kindHooks.unqualified = key => keywords.has(key) ? freshKeyword(null, name(key)) : symbol(null, name(key));
  function printNumber(value) {
    return Number.isNaN(value) ? "##NaN" : value === Infinity ? "##Inf" : value === -Infinity ? "##-Inf" : Object.is(value, -0) ? "-0.0" : String(value);
  }
  // The printer's hooks, which the units that can use them fill in.
  const printHooks = {};
  // The printer for code that must not keep it; `printWriterImpl.install`
  // replaces it where the program can print. A collection's `toString`...
  printHooks.collection = value => Object.prototype.toString.call(value);
  function printWriter(value, writer, options) {
    return options == null ? printWriterImpl(value, writer, options) : printHooks.optionsWriter(value, writer, options);
  }
  // A key of a JavaScript object prints as a keyword where it can be one. The
  // keyword is built only where options or protocol implementations could
  // see it, or the program keeps the keyword type (`printHooks.jsKeyword`).
  function printJsKey(key, writer, options) {
    if (!/^[A-Za-z_*+?!\-'][\w*+?!\-']*$/.test(key)) return printWriter(key, writer, options);
    return printHooks.jsKeyword ? printWriter(printHooks.jsKeyword(key), writer, options) : write(writer, ":" + key);
  }
  printHooks.jsKeyword = key => freshKeyword(null, key);
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
  // Only values of these kinds, and of types with protocol implementations,
  // have metadata to print.
  kindHooks.printMetadata = (value, writer, options) => {
    if (printsMeta(options, value)) { write(writer, "^"); printWriter(meta(value), writer, options); write(writer, " "); }
  };
  // The kind printer chain with `print` for the values `test` takes: each kind's
  // printer passes other values on, to `printHost` last.
  const printKind = (previous = printHost, test, print) => (value, writer, options) => (test(value) ? print : previous)(value, writer, options);
  kindHooks.print = printKind(kindHooks.print, value => transients.has(value), (value, writer) => {
    const state = transients.get(value);
    return write(writer, "#object[cljs.core." + (state.kind === "vector" ? "TransientVector" : state.kind === "set" ? "TransientHashSet" : state.mapKind === "array-map" ? "TransientArrayMap" : "TransientHashMap") + "]");
  });
  kindHooks.print = printKind(kindHooks.print, value => keywords.has(value) || symbols.has(value) || vars.has(value), (value, writer) => write(writer, String(value)));
  // A vector prints the items `seq` walks, through `seq` where protocol
  // implementations could see it.
  kindHooks.print = printKind(kindHooks.print, value => vectors.has(value), (value, writer, options) => {
    if (protocolHooks.publicMethod) return printSequential(writer, printWriter, "[", " ", "]", options, value);
    const storage = vectorStorage(value) ?? [];
    return printSequential(writer, printWriter, "[", " ", "]", options, Array.isArray(storage) ? storage : Array.from(storage));
  });
  kindHooks.print = printKind(kindHooks.print, value => cells.has(value) || views.has(value) || queues.has(value) || lazyValues.has(value),
    (value, writer, options) => printSequential(writer, printWriter, queues.has(value) ? "#queue [" : "(", " ", queues.has(value) ? "]" : ")", options, value));
  kindHooks.print = printKind(kindHooks.print, value => maps.has(value), (value, writer, options) => printMapEntries(mapTraversal(value), writer, options));
  // What `printSequential` walks other than arrays: the seqs these build, and
  // what the public entry and protocol implementations pass.
  printHooks.seq = seq;
  printHooks.first = first;
  printHooks.next = next;
  function print(value, readable) {
    const saved = coreCells.printReadably; coreCells.printReadably = readable;
    try { const {buffer, writer} = stringWriter(); printWriterImpl(value, writer, null, true); flushWriter(writer); return buffer.text; }
    finally { coreCells.printReadably = saved; }
  }
  // Only a program that can print has collection `toString`, where it keeps a
  // collection type, and prints values into messages.
  printHooks.collection = value => print(value, true);
  printHooks.text = value => print(value, false);
  printHooks.writer = printWriter;
  printHooks.sequentialWriter = printSequential;
  function str(...values) { return values.map(value => value == null ? "" : typeof value === "string" ? value : sequential(value) || (maps.has(value) && !recordTypes.has(value)) || sets.has(value) ? kindHooks.strCollection(value) : String(value)).join(""); }
  kindHooks.strCollection = value => print(value, true);
  // `str` of one value for error messages, through `toString` for collections.
  const displayStr = value => value == null ? "" : String(value);

  function variadicCore(fn, fixed, delegate) {
    fn.cljs$lang$maxFixedArity = fixed;
    fn.cljs$lang$applyTo = args => fnApplyTo(delegate, fixed, args);
    fn.cljs$core$IFn$_invoke$arity$variadic = delegate;
    return fn;
  }
  variadicCore(vector, 0, args => vectorFromArray(toArray(args)));
  // Applied collection arguments remain a sequence. Materializing the rest
  // here would demand collections after the shortest input has already ended.
  variadicCore(mapSeq, 4, (fn, c1, c2, c3, colls) =>
    mapSeq(args => apply(fn, args), mapManySteps(cons(c1, cons(c2, cons(c3, colls))))));
  variadicCore(str, 1, (x, more) => {
    let result = str(x);
    for (let s = seq(more); s !== null; s = next(s)) result += str(first(s));
    return result;
  });

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
  const write = coreMethod("IWriter/-write", [2], (writer, text) => writer.sb.append(text), writerValue);
  const flushWriter = coreMethod("IWriter/-flush", [1], () => null, writerValue);
  // The `-nth` of the runtime's indexed values.
  function indexedNth(value, index, notFound) { return transients.has(value) ? transientNth(value, index, arguments.length === 2 ? missing : notFound) : nth(...arguments); }
  const countProtocol = coreMethod("ICounted/-count", [1], count, value => value == null || countedValue(value));
  const nthProtocol = coreMethod("IIndexed/-nth", [2, 3], indexedNth, indexedValue);
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
  function seqIndexes(Type) {
    Object.defineProperties(Type.prototype, Object.getOwnPropertyDescriptors({
      indexOf: seqMethods.indexOf,
      lastIndexOf: seqMethods.lastIndexOf
    }));
  }
  seqIndexes(Cons);
  Cons.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  seqIndexes(ListValue);
  ListValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  seqIndexes(EmptyListValue);
  EmptyListValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  seqIndexes(ChunkedSeqValue);
  ChunkedSeqValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  seqIndexes(IndexedSeqValue);
  IndexedSeqValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  seqIndexes(RangeValue);
  RangeValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  seqIndexes(IntegerRangeValue);
  IntegerRangeValue.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  Object.defineProperties(LazySeq.prototype, Object.getOwnPropertyDescriptors({
    equiv(other) { return objectEquiv(this, other); },
    indexOf: seqMethods.indexOf,
    lastIndexOf: seqMethods.lastIndexOf,
    [Symbol.iterator]: seqMethods[Symbol.iterator]
  }));
  NodeSeqType.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  ChunkedCons.prototype[Symbol.iterator] = seqMethods[Symbol.iterator];
  // Mutable core cells: dynamic Vars, host hooks, and operations the runtime reads
  // through a cell so `set!` reaches it. The `core` hub extends this object.
  const coreCells = {
    replOne: null, replTwo: null, replThree: null, replError: null, sourceFile: null,
    uncheckedArrays: false, commandLineArgs: null, printFn: null, printErrFn: null, printNewline: true,
    printReadably: true, printLength: null, printLevel: null, printNamespaceMaps: false,
    printMeta: false, printDup: false, flushOnNewline: true, printFnBodies: false,
    readerCurrentInputValue: null, currentNs: null, assertions: true,
    hostClock: null, hostRandom: null, execTapFn: null, rand: null, flush};
  const asTransientProtocol = coreMethod("IEditableCollection/-as-transient", [1], asTransient, editableValue);
  const persistentBangProtocol = coreMethod("ITransientCollection/-persistent!", [1], persistTransient, transientValue);
  const conjBangProtocol = coreMethod("ITransientCollection/-conj!", [2], conjTransient, transientValue);
  const assocBangProtocol = coreMethod("ITransientAssociative/-assoc!", [3], assocTransient, transientAssociativeValue);
  const dropFirstProtocol = coreMethod("IChunk/-drop-first", [1], dropFirstChunk, chunkValue);
  const keyProtocol = coreMethod("IMapEntry/-key", [1], entryKey, mapEntryValue);
  const valProtocol = coreMethod("IMapEntry/-val", [1], entryVal, mapEntryValue);
  // `-key` and `-val` of the runtime's map entries, which are all that
  // implement IMapEntry where no protocol implementation exists.
  function entryKey(entry) {
    if (!mapEntries.has(entry)) throw new Error("No protocol method IMapEntry/-key defined for this value");
    return nth(entry, 0);
  }
  function entryVal(entry) {
    if (!mapEntries.has(entry)) throw new Error("No protocol method IMapEntry/-val defined for this value");
    return nth(entry, 1);
  }
  protocolHooks.builtin = builtin;
  // Every protocol implementation goes through `installMethods`.
  protocolHooks.publicMethod = ownMethod;
  protocolHooks.declaredMethod = ownDeclared;
  // The generic operations test core protocols by name: without `extend-type`
  // or `specify!` the protocols a value declares, then their native tests ...
  protocolHooks.satisfies = ownProtocol;
  protocolHooks.implements = ownProtocol;
  // One install per core dispatcher the generic operations call.
  protocolHooks.countProtocol = countProtocol;
  protocolHooks.nthProtocol = nthProtocol;
  // A protocol implementation can give the value of a `reduced`.
  protocolHooks.deref = deref;
  // A function with a protocol implementation reaches `-with-meta`.
  protocolHooks.fnWithMeta = fnWithMeta;
  const chunkedFirstProtocol = coreMethod("IChunkedSeq/-chunked-first", [1], ownedChunkFirst, chunkedSeqValue);
  const chunkedRestProtocol = coreMethod("IChunkedSeq/-chunked-rest", [1], ownedChunkRest, chunkedSeqValue);
  const conjProtocol = coreMethod("ICollection/-conj", [2], conj, collectionValue);
  const iteratorProtocol = coreMethod("IIterable/-iterator", [1], iterator, iterableValue);
  coreCells.currentNs = createNs(symbol(null, "cljs.user"));
  createNs(symbol(null, "cljs.core"));
  return Object.freeze({PersistentArrayMapValue, PersistentVectorValue, add, assoc, conj, count, equal, first, gensym, get, inc, into, isSymbol, list, macroReaderFunction, mapv, multiply, next, rest, seq, symbolCore, vec, vectorFromArray});
})()
