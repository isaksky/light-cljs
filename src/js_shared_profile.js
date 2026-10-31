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
    // Dead: `identityHashes` answers every host object from the engine.
    identityCounter: 0,
    globalHierarchy: null,
    get tapListeners() { return sharedTaskOwn(this, "tapListeners", new Set()); },
    get tapQueue() { return sharedTaskOwn(this, "tapQueue", []); },
    tapDraining: false, tapDelivered: 0, tapPending: 0, tapLimit: 1024,
    // The slot-name memo of `protocolSlot`: a pure function's cache, which
    // the boot task fills for the runtime's own keys (`sharedProfileBoot`)
    // and every task fills for the keys it meets.
    get protocolSlots() { return sharedTaskOwn(this, "protocolSlots", new Map()); },
    // The hashes the runtime computes for engine sequences
    // (`sharedSequenceHashes`): the task's own cache, as the V8 profile's
    // `constructorHashes` WeakMap is, keyed by the form.
    get formHashes() { return sharedTaskOwn(this, "formHashes", new WeakMap()); },
    // The dispatch and declaration registries that `defprotocol`,
    // `extend-type`, `reify` and the declaration journal write after boot:
    // each task's own Map, seeded with the boot's entries
    // (`sharedSnapshots`), whose values the boot sealed. A registration
    // made after boot is the registering task's, as the per-task fallback
    // has it, until light-cljs-649 moves them to engine tables; a write to
    // a sealed boot entry (a dispatch cell's `natives`) throws until then.
    get coreDispatchers() { return sharedTaskOwn(this, "coreDispatchers", sharedRegistry("coreDispatchers")); },
    get nativeMethods() { return sharedTaskOwn(this, "nativeMethods", sharedRegistry("nativeMethods")); },
    get protocolValues() { return sharedTaskOwn(this, "protocolValues", sharedRegistry("protocolValues")); },
    get protocolFunctions() { return sharedTaskOwn(this, "protocolFunctions", sharedRegistry("protocolFunctions")); },
    get protocolVariables() { return sharedTaskOwn(this, "protocolVariables", sharedRegistry("protocolVariables")); },
    get protocolGetters() { return sharedTaskOwn(this, "protocolGetters", sharedRegistry("protocolGetters")); },
    get reifySites() { return sharedTaskOwn(this, "reifySites", sharedRegistry("reifySites")); },
  };
  // The registries' boot entries, sealed with the rest at the end of the
  // boot (`sharedProfileBoot`); a task's registry starts as a copy.
  const sharedSnapshots = {};
  const sharedRegistry = name => new Map(sharedSnapshots[name] ?? []);
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
  //@unit sharedTaskCells requires: coreCells, sharedTaskDefaults
  // A task's cells: an object over the published hub with the hub's cells
  // and the defaults' data properties (`sharedTaskDefaults.cellNames`: the
  // gensym counter, the print state, the evaluator services, ...) as
  // writable own properties, since an assignment to a property inherited
  // from the sealed hub throws. The registries the defaults make on first
  // use (their getters) stay lazy. Every task makes its cells before it
  // reads the runtime: the boot task at the end of `sharedProfileBoot`,
  // every other task through `core.sharedTaskCells` (light-cljs-648). The
  // defaults are read from the hub itself (`Reflect.get`, which the
  // assembly's rewrite of `coreCells[` to the task's cells leaves alone),
  // not from the cells of whatever task is calling, which may not exist
  // yet and whose state (a gensym counter) is its own.
  function sharedTaskCells() {
    const cells = Object.create(coreCells);
    for (const name of sharedTaskDefaults.cellNames) {
      Object.defineProperty(cells, name, {value: Reflect.get(coreCells, name), writable: true, configurable: true, enumerable: true});
    }
    return cells;
  }
  //@unit sharedHost
  // Whether a value can carry a private field: an object or function the
  // task owns, not an engine form or atom (which have no private storage;
  // a private `in` on one throws).
  const sharedHost = value => (typeof value === "object" ? value !== null : typeof value === "function") && __rt.formKind(value) === null && !__rt.cljIsAtom(value);
  //@unit sharedStore requires: sharedHost
  // A brand store of a kind the engine does not own (design point 8: a
  // task-owned runtime value), with a WeakMap's `has`, `get`, `set` and
  // `delete` over a private field, as `privateStore` keeps them: the store
  // itself is a class, which seals and publishes, where a WeakMap never
  // does. `set` replaces an existing payload, as a WeakMap's does.
  function sharedStore() {
    const absent = __rt.seal({});
    const Store = class extends (function (value) { return value; }) {
      #s;
      static has(value) { return sharedHost(value) && #s in value && value.#s !== absent; }
      static get(value) {
        if (sharedHost(value) && #s in value) { const payload = value.#s; return payload === absent ? undefined : payload; }
      }
      static set(value, payload) {
        if (!sharedHost(value)) throw new TypeError("shared profile: a private store cannot brand an engine value");
        if (#s in value) value.#s = payload; else new Store(value).#s = payload;
        return Store;
      }
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
  //@unit sharedMap
  // A registry complete after boot, a Map in the V8 profile: a Map's `get`,
  // `has`, `size` and entry iteration over a sealed array of its entries
  // (the boot's walk seals it), keyed by identity; string keys through a
  // sealed dictionary. A write after boot has no method to call.
  function sharedMap(entries) {
    const pairs = Array.from(entries, ([key, value]) => [key, value]);
    const dict = pairs.every(([key]) => typeof key === "string") ? Object.create(null) : null;
    if (dict !== null) for (const [key, value] of pairs) dict[key] = value;
    const index = key => { for (let i = 0; i < pairs.length; i++) if (pairs[i][0] === key) return i; return -1; };
    return {
      pairs, dict,
      get(key) { if (dict !== null) return typeof key === "string" ? dict[key] : undefined; const i = index(key); return i < 0 ? undefined : pairs[i][1]; },
      has(key) { return dict !== null ? typeof key === "string" && key in dict : index(key) >= 0; },
      get size() { return pairs.length; },
      [Symbol.iterator]() { return pairs[Symbol.iterator](); }
    };
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
  // and seals them with the keys of `methodKeys`. `maxArity` is the largest
  // arity a dispatcher declares (`invokeProtocol`: 22), so the boot fills a
  // slot for every arity a dispatcher can ask for.
  const sharedMethodKeys = Object.assign([], {maxArity: 0});
  //@unit sharedProfileBoot
  // The end of the boot: what every top-level binding holds is sealed in
  // place (`__rt.seal`, shallow; walked deeply through the properties and
  // prototypes that hold them: the registries, sentinels, constructors,
  // prototypes and every function's own-property record), then, through the
  // closure captures no JavaScript walk follows (`__rt.sealReachable`), the
  // bindings the runtime reassigned while loading and the own-property
  // records and untouched default prototypes of the functions made while
  // booting (dispatchers, IFn methods, wrappers). The boot task then gets a
  // task's cells of its own over the hub, like every other task.
  // What a seal cannot take (a Map, a WeakMap, an object with internal
  // slots) is left; publication reports it with its path
  // (tests/bbp_macros/shared_profile.rs). The binding list is generated by
  // build.rs. `kindTables` is sealed too: `extend-type` and `specify!` on a
  // runtime kind, which empty it in the V8 profile, throw until
  // light-cljs-649 moves the dispatch state to engine tables.
  function sharedProfileBoot(exports) {
    // The lazily filled memos of the resolved method keys, filled now:
    // the slot of every arity a dispatcher can ask for (through the
    // largest a dispatcher declares, at least the 21 of the runtime's own
    // readers), the dispatch cell and the public-method info (design
    // point 5).
    const maxArity = Math.max(21, sharedMethodKeys.maxArity);
    for (const key of [...methodKeys, ...sharedMethodKeys]) {
      if (key == null) continue;
      for (let arity = 0; arity <= maxArity; arity++) keySlot(key, arity);
      key.cell ??= dispatchCell(key.key);
      key.cell.protocol ??= coreProtocolsByName.get(key.name) ?? null;
      key.info ??= publicMethodInfo(key.key);
    }
    // The boot's writes to the hub's cells are the hub's defaults; its
    // registries' entries seed every task's copy.
    const booting = globalThis.__lightSharedCells;
    for (const name of ["coreDispatchers", "nativeMethods", "protocolValues", "protocolFunctions", "protocolVariables", "protocolGetters", "reifySites"]) {
      sharedSnapshots[name] = Array.from(Object.getOwnPropertyDescriptor(booting, name) !== undefined ? booting[name] : [], ([key, value]) => [key, value]);
    }
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
    __rt.sealReachable(bindings);
    // The migration trace (`sharedView`) also names the top-level bindings
    // a seal cannot take: the registries that stay Maps and Sets.
    const trace = globalThis.__lightSharedSetTrace;
    if (trace !== undefined) {
      const names = [
        /* SHARED_BINDING_NAMES */];
      for (let i = 0; i < names.length; i++) {
        const tag = Object.prototype.toString.call(bindings[i + 1]).slice(8, -1);
        if (tag === "Map" || tag === "Set" || tag === "WeakMap" || tag === "WeakSet") trace.set("unsealed " + names[i], tag + (bindings[i + 1].size === undefined ? "" : " of " + bindings[i + 1].size));
      }
    }
    globalThis.__lightSharedCells = sharedTaskCells();
    return exports;
  }
  //@unit sharedView requires: sharedHost
  // A brand store as a read-only view over an engine tag (design point 3):
  // `has` and `get` answer engine forms of the view's kind; `set` is only
  // called by the swapped constructors, so it throws. While the profile is
  // assembled, a runtime-built value of the kind takes a private store (the
  // store's own, since the view loads before `privateStore`). A task that
  // sets `globalThis.__lightSharedSetTrace` to a Map before it writes gets
  // the first writer's stack per view, for the migration list
  // (tests/bbp_macros/shared_profile.rs); otherwise a write keeps nothing.
  function sharedView(name, has, get) {
    const fallback = class extends (function (value) { return value; }) {
      #s;
      static has(value) { return sharedHost(value) && #s in value; }
      static get(value) { if (fallback.has(value)) return value.#s; }
      static set(value, payload) { new fallback(value).#s = payload; }
      static reset(value, payload) { value.#s = payload; }
    };
    const record = () => {
      const trace = globalThis.__lightSharedSetTrace;
      if (trace !== undefined && !trace.has(name)) trace.set(name, new Error().stack);
    };
    return {
      has: value => has(value) || fallback.has(value),
      get: value => has(value) ? get(value) : fallback.get(value),
      set(value, payload) { record(); fallback.set(value, payload); },
      reset(value, payload) { record(); fallback.reset(value, payload); },
      delete() { throw new TypeError("shared profile: " + name + ".delete"); }
    };
  }
  //@unit keywords requires: sharedView
  // A keyword is an engine keyword form; its payload is read from the form.
  const keywords = sharedView("keywords", value => __rt.formKind(value) === "keyword", value => {
    const namespace = __rt.formNamespace(value), name = __rt.formName(value);
    return {namespace, name, spelling: namespace == null ? name : namespace + "/" + name};
  });
  //@unit makeKeyword
  // Engine keywords compare by value and hash in the engine; `spelling` is
  // the form's.
  function makeKeyword(namespace, name, spelling) {
    return __rt.formKeyword(namespace, name);
  }
  //@unit keyword
  // A keyword built at run time is fresh, as upstream's `(keyword ns name)`:
  // the V8 profile interns the literals it builds this way, which the Pants
  // build lowers to engine forms instead.
  function keyword(namespace, name) {
    return __rt.formKeyword(namespace, name);
  }
  //@unit sharedVectorItems
  // A new array of the items `from` to `to` of an engine vector.
  function sharedVectorItems(form, from, to) {
    const items = new Array(Math.max(0, to - from));
    for (let i = from; i < to; i++) items[i - from] = __rt.formNth(form, i);
    return items;
  }
  //@unit sharedVectorStore requires: sharedVectorItems, vectorTailOffset
  // The store of an engine vector, as the verbatim units read one: `cnt`,
  // and `tail`, the items from the tail offset (every item of a vector of
  // at most 32), which the units that read a small vector's or a map
  // entry's items directly take; `root` and `shift` are read only by the
  // swapped storage units, which use `form` instead.
  function sharedVectorStore(form) {
    const cnt = __rt.formCount(form);
    return {cnt, form, get tail() { return sharedVectorItems(form, vectorTailOffset(cnt), cnt); }};
  }
  //@unit sharedVectorForm requires: vectorSlice
  // The engine vector of a store: its own, or one of the items of a trie
  // store (a transient's, a map entry's) or a view (a subvector's).
  function sharedVectorForm(store) {
    if (store.form !== undefined) return store.form;
    const items = vectorSlice(store, 0, store.cnt);
    let value = __rt.formVector();
    for (let i = 0; i < items.length; i++) value = __rt.formConj(value, items[i]);
    return value;
  }
  //@unit vectors requires: sharedVectorStore, sharedView
  // A vector is an engine vector form.
  const vectors = sharedView("vectors", value => __rt.formKind(value) === "vector", sharedVectorStore);
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
  //@unit sharedSequenceHashes
  // The hash cache of the engine sequences (vectors, lists, conses, lazy
  // seqs): the engine hashes a host object inside a form by its identity,
  // which is the runtime's hash of a function or a plain object
  // (`identityHashes`) but not of a symbol, a list of `listFromArray`, a
  // map or a Date, which the runtime hashes by value while equality
  // compares them by value, so a sequence holding one hashes as the runtime
  // hashes it, through `hash` of its items (`hashOrderedColl`), and the
  // result is cached in the task's cells (`formHashes`), as the V8
  // profile's WeakMap caches it. A sequence of engine values hashes the
  // same either way. Every other form (a keyword, a map, a set) hashes in
  // the engine (`formHash`).
  const sharedSequenceHashes = {
    kinds: __rt.seal(["vector", "list", "cons", "chunked-cons", "lazy-seq"]),
    is: kind => sharedSequenceHashes.kinds.includes(kind),
    cache() { const cells = globalThis.__lightSharedCells; return cells.formHashes; },
    has: value => sharedSequenceHashes.cache().has(value),
    get: value => sharedSequenceHashes.cache().get(value),
    set(value, hash) { sharedSequenceHashes.cache().set(value, hash); }
  };
  //@unit constructorHashes requires: sharedSequenceHashes, sharedStore
  // The hash cache: an engine sequence's hash is the runtime's, cached per
  // task (`sharedSequenceHashes`); any other engine form's is computed and
  // cached by the engine (`formHash`), so the cache answers it from there
  // and drops a hash given for one; a runtime-built value keeps its own.
  const constructorHashes = (store => ({
    has(value) {
      const kind = __rt.formKind(value);
      return kind === null ? store.has(value) : sharedSequenceHashes.is(kind) ? sharedSequenceHashes.has(value) : true;
    },
    get(value) {
      const kind = __rt.formKind(value);
      return kind === null ? store.get(value) : sharedSequenceHashes.is(kind) ? sharedSequenceHashes.get(value) : __rt.formHash(value);
    },
    set(value, hash) {
      const kind = __rt.formKind(value);
      if (kind === null) store.set(value, hash); else if (sharedSequenceHashes.is(kind)) sharedSequenceHashes.set(value, hash);
      return this;
    }
  }))(sharedStore());
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
  //@unit identityHashes
  // The identity hashes of host objects are the engine's (`identityHash`:
  // the same in every task for the object's lifetime, never reused after
  // its collection; design point 6), so every object has one and the
  // verbatim `hash` unit's counter is never reached.
  const identityHashes = {has: () => true, get: value => __rt.identityHash(value), set() { return this; }};
  //@unit uuids requires: sharedStore
  const uuids = sharedStore();
  //@unit uuidHashes requires: sharedStore
  const uuidHashes = sharedStore();
  //@unit vars requires: sharedStore
  const vars = sharedStore();
  //@unit atoms requires: sharedStore
  const atoms = sharedStore(), volatiles = sharedStore();
  //@unit lazyValues requires: sharedSequenceHashes, sharedView
  // A lazy payload over the engine's once-cell: `thunk` is null once the
  // cell is realized (`cljRealized`, which realizes nothing), else a
  // function that realizes the cell, so the verbatim `deferredValue` and
  // `realized?` read the cell's state; `value` is the cell's seq (realized
  // or parked on by `cljSeq`). The writes `deferredValue` makes after a
  // call are dropped: the cell holds the value. A delay's `value` is read
  // without running its body (`cljDelayState`): its seed until realized.
  // A lazy seq's `hash` is the task's cache of the runtime's hash
  // (`sharedSequenceHashes`), which `hashSequential` fills.
  const lazyValues = sharedView("lazyValues", value => __rt.formKind(value) === "lazy-seq",
      value => ({
        get thunk() { return __rt.cljRealized(value) ? null : () => __rt.cljSeq(value); }, set thunk(_) {},
        get value() { return __rt.cljSeq(value); }, set value(_) {},
        get hash() { return sharedSequenceHashes.get(value); }, set hash(hash) { sharedSequenceHashes.set(value, hash); }
      })),
    // A delay's `thunk` is `false`, not null, while the cell is realized but
    // prints as pending (`cljDelayState` reads `[ready value]`): one made
    // with a `false` thunk, whose `realized?` is true (`!truth(false)`) and
    // whose printer shows `:pending` (`thunk == null`), as the V8 profile's
    // state has it. Its `value` is the state's in every case: the seed while
    // pending or after a failed realization, else the realized value.
    delays = sharedView("delays", value => __rt.formKind(value) === "delay",
      value => ({
        get thunk() {
          if (!__rt.cljRealized(value)) return () => __rt.cljForce(value);
          return __rt.formNth(__rt.cljDelayState(value), 0) ? null : false;
        }, set thunk(_) {},
        get value() { return __rt.formNth(__rt.cljDelayState(value), 1); }, set value(_) {}
      }));
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
  //@unit sharedSeqForm requires: first, rest, seq
  // A runtime-owned sequence (design point 8: a list of `listFromArray`, an
  // indexed seq over a vector's items, the runtime's own `iterate` cell) as
  // an engine sequence, for the engine operations that read a tail or a
  // thunk's result (`cljCons`, a lazy seq's realization), which cannot read
  // a private store: a lazy seq that takes one element at a time through
  // the verbatim `seq`, `first` and `rest`, so an infinite sequence is not
  // traversed. `nil`, a primitive and an engine form are themselves.
  function sharedSeqForm(value) {
    if (value == null || __rt.formKind(value) !== null || typeof value !== "object" && typeof value !== "function") return value;
    return __rt.lazySeq(() => {
      const current = seq(value);
      return current == null ? null : __rt.cljCons(first(current), sharedSeqForm(rest(current)));
    });
  }
  //@unit cons requires: count, sharedSeqForm
  // A cons onto a runtime-owned tail goes through `sharedSeqForm`; a list
  // tail's count is the runtime's (`count`), the engine's for an engine form.
  function cons(head, tail, kind = "cons") {
    if (tail == null) return __rt.formList(head);
    if (kind === "list" && (__rt.formKind(tail) !== null ? __rt.cljCount(tail) : count(tail)) === 0) return __rt.formList(head);
    return __rt.cljCons(head, sharedSeqForm(tail));
  }
  //@unit subvecSources requires: sharedStore
  const subvecSources = sharedStore();
  //@unit subvec requires: count, isVector, makeVector, methodKeys, nth, protocolHooks,
  //@ vectorFromArray, vectors, VectorView
  // An engine vector of the items: the engine has no view kind, so the
  // result is a vector of its own, not upstream's `Subvec` with its source.
  function subvec(value, start, end = count(value)) {
    if (start == null || end == null) throw new Error("Assert failed: subvec indices cannot be nil");
    start = start | 0; end = end | 0;
    if (start < 0 || end < start || end > count(value)) throw new Error("Index out of bounds");
    if (!isVector(value)) throw new Error("subvec requires a vector");
    const store = !protocolHooks.publicMethod?.(value, methodKeys[25], 2, true, true) && vectors.get(value);
    return store ? makeVector(null, new VectorView(store, start, end - start))
      : vectorFromArray(Array.from({length: end - start}, (_, i) => nth(value, start + i)));
  }
  //@unit treeRoots requires: sharedStore
  const treeRoots = sharedStore(), treeSetMaps = sharedStore();
  //@unit hashRoots requires: sharedStore
  const hashRoots = sharedStore();
  //@unit hashTraversals requires: sharedStore
  const hashTraversals = sharedStore();
  //@unit makeVector requires: sharedVectorForm
  // A vector of the new array `items`, or of the store an operation built:
  // an engine store's own vector, or a vector of a trie's or a view's items
  // (a persisted transient, a `subvec`; light-cljs-650's parity work
  // re-targets those).
  function makeVector(items, store) {
    if (store !== undefined) return sharedVectorForm(store);
    let value = __rt.formVector();
    for (let i = 0; i < items.length; i++) value = __rt.formConj(value, items[i]);
    return value;
  }
  //@unit vectorLeaf requires: sharedVectorItems, vectorTailOffset
  // The leaf, or the tail, that holds index `i` of a store; of an engine
  // store, a new array of its items from index `i & -32`; of a view
  // (`VectorView`), a new array of its items from index `i & -32`.
  function vectorLeaf(store, i) {
    if (store.sub) return store.leaf(i);
    if (store.form !== undefined) return sharedVectorItems(store.form, i & -32, Math.min((i & -32) + 32, store.cnt));
    if (i >= vectorTailOffset(store.cnt)) return store.tail;
    let node = store.root;
    for (let level = store.shift; level > 0; level -= 5) node = node[(i >>> level) & 31];
    return node;
  }
  //@unit vectorSlice requires: sharedVectorItems, vectorLeaf
  // A new array of a store's items from index `from` to `to`.
  function vectorSlice(store, from, to) {
    if (store.form !== undefined) return sharedVectorItems(store.form, from, to);
    const items = [];
    for (let i = from; i < to; i = (i | 31) + 1) items.push(...vectorLeaf(store, i).slice(i & 31, (i & 31) + to - i));
    return items;
  }
  //@unit vectorConj requires: sharedVectorForm, sharedVectorStore
  // `store` with `item` added: the engine's `conj`.
  function vectorConj(store, item) {
    return sharedVectorStore(__rt.formConj(sharedVectorForm(store), item));
  }
  //@unit vectorAssoc requires: sharedVectorForm, sharedVectorStore
  // `store` with `item` at its index `i`: the engine's `assoc`. The callers
  // check the bounds; a fractional index truncates, as the V8 profile's
  // bitwise indexing of its trie has it, where the engine wants an integer.
  function vectorAssoc(store, i, item) {
    if (store.sub) return store.assoc(i, item);
    return sharedVectorStore(__rt.formAssoc(sharedVectorForm(store), Math.trunc(i), item));
  }
  //@unit vectorPop requires: sharedVectorForm, sharedVectorStore
  // `store`, of at least one item, without its last: the engine's `pop`.
  function vectorPop(store) {
    if (store.sub) return store.pop();
    return sharedVectorStore(__rt.cljPop(sharedVectorForm(store)));
  }
  //@unit vectorNth requires: missing, outOfBounds, vectorLeaf
  // The item of a vector's store at a number `index`, truncated once in
  // bounds (a negative fraction keeps its fallback); a view's is that of
  // the store it views.
  function vectorNth(store, index, notFound) {
    if (index >= 0 && index < store.cnt) {
      if (store.sub) return vectorNth(store.sub, store.start + index, notFound);
      return store.form !== undefined ? __rt.formNth(store.form, Math.trunc(index)) : vectorLeaf(store, index)[index & 31];
    }
    if (notFound === missing) return outOfBounds();
    return notFound;
  }
  //@unit makeVector.transient effect: makeVector, mapEntry when: asTransient requires: kindHooks,
  //@ vectorSlice, vectorTrie
  // A transient vector edits a trie of the vector's items, a task-owned
  // value (design point 8), as the V8 profile's edits a copy of the tail
  // and the nodes along the paths it changes, which it owns
  // (`vectorEditor`): a weak set, which keeps no node a `pop!` discarded.
  kindHooks.transientVector = store => {
    const trie = store.form !== undefined ? vectorTrie(vectorSlice(store, 0, store.cnt)) : store;
    const tail = trie.tail.slice();
    return {cnt: trie.cnt, shift: trie.shift, root: trie.root, tail, owned: new WeakSet([tail])};
  };
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
  //@unit Delay
  // An engine delay: realized to `result` at once where `thunk` is nil or
  // false, as upstream's `(Delay. thunk result)`.
  function Delay(thunk, result) {
    return __rt.delay(thunk, result);
  }
  //@unit LazySeq requires: sharedSeqForm
  // An engine lazy seq of `thunk`, whose result crosses into the engine
  // through `sharedSeqForm` when the runtime owns it; one made with no
  // thunk holds the computed `result`, so its cell is realized to it at
  // once. The engine caches the hash (`constructorHashes`), so `cachedHash`
  // is not kept.
  function LazySeq(metadata, thunk, result, cachedHash) {
    let value;
    if (thunk == null) { value = __rt.lazySeq(() => sharedSeqForm(result)); __rt.cljSeq(value); }
    else value = __rt.lazySeq(() => sharedSeqForm(thunk.call(null)));
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
    sharedMethodKeys.maxArity = Math.max(sharedMethodKeys.maxArity, ...arities);
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
  //@unit protocolMasks requires: protocolMaskNames, sharedMap
  // Membership checks are on every generic collection operation. Resolve the
  // fixed ABI once as [property, bit], including the last occurrence of
  // duplicate protocol names.
  const protocolMasks = sharedMap(protocolMaskNames.map((name, index) => [name,
    ["cljs$lang$protocol_mask$partition" + (index >> 5) + "$", 1 << (index % 32)]]));
  //@unit coreProtocolNames requires: ASeq, Fn, IAssociative, IAtom, IChunk, IChunkedNext,
  //@ IChunkedSeq, ICloneable, ICollection, IComparable, ICounted, IDeref, IDerefWithTimeout, IDrop,
  //@ IEditableCollection, IEmptyableCollection, IEncodeClojure, IEncodeJS, IEquiv, IFind, IFn,
  //@ IHash, IIndexed, IIterable, IKVReduce, IList, ILookup, IMap, IMapEntry, IMeta, IMultiFn,
  //@ INamed, INext, Inst, IPending, IPrintWithWriter, IRecord, IReduce, IReset, IReversible, ISeq,
  //@ ISeqable, ISequential, ISet, ISorted, IStack, ISwap, ITransientAssociative,
  //@ ITransientCollection, ITransientMap, ITransientSet, ITransientVector, IUUID, IVector,
  //@ IVolatile, IWatchable, IWithMeta, IWriter, sharedMap
  const coreProtocolNames = sharedMap([[Inst, "Inst"], [IUUID, "IUUID"], [ASeq, "ASeq"], [IDerefWithTimeout, "IDerefWithTimeout"], [INamed, "INamed"], [IMultiFn, "IMultiFn"], [IEditableCollection, "IEditableCollection"], [ITransientCollection, "ITransientCollection"], [ITransientAssociative, "ITransientAssociative"], [ITransientMap, "ITransientMap"], [ITransientVector, "ITransientVector"], [ITransientSet, "ITransientSet"], [IEncodeJS, "IEncodeJS"], [IEncodeClojure, "IEncodeClojure"], [IChunk, "IChunk"], [IChunkedNext, "IChunkedNext"], [IDrop, "IDrop"], [IPending, "IPending"], [Fn, "Fn"], [IList, "IList"], [IMapEntry, "IMapEntry"], [IFind, "IFind"], [ISorted, "ISorted"], [IChunkedSeq, "IChunkedSeq"], [IRecord, "IRecord"], [ICloneable, "ICloneable"], [IPrintWithWriter, "IPrintWithWriter"], [IWriter, "IWriter"], [IReduce, "IReduce"], [IKVReduce, "IKVReduce"], [IVector, "IVector"], [IMap, "IMap"], [ICollection, "ICollection"], [IEmptyableCollection, "IEmptyableCollection"], [ISet, "ISet"], [IStack, "IStack"], [IReversible, "IReversible"], [IComparable, "IComparable"], [IIterable, "IIterable"], [IHash, "IHash"], [IEquiv, "IEquiv"], [IFn, "IFn"], [ISeqable, "ISeqable"], [ISeq, "ISeq"], [INext, "INext"], [ICounted, "ICounted"], [IIndexed, "IIndexed"], [ILookup, "ILookup"], [ISequential, "ISequential"], [IAssociative, "IAssociative"], [IMeta, "IMeta"], [IWithMeta, "IWithMeta"], [IDeref, "IDeref"], [IAtom, "IAtom"], [IReset, "IReset"], [ISwap, "ISwap"], [IVolatile, "IVolatile"], [IWatchable, "IWatchable"]]);
  //@unit coreProtocolsByName requires: coreProtocolNames, sharedMap
  const coreProtocolsByName = sharedMap([...coreProtocolNames].map(([protocol, name]) => [name, protocol]));
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
  //@ seqValue, setValue, sharedMap, sortedValue, stackValue, transientAssociativeValue,
  //@ transientMapValue, transientSetValue, transientValue, transientVectorValue,
  //@ uuidImplementation, vectorValue, volatileValue, watchableValue, withMetaValue, writerValue
  function builtinCoreImplementation(protocol, value) { return !!nativeImplementations.get(protocol)?.(value); }
  const nativeImplementations = sharedMap([[Inst, instValue], [IUUID, uuidImplementation], [ASeq, aseqValue], [INamed, namedValue], [IMultiFn, multiFnValue], [IFn, ifnValue], [IEditableCollection, editableValue], [ITransientCollection, transientValue], [ITransientAssociative, transientAssociativeValue], [ITransientVector, transientVectorValue], [ITransientMap, transientMapValue], [ITransientSet, transientSetValue], [Fn, fnValue], [IList, listValue], [IChunk, chunkValue], [IChunkedNext, chunkedSeqValue], [IDrop, dropValue], [IReduce, reduceValue], [IKVReduce, kvReduceValue], [IPrintWithWriter, printableValue], [IHash, hashableValue], [IEquiv, hashableValue], [IMapEntry, mapEntryValue], [IFind, findValue], [ISorted, sortedValue], [IChunkedSeq, chunkedSeqValue], [ICloneable, cloneablePayload], [IVector, vectorValue], [IMap, mapValue], [ISet, setValue], [ICollection, collectionValue], [IEmptyableCollection, emptyableValue], [IStack, stackValue], [IReversible, reversibleValue], [IIterable, iterableValue], [IComparable, comparableValue], [ISeqable, seqableValue], [ISeq, seqValue], [INext, nextValue], [ISequential, sequential], [ICounted, countedValue], [IIndexed, indexedValue], [ILookup, lookupValue], [IAssociative, associativeValue], [IMeta, metaValue], [IWithMeta, withMetaValue], [IAtom, atomValue], [IPending, pendingValue], [IDeref, derefValue], [IVolatile, volatileValue], [IWatchable, watchableValue], [IWriter, writerValue]]);
  //@unit maps.records effect: recordMethods when: maps requires: maps, recordTypes
  // A record is a map without entries of its own until `updatedRecord` gives
  // it some, so constructing one writes no store: these answer for it, with
  // `none` as its entries (a record type is a function). `none` is sealed,
  // not frozen: the wrapper captures it.
  maps.has = (has => value => has.call(maps, value) || recordTypes.has(value))(maps.has);
  maps.get = ((get, none) => value => get.call(maps, value) ?? (recordTypes.get(value) && none))(maps.get, __rt.seal([]));
  //@unit PersistentHashSetValue.statics effect: PersistentHashSetValue when: typeStatics requires: makeSet,
  //@ PersistentHashSetValue
  // The empty statics of the kinds the runtime still builds as its own
  // objects (sets, maps, queues; design point 8) are task-owned, so each
  // is a getter that builds the task's own on first read.
  Object.defineProperty(PersistentHashSetValue, "EMPTY", {configurable: true, enumerable: true,
    get() { const cells = globalThis.__lightSharedCells; return cells.emptyHashSet ??= makeSet(PersistentHashSetValue, [], "array-map"); }});
  //@unit PersistentQueueValue.statics effect: PersistentQueueValue when: reader, typeStatics
  //@ requires: PersistentQueueValue, queueFromArray
  Object.defineProperty(PersistentQueueValue, "EMPTY", {configurable: true, enumerable: true,
    get() { const cells = globalThis.__lightSharedCells; return cells.emptyQueue ??= queueFromArray([]); }});
  //@unit PersistentArrayMapValue.statics effect: PersistentArrayMapValue when: typeStatics
  //@ requires: makeMap, PersistentArrayMapValue
  Object.defineProperty(PersistentArrayMapValue, "EMPTY", {configurable: true, enumerable: true,
    get() { const cells = globalThis.__lightSharedCells; return cells.emptyArrayMap ??= makeMap(PersistentArrayMapValue, [], "array-map"); }});
  PersistentArrayMapValue.HASHMAP_THRESHOLD = 8;
  //@unit PersistentHashMapValue.statics effect: PersistentHashMapValue when: typeStatics requires: aclone,
  //@ displayStr, hashTrie, makeMap, PersistentHashMapValue, putEntry
  Object.defineProperty(PersistentHashMapValue, "EMPTY", {configurable: true, enumerable: true,
    get() { const cells = globalThis.__lightSharedCells; return cells.emptyHashMap ??= makeMap(PersistentHashMapValue, hashTrie([]), "hash-map"); }});
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
  //@unit makeKeyword.print effect: makeKeyword, makeVar, SymbolValue, symbol when: printWriterImpl
  //@ requires: keywords, kindHooks, printKind, symbols, vars, write
  // An engine keyword prints from its payload; a runtime-built symbol or
  // Var through its `toString`, as in the V8 profile.
  kindHooks.print = printKind(kindHooks.print, value => keywords.has(value) || symbols.has(value) || vars.has(value),
    (value, writer) => write(writer, keywords.has(value) ? ":" + keywords.get(value).spelling : String(value)));
  //@unit coreCells effect requires: flush, sharedTaskCells, sharedTaskDefaults
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
  // The names a task's cells own (`sharedTaskCells`): the hub's cells and
  // the defaults that are data, not registries made on first use.
  Object.defineProperty(sharedTaskDefaults, "cellNames", {value: __rt.seal([...Object.keys(coreCells),
    ...Object.keys(sharedTaskDefaults).filter(name => "value" in Object.getOwnPropertyDescriptor(sharedTaskDefaults, name))])});
  // Reached from the hub, for the tasks light-cljs-648 starts.
  Object.defineProperty(coreCells, "sharedTaskCells", {value: sharedTaskCells});
  //@exports
  return sharedProfileBoot(Object.freeze({setDeclarationDemand, declarationReceiver, declarationTransaction, applyDeclarationPatches, snapshotDeclarationPatches, restoreDeclarationPatches, compilerExceptionMessage, compilerForms, keyword, keywordFqn, get, symbol, vector, vectorFromArray, listFromArray, mapFromArray, arrayMapFromArray, mapFromArrays, mapShape, mapFromShape, hashMapShape, hashMapFromShape, setFromArray, arraySetFromArray, constantMapFromArray, constantSetFromArray, literalSetFromArray, setShape, setFromShape, queueFromArray, jsObject, makeVar, definitionVar, makeIFn, protocol, protocolPredicate, protocolMethod, metadataMethod, extendType, extendInstance, makeReify, reify, makeType, typeMasks, recordMethods, recordFromMap, Loadable, fnRest, fnApplyTo, displayStr, setMacroexpandServices, compilerFormBridge, core, inspect, hash, hashed}));
})()
