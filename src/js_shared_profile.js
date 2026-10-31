// The shared-heap profile of src/js_runtime.js (docs/real-core-design.md,
// section 1; light-cljs-647): the units that build or read a representation,
// rewritten over the engine's intrinsics (`__rt`), in the manifest format of
// src/js_runtime.js. build.rs assembles `js_shared_runtime.js` from the two:
// a unit here replaces the runtime's unit of the same name; a unit named here
// only is placed first; the export section here replaces the runtime's and
// seals the booted runtime for publication; every other unit is the V8
// profile's text, with `coreCells.X` and the task's own bindings
// (`SHARED_TASK_STATE` in build.rs) rewritten to the task's cells,
// `globalThis.__lightSharedCells`, and the immutable vocabularies
// (`SHARED_WORDS`) built by `sharedWords`. The profile boots once in a boot
// task, publishes, and every macro task reads the one published runtime
// (light-cljs-648 wires it in). tests/compiler/runtime_manifest.rs validates
// the assembled manifest; tests/bbp_macros/shared_profile.rs boots it.
(() => {
  "use strict";

  //@unit sharedTaskDefaults effect
  // The defaults of the state the runtime keeps per task: the module-level
  // `let`s the V8 profile's functions assign after boot (gensym and identity
  // counters, tap and declaration-journal state, the global hierarchy) and
  // the registries next to them. A task's cells (`globalThis.__lightSharedCells`,
  // an object whose prototype is the published `coreCells` hub, whose
  // prototype is this object) hold the task's writes; a read that finds no
  // write falls through to the hub's cell default or to these. A registry
  // (a Set, Map or WeakMap, which cannot be shared) is made on first use
  // in the reading task, never in the boot task.
  const sharedTaskDefaults = {
    gensymCounter: 0, gensymLog: null, gensymScope: null,
    get gensymScopes() { return sharedTaskOwn(this, "gensymScopes", new Map()); },
    get generatedNames() { return sharedTaskOwn(this, "generatedNames", new Set()); },
    macroexpandServices: {},
    declarationJournal: null,
    demandProtocol: () => {}, demandMethod: () => {}, demandMulti: () => {},
    demandGate: null, demandHost: null,
    patchDeclaration: (value, key, descriptor) => { Object.defineProperty(value, key, descriptor); },
    setDeclaration: (store, key, value) => { store.set(key, value); },
    get identityHashes() { return sharedTaskOwn(this, "identityHashes", new WeakMap()); },
    identityCounter: 0,
    globalHierarchy: null,
    get tapListeners() { return sharedTaskOwn(this, "tapListeners", new Set()); },
    get tapQueue() { return sharedTaskOwn(this, "tapQueue", []); },
    tapDraining: false, tapDelivered: 0, tapPending: 0, tapLimit: 1024,
    // The slot-name memo of `protocolSlot`: a pure function's cache, which
    // the boot task fills for the runtime's own keys (`sharedProfileBoot`)
    // and every task fills for the keys it meets.
    get protocolSlots() { return sharedTaskOwn(this, "protocolSlots", new Map()); },
  };
  function sharedTaskOwn(cells, name, value) {
    if (cells === sharedTaskDefaults) {
      throw new TypeError("shared profile: per-task registry " + name + " read while booting");
    }
    Object.defineProperty(cells, name, {value, writable: true, configurable: true, enumerable: true});
    return value;
  }
  // The boot task's cells: over the defaults until the `coreCells` unit
  // loads, then over the hub; `sharedProfileBoot` moves its writes to the
  // hub's cells into the hub.
  globalThis.__lightSharedCells = Object.create(sharedTaskDefaults);
  //@unit sharedStore
  // A brand store of a kind the engine does not own (design point 8: a
  // task-owned runtime value), with a WeakMap's `has`, `get`, `set` and
  // `delete` over a private field, as `privateStore` keeps them: the store
  // itself is a class, which seals and publishes, where a WeakMap never
  // does. `set` replaces an existing payload, as a WeakMap's does.
  function sharedStore() {
    const absent = __rt.seal({});
    const Store = class extends (function (value) { return value; }) {
      #s;
      static has(value) { return (typeof value === "object" ? value !== null : typeof value === "function") && #s in value && value.#s !== absent; }
      static get(value) {
        if ((typeof value === "object" ? value !== null : typeof value === "function") && #s in value) { const payload = value.#s; return payload === absent ? undefined : payload; }
      }
      static set(value, payload) { if (#s in value) value.#s = payload; else new Store(value).#s = payload; return Store; }
      static reset(value, payload) { value.#s = payload; }
      static delete(value) { if (Store.has(value)) { value.#s = absent; return true; } return false; }
    };
    return Store;
  }
  //@unit sharedSet requires: sharedStore
  // A WeakSet likewise: `has`, `add` and `delete`.
  function sharedSet() {
    const store = sharedStore();
    return {has: value => store.has(value), add(value) { store.set(value, true); return this; }, delete: value => store.delete(value)};
  }
  //@unit sharedWords
  // An immutable vocabulary (`reservedNames`, `readerCoreNames`), a Set in
  // the V8 profile: membership over the sealed array of its words.
  function sharedWords(words) {
    __rt.seal(words);
    return {has: name => words.includes(name)};
  }
  //@unit sharedMethodKeys
  // The resolved keys `coreMethod` makes for the core protocol dispatchers,
  // which only their dispatchers hold: `sharedProfileBoot` fills their memos
  // and seals them with the keys of `methodKeys`.
  const sharedMethodKeys = [];
  //@unit sharedProfileBoot
  // The end of the boot: what every top-level binding holds is sealed in
  // place (`__rt.seal`, shallow; walked deeply through the properties and
  // prototypes that hold them: the registries, sentinels, constructors,
  // prototypes and every function's own-property record), then the bindings
  // the runtime reassigned while loading (`__rt.sealBindings`). The boot task
  // then gets a task's cells of its own over the hub, like every other task.
  // What a seal cannot take (a Map, a WeakMap, an object with internal
  // slots) is left; publication reports it with its path
  // (tests/bbp_macros/shared_profile.rs). The binding list is generated by
  // build.rs. `kindTables` is sealed too: `extend-type` and `specify!` on a
  // runtime kind, which empty it in the V8 profile, throw until
  // light-cljs-649 moves the dispatch state to engine tables.
  function sharedProfileBoot(exports) {
    // The lazily filled memos of the resolved method keys, filled now:
    // the slot of every arity a dispatcher can ask for, the dispatch cell
    // and the public-method info (design point 5).
    for (const key of [...methodKeys, ...sharedMethodKeys]) {
      if (key == null) continue;
      for (let arity = 0; arity <= 21; arity++) keySlot(key, arity);
      key.cell ??= dispatchCell(key.key);
      key.cell.protocol ??= coreProtocolsByName.get(key.name) ?? null;
      key.info ??= publicMethodInfo(key.key);
    }
    // The boot's writes to the hub's cells are the hub's defaults.
    const booting = globalThis.__lightSharedCells;
    for (const key of Object.keys(booting)) if (Object.getOwnPropertyDescriptor(coreCells, key) !== undefined) coreCells[key] = booting[key];
    const seen = new Set();
    const seal = value => {
      if (value === null || (typeof value !== "object" && typeof value !== "function") || seen.has(value)) return;
      seen.add(value);
      for (const key of Reflect.ownKeys(value)) {
        const descriptor = Object.getOwnPropertyDescriptor(value, key);
        if ("value" in descriptor) seal(descriptor.value);
        else { seal(descriptor.get); seal(descriptor.set); }
      }
      const prototype = Object.getPrototypeOf(value);
      if (prototype !== null && prototype !== Object.prototype && prototype !== Array.prototype && prototype !== Function.prototype) seal(prototype);
      try { __rt.seal(value); } catch (error) { if (!(error instanceof TypeError)) throw error; }
    };
    const bindings = [exports,
      /* SHARED_BINDINGS */];
    for (const binding of bindings) seal(binding);
    __rt.sealBindings(bindings);
    globalThis.__lightSharedCells = Object.create(coreCells);
    return exports;
  }
  //@unit sharedView
  // A brand store as a read-only view over an engine tag (design point 3):
  // `has` and `get` answer engine forms of the view's kind; `set` is only
  // called by the swapped constructors, so it throws. While the profile is
  // assembled, a runtime-built value of the kind takes a private store (the
  // store's own, since the view loads before `privateStore`), and the write
  // is recorded for the migration list (tests/bbp_macros/shared_profile.rs
  // prints `globalThis.__lightSharedSetCalls`).
  function sharedView(name, has, get) {
    const fallback = class extends (function (value) { return value; }) {
      #s;
      static has(value) { return (typeof value === "object" ? value !== null : typeof value === "function") && #s in value; }
      static get(value) { if (fallback.has(value)) return value.#s; }
      static set(value, payload) { new fallback(value).#s = payload; }
      static reset(value, payload) { value.#s = payload; }
    };
    const record = () => { (globalThis.__lightSharedSetCalls ??= []).push(name + "\n" + new Error().stack); };
    return {
      has: value => has(value) || fallback.has(value),
      get: value => has(value) ? get(value) : fallback.get(value),
      set(value, payload) { record(); fallback.set(value, payload); },
      reset(value, payload) { record(); fallback.reset(value, payload); },
      delete() { throw new TypeError("shared profile: " + name + ".delete"); }
    };
  }
  //@unit keywords requires: sharedStore
  const keywords = sharedStore();
  //@unit vectors requires: sharedView
  // A vector is an engine vector form.
  const vectors = sharedView("vectors", value => __rt.formKind(value) === "vector", value => ({cnt: __rt.formCount(value)}));
  //@unit symbols requires: sharedStore
  const symbols = sharedStore();
  //@unit sortedComparators requires: sharedStore
  const sortedComparators = sharedStore();
  //@unit mapEntries requires: sharedSet
  const mapEntries = sharedSet();
  //@unit cells requires: sharedView
  const cells = sharedView("cells",
    value => { const kind = __rt.formKind(value); return kind === "cons" || kind === "chunked-cons" || kind === "list"; },
    value => {
      const kind = __rt.formKind(value);
      if (kind === "list") return {kind, head: __rt.cljFirst(value), tail: __rt.cljRest(value), count: __rt.cljCount(value)};
      return {kind: "cons", head: __rt.cljFirst(value), tail: __rt.cljRest(value)};
    });
  //@unit stringBufferWriters requires: sharedSet
  const stringBufferWriters = sharedSet();
  //@unit constructorHashes requires: sharedStore
  const constructorHashes = sharedStore();
  //@unit publicCollectionSlots requires: sharedStore
  const publicCollectionSlots = sharedStore();
  //@unit vectorRoots requires: sharedStore
  const vectorRoots = sharedStore();
  //@unit canonicalVectorRoots requires: sharedStore
  const canonicalVectorRoots = sharedStore();
  //@unit metadataValues requires: sharedStore
  const metadataValues = sharedStore();
  //@unit deferredVarMetadata requires: sharedStore
  const deferredVarMetadata = sharedStore();
  //@unit uuids requires: sharedStore
  const uuids = sharedStore();
  //@unit uuidHashes requires: sharedStore
  const uuidHashes = sharedStore();
  //@unit vars requires: sharedStore
  const vars = sharedStore();
  //@unit atoms requires: sharedStore
  const atoms = sharedStore(), volatiles = sharedStore();
  //@unit lazyValues requires: sharedView
  // A lazy payload's `value` is the engine cell's seq (realized or parked
  // on by `cljSeq`); its `thunk` is null, which the verbatim `seq` units
  // read as realized.
  const lazyValues = sharedView("lazyValues", value => __rt.formKind(value) === "lazy-seq",
      value => ({thunk: null, get value() { return __rt.cljSeq(value); }, set value(_) {}})),
    delays = sharedView("delays", value => __rt.formKind(value) === "delay",
      value => ({thunk: null, get value() { return __rt.cljForce(value); }, set value(_) {}}));
  //@unit eductions requires: sharedStore
  const eductions = sharedStore();
  //@unit ifnValues requires: sharedStore
  const ifnValues = sharedStore(), ifnVariadics = sharedStore();
  //@unit metaFunctions requires: sharedStore
  const metaFunctions = sharedStore();
  //@unit typeNames requires: sharedStore
  const typeNames = sharedStore();
  //@unit typeFields requires: sharedStore
  const typeFields = sharedStore();
  //@unit recordTypes requires: sharedStore
  const recordTypes = sharedStore();
  //@unit reifyValues requires: sharedSet
  const reifyValues = sharedSet();
  //@unit emptyList
  const emptyList = __rt.formList();
  //@unit cons
  function cons(head, tail, kind = "cons") {
    return tail == null || kind === "list" && __rt.cljCount(tail) === 0 ? __rt.formList(head) : __rt.cljCons(head, tail);
  }
  //@unit subvecSources requires: sharedStore
  const subvecSources = sharedStore();
  //@unit hashRoots requires: sharedStore
  const hashRoots = sharedStore();
  //@unit hashTraversals requires: sharedStore
  const hashTraversals = sharedStore();
  //@unit makeVector requires: vectorSlice
  // A vector of the new array `items`, or of the store an operation built
  // (a trie the V8 profile's operations still build; light-cljs-650's
  // parity work re-targets them).
  function makeVector(items, store) {
    const array = store === undefined ? items : vectorSlice(store, 0, store.cnt);
    let value = __rt.formVector();
    for (let i = 0; i < array.length; i++) value = __rt.formConj(value, array[i]);
    return value;
  }
  //@unit storesPredicate
  // Shareable once booted: the captured rest array is sealed.
  function storesPredicate(...stores) {
    __rt.seal(stores);
    return value => {
      for (let i = 0; i < stores.length; i++) if (stores[i].has(value)) return true;
      return false;
    };
  }
  //@unit multiFns requires: sharedSet
  const multiFns = sharedSet();
  //@unit leanTables requires: entryIndex, sharedStore
  const leanTables = sharedStore();
  // The value of `key` in `entries`, as `get` of an array map gives it.
  const leanGet = (entries, key) => entries[entryIndex(entries, key)]?.value;
  // As `assoc` of an array map: a replaced entry keeps its first key.
  function leanPut(entries, key, value) {
    const index = entryIndex(entries, key);
    if (index < 0) entries.push({key, value}); else entries[index] = {key: entries[index].key, value};
  }
  //@unit LazySeq
  function LazySeq(metadata, thunk, result, cachedHash) {
    const value = __rt.lazySeq(thunk);
    return metadata !== null ? __rt.cljWithMeta(value, metadata) : value;
  }
  //@unit protocolKeys requires: sharedStore
  const protocolKeys = sharedStore(), protocolImplementations = sharedStore();
  //@unit protocolNames requires: sharedStore
  // The protocol registries stay Maps until light-cljs-649 moves them to
  // engine tables (`extend-type` and `defprotocol` fill them after boot).
  const protocolNames = sharedStore(), protocolValues = new Map(), protocolFunctions = new Map(), protocolVariables = new Map(), protocolGetters = new Map();
  //@unit chunks requires: privateStore, sharedStore
  // Chunk values expose indexed/count/reduce protocols, without being seqs.
  const chunks = privateStore(), chunkBuffers = sharedStore();
  //@unit rangeChunks requires: sharedStore
  const rangeChunks = sharedStore();
  //@unit exceptionMessages requires: sharedStore
  const exceptionMessages = sharedStore();
  //@unit coreMethod mutator requires: dispatchCell, invalidArity, protocolHooks, sharedMethodKeys
  // Shareable once booted: the arities array is sealed and the method key
  // resolved at boot, not memoized in a `let` the dispatcher assigns.
  function coreMethod(key, arities, fallback, native) {
    __rt.seal(arities);
    const resolved = protocolHooks.methodKey?.(key);
    if (resolved !== undefined) sharedMethodKeys.push(resolved);
    const dispatch = function (value, ...args) {
      const arity = arities.length === 1 ? arities[0] : arguments.length;
      if (!arities.includes(arity)) return invalidArity(arguments.length);
      if (arities.length === 1) args = args.slice(0, arity - 1);
      const method = protocolHooks.builtin?.(value, resolved ?? protocolHooks.methodKey(key), arity);
      if (method != null) return method.call(value, value, ...args);
      if (native(value)) return fallback(value, ...args);
      throw new Error("No protocol method " + key + " defined for this value");
    };
    dispatchCell(key).dispatcher = dispatch;
    return dispatch;
  }
  //@unit coreCells effect requires: flush, sharedTaskDefaults
  // The hub's cells as the V8 profile declares them (the `core` hub extends
  // this object), over the per-task defaults. The boot task's cells now
  // read through it; `sharedProfileBoot` moves the boot's writes to hub
  // cells (`hostClock`, `currentNs`, ...) into the hub.
  const coreCells = {
    replOne: null, replTwo: null, replThree: null, replError: null, sourceFile: null,
    uncheckedArrays: false, commandLineArgs: null, printFn: null, printErrFn: null, printNewline: true,
    printReadably: true, printLength: null, printLevel: null, printNamespaceMaps: false,
    printMeta: false, printDup: false, flushOnNewline: true, printFnBodies: false,
    readerCurrentInputValue: null, currentNs: null, assertions: true,
    hostClock: null, hostRandom: null, execTapFn: null, rand: null, flush};
  Object.setPrototypeOf(coreCells, sharedTaskDefaults);
  Object.setPrototypeOf(globalThis.__lightSharedCells, coreCells);
  //@exports
  return sharedProfileBoot(Object.freeze({setDeclarationDemand, declarationReceiver, declarationTransaction, applyDeclarationPatches, snapshotDeclarationPatches, restoreDeclarationPatches, compilerExceptionMessage, compilerForms, keyword, keywordFqn, get, symbol, vector, vectorFromArray, listFromArray, mapFromArray, arrayMapFromArray, mapFromArrays, mapShape, mapFromShape, hashMapShape, hashMapFromShape, setFromArray, arraySetFromArray, constantMapFromArray, constantSetFromArray, literalSetFromArray, setShape, setFromShape, queueFromArray, jsObject, makeVar, definitionVar, makeIFn, protocol, protocolPredicate, protocolMethod, metadataMethod, extendType, extendInstance, makeReify, reify, makeType, typeMasks, recordMethods, recordFromMap, Loadable, fnRest, fnApplyTo, displayStr, setMacroexpandServices, compilerFormBridge, core, inspect, hash, hashed}));
})()
