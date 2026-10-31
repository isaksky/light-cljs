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
    // Whether the task has branded a class's prototype (`ClassBrand`).
    classBrands: false,
    // The slot-name memo of `protocolSlot`: a pure function's cache, which
    // the boot task fills for the runtime's own keys (`sharedProfileBoot`)
    // and every task fills for the keys it meets.
    get protocolSlots() { return sharedTaskOwn(this, "protocolSlots", new Map()); },
    // The hashes the runtime computes for engine sequences and maps
    // (`sharedFormHashes`): the task's own cache, as the V8 profile's
    // `constructorHashes` WeakMap is, keyed by the form.
    get formHashes() { return sharedTaskOwn(this, "formHashes", new WeakMap()); },
    // The entries arrays the task built for engine maps (`sharedMapEntries`).
    get formEntries() { return sharedTaskOwn(this, "formEntries", new WeakMap()); },
    get varMetadata() { return sharedTaskOwn(this, "varMetadata", new WeakMap()); },
    get atomOwners() { return sharedTaskOwn(this, "atomOwners", new WeakMap()); },
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
    // Two brand stores (WeakMaps in the V8 profile) whose keys the boot
    // seals: a private field on a function's record or on a prototype
    // keeps the engine from sealing it, so these are registries keyed by
    // identity on the task's cells (`sharedRegistryStore`; light-cljs-666).
    // `protocolImplementations`, what `installMethods` records per value,
    // read back only by itself (to merge a later `extend-type`) and by
    // `collectionWithMeta`; its boot entries are sealed arrays of the method
    // pairs, which a task's copy reads as the Maps they were. `typeNames`,
    // the names of the types `defineType` names (`Loadable`'s). After boot
    // they are WeakMaps seeded from the boot's snapshot, so a reify or type
    // a task discards is reclaimed (light-cljs-683); the boot task's are
    // Maps, enumerable for the snapshot.
    get protocolImplementations() { return sharedTaskOwn(this, "protocolImplementations", sharedWeakRegistry("protocolImplementations")); },
    get typeNames() { return sharedTaskOwn(this, "typeNames", sharedWeakRegistry("typeNames")); },
    // The namespace registry `createNs` writes after boot, seeded with the
    // boot's `cljs.user` and `cljs.core` (light-cljs-666).
    get namespaces() { return sharedTaskOwn(this, "namespaces", sharedRegistry("namespaces")); },
    // The reader's own state (`SHARED_READER_STATE` in build.rs: the
    // declarations at the top of the `reader` unit's function, rewritten
    // to the task's cells like the module-level `let`s; light-cljs-666):
    // the stream and reader-map registries, the tag tables (the builtin
    // table filled while booting, `tags` registered into after boot; both
    // seeded from the boot's entries) and the default tag and read depth.
    get streams() { return sharedTaskOwn(this, "streams", new WeakMap()); },
    get readerMaps() { return sharedTaskOwn(this, "readerMaps", new WeakMap()); },
    get tags() { return sharedTaskOwn(this, "tags", sharedRegistry("tags")); },
    get builtinTags() { return sharedTaskOwn(this, "builtinTags", sharedRegistry("builtinTags")); },
    defaultTag: null, activeDepth: 0,
  };
  // The registries' boot entries, sealed with the rest at the end of the
  // boot (`sharedProfileBoot`); a task's registry starts as a copy.
  const sharedSnapshots = {};
  const sharedRegistry = name => new Map(sharedSnapshots[name] ?? []);
  const sharedWeakRegistry = name => sharedSnapshots[name] === undefined ? new Map() : new WeakMap(sharedSnapshots[name]);
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
  //@unit sharedRegistryStore
  // A brand store kept on the task's cells (`sharedTaskDefaults`: a
  // registry seeded with the boot's entries), with a WeakMap's `has`,
  // `get`, `set` and `delete`, for the stores whose keys the boot seals.
  function sharedRegistryStore(name) {
    const registry = () => { const cells = globalThis.__lightSharedCells; return cells[name]; };
    return {
      has: value => registry().has(value),
      get: value => registry().get(value),
      set(value, payload) { registry().set(value, payload); return this; },
      delete: value => registry().delete(value)
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
    for (const name of ["coreDispatchers", "nativeMethods", "protocolValues", "protocolFunctions", "protocolVariables", "protocolGetters", "reifySites", "protocolImplementations", "typeNames", "namespaces", "tags", "builtinTags"]) {
      sharedSnapshots[name] = Array.from(Object.getOwnPropertyDescriptor(booting, name) !== undefined ? booting[name] : [], ([key, value]) => [key, value]);
    }
    // The implementations are Maps of Maps: sealed arrays of their pairs.
    sharedSnapshots.protocolImplementations = sharedSnapshots.protocolImplementations.map(([value, implementations]) =>
      [value, Array.from(implementations, ([key, entries]) => [key, Array.from(entries)])]);
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
  //@unit sharedFormKind
  // The kind of an engine form, through the occurrence the Rust bridge
  // wraps an imported form in (docs/native-macros.md, identity and
  // provenance: a macro argument's vector, map, symbol or keyword is the
  // form behind an occurrence, whose identity export maps back to the
  // compiler's Datum), and the form itself (`sharedFormOf`): the views
  // read the imported form as they read one the runtime built, and keep
  // the occurrence where the value is kept (an item, a tail), so a value
  // returned unchanged exports with its identity (light-cljs-666). Every
  // other kind of value is its own kind, and `null` is a host value.
  const sharedFormKind = value => { const kind = __rt.formKind(value); return kind === "occurrence" ? __rt.formKind(__rt.formValue(value)) : kind; };
  const sharedFormOf = value => __rt.formKind(value) === "occurrence" ? __rt.formValue(value) : value;
  //@unit identical requires: sharedFormOf
  // Occurrences carry source identity, not a distinct language value.
  // Read the underlying reference without changing stored or returned values.
  const identical = (a, b) => sharedFormOf(a) === sharedFormOf(b);
  //@unit sharedScalar
  // Read a scalar at a computation boundary. Keep collections and named forms
  // in their occurrences, and never rewrite a stored item or a returned input.
  const sharedScalar = value => {
    if (value === null || typeof value !== "object") return value;
    const raw = __rt.formValue(value);
    return raw === null || (typeof raw !== "object" && typeof raw !== "function") ? raw : value;
  };
  //@unit sharedView requires: sharedFormKind, sharedFormOf, sharedHost
  // A brand store as a read-only view over an engine tag (design point 3):
  // `has` answers engine forms of the view's kind (`has` is given the
  // form's kind) and `get` reads the form (`get` is given the form, through
  // its occurrence, and its kind); `set` is only called by the swapped
  // constructors, so it throws. While the profile is assembled, a
  // runtime-built value of the kind takes a private store (the store's own,
  // since the view loads before `privateStore`). A task that sets
  // `globalThis.__lightSharedSetTrace` to a Map before it writes gets the
  // first writer's stack per view, for the migration list
  // (tests/bbp_macros/shared_profile.rs); otherwise a write keeps nothing.
  function sharedView(name, has, get) {
    const fallback = class extends (function (value) { return value; }) {
      #s;
      static has(value) { return sharedHost(value) && #s in value; }
      static get(value) { if (fallback.has(value)) return value.#s; }
      static set(value, payload) { if (#s in value) value.#s = payload; else new fallback(value).#s = payload; }
      static reset(value, payload) { value.#s = payload; }
    };
    const record = () => {
      const trace = globalThis.__lightSharedSetTrace;
      if (trace !== undefined && !trace.has(name)) trace.set(name, new Error().stack);
    };
    return {
      has: value => has(sharedFormKind(value)) || fallback.has(value),
      get: value => { const kind = sharedFormKind(value); return has(kind) ? get(sharedFormOf(value), kind) : fallback.get(value); },
      set(value, payload) { record(); fallback.set(value, payload); },
      reset(value, payload) { record(); fallback.reset(value, payload); },
      delete() { throw new TypeError("shared profile: " + name + ".delete"); }
    };
  }
  //@unit keywords requires: sharedView
  // A keyword is an engine keyword form; its payload is read from the form.
  const keywords = sharedView("keywords", kind => kind === "keyword", value => {
    const namespace = __rt.formNamespace(value), name = __rt.formName(value);
    return {namespace, name, spelling: namespace == null ? name : namespace + "/" + name};
  });
  //@unit makeKeyword requires: sharedScalar
  // Engine keywords compare by value and hash in the engine; `spelling` is
  // the form's.
  function makeKeyword(namespace, name, spelling) {
    return __rt.formKeyword(sharedScalar(namespace), sharedScalar(name));
  }
  //@unit keyword requires: sharedScalar
  // A keyword built at run time is fresh, as upstream's `(keyword ns name)`:
  // the V8 profile interns the literals it builds this way, which the Pants
  // build lowers to engine forms instead.
  function keyword(namespace, name) {
    return __rt.formKeyword(sharedScalar(namespace), sharedScalar(name));
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
  //@unit sharedMapEntries requires: entryHashing
  // The entries of an engine map, as the verbatim units read an array
  // map's payload: a frozen array of `{key, value, hash}` in the engine's
  // print order (`formPrintItems`), built once per task and map
  // (`formEntries` on the task's cells), since an engine map carries no
  // host payload (light-cljs-666, the maps view).
  function sharedMapEntries(form) {
    const cells = globalThis.__lightSharedCells, cache = cells.formEntries;
    let entries = cache.get(form);
    if (entries !== undefined) return entries;
    const items = __rt.formPrintItems(form), length = __rt.formCount(items);
    entries = [];
    for (let i = 0; i < length; i++) {
      const item = __rt.formNth(items, i), key = __rt.formNth(item, 0), value = __rt.formNth(item, 1);
      let cachedHash;
      entries.push(Object.freeze({key, value, get hash() { return cachedHash === undefined ? cachedHash = entryHashing.hash(key) : cachedHash; }}));
    }
    cache.set(form, Object.freeze(entries));
    return entries;
  }
  //@unit sharedMapForm requires: kindHooks
  // An engine map of `entries` (an array of `{key, value}` in order, or the
  // V8 profile's hash trie, walked in its order): an insertion-ordered map
  // for the `array-map` kind (`formLiteralMap`) and a hash map for the
  // `hash-map` kind (`formMap`), the two kinds the V8 profile keeps apart
  // (promotion at the ninth entry, `updatedMap`), so a map the real core
  // builds or updates is an engine map, which macro code's intrinsics
  // read and which publishes (light-cljs-666). Positional entries are
  // already built: preserve equal keys and undefined pairs verbatim.
  // Ordinary builders deduplicate through `putEntry` before conversion.
  function sharedMapForm(entries, kind) {
    const ordered = Array.isArray(entries) ? entries : kindHooks.hashTraversal(entries);
    if (kind === "array-map") {
      let pairs = __rt.formVector();
      for (let i = 0; i < ordered.length; i++) {
        const entry = ordered[i];
        pairs = __rt.formConj(__rt.formConj(pairs, entry.key), entry.value);
      }
      return __rt.formLiteralMap(pairs);
    }
    let value = __rt.formMap();
    for (let i = 0; i < ordered.length; i++) value = __rt.formAssoc(value, ordered[i].key, ordered[i].value);
    return value;
  }
  //@unit sharedOwnedMap requires: mapKinds, maps
  // A public constructor must return an object on which constructorSlots can
  // retain the caller's slots, metadata and cached hash. Keep its canonical
  // payload intact (including live array getters and hash tries); only the
  // constructor units use this allocation boundary during shared assembly.
  // Sorted maps use the same runtime-owned representation.
  function sharedOwnedMap(Type, entries, kind) {
    const value = Object.create(Type.prototype);
    maps.set(value, Object.freeze(entries));
    mapKinds.set(value, kind);
    return value;
  }
  //@unit makeMap requires: sharedMapForm, sharedOwnedMap, sortedComparators
  // A map of the `array-map` or `hash-map` kind is an engine map
  // (`sharedMapForm`); a sorted map keeps the V8 profile's object.
  function makeMap(Type, entries, kind, comparator) {
    if (kind !== "sorted-map") return sharedMapForm(entries, kind);
    const value = sharedOwnedMap(Type, entries, kind);
    sortedComparators.set(value, comparator);
    return value;
  }
  //@unit mapFromEntries requires: makeMap, PersistentArrayMapValue, PersistentHashMapValue
  // The V8 profile's, without building a hash trie of the entries: the
  // engine hashes them (`sharedMapForm`).
  function mapFromEntries(entries, hash) {
    return hash ? makeMap(PersistentHashMapValue, entries, "hash-map") : makeMap(PersistentArrayMapValue, entries, "array-map");
  }
  //@unit sharedOwnedSet requires: setKinds, sets
  // The set counterpart: preserve the constructor's source-map slot on a
  // wrapper without changing ordinary builders' engine-form return values.
  function sharedOwnedSet(Type, entries, kind) {
    const value = Object.create(Type.prototype);
    sets.set(value, Object.freeze(entries));
    setKinds.set(value, kind);
    return value;
  }
  //@unit makeSet requires: kindHooks, sharedOwnedSet, sortedComparators
  // A set of the `array-map` kind is a positional engine set
  // (`formLiteralSet`, in the members' order) and one of the `hash-map`
  // kind an engine hash set (`formSet`); a sorted set keeps the V8
  // profile's object (light-cljs-666).
  function makeSet(Type, entries, kind, comparator) {
    if (kind !== "sorted-set") {
      const ordered = Array.isArray(entries) ? entries : kindHooks.hashTraversal(entries);
      if (kind === "array-map") {
        let members = __rt.formVector();
        for (let i = 0; i < ordered.length; i++) members = __rt.formConj(members, ordered[i].key);
        return __rt.formLiteralSet(members);
      }
      let value = __rt.formSet();
      for (let i = 0; i < ordered.length; i++) value = __rt.formConj(value, ordered[i].key);
      return value;
    }
    const value = sharedOwnedSet(Type, entries, kind);
    sortedComparators.set(value, comparator);
    return value;
  }
  //@unit makeMap.seq effect: makeMap, treeMapConstructor, recordMethods when: first, rest, seq
  //@ requires: IndexedSeqValue, kindHooks, mapEntry, mapKinds, mapTraversal, NodeSeqType,
  //@ publicCollectionSlots, view
  // The V8 profile's: a map entry is an engine entry (`mapEntry`), so the
  // seq of an engine map crosses into macro code.
  kindHooks.seqMap = value => {
    const treeSlots = publicCollectionSlots.get(value);
    if (treeSlots && "tree" in treeSlots) return kindHooks.seqTree(treeSlots);
    const entries = mapTraversal(value), kind = mapKinds.get(value);
    return entries.length ? view(kind === "hash-map" ? NodeSeqType : kind === "sorted-map" ? kindHooks.sortedMapSeq : IndexedSeqValue,
      Object.freeze(entries.map(entry => mapEntry(entry.key, entry.value))), 0, false, kind) : null;
  };
  //@unit mapTraversal requires: collectionTraversal, mapKinds, maps, sharedFormKind
  // An engine map's entries are in its order already (`sharedMapEntries`).
  function mapTraversal(value) {
    return sharedFormKind(value) === "map" ? maps.get(value) : collectionTraversal(maps.get(value), mapKinds.get(value));
  }
  //@unit setTraversal requires: collectionTraversal, setKinds, sets, sharedFormKind
  function setTraversal(value) {
    return sharedFormKind(value) === "set" ? sets.get(value) : collectionTraversal(sets.get(value), setKinds.get(value));
  }
  //@unit collectionEntryIndex requires: entryIndex, kindHooks, mapKinds, setKinds, sharedFormKind,
  //@ sortedComparators
  // An engine map's or set's entries are positional whatever its kind.
  function collectionEntryIndex(value, entries, key) {
    const comparator = sortedComparators.get(value);
    if (comparator) return kindHooks.sortedEntryIndex(entries, key, comparator);
    const form = sharedFormKind(value), kind = form === "map" || form === "set" ? "array-map" : mapKinds.get(value) ?? setKinds.get(value);
    return kind === "array-map" ? entryIndex(entries, key) : kindHooks.hashEntryIndex(entries, key, kind);
  }
  //@unit maps requires: kindedStore, sharedFormKind, sharedFormOf, sharedMapEntries
  // A map is an engine map form (an imported one through its occurrence),
  // read as a positional map of its entries (`sharedMapEntries`, in the
  // engine's order: the verbatim units take their positional paths,
  // `collectionEntryIndex`, and never a hash trie), of the `array-map`
  // kind when it is positional (`formArrayMap`, a literal) and the
  // `hash-map` kind when the engine hashes it (`formMap`), as the V8
  // profile keeps the two apart; or a runtime-owned
  // map over the V8 profile's kinded store (`maps.store`, a property so the
  // boot seals it and its kinds): a sorted map or a record (light-cljs-666).
  // A form takes no payload: `set` on one throws.
  const maps = {
    store: kindedStore(),
    has: value => sharedFormKind(value) === "map" || maps.store.has(value),
    get: value => sharedFormKind(value) === "map" ? sharedMapEntries(sharedFormOf(value)) : maps.store.get(value),
    set(value, payload) {
      if (__rt.formKind(value) !== null) throw new TypeError("shared profile: maps.set on an engine form");
      maps.store.set(value, payload);
    },
    reset(value, payload) { maps.store.reset(value, payload); },
    kinds: {
      has: value => sharedFormKind(value) === "map" || maps.store.kinds.has(value),
      get: value => sharedFormKind(value) === "map" ? (__rt.formIsArrayMap(sharedFormOf(value)) ? "array-map" : "hash-map") : maps.store.kinds.get(value),
      set(value, kind) { maps.store.kinds.set(value, kind); return this; }
    }
  };
  //@unit sharedSetMembers requires: entryHashing
  // The members of an engine set, as the verbatim units read a positional
  // set's payload: a frozen array of `{key, hash}` in the engine's print
  // order (`formPrintItems`), built once per task and set (`formEntries`
  // on the task's cells, with the maps' entries; light-cljs-666).
  function sharedSetMembers(form) {
    const cells = globalThis.__lightSharedCells, cache = cells.formEntries;
    let members = cache.get(form);
    if (members !== undefined) return members;
    const items = __rt.formPrintItems(form), length = __rt.formCount(items);
    members = [];
    for (let i = 0; i < length; i++) {
      const key = __rt.formNth(items, i);
      let cachedHash;
      members.push(Object.freeze({key, get hash() { return cachedHash === undefined ? cachedHash = entryHashing.hash(key) : cachedHash; }}));
    }
    cache.set(form, Object.freeze(members));
    return members;
  }
  //@unit sets requires: kindedStore, sharedFormKind, sharedFormOf, sharedSetMembers
  // A set is an engine set form (an imported one through its occurrence),
  // read as a positional set of its members (`sharedSetMembers`, in the
  // engine's order; the verbatim units take their positional paths:
  // `set?`, `count`, `contains?`, `get`, `seq`, printing, equality and
  // hashing, `conj` and `disj`, which build an engine set again,
  // `makeSet`), of the `array-map` kind when it is positional
  // (`formLiteralSet`) and the `hash-map` kind when the engine hashes it
  // (`formSet`); or a runtime-owned set over the V8 profile's kinded store
  // (`sets.store`): a sorted set (light-cljs-666). A form takes no
  // payload: `set` on one throws.
  const sets = {
    store: kindedStore(),
    has: value => sharedFormKind(value) === "set" || sets.store.has(value),
    get: value => sharedFormKind(value) === "set" ? sharedSetMembers(sharedFormOf(value)) : sets.store.get(value),
    set(value, payload) {
      if (__rt.formKind(value) !== null) throw new TypeError("shared profile: sets.set on an engine form");
      sets.store.set(value, payload);
    },
    reset(value, payload) { sets.store.reset(value, payload); },
    kinds: {
      has: value => sharedFormKind(value) === "set" || sets.store.kinds.has(value),
      get: value => sharedFormKind(value) === "set" ? (__rt.formIsArrayMap(sharedFormOf(value)) ? "array-map" : "hash-map") : sets.store.kinds.get(value),
      set(value, kind) { sets.store.kinds.set(value, kind); return this; }
    }
  };
  //@unit mapEntry.vector effect: mapEntry when: makeVector requires: mapEntry
  // A map entry is an engine map entry form (the first entry of a
  // one-entry map's seq: a vector form to `formKind` and the `vectors`
  // view, equality, hashing and printing, and a map entry to
  // `cljIsMapEntry` and so to `isMapEntry`, `key` and `val`), so the
  // entries of a map's seq and of `find` cross into macro code and publish
  // (light-cljs-666). `MapEntryType.prototype` is not its prototype, as an
  // engine keyword has no `Keyword` prototype.
  mapEntry = (key, value) => __rt.cljFirst(__rt.formArrayMap(__rt.formVector(key, value)));
  //@unit sharedTreeEntry requires: mapEntries, MapEntryType, vectors
  // publicTree attaches fields to its nodes, so its allocation boundary must
  // return a runtime-owned entry. Keep the canonical builder and cache intact;
  // ordinary map entries still use the immutable engine form above.
  function sharedTreeEntry(key, value) {
    const pair = Object.create(MapEntryType.prototype);
    vectors.set(pair, {cnt: 2, shift: 5, root: [], tail: [key, value]});
    mapEntries.add(pair);
    return pair;
  }
  //@unit mapEntryValue requires: mapEntries
  const mapEntryValue = value => __rt.cljIsMapEntry(value) || mapEntries.has(value);
  //@unit entryKey requires: mapEntryValue, nth
  // `-key` and `-val` of a map entry (an engine entry, or the runtime's),
  // which are all that implement IMapEntry where no protocol
  // implementation exists.
  function entryKey(entry) {
    if (!mapEntryValue(entry)) throw new Error("No protocol method IMapEntry/-key defined for this value");
    return nth(entry, 0);
  }
  //@unit entryVal requires: mapEntryValue, nth
  function entryVal(entry) {
    if (!mapEntryValue(entry)) throw new Error("No protocol method IMapEntry/-val defined for this value");
    return nth(entry, 1);
  }
  //@unit vectors requires: sharedVectorStore, sharedView
  // A vector is an engine vector form (an engine map entry is one).
  const vectors = sharedView("vectors", kind => kind === "vector", sharedVectorStore);
  //@unit symbols requires: sharedView
  // A symbol is an engine symbol form; its payload is read from the form.
  // The two symbols the engine's forms do not carry (`sharedSymbol`) take
  // the view's fallback, with the V8 profile's prototype (light-cljs-666).
  const symbols = sharedView("symbols", kind => kind === "symbol", value => {
    const namespace = __rt.formNamespace(value), name = __rt.formName(value);
    return {namespace, name, spelling: namespace == null ? name : namespace + "/" + name};
  });
  //@unit sharedNamedSplits
  // Whether an engine keyword or symbol can carry the pair: its spelling
  // splits back into the namespace and name at its first `/` (the engine's
  // `FormName::splits_back`; `formSymbol` of another pair is an error).
  const sharedNamedSplits = (namespace, name) => namespace == null ? name === "/" || !name.includes("/") : !namespace.includes("/") && (namespace !== "" || name !== "");
  //@unit sharedSymbol requires: constructorHashes, metadataValues, sharedFormKind, sharedFormOf,
  //@ sharedNamedSplits, symbols, SymbolValue
  // A fresh engine symbol, with its metadata when that is an engine map
  // (`formWithMeta`). The engine's forms carry neither a pair that does not
  // split back (a `/` inside an unqualified name, as `(symbol nil "a/b")`
  // asks for) nor runtime-built metadata (a map of the verbatim `maps`,
  // until the maps view): such a symbol is a runtime-owned object over the
  // views' fallbacks with the V8 profile's prototype, which `symbols`,
  // equality, hashing and printing read alike, and its task's own.
  function sharedSymbol(namespace, name, metadata = null, cachedHash = null, spelling = null) {
    const canonical = namespace == null ? name : namespace + "/" + name;
    if ((spelling === null || spelling === canonical) && sharedNamedSplits(namespace, name) && (metadata == null || sharedFormKind(metadata) === "map")) {
      const value = __rt.formSymbol(namespace, name);
      if (cachedHash === null || cachedHash === __rt.formHash(value)) return metadata == null ? value : __rt.formWithMeta(value, sharedFormOf(metadata));
    }
    const value = Object.create(SymbolValue.prototype);
    symbols.set(value, Object.freeze({namespace, name, spelling: spelling === null ? canonical : spelling}));
    if (cachedHash !== null) constructorHashes.set(value, cachedHash);
    if (metadata != null) metadataValues.set(value, metadata);
    return value;
  }
  //@unit symbol requires: sharedSymbol
  function symbol(namespace, name) {
    return sharedSymbol(namespace, name);
  }
  //@unit SymbolValue requires: invalidArity, sharedSymbol
  // The positional constructor keeps the slots it is given: a spelling
  // other than the pair's or a cached hash other than the engine's makes a
  // runtime-owned symbol (`sharedSymbol`), as the V8 profile's
  // `public-symbol-slots` asserts them.
  function SymbolValue(namespace, name, spelling, cachedHash, metadata) {
    if (arguments.length !== 5) return invalidArity(arguments.length);
    return sharedSymbol(namespace, name, metadata, cachedHash, spelling);
  }
  //@unit collectionWithMeta.symbols effect: symbol, SymbolValue when: collectionWithMeta
  //@ requires: collectionWithMeta, constructorHashes, sharedFormKind, sharedSymbol, symbols
  // `with-meta` of a symbol builds a symbol of the metadata's kind
  // (`sharedSymbol`): an engine symbol, or a runtime-owned one for
  // runtime-built metadata; a runtime-owned symbol given engine metadata or
  // none is an engine symbol again, unless its spelling or cached hash is
  // its own (the positional constructor's), which the copy keeps.
  collectionWithMeta = (operation => (value, metadata) => {
    const own = __rt.formKind(value) === null;
    const data = !own || (symbols.has(value) && (metadata == null || sharedFormKind(metadata) === "map")) ? symbols.get(value) : undefined;
    return data === undefined ? operation(value, metadata)
      : sharedSymbol(data.namespace, data.name, metadata, own && constructorHashes.has(value) ? constructorHashes.get(value) : null, own ? data.spelling : null);
  })(collectionWithMeta);
  //@unit sharedNamedString requires: sharedEngineVar, sharedFormKind, sharedFormOf
  // An engine keyword's or symbol's string (an imported one's through its
  // occurrence), for `str`, `displayStr` and the printer: a form has no
  // `toString`, so `String` of one throws.
  function sharedNamedString(value) {
    if (sharedEngineVar(value)) return "#'" + sharedNamedString(__rt.structField(sharedFormOf(value), "sym"));
    const kind = sharedFormKind(value);
    if (kind !== "keyword" && kind !== "symbol") return undefined;
    value = sharedFormOf(value);
    const namespace = __rt.formNamespace(value), name = __rt.formName(value);
    return (kind === "keyword" ? ":" : "") + (namespace == null ? name : namespace + "/" + name);
  }
  //@unit str requires: kindHooks, maps, recordTypes, sequential, sets, sharedNamedString, sharedScalar
  function str(...values) { return values.map(value => { value = sharedScalar(value); return value == null ? "" : typeof value === "string" ? value : sharedNamedString(value) ?? (sequential(value) || (maps.has(value) && !recordTypes.has(value)) || sets.has(value) ? kindHooks.strCollection(value) : String(value)); }).join(""); }
  //@unit displayStr requires: sharedNamedString, sharedScalar
  const displayStr = value => { value = sharedScalar(value); return value == null ? "" : sharedNamedString(value) ?? String(value); };
  //@unit Namespace effect requires: equalPair, hash, installMethods, keyword, makeVar,
  //@ mapFromArray, mungeStr, str, symbol
  // The V8 profile's, with `toString` through `str`: the name is an engine
  // symbol, which `String` cannot take.
  function Namespace(obj, name) {
    if (!(this instanceof Namespace)) return new Namespace(obj, name);
    this.obj = obj; this.name = name;
    installMethods(this, [["IEquiv/-equiv", 2, (_self, other) => other instanceof Namespace && equalPair(name, other.name)],
      ["IHash/-hash", 1, () => hash(name)]]);
  }
  Namespace.prototype.getName = function () { return this.name; };
  Namespace.prototype.toString = function () { return str(this.name); };
  Namespace.prototype.findInternedVar = function (local) {
    if (this.obj == null) return null;
    const key = mungeStr(str(local));
    if (!Object.prototype.hasOwnProperty.call(this.obj, key)) return null;
    return makeVar(() => this.obj[key], symbol(str(this.name), str(local)), mapFromArray([keyword(null, "ns"), this]), true);
  };
  //@unit preserveMeta requires: collectionWithMeta, meta, metadataValues, sharedFormKind, sharedFormOf
  // An operation's engine result (a vector, a list) takes its input's
  // metadata as a form (`formWithMeta`; the form itself when it is the same
  // map already), since a form's metadata is given where the form is built
  // and `metadataValues.set` on one throws; runtime-built metadata (a map
  // of the verbatim `maps`, until the maps view) takes the wrapper
  // `with-meta` builds over the views' fallbacks. An input without
  // metadata gives a result without: the input may be the wrapper
  // `with-meta` of nil builds over an engine form that carries metadata,
  // which the operation's engine result keeps (light-cljs-699). A
  // runtime-owned result keeps the V8 profile's store path (light-cljs-693).
  function preserveMeta(original, result) {
    const metadata = meta(original);
    if (__rt.formKind(result) !== null) {
      if (metadata == null) return __rt.formMeta(result) == null ? result : __rt.formWithMeta(result, null);
      return sharedFormKind(metadata) === "map" ? __rt.formWithMeta(result, sharedFormOf(metadata)) : collectionWithMeta(result, metadata);
    }
    if (metadata != null || metadataValues.has(original)) metadataValues.set(result, metadata);
    return result;
  }
  //@unit varPrototype requires: ifnApply, ifnPrototype, meta, str, Var, vars
  // The V8 profile's, with `toString` through `str`: a Var's name is an
  // engine symbol, which `String` cannot take (light-cljs-692).
  const varPrototype = Object.freeze({
    constructor: Var,
    isMacro() { return vars.get(this).get.call(null).cljs$lang$macro; },
    get val() { return vars.get(this).get; },
    get sym() { return vars.get(this).symbol; },
    get _meta() { return meta(this); },
    apply(_self, args) { return ifnApply(this, args); },
    call(_self, ...args) { return ifnPrototype.call.call(this, _self, ...args); },
    toString() { return "#'" + str(vars.get(this).symbol); }
  });
  //@unit sortedComparators requires: sharedStore
  const sortedComparators = sharedStore();
  //@unit mapEntries requires: sharedSet
  const mapEntries = sharedSet();
  //@unit cells requires: arrayChunk, sharedFormHashes, sharedVectorItems, sharedView
  const cells = sharedView("cells",
    kind => kind === "cons" || kind === "chunked-cons" || kind === "list",
    (value, kind) => {
      if (kind === "list") return {kind, head: __rt.cljFirst(value), tail: __rt.cljRest(value), count: __rt.cljCount(value)};
      if (__rt.cljChunkedSeq(value)) {
        const chunk = __rt.cljChunkFirst(value);
        return {kind: "chunked-cons", head: __rt.cljFirst(value), tail: __rt.cljRest(value),
          chunk: arrayChunk(sharedVectorItems(chunk, 0, __rt.formCount(chunk))), more: __rt.cljChunkRest(value),
          chunkedState: {get hash() { return sharedFormHashes.get(value); }, set hash(hash) { sharedFormHashes.set(value, hash); }}};
      }
      return {kind: "cons", head: __rt.cljFirst(value), tail: __rt.cljRest(value)};
    });
  //@unit stringBufferWriters requires: sharedSet
  const stringBufferWriters = sharedSet();
  //@unit sharedFormHashes
  // The hash cache of the engine collections that hold values (vectors,
  // lists, conses, lazy seqs, maps, sets): the engine hashes a host object inside
  // a form by its identity, which is the runtime's hash of a function or a
  // plain object (`identityHashes`) but not of a symbol, a list of
  // `listFromArray`, a map or a Date, which the runtime hashes by value
  // while equality compares them by value, so a collection holding one
  // hashes as the runtime hashes it, through `hash` of its items
  // (`hashOrderedColl`; the map branch of `hash` over its entries,
  // light-cljs-698), and the result is cached in the task's cells
  // (`formHashes`), as the V8 profile's WeakMap caches it. A collection of
  // engine values hashes the same either way. Every other form (a keyword,
  // a symbol) hashes in the engine (`formHash`).
  const sharedFormHashes = {
    kinds: __rt.seal(["vector", "list", "cons", "chunked-cons", "lazy-seq", "map", "set"]),
    is: kind => sharedFormHashes.kinds.includes(kind),
    cache() { const cells = globalThis.__lightSharedCells; return cells.formHashes; },
    has: value => sharedFormHashes.cache().has(value),
    get: value => sharedFormHashes.cache().get(value),
    set(value, hash) { sharedFormHashes.cache().set(value, hash); }
  };
  //@unit constructorHashes requires: sharedFormHashes, sharedFormKind, sharedFormOf, sharedStore
  // The hash cache: an engine collection's hash is the runtime's, cached
  // per task (`sharedFormHashes`, keyed by the form behind an imported
  // form's occurrence); any other engine form's is computed and
  // cached by the engine (`formHash`), so the cache answers it from there
  // and drops a hash given for one; a runtime-built value keeps its own.
  const constructorHashes = (store => ({
    has(value) {
      const kind = sharedFormKind(value);
      return kind === null ? store.has(value) : sharedFormHashes.is(kind) ? sharedFormHashes.has(sharedFormOf(value)) : true;
    },
    get(value) {
      const kind = sharedFormKind(value);
      return kind === null ? store.get(value) : sharedFormHashes.is(kind) ? sharedFormHashes.get(sharedFormOf(value)) : __rt.formHash(value);
    },
    set(value, hash) {
      const kind = sharedFormKind(value);
      if (kind === null) store.set(value, hash); else if (sharedFormHashes.is(kind)) sharedFormHashes.set(sharedFormOf(value), hash);
      return this;
    }
  }))(sharedStore());
  //@unit publicCollectionSlots requires: sharedStore
  const publicCollectionSlots = sharedStore();
  //@unit vectorRoots requires: sharedStore
  const vectorRoots = sharedStore();
  //@unit canonicalVectorRoots requires: sharedStore
  const canonicalVectorRoots = sharedStore();
  //@unit metadataValues requires: sharedEngineVar, sharedFormOf, sharedStore, sharedVarMeta
  // Metadata is the engine's on an engine form (`formMeta`, of an imported
  // form through its occurrence; a form's is given where the form is
  // built, `formWithMeta`, so `set` on one throws) and the fallback's on a
  // runtime-owned value (light-cljs-666).
  const metadataValues = (store => ({
    has: value => sharedEngineVar(value) || (__rt.formKind(value) !== null ? __rt.formMeta(sharedFormOf(value)) != null : store.has(value)),
    get: value => sharedEngineVar(value) ? sharedVarMeta(value) : __rt.formKind(value) !== null ? __rt.formMeta(sharedFormOf(value)) : store.get(value),
    set(value, metadata) {
      if (__rt.formKind(value) !== null) throw new TypeError("shared profile: metadataValues.set on an engine form");
      store.set(value, metadata);
      return this;
    },
    delete: value => __rt.formKind(value) === null && store.delete(value)
  }))(sharedStore());
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
  //@unit sharedEngineVar requires: sharedFormOf
  // A Var's keyed type, also recognized for a Var another task (or the
  // compiler's declaration adapter) constructed. Reading its brand never
  // demands the Var's root.
  function sharedEngineVar(value) {
    const type = __rt.structTypeOf(sharedFormOf(value));
    return type !== null && __rt.formName(type) === "cljs.core/Var";
  }
  //@unit sharedVarMeta requires: sharedFormOf
  function sharedVarMeta(value) {
    value = sharedFormOf(value);
    const metadata = __rt.structField(value, "__metadata"), deferred = __rt.structField(value, "__deferred");
    if (deferred == null) return metadata;
    const cells = globalThis.__lightSharedCells, cache = cells.varMetadata;
    if (!cache.has(value)) cache.set(value, __rt.cljAssoc(metadata, __rt.formKeyword(null, "test"), deferred.call(null)));
    return cache.get(value);
  }
  //@unit vars requires: sharedEngineVar, sharedFormOf, sharedStore
  const vars = (store => ({
    has: value => sharedEngineVar(value) || store.has(value),
    get: value => sharedEngineVar(value) ? {get: __rt.structField(sharedFormOf(value), "val"), symbol: __rt.structField(sharedFormOf(value), "sym")} : store.get(value),
    set(value, payload) { store.set(value, payload); return this; }
  }))(sharedStore());
  //@unit collectionWithMeta.vars effect: makeVar when: collectionWithMeta
  //@ requires: collectionWithMeta, sharedEngineVar, sharedFormOf
  collectionWithMeta = (operation => (value, metadata) => sharedEngineVar(value)
    ? __rt.structNew(__rt.structTypeOf(sharedFormOf(value)), __rt.structField(sharedFormOf(value), "val"), __rt.structField(sharedFormOf(value), "sym"), metadata, null)
    : operation(value, metadata))(collectionWithMeta);
  //@unit sharedAtomOptions
  // The engine atom's auxiliary registry is shared with the compiler's
  // adapter: the "atom" slot holds [metadata validator watches]. Its
  // internal map is keyed by identity, never by language equality.
  function sharedAtomOptions(value, create = false) {
    const auxiliary = __rt.atomAux(value);
    let candidate;
    for (;;) {
      const current = __rt.atomLoad(auxiliary), found = current === null ? null : __rt.getOr(current, "atom", null);
      if (found !== null || !create) return found;
      if (candidate === undefined) candidate = __rt.atomNew(__rt.formVector(null, null, null));
      const next = __rt.assoc(current === null ? __rt.map() : current, "atom", candidate);
      if (Object.is(__rt.atomCas(auxiliary, current, next), current)) return candidate;
    }
  }
  function sharedAtomStore(value, next) {
    for (;;) {
      const current = __rt.atomLoad(value);
      if (Object.is(__rt.atomCas(value, current, next), current)) return current;
    }
  }
  function sharedAtomLane(value) {
    // Standalone profile tasks have no compiler scheduler. Macro tasks
    // use the same source-order lane and saved continuation as the port.
    if (globalThis.__lightNativeRuntime !== undefined) {
      const cells = globalThis.__lightSharedCells;
      const cell = __rt.effect(65532, cells.atomOwners.get(value) ?? 0);
      if (cell !== 0) __rt.need(cell, 5);
    }
  }
  function sharedAtomRead(value, index) {
    sharedAtomLane(value);
    const slot = sharedAtomOptions(value);
    return slot === null ? null : __rt.formNth(__rt.atomLoad(slot), index);
  }
  function sharedAtomWrite(value, index, item) {
    sharedAtomLane(value);
    const slot = sharedAtomOptions(value, true);
    for (;;) {
      const current = __rt.atomLoad(slot), next = __rt.formAssoc(current, index, item);
      if (Object.is(__rt.atomCas(slot, current, next), current)) return item;
    }
  }
  //@unit atoms requires: sharedStore
  const atoms = (store => ({
    has: value => __rt.cljIsAtom(value) || store.has(value),
    get: value => __rt.cljIsAtom(value) ? value : store.get(value),
    set(value, payload) { store.set(value, payload); return this; }
  }))(sharedStore()), volatiles = sharedStore();
  //@unit Atom requires: sharedAtomOptions, sharedAtomStore
  function Atom(state, metadata, validator, watches) {
    const value = __rt.atomNew(state);
    if (globalThis.__lightNativeRuntime !== undefined) {
      const cells = globalThis.__lightSharedCells, owner = __rt.effect(65532, -1);
      if (owner !== 0) cells.atomOwners.set(value, owner);
    }
    if (metadata != null || validator != null || watches != null)
      sharedAtomStore(sharedAtomOptions(value, true), __rt.formVector(metadata, validator, watches));
    return value;
  }
  //@unit sharedAtomOperations effect: Atom
  //@ requires: addWatchProtocol, alterMeta, apply, assoc, derefProtocol, dissoc, first, getValidator,
  //@ meta, next, notifyWatches, nth, removeWatchProtocol, resetAtom, resetMeta, rest, seq,
  //@ setValidator, sharedAtomLane, sharedAtomRead, sharedAtomStore, sharedAtomWrite, sharedScalar, swapState,
  //@ toArray, truth, vector
  derefProtocol = (operation => value => {
    if (!__rt.cljIsAtom(value)) return operation(value);
    sharedAtomLane(value); return __rt.atomLoad(value);
  })(derefProtocol);
  meta = (operation => value => __rt.cljIsAtom(value) ? sharedAtomRead(value, 0) : operation(value))(meta);
  resetMeta = (operation => (value, metadata) => __rt.cljIsAtom(value) ? sharedAtomWrite(value, 0, metadata) : operation(value, metadata))(resetMeta);
  alterMeta = (operation => (value, fn, ...args) => __rt.cljIsAtom(value) ? resetMeta(value, apply(fn, meta(value), args)) : operation(value, fn, ...args))(alterMeta);
  getValidator = (operation => value => __rt.cljIsAtom(value) ? sharedAtomRead(value, 1) : operation(value))(getValidator);
  setValidator = (operation => (value, validator) => {
    if (!__rt.cljIsAtom(value)) return operation(value, validator);
    if (sharedScalar(validator) != null && !truth(validator.call(null, derefProtocol(value)))) throw new Error("Validator rejected reference state");
    return sharedAtomWrite(value, 1, validator);
  })(setValidator);
  notifyWatches = (operation => (value, oldState, state) => {
    if (!__rt.cljIsAtom(value)) return operation(value, oldState, state);
    const watches = sharedAtomRead(value, 2);
    for (let s = seq(watches); s !== null; s = next(s)) {
      const entry = first(s); nth(entry, 1).call(null, nth(entry, 0), value, oldState, state);
    }
    return null;
  })(notifyWatches);
  addWatchProtocol = (operation => (value, key, fn) => {
    if (!__rt.cljIsAtom(value)) return operation(value, key, fn);
    sharedAtomWrite(value, 2, assoc(sharedAtomRead(value, 2), key, fn)); return value;
  })(addWatchProtocol);
  removeWatchProtocol = (operation => (value, key) => __rt.cljIsAtom(value)
    ? sharedAtomWrite(value, 2, dissoc(sharedAtomRead(value, 2), key)) : operation(value, key))(removeWatchProtocol);
  resetAtom = (operation => (value, state, pair) => {
    if (!__rt.cljIsAtom(value)) return operation(value, state, pair);
    const validator = sharedAtomRead(value, 1);
    if (sharedScalar(validator) != null && !truth(validator.call(null, state))) throw new Error("Validator rejected reference state");
    const previous = sharedAtomStore(value, state);
    notifyWatches(value, previous, state);
    return pair ? vector(previous, state) : state;
  })(resetAtom);
  swapState = (operation => (value, fn, args, pair, variadic = false) => {
    if (!__rt.cljIsAtom(value)) return operation(value, fn, args, pair, variadic);
    const previous = derefProtocol(value);
    const state = variadic ? apply(fn, previous, first(args), first(next(args)), rest(next(args))) : fn.call(null, previous, ...toArray(args));
    return resetAtom(value, state, pair);
  })(swapState);
  //@unit Atom.print effect: Atom, Volatile when: printWriterImpl
  //@ requires: atoms, derefProtocol, kindHooks, printKind, printWriter, volatiles, write
  kindHooks.print = printKind(kindHooks.print, value => atoms.has(value) || volatiles.has(value), (value, writer, options) => {
    write(writer, "#object[cljs.core." + (atoms.has(value) ? "Atom" : "Volatile") + " {:val ");
    printWriter(__rt.cljIsAtom(value) ? derefProtocol(value) : value.state, writer, options); return write(writer, "}]");
  });
  //@unit lazyValues requires: sharedFormHashes, sharedView
  // A lazy payload over the engine's once-cell: `thunk` is null once the
  // cell is realized (`cljRealized`, which realizes nothing), else a
  // function that realizes the cell, so the verbatim `deferredValue` and
  // `realized?` read the cell's state; `value` is the cell's seq (realized
  // or parked on by `cljSeq`). The writes `deferredValue` makes after a
  // call are dropped: the cell holds the value. A delay's `value` is read
  // without running its body (`cljDelayState`): its seed until realized.
  // A lazy seq's `hash` is the task's cache of the runtime's hash
  // (`sharedFormHashes`), which `hashSequential` fills.
  const lazyValues = sharedView("lazyValues", kind => kind === "lazy-seq",
      value => ({
        get thunk() { return __rt.cljRealized(value) ? null : () => __rt.cljSeq(value); }, set thunk(_) {},
        get value() { return __rt.cljSeq(value); }, set value(_) {},
        get hash() { return sharedFormHashes.get(value); }, set hash(hash) { sharedFormHashes.set(value, hash); }
      })),
    // A delay's `thunk` is `false`, not null, while the cell is realized but
    // prints as pending (`cljDelayState` reads `[ready value]`): one made
    // with a `false` thunk, whose `realized?` is true (`!truth(false)`) and
    // whose printer shows `:pending` (`thunk == null`), as the V8 profile's
    // state has it. Its `value` is the state's in every case: the seed while
    // pending or after a failed realization, else the realized value.
    delays = sharedView("delays", kind => kind === "delay",
      value => ({
        get thunk() {
          if (!__rt.cljRealized(value)) return () => __rt.cljForce(value);
          return __rt.formNth(__rt.cljDelayState(value), 0) ? null : false;
        }, set thunk(_) {},
        get value() { return __rt.formNth(__rt.cljDelayState(value), 1); }, set value(_) {}
      }));
  //@unit reducedValues requires: sharedStore
  // A reduced value is the engine's marker (`cljReduced`, which macro
  // code's lowering builds for `(reduced x)`), read through
  // `cljIsReduced` and `cljUnreduced`; the store answers a runtime-owned
  // marker only until `reduced` below builds engine ones (light-cljs-666).
  const reducedValues = (store => ({
    has: value => __rt.cljIsReduced(value) || store.has(value),
    get: value => __rt.cljIsReduced(value) ? __rt.cljUnreduced(value) : store.get(value),
    set(value, payload) { store.set(value, payload); },
    reset(value, payload) { store.reset(value, payload); }
  }))(sharedStore());
  //@unit reduced
  // An engine reduced marker, which `reduce` on either side recognizes
  // and which publishes.
  function reduced(value) {
    return __rt.cljReduced(value);
  }
  //@unit eductions requires: sharedStore
  const eductions = sharedStore();
  //@unit ifnValues requires: sharedStore
  const ifnValues = sharedStore(), ifnVariadics = sharedStore();
  //@unit metaFunctions requires: sharedStore
  const metaFunctions = sharedStore();
  //@unit typeNames requires: sharedRegistryStore
  // A registry on the task's cells: a brand on a type's function would keep
  // its record from sealing (`sharedTaskDefaults`).
  const typeNames = sharedRegistryStore("typeNames");
  //@unit typeFields requires: sharedStore
  const typeFields = sharedStore();
  //@unit recordTypes requires: sharedStore
  const recordTypes = sharedStore();
  //@unit typeFields.brands effect: TypeBrand when: typeFields
  //@ requires: sharedFormKind, typeFields, TypeBrand
  typeFields.has = value => ["struct", "record"].includes(sharedFormKind(value)) || TypeBrand.type(value) !== undefined;
  //@unit recordTypes.brands effect: recordMethods when: recordTypes
  //@ requires: recordTypes, sharedFormKind, sharedFormOf, TypeBrand
  recordTypes.has = value => sharedFormKind(value) === "record" || !!TypeBrand.type(value);
  recordTypes.get = value => sharedFormKind(value) === "record" ? __rt.structTypeOf(sharedFormOf(value)) : TypeBrand.type(value) ?? undefined;
  //@unit isRecord requires: methodKeys, protocolHooks, recordTypes, sharedFormKind
  const isRecord = value => sharedFormKind(value) === "record" || (protocolHooks.satisfies?.(methodKeys[42], value) ?? recordTypes.has(value));
  //@unit isInstance requires: Atom, Delay, MultiFn, sharedEngineVar, sharedFormKind, sharedFormOf, Var
  const isInstance = (type, value) => sharedFormKind(type) === "type" ? __rt.structTypeOf(sharedFormOf(value)) === sharedFormOf(type)
    : type === Var && sharedEngineVar(value) || type === Atom && __rt.cljIsAtom(value)
    || type === Delay && sharedFormKind(value) === "delay"
    || type === MultiFn && sharedFormKind(value) === "struct" && __rt.formName(__rt.structTypeOf(sharedFormOf(value))) === "cljs.core/MultiFn"
    || value instanceof type;
  //@unit sharedRecordOperations effect: recordMethods
  //@ requires: assoc, collectionWithMeta, dissoc, get, seq, sharedFormKind, sharedFormOf
  // Engine records already own their fields, extension map and metadata.
  // Preserve that representation on updates, leaving dispatch registration
  // (including user methods on structs) to the frontier tables of 649.
  get = (operation => function (value, key, fallback) {
    return sharedFormKind(value) === "record" && (arguments.length === 2 || arguments.length === 3)
      ? __rt.cljGet(sharedFormOf(value), key, arguments.length === 2 ? null : fallback) : operation(...arguments);
  })(get);
  assoc = (operation => function (value, key, item, ...pairs) {
    if (sharedFormKind(value) !== "record") return operation(...arguments);
    let result = __rt.cljAssoc(sharedFormOf(value), key, item);
    for (let i = 0; i < pairs.length; i += 2) result = __rt.cljAssoc(result, pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null);
    return result;
  })(assoc);
  dissoc = (operation => function (value, ...keys) {
    if (sharedFormKind(value) !== "record") return operation(...arguments);
    let result = value;
    for (const key of keys) result = __rt.cljDissoc(result, key);
    return result;
  })(dissoc);
  seq = (operation => value => sharedFormKind(value) === "record" ? __rt.cljSeq(sharedFormOf(value)) : operation(value))(seq);
  collectionWithMeta = (operation => (value, metadata) => sharedFormKind(value) === "record"
    ? __rt.cljWithMeta(sharedFormOf(value), metadata) : operation(value, metadata))(collectionWithMeta);
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
  //@unit cons requires: cells, count, emptyList, ListValue, methodKeys, protocolHooks, seq,
  //@ seqValue, sharedFormKind, sharedSeqForm
  // A tail that is not a seq (an array, a vector, an invalid object) is
  // normalized through `seq` now, as the V8 profile's `cons` has it, so a
  // later change to it is not seen and an invalid one throws here; a seq
  // tail is kept as it is. One that normalizes to nil (an empty array,
  // vector or string) keeps the cons kind, an engine cons over the empty
  // list, not counted, as the V8 profile's `Cons` with a nil tail is; only
  // an originally nil tail makes a counted list (light-cljs-682). A list
  // (`conj` onto a list asks for one) stays a counted list: the engine's own
  // prepend onto an engine list, a `ListValue` of the runtime's with its
  // count onto a runtime-owned one. A cons onto a runtime-owned tail goes
  // through `sharedSeqForm`.
  function cons(head, tail, kind = "cons") {
    if (kind === "cons" && tail != null && !(protocolHooks.implements?.(methodKeys[49], tail) ?? seqValue(tail))) {
      tail = seq(tail);
      if (tail == null) return __rt.cljCons(head, emptyList);
    }
    if (tail == null) return __rt.formList(head);
    if (kind === "list") {
      if (sharedFormKind(tail) === "list") return __rt.cljConj(tail, head);
      if (count(tail) === 0) return __rt.formList(head);
      const value = Object.create(ListValue.prototype);
      cells.set(value, {kind, head, tail, count: count(tail) + 1});
      return value;
    }
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
  //@unit callable requires: fnApplyTo, fnRest
  // The V8 profile's `callable`, with the arity table it captures sealed:
  // the table is a literal of the calling unit, never written, and held
  // only by the closure (`concat` and the other combinators built while
  // booting reached it through 454 of the 748 members; light-cljs-666).
  // `fn`'s own properties are sealed by the boot's reachable seal.
  function callable(fixed, methods, variadic) {
    __rt.seal(methods);
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
  //@unit ClassBrand
  // The V8 profile's `ClassBrand`, whose "some class is branded" flag is the
  // task's cell `classBrands` instead of a private static of the class,
  // which would keep the class's record from sealing (light-cljs-666).
  const ClassBrand = class extends (class { constructor(value) { return value; } }) {
    #c;
    constructor(prototype, brand) { super(prototype); this.#c = brand; const cells = globalThis.__lightSharedCells; cells.classBrands = true; }
    static type(value) {
      const cells = globalThis.__lightSharedCells;
      if (cells.classBrands) for (let p = Object.getPrototypeOf(value), n = 64; p !== null && n--; p = Object.getPrototypeOf(p)) if (#c in p) return p.#c(value);
    }
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
  //@unit demungePattern requires: demungeCharacters
  // The source of the V8 profile's global `RegExp`, a string: a regex
  // object is exotic data a seal cannot take, and an engine regex form
  // takes no `g` flag (its `lastIndex` would be state), so `demunge` builds
  // the global regex when it is called (light-cljs-666).
  const demungePattern = Object.keys(demungeCharacters).sort((a, b) => b.length - a.length).join("|") + "|\\$";
  //@unit demunge requires: demungeCharacters, demungePattern, str, symbolCore, symbols
  function demunge(value) { let result = str(value); result = result === "_DOT__DOT_" ? ".." : result.replace(/\$$/, "").replace(new RegExp(demungePattern, "g"), key => key === "$" ? "/" : demungeCharacters[key]); return symbols.has(value) ? symbolCore(result) : result; }
  //@unit sharedIntrinsicRegExp
  const sharedIntrinsicRegExp = RegExp;
  //@unit sharedRegExpCall requires: sharedFormOf
  function sharedRegExpCall(constructor, args) {
    const values = args.map(sharedFormOf), value = constructor(...values);
    // RegExp(existing) preserves identity, including task-owned regexes.
    if (values.length && value === values[0]) return args[0];
    return value.global || value.sticky ? value : __rt.formRegex(value.source, value.flags);
  }
  //@unit sharedRegExpCallback requires: sharedIntrinsicRegExp, sharedRegExpCall
  // Adapt the constructor value at the callback boundary, leaving the
  // canonical operations' arity, receiver and sequence handling intact.
  // This is the same constructor representation the evaluator's invoke
  // adapter uses for an intrinsic RegExp passed as a function.
  function sharedRegExpCallback(operation) {
    return function (...args) {
      if (args.length && args[0] === sharedIntrinsicRegExp) args[0] = sharedRegExpFunction;
      return operation(...args);
    };
  }
  const sharedRegExpFunction = (...args) => sharedRegExpCall(sharedIntrinsicRegExp, args);
  //@unit sharedRegExpCallbacks effect: rePattern
  //@ requires: apply, applyTo, mapSeq, mapv, sharedRegExpCallback
  apply = sharedRegExpCallback(apply);
  applyTo = sharedRegExpCallback(applyTo);
  mapSeq = sharedRegExpCallback(mapSeq);
  mapv = sharedRegExpCallback(mapv);
  //@unit rePattern requires: sharedFormOf, sharedIntrinsicRegExp
  function rePattern(pattern) {
    pattern = sharedFormOf(pattern);
    if (pattern instanceof RegExp) return pattern;
    if (typeof pattern !== "string") throw new TypeError("regex pattern requires a string");
    const prefix = /^\(\?([dgimsuxy]*)\)/.exec(pattern);
    const value = new RegExp(prefix ? pattern.slice(prefix[0].length) : pattern, prefix ? prefix[1] : "");
    return RegExp !== sharedIntrinsicRegExp || value.global || value.sticky ? value : __rt.formRegex(value.source, value.flags);
  }
  //@unit reMatch requires: sharedFormOf, vectorFromArray
  function reMatch(pattern, input, full) {
    const re = sharedFormOf(pattern), text = sharedFormOf(input);
    if (typeof text !== "string") throw new TypeError("regex matching requires a string");
    const result = re.exec(text);
    if (result === null || (full && result[0] !== text)) return null;
    return result.length === 1 ? result[0] : vectorFromArray(Array.from(result));
  }
  //@unit reSeq requires: cons, lazySequence, sharedFormOf, vectorFromArray
  function reSeq(pattern, input) {
    const re = sharedFormOf(pattern), text = sharedFormOf(input);
    if (typeof text !== "string") throw new TypeError("regex matching requires a string");
    const match = re.exec(text);
    if (match === null) return null;
    const value = match.length === 1 ? match[0] : vectorFromArray(Array.from(match));
    // Only the offset survives in the lazy tail. Capturing exec's mutable
    // result array would prevent publication even for an engine regex.
    const offset = match.index + Math.max(1, match[0].length);
    return cons(value, lazySequence(() => offset <= text.length ? reSeq(re, text.slice(offset)) : null));
  }
  //@unit multiFns requires: sharedFormOf, sharedSet
  // Only the brand view belongs here. Mutable method/preference registries
  // and dispatch frontier installation remain light-cljs-649's.
  const multiFns = (store => ({
    has(value) { const type = __rt.structTypeOf(sharedFormOf(value)); return type !== null && __rt.formName(type) === "cljs.core/MultiFn" || store.has(value); },
    add(value) { store.add(value); return this; },
    delete: value => store.delete(value)
  }))(__rt.seal(sharedSet()));
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
  //@unit mapSeq requires: apply, arrayChunk, callable, chunkCons, chunkedSeqValue, chunkFirst,
  //@ chunkItem, chunkRest, chunkStore, cons, count, first, invalidArity, lazySequence,
  //@ listFromArray, mapManySteps, methodKeys, protocolHooks, rest, seq, transform
  // The V8 profile's `map`, with the collections it was given sealed: the
  // lazy result's thunk captures the rest-argument array, which is only
  // read, so a lazy `map` result publishes and one task's result is walked
  // by others (light-cljs-666).
  function mapSeq(fn, ...colls) {
    if (arguments.length === 0) return lazySequence(() => null);
    if (arguments.length === 1) return transform(rf => callable(2, {
      0: () => rf.call(null), 1: result => rf.call(null, result),
      2: (result, input) => rf.call(null, result, fn.call(null, input))
    }, (result, input, inputs) => rf.call(null, result, apply(fn, input, inputs))));
    if (!colls.length) return invalidArity(arguments.length);
    __rt.seal(colls);
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
  //@unit protocolKeys requires: sharedRegistryStore, sharedStore
  // `protocolImplementations` is a registry on the task's cells.
  const protocolKeys = sharedStore(), protocolImplementations = sharedRegistryStore("protocolImplementations");
  //@unit protocolNames requires: sharedStore
  // The protocol registries stay Maps until light-cljs-649 moves them to
  // engine tables (`extend-type` and `defprotocol` fill them after boot).
  const protocolNames = sharedStore(), protocolValues = new Map(), protocolFunctions = new Map(), protocolVariables = new Map(), protocolGetters = new Map();
  //@unit chunks requires: privateStore, sharedStore
  // Chunk values expose indexed/count/reduce protocols, without being seqs.
  const chunks = privateStore(), chunkBuffers = sharedStore();
  //@unit ChunkedCons requires: count, nth, sharedSeqForm
  // Keep the chunk and the lazy tail in engine storage. In particular, a
  // mapped chunk must not retain the runtime's private ChunkedCell inside
  // a lazy thunk: that value cannot cross a task boundary.
  function ChunkedCons(chunk, more, metadata, cachedHash) {
    let items = __rt.formVector();
    for (let i = 0, n = count(chunk); i < n; i++) items = __rt.formConj(items, nth(chunk, i));
    const value = __rt.cljChunkCons(items, sharedSeqForm(more));
    return metadata == null ? value : __rt.formWithMeta(value, metadata);
  }
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
  //@unit maps.records effect: recordMethods when: maps requires: maps, recordTypes, sharedFormKind, sharedFormOf, sharedMapEntries
  // A record is a map without entries of its own until `updatedRecord` gives
  // it some, so constructing one writes no store: these answer for it, with
  // `none` as its entries (a record type is a function). `none` is sealed,
  // not frozen: the wrapper captures it.
  maps.has = (has => value => has.call(maps, value) || recordTypes.has(value))(maps.has);
  maps.get = ((get, none) => value => sharedFormKind(value) === "record" ? sharedMapEntries(sharedFormOf(value)) : get.call(maps, value) ?? (recordTypes.get(value) && none))(maps.get, __rt.seal([]));
  //@unit makeMap.print effect: makeMap, recordMethods, treeMapConstructor when: printWriterImpl
  //@ requires: kindHooks, maps, mapTraversal, printKind, printMapEntries, sharedFormKind, sharedFormOf
  kindHooks.print = printKind(kindHooks.print, value => maps.has(value), (value, writer, options) =>
    printMapEntries(mapTraversal(value), writer, options, sharedFormKind(value) === "record"
      ? "#" + __rt.formName(__rt.structTypeOf(sharedFormOf(value))).replaceAll("/", ".") : ""));
  //@unit PersistentHashSetValue.statics effect: PersistentHashSetValue when: typeStatics requires: makeSet,
  //@ PersistentHashSetValue
  // The empty set and maps are engine forms (`makeSet`, `makeMap`), built
  // once at boot; the empty queue, a kind the runtime still builds as its
  // own object (design point 8), is task-owned, so it is a getter that
  // builds the task's own on first read.
  PersistentHashSetValue.EMPTY = makeSet(PersistentHashSetValue, [], "array-map");
  //@unit PersistentQueueValue.statics effect: PersistentQueueValue when: reader, typeStatics
  //@ requires: PersistentQueueValue, queueFromArray
  Object.defineProperty(PersistentQueueValue, "EMPTY", {configurable: true, enumerable: true,
    get() { const cells = globalThis.__lightSharedCells; return cells.emptyQueue ??= queueFromArray([]); }});
  //@unit PersistentArrayMapValue.statics effect: PersistentArrayMapValue when: typeStatics
  //@ requires: makeMap, PersistentArrayMapValue
  PersistentArrayMapValue.EMPTY = makeMap(PersistentArrayMapValue, [], "array-map");
  PersistentArrayMapValue.HASHMAP_THRESHOLD = 8;
  //@unit PersistentHashMapValue.statics effect: PersistentHashMapValue when: typeStatics requires: aclone,
  //@ displayStr, makeMap, PersistentHashMapValue, putEntry
  // The V8 profile's, without the hash tries: the engine hashes the entries.
  PersistentHashMapValue.EMPTY = makeMap(PersistentHashMapValue, [], "hash-map");
  PersistentHashMapValue.fromArray = (array, noClone) => {
    const items = noClone ? array : aclone(array), entries = [], length = items.length;
    const edit = {};
    for (let i = 0; i < length; i += 2) putEntry(entries, items[i], items[i + 1], "hash-map", edit);
    return makeMap(PersistentHashMapValue, entries, "hash-map");
  };
  PersistentHashMapValue.fromArrays = (keys, values) => {
    const length = keys.length, entries = [], edit = {};
    for (let i = 0; i < length; i++) {
      if (values.length <= i) throw new Error("No value supplied for key: " + displayStr(keys[i]));
      putEntry(entries, keys[i], values[i], "hash-map", edit);
    }
    return makeMap(PersistentHashMapValue, entries, "hash-map");
  };
  //@unit makeKeyword.print effect: makeKeyword, makeVar, SymbolValue, symbol when: printWriterImpl
  //@ requires: keywords, kindHooks, printKind, sharedNamedString, symbols, vars, write
  // An engine keyword or symbol prints from its payload; a runtime-built
  // symbol or a Var through its `toString`, as in the V8 profile.
  kindHooks.print = printKind(kindHooks.print, value => keywords.has(value) || symbols.has(value) || vars.has(value),
    (value, writer) => write(writer, keywords.has(value) ? ":" + keywords.get(value).spelling : symbols.has(value) ? symbols.get(value).spelling : sharedNamedString(value) ?? String(value)));
  //@unit isArray requires: sharedFormOf
  // Reader-created #js arrays and imported literal occurrences are engine
  // descriptors; ordinary JavaScript arrays keep the host predicate.
  const isArray = x => {
    x = sharedFormOf(x);
    return __rt.formKind(x) === "compiler-literal" ? __rt.formName(x) === "js-array" : Array.isArray(x);
  };
  //@unit compilerForms compile-time effect requires: readerJsLiterals,
  //@ vectorFromArray
  // The V8 form bridge's `decode`/`encode` belong to the V8 evaluator;
  // the Pants evaluator has its own bridge and never uses them. A
  // `#js` literal the reader reads for a macro is an engine compiler
  // literal, as the Pants build's reader builds one (src/bbp_runtime.js,
  // `jsArray` and `jsObject`), not the bridge's branded array or object:
  // the bridge's WeakMap stood behind every reader entry point
  // (`core.read`, `core.readString`, the tag tables; light-cljs-666). The
  // object keeps JavaScript own-property order and last writes, drops
  // `__proto__`, and its payload is a positional map of string keys in the
  // reader's order (`formLiteralMap`, `formReaderCollection`), as the
  // port's is.
  const compilerForms = {};
  compilerForms.jsArray = readerJsLiterals.array = items => __rt.formCompilerLiteral("js-array", vectorFromArray(Array.from(items)));
  compilerForms.jsObject = readerJsLiterals.object = pairs => {
    const object = {};
    for (let i = 0; i < pairs.length; i += 2) if (pairs[i] !== "__proto__") Object.defineProperty(object, pairs[i], {value: pairs[i + 1], enumerable: true, configurable: true});
    const flat = Object.keys(object).flatMap(key => [key, object[key]]);
    return __rt.formCompilerLiteral("js-object", __rt.formReaderCollection(__rt.formLiteralMap(vectorFromArray(flat))));
  };
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
