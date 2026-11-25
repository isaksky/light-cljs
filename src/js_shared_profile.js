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
    gensymCounter: 0, gensymLog: null, gensymScope: null, namespaceLog: null,
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
    // Cached probe of whether native `Error#stack` is an own getter.
    lazyErrorStack: undefined,
    // The slot-name memo of `protocolSlot`: a pure function's cache, which
    // the boot task fills for the runtime's own keys (`sharedProfileBoot`)
    // and every task fills for the keys it meets.
    get protocolSlots() { return sharedTaskOwn(this, "protocolSlots", new Map()); },
    // Whether the slot-first wrappers run (`slotFirst`): until the task's
    // `setDeclarationDemand` turns them off.
    slotFirst: true,
    // The hashes the runtime computes for engine sequences and maps
    // (`sharedFormHashes`): the task's own cache, keyed by the form, where
    // the V8 profile's values hold theirs in their `hash` field.
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
  //@unit storeNativeProtocols requires: sharedWords
  // None: the boot seals each resolved key's `info`, so `extendedMethod` and
  // the `satisfies` hooks could not fill its per-kind memo (`kinds`,
  // light-cljs-988) afterwards; here the native test answers every call.
  const storeNativeProtocols = sharedWords([]);
  //@unit builtinSlots
  // The runtime's own slot methods, marked with a non-enumerable keyed flag
  // while the runtime loads, before the boot's walk seals it.
  const builtinSlots = {has: method => method != null && method.builtin$LKEY !== undefined};
  function builtinSlot(method, tag = true) { Object.defineProperty(method, "builtin$LKEY", {value: tag}); return method; }
  //@unit slotFirst
  // The V8 profile's switch, whose object the boot seals, so that no
  // assignment reaches it: `on` reads the task's cells
  // (`sharedTaskDefaults.slotFirst`) and `off` writes them, so that
  // `setDeclarationDemand` turns the wrappers off in its own task only.
  const slotFirst = {
    get on() { const cells = globalThis.__lightSharedCells; return cells.slotFirst; },
    off() { const cells = globalThis.__lightSharedCells; cells.slotFirst = false; }
  };
  //@unit operationPaths requires: assocPath, conjPath, containsPath, countPath, disjPath,
  //@ dissocPath, firstPath, getPath, nextPath, nthPath, peekPath, popPath, reduceKvPath, reduceOwn,
  //@ reducePath, restPath, seqPath
  // The V8 profile's switch to the operations' own paths, which every
  // task takes for a value without the operation's slot: the engine forms
  // (vectors, map entries, maps, sets and list cells) hold no slots, and
  // the own paths answer them through this profile's units, as before
  // light-cljs-1054 (the seq walk), light-cljs-1066 (the lookups),
  // light-cljs-1068 (the updates) and light-cljs-1072 (the reductions);
  // `=` and `hash` take theirs from `equalPairOf` and `hashOf` below.
  // `nth.protocol`, `get.own`, `contains.own` and `reduce.protocol`
  // rewrap `nthPath`, `getPath`, `containsPath` and `reducePath` after this
  // unit loads, so those entries read the binding when called, with the
  // caller's arguments; `reduceOwn`, the unwrapped `reducePath`, is a
  // binding declared after it.
  const operationPaths = {seq: seqPath, first: firstPath, rest: restPath, next: nextPath,
    count: countPath, peek: peekPath,
    nth: function () { return nthPath.apply(null, arguments); },
    get: function () { return getPath.apply(null, arguments); },
    contains: function () { return containsPath.apply(null, arguments); },
    conj: conjPath, assoc: assocPath, dissoc: dissocPath, disj: disjPath, pop: popPath,
    reduce: function () { return reducePath.apply(null, arguments); },
    reduceOwn: function () { return reduceOwn.apply(null, arguments); }, reduceKv: reduceKvPath};
  //@unit equalPairOf requires: equalPairPath
  // The V8 profile's upstream-shaped `=` of a value without an `-equiv`
  // slot holding a method: here the old path (`equalPairPath`), as before
  // light-cljs-1074, whose units answer the engine forms. Its own unit, not
  // an entry of `operationPaths`, so that `=` keeps none of the other
  // operations' paths.
  function equalPairOf(left, right) { return equalPairPath(left, right); }
  //@unit hashOf requires: hashPath
  // And `hash`'s, the old path (`hashPath`).
  function hashOf(value) { return hashPath(value); }
  //@unit objectEquiv requires: builtinSlots, comparedSequential, equalPair, maps, methodKeys,
  //@ namedOf, protocolHooks, sets, structuralEqualPair
  // The V8 profile's `.equiv` as before light-cljs-1074: the engine forms
  // have no keyed kind and no `-equiv` slot, so an identical receiver is
  // told by this profile's stores, and a vector, list, map or set holding
  // NaN is still not `.equiv` to itself (the Sol review's finding).
  function objectEquiv(value, other) {
    const equiv = protocolHooks.builtin?.(value, methodKeys[18], 2);
    if (equiv && !builtinSlots.has(equiv)) return equiv.call(value, value, other);
    if (maps.has(value)) return structuralEqualPair(value, other);
    return sets.has(value) || namedOf(value) !== undefined || comparedSequential(value)
      ? structuralEqualPair(value, other) : equalPair(value, other);
  }
  //@unit ownSeqs
  // The V8 profile's switch for `first` and `rest` of a collection whose
  // seq a program extended: this profile takes the own paths
  // (`operationPaths`), which read the seq's fields, so its writes, by
  // `extend-type` and `specify!`, which the boot's seal would refuse, do
  // nothing.
  const ownSeqs = {get on() { return false; }, set on(value) {}};
  //@unit hostExt
  // In the shared-heap profile top-level objects are sealed on boot and engine
  // forms hold no protocol-mask properties; report all bits set, ignore writes,
  // and leave `protocolHooks.pred` unset so predicates route through the
  // protocol hooks and native store views.
  const hostExt = {get p0() { return -1; }, set p0(_) {}, get p1() { return -1; }, set p1(_) {}, get fn() { return true; }, set fn(_) {}, get seq() { return true; }, set seq(_) {}, imeta: {get on() { return true; }, set on(_) {}}};
  //@unit hostExt.hook effect: extendType, extendInstance when: implementsSeq, isChunkedSeq, isColl,
  //@ isCounted, isFn, isList, isMap, isMapEntry, isRecord, isSeq, isSequential, isSet, isVector
  //@ requires: protocolHooks
  protocolHooks.pred = null;
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
      key.info ??= protocolMethodInfo(key.key);
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
  //@unit keywordOf requires: sharedFormKind, sharedFormOf
  // A keyword is an engine keyword form: its fields (the V8 profile's
  // `keywordOf`) are read from the form, in a fresh object, with the
  // engine's hash. No keyword of the V8 profile's class is built here
  // (`makeKeyword`, `keyword`).
  function keywordOf(value) {
    if (sharedFormKind(value) !== "keyword") return undefined;
    const form = sharedFormOf(value), ns = __rt.formNamespace(form), name = __rt.formName(form);
    return {ns$LKEY: ns, name$LKEY: name, fqn$LKEY: ns == null ? name : ns + "/" + name, hash$LKEY: __rt.formHash(value)};
  }
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
    return {cnt$LKEY: cnt, form$LKEY: form, get tail$LKEY() { return sharedVectorItems(form, vectorTailOffset(cnt), cnt); }};
  }
  //@unit sharedVectorForm requires: vectorSlice
  // The engine vector of a store: its own, that of an engine store a view
  // covers (`vectorCellCopy`), or one of the items of a trie store (a
  // transient's, a map entry's) or of a view (a subvector's).
  function sharedVectorForm(store) {
    if (store.form$LKEY !== undefined) return store.form$LKEY;
    const sub = store.sub$LKEY;
    if (sub !== undefined && sub.form$LKEY !== undefined && store.begin$LKEY === 0 && store.cnt$LKEY === sub.cnt$LKEY) return sub.form$LKEY;
    const items = vectorSlice(store, 0, store.cnt$LKEY);
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
  //@unit sharedOwnedMap requires: flatPairs, kindHooks
  // A public constructor must return an object on which constructorSlots can
  // retain the caller's slots, metadata and cached hash. Keep its canonical
  // payload intact (including live array getters and hash tries); only the
  // constructor units use this allocation boundary during shared assembly.
  // Sorted maps use the same runtime-owned representation: the V8 profile's
  // map classes (light-cljs-1012), of the class `Type`'s unit gives it, an
  // array map with their keys and values, flat (`flatPairs`, light-cljs-1034),
  // a sorted map with its comparator and the tree of its entries
  // (`treeOfEntries`, light-cljs-1035).
  function sharedOwnedMap(Type, entries, _kind, comparator) {
    const proto = Type.prototype, kind = proto.cljs$lang$kind$$LKEY;
    return kind === 28 ? new proto.cljs$lang$cell$$LKEY(entries.length, flatPairs(entries))
      : kind === 29 ? new proto.cljs$lang$cell$$LKEY(entries.length, entries.root, entries.nil, entries.lit ?? null)
      : new proto.cljs$lang$cell$$LKEY(comparator, kindHooks.treeOfEntries(entries), entries.length);
  }
  //@unit mapCellCopy requires: comparatorOf, flatPairs, hashTrie, kindHooks, mapKinds, maps, mapTypes
  // The V8 profile's, and of an engine map (`maps`): a map of the class its
  // kind gives, holding the entries the profile reads of it (an array map
  // their keys and values, flat).
  function mapCellCopy(value) {
    const Cell = mapTypes[mapKinds.get(value)].prototype.cljs$lang$cell$$LKEY;
    const kind = value.cljs$lang$kind$$LKEY;
    if (kind === 28) return new Cell(value.cnt$LKEY, value.arr$LKEY);
    if (kind === 29) return new Cell(value.cnt$LKEY, value.root$LKEY, value.nil$LKEY, value.lit$LKEY);
    if (kind === 30) return new Cell(value.comp$LKEY, value.tree$LKEY, value.cnt$LKEY);
    const entries = maps.get(value), copied = Cell.prototype.cljs$lang$kind$$LKEY;
    if (copied === 29) { const trie = hashTrie(entries); return new Cell(trie.length, trie.root, trie.nil, trie.lit ?? null); }
    return copied === 28 ? new Cell(entries.length, flatPairs(entries)) : new Cell(comparatorOf(value), kindHooks.treeOfEntries(entries), entries.length);
  }
  //@unit sharedOccurrenceNaN requires: sharedScalar
  // Scalar occurrences normally keep their stored identity. NaN alone is
  // unequal even to itself at the canonical runtime's scalar boundary, while
  // engine key equality accepts the identical occurrence. Keep hashed entries
  // on runtime-owned collections so rebuilding cannot merge distinct slots.
  const sharedOccurrenceNaN = entry => typeof entry.key === "object" && Number.isNaN(sharedScalar(entry.key));
  //@unit makeMap requires: hashTrie, kindHooks, sharedMapForm, sharedOccurrenceNaN,
  //@ sharedOwnedMap
  // A map of the `array-map` or `hash-map` kind is an engine map
  // (`sharedMapForm`); a sorted map or a NaN occurrence key keeps the V8
  // profile's object and canonical key comparison.
  function makeMap(Type, entries, kind, comparator) {
    if (kind !== "sorted-map") {
      const ordered = Array.isArray(entries) ? entries : kindHooks.hashTraversal(entries);
      // Inspect the traversal, but retain the canonical trie or the builder
      // array's cached routing: rebuilding ordered can merge NaN occurrences.
      if (kind === "hash-map" && ordered.some(sharedOccurrenceNaN))
        return sharedOwnedMap(Type, hashTrie(entries), kind);
      return sharedMapForm(ordered, kind);
    }
    return sharedOwnedMap(Type, entries, kind, comparator);
  }
  //@unit arrayMapFromArray requires: makeMap, PersistentArrayMapValue, putEntry
  // The builders of array maps allocate through `makeMap`, as before the V8
  // profile's built its class directly (light-cljs-1034), so that an ordinary
  // array map is an engine map (`sharedMapForm`).
  function arrayMapFromArray(pairs) {
    const entries = [];
    for (let i = 0; i < pairs.length; i += 2) putEntry(entries, pairs[i], pairs[i + 1], "array-map", null);
    return makeMap(PersistentArrayMapValue, entries, "array-map");
  }
  //@unit mapFromShape requires: ArrayMapEntry, makeMap, PersistentArrayMapValue
  function mapFromShape({keys, hashes}, values) {
    const entries = [];
    for (let i = 0; i < keys.length; i++) entries.push(new ArrayMapEntry(keys[i], values[i], hashes[i]));
    return makeMap(PersistentArrayMapValue, entries, "array-map");
  }
  //@unit hashMapAssoc requires: constructorSlots, kindHooks, mapTraversal, meta,
  //@ PersistentHashMapValue, sharedOwnedMap, updatedMap
  // `assoc` of a runtime-owned hash map (a public constructor's or one with
  // a NaN occurrence key) builds through `updatedMap`, as before the V8
  // profile's built its class directly, so that the result is an engine map
  // where the profile makes one. A public constructor's map, or one updated
  // from it, whose count is its field and may differ from its entries'
  // (light-cljs-1036), gives a map of its class with that count and slots,
  // as `updatedMap` gave it before.
  function hashMapAssoc(self, key, item) {
    const cnt = self.cnt$LKEY;
    const exact = !self.slots$LKEY && cnt === mapTraversal(self).length;
    const trie = {root: self.root$LKEY, nil: self.nil$LKEY, length: cnt};
    kindHooks.putHashEntry(trie, key, item, null);
    if (exact) return updatedMap(self, trie);
    const result = sharedOwnedMap(PersistentHashMapValue, trie, "hash-map"), nil = trie.nil;
    return constructorSlots(result, {root: result, "has-nil?": nil != null, "nil-val": nil?.value}, meta(self), null);
  }
  //@unit mapFromEntries requires: makeMap, PersistentArrayMapValue, PersistentHashMapValue
  // The V8 profile's, without building a hash trie of the entries: the
  // engine hashes them (`sharedMapForm`).
  function mapFromEntries(entries, hash) {
    return hash ? makeMap(PersistentHashMapValue, entries, "hash-map") : makeMap(PersistentArrayMapValue, entries, "array-map");
  }
  //@unit arrayMapPromote requires: mapAssocEntries
  // Shared-heap promotion delegates to `mapAssocEntries` so promoted cloned
  // array maps allocate through `sharedUpdatedMapFromEntries` as engine maps
  // (Sol #4, light-cljs-1109).
  function arrayMapPromote(self, _arr, key, item) {
    return mapAssocEntries(self, key, item);
  }
  //@unit sharedUpdatedMapFromEntries requires: hashTrie, mapFromEntries, PersistentArrayMapValue,
  //@ PersistentHashMapValue, sharedOwnedMap
  // updatedMap retains public counts and slots on its result. Its allocation
  // boundary must therefore keep a wrapper, just as the public constructors do.
  function sharedUpdatedMapFromEntries(original, entries, hash) {
    if (original?.slots$LKEY?.cnt != null)
      return sharedOwnedMap(hash ? PersistentHashMapValue : PersistentArrayMapValue, hash ? hashTrie(entries) : entries, hash ? "hash-map" : "array-map");
    return mapFromEntries(entries, hash);
  }
  //@unit sharedUpdates requires: assoc, preserveMeta, sharedFormKind, sharedFormOf,
  //@ sharedMapEntries, sharedOccurrenceNaN, sharedScalar, sharedSetMembers
  // Native key operations are safe for primitive and canonical named keys.
  // NaN, composite and host-backed keys retain the canonical runtime path: a
  // runtime list, Date or user IHash/IEquiv can be equal to a form key while
  // the engine's hostless hash/equality would instead use object identity.
  // Keep the protocol checks and fallback algorithms in their original units;
  // shared assembly inserts these leaves before those units read entry arrays.
  const sharedUpdates = {
    key(value) {
      const scalar = sharedScalar(value), kind = sharedFormKind(value);
      return !Number.isNaN(scalar) && (scalar == null || typeof scalar !== "object" && typeof scalar !== "function" || kind === "keyword" || kind === "symbol");
    },
    accepts(value, keys, kind, mayPromote = false) {
      if (sharedFormKind(value) !== kind) return false;
      const form = sharedFormOf(value);
      // A safe probe can still promote an array map/set and reinsert its
      // existing NaN occurrences under the engine's identity shortcut.
      return !__rt.formHasHostKeys(form) && keys.every(sharedUpdates.key) &&
        (!mayPromote || !__rt.formIsArrayMap(form) || __rt.formCount(form) < 8 ||
          keys.every(key => __rt.formGet(form, key, sharedUpdates) !== sharedUpdates) ||
          !(kind === "map" ? sharedMapEntries(form) : sharedSetMembers(form)).some(sharedOccurrenceNaN));
    },
    assoc(value, key, item, pairs) {
      const form = sharedFormOf(value), updated = __rt.formAssoc(form, key, item);
      let result = updated === form ? value : preserveMeta(value, updated);
      for (let i = 0; i < pairs.length; i += 2)
        result = assoc(result, pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null);
      return result;
    },
    dissoc(value, keys) {
      const form = sharedFormOf(value);
      let result = form;
      for (const key of keys) result = __rt.formDissoc(result, key);
      return result === form ? value : preserveMeta(value, result);
    },
    conj(value, item) {
      const form = sharedFormOf(value), result = __rt.formConj(form, item);
      return result === form ? value : preserveMeta(value, result);
    },
    disj(value, keys) {
      const form = sharedFormOf(value);
      let result = form;
      for (const key of keys) result = __rt.formDisj(result, key);
      return result === form ? value : preserveMeta(value, result);
    },
    lookup(value, key, notFound) { return __rt.formGet(sharedFormOf(value), key, notFound); }
  };
  //@unit sharedOwnedSet
  // The set counterpart: preserve the constructor's source-map slot on a
  // wrapper without changing ordinary builders' engine-form return values.
  // A runtime-owned set is the V8 profile's class of its type
  // (`HashSetCell`, light-cljs-1038), its members frozen.
  function sharedOwnedSet(Type, entries, kind) {
    return new Type.prototype.cljs$lang$cell$$LKEY(Object.freeze(entries), kind);
  }
  //@unit setQueueCopy requires: HashSetCell, queues, setCellOf, setKinds, sets, sharedFormKind,
  //@ treeSetOf
  // The V8 profile's, and the copy `with-meta` and `clone` make of an engine
  // set: a runtime-owned set of its members and kind (`HashSetCell`), as
  // the copy its kinded store held was before light-cljs-1038.
  function setQueueCopy(value) {
    if (sharedFormKind(value) === "set") return new HashSetCell(sets.get(value), setKinds.get(value));
    const set = setCellOf(value);
    return set !== undefined ? new set.cljs$lang$cell$$LKEY(set.entries$LKEY, set.skind$LKEY)
      : treeSetOf(value) !== undefined ? new value.cljs$lang$cell$$LKEY(value.treeMap$LKEY)
      : queues.has(value) ? new value.cljs$lang$cell$$LKEY(value.arr$LKEY) : undefined;
  }
  //@unit makeSet requires: hashTrie, kindHooks, sharedOccurrenceNaN, sharedOwnedSet
  // A set of the `array-map` kind is a positional engine set
  // (`formLiteralSet`, in the members' order) and one of the `hash-map`
  // kind an engine hash set (`formSet`); a sorted set is the V8 profile's
  // class, which `makeSet` never builds (light-cljs-1038). NaN occurrence
  // members likewise keep the runtime representation so canonical
  // insertion retains every slot.
  function makeSet(Type, entries, kind) {
    const ordered = Array.isArray(entries) ? entries : kindHooks.hashTraversal(entries);
    // As with maps, the traversal must not replace the canonical routing.
    if (kind === "hash-map" && ordered.some(sharedOccurrenceNaN))
      return sharedOwnedSet(Type, hashTrie(entries), kind);
    if (kind === "array-map") {
      let members = __rt.formVector();
      for (let i = 0; i < ordered.length; i++) members = __rt.formConj(members, ordered[i].key);
      return __rt.formLiteralSet(members);
    }
    let value = __rt.formSet();
    for (let i = 0; i < ordered.length; i++) value = __rt.formConj(value, ordered[i].key);
    return value;
  }
  //@unit NodeSeqCell
  // The shared-heap profile's view classes for map seqs (`makeMap.seq`):
  // engine maps pre-build frozen arrays of engine entries so that published
  // seqs stay immutable (light-cljs-1081).
  function NodeSeqCell(arr, i, indexed, kind) { this.arr$LKEY = arr; this.i$LKEY = i; this.indexed$LKEY = indexed; this.view$LKEY = kind; /*@brand*/ }
  //@unit ArrayNodeSeqCell
  function ArrayNodeSeqCell(arr, i, indexed, kind) { this.arr$LKEY = arr; this.i$LKEY = i; this.indexed$LKEY = indexed; this.view$LKEY = kind; /*@brand*/ }
  //@unit ArrayMapSeqCell
  function ArrayMapSeqCell(arr, i, indexed, kind) { this.arr$LKEY = arr; this.i$LKEY = i; this.indexed$LKEY = indexed; this.view$LKEY = kind; /*@brand*/ }
  //@unit PersistentArrayMapSeqType effect requires: ArrayMapSeqCell, builtinType,
  //@ sequencePrototype, viewType
  const PersistentArrayMapSeqType = viewType(builtinType(function () {}, sequencePrototype), ArrayMapSeqCell, 9);
  //@unit hashMapSeqFirst requires: viewFirstSlot
  const hashMapSeqFirst = viewFirstSlot;
  //@unit hashMapSeqNext requires: builtinSlot
  const hashMapSeqNext = builtinSlot(function (self) {
    const arr = self.arr$LKEY, i = self.i$LKEY + 1;
    return i < arr.length ? new self.cljs$lang$cell$$LKEY(arr, i, self.indexed$LKEY, self.view$LKEY) : null;
  });
  //@unit hashMapSeqCountSlot requires: viewCountSlot
  const hashMapSeqCountSlot = viewCountSlot;
  //@unit hashMapSeqReduce requires: reduceSlot, reduceWalk
  function hashMapSeqReduce() { return reduceSlot(reduceWalk); }
  //@unit NodeSeqType.seqSlot effect: NodeSeqType when: seq, seqOf, seqSite requires: NodeSeqType,
  //@ viewSeqSlot
  NodeSeqType.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = viewSeqSlot;
  //@unit ArrayNodeSeqType.seqSlot effect: ArrayNodeSeqType when: seq, seqOf, seqSite
  //@ requires: ArrayNodeSeqType, viewSeqSlot
  ArrayNodeSeqType.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = viewSeqSlot;
  //@unit arrayMapSeqFirst requires: viewFirstSlot
  const arrayMapSeqFirst = viewFirstSlot;
  //@unit arrayMapSeqNext requires: hashMapSeqNext
  const arrayMapSeqNext = hashMapSeqNext;
  //@unit PersistentArrayMapSeqType.seqSlot effect: PersistentArrayMapSeqType when: seq, seqOf,
  //@ seqSite requires: PersistentArrayMapSeqType, viewSeqSlot
  PersistentArrayMapSeqType.prototype.cljs$core$ISeqable$_seq$arity$1$LKEY = viewSeqSlot;
  //@unit PersistentArrayMapSeqType.countSlot effect: PersistentArrayMapSeqType when: count,
  //@ countOf, countSite, countSlot requires: PersistentArrayMapSeqType, viewCountSlot
  PersistentArrayMapSeqType.prototype.cljs$core$ICounted$_count$arity$1$LKEY = viewCountSlot;
  //@unit PersistentArrayMapSeqType.reduceSlot effect: PersistentArrayMapSeqType when: reduce,
  //@ reduceOf requires: hashMapSeqReduce, PersistentArrayMapSeqType
  PersistentArrayMapSeqType.prototype.cljs$core$IReduce$_reduce$arity$2$LKEY = PersistentArrayMapSeqType.prototype.cljs$core$IReduce$_reduce$arity$3$LKEY = hashMapSeqReduce();
  //@unit arrayMapSeq requires: kindHooks
  function arrayMapSeq(value) { return kindHooks.seqMap(value); }
  //@unit hashMapSeq requires: kindHooks
  function hashMapSeq(value) { return kindHooks.seqMap(value); }
  //@unit makeMap.seq effect: ArrayMapCell, makeMap, treeMapConstructor, recordMethods when: first, rest, seq
  //@ requires: IndexedSeqValue, isTreeMap, kindHooks, mapEntry, mapKinds, mapTraversal,
  //@ PersistentArrayMapSeqType, view
  // The V8 profile's: a map entry is an engine entry (`mapEntry`), so the
  // seq of an engine map crosses into macro code.
  kindHooks.seqMap = value => {
    // A sorted map's seq walks its tree (light-cljs-1035).
    if (isTreeMap(value)) return kindHooks.seqTree(value, true);
    const entries = mapTraversal(value), kind = mapKinds.get(value);
    return entries.length ? view(kind === "hash-map" ? kindHooks.hashMapSeqType(value)
      : kind === "array-map" ? PersistentArrayMapSeqType : IndexedSeqValue,
      Object.freeze(entries.map(entry => mapEntry(entry.key, entry.value))), 0, false, kind) : null;
  };
  //@unit makeMap.persist effect: ArrayMapCell, makeMap, treeMapConstructor, recordMethods when: persistTransient,
  //@ TransientArrayMapType.persistentBang, TransientHashMapType.persistentBang
  //@ requires: ArrayMapEntry, kindHooks, makeMap, PersistentArrayMapValue, PersistentHashMapValue
  // The V8 profile's, through `makeMap`, so that a persisted map transient
  // is an engine map where the profile makes one, as before the transients
  // held their fields (light-cljs-1036).
  kindHooks.persistMap = value => {
    if (value.cljs$lang$kind$$LKEY === 31) {
      value.editable$LKEY = false;
      const arr = value.arr$LKEY, entries = [];
      for (let i = 0; i < arr.length; i += 2) entries.push(new ArrayMapEntry(arr[i], arr[i + 1]));
      return makeMap(PersistentArrayMapValue, entries, "array-map");
    }
    if (value.edit$LKEY) value.edit$LKEY.dead = true;
    value.edit$LKEY = null;
    return makeMap(PersistentHashMapValue, {root: value.root$LKEY, nil: value.nil$LKEY, length: value.cnt$LKEY}, "hash-map");
  };
  //@unit mapTraversal requires: arrayMapEntries, collectionTraversal, kindHooks, mapKinds, maps,
  //@ sharedFormKind
  // An engine map's entries are in its order already (`sharedMapEntries`);
  // a runtime-owned array or hash map's are of its fields, as the V8
  // profile's (light-cljs-1036).
  function mapTraversal(value) {
    if (sharedFormKind(value) === "map") return maps.get(value);
    const kind = value?.cljs$lang$kind$$LKEY;
    if (kind === 28 && value.arr$LKEY !== undefined) return arrayMapEntries(value.arr$LKEY);
    if (kind === 29 && (value.cnt$LKEY !== undefined || "cnt$LKEY" in value)) return kindHooks.hashMapTraversal(value);
    if (kind === 30 && (value.cnt$LKEY !== undefined || "cnt$LKEY" in value)) return kindHooks.treeMapEntries(value);
    return collectionTraversal(maps.get(value), mapKinds.get(value));
  }
  //@unit setTraversal requires: collectionTraversal, setKinds, sets, sharedFormKind
  function setTraversal(value) {
    return sharedFormKind(value) === "set" ? sets.get(value) : collectionTraversal(sets.get(value), setKinds.get(value));
  }
  //@unit collectionEntryIndex requires: comparatorOf, entryIndex, kindHooks, mapKinds, setKinds,
  //@ sharedFormKind
  // An engine map's or set's entries are positional whatever its kind.
  function collectionEntryIndex(value, entries, key) {
    const comparator = comparatorOf(value);
    if (comparator) return kindHooks.sortedEntryIndex(entries, key, comparator);
    const form = sharedFormKind(value), kind = form === "map" || form === "set" ? "array-map" : mapKinds.get(value) ?? setKinds.get(value);
    return kind === "array-map" ? entryIndex(entries, key) : kindHooks.hashEntryIndex(entries, key, kind);
  }
  //@unit maps requires: mapOf, sharedFormKind, sharedFormOf, sharedMapEntries
  // A map is an engine map form (an imported one through its occurrence),
  // read as a positional map of its entries (`sharedMapEntries`, in the
  // engine's order: the verbatim units take their positional paths,
  // `collectionEntryIndex`, and never a hash trie), of the `array-map`
  // kind when it is positional (`formArrayMap`, a literal) and the
  // `hash-map` kind when the engine hashes it (`formMap`), as the V8
  // profile keeps the two apart; or a runtime-owned map of the V8
  // profile's classes (`mapOf`, light-cljs-1012): a sorted map, a map a
  // public constructor built or one with a NaN occurrence key, whose
  // entries are its fields' (`mapTraversal`), which `get` gives none of
  // (light-cljs-1036, and a sorted map's since light-cljs-1035). An engine
  // record form is a map too, with its fields' and extension map's entries
  // and the kind "record", where the V8 profile's records, classes since
  // light-cljs-1037, are maps only to the native tests (`recordCells`).
  const maps = {
    has: value => { const form = sharedFormKind(value); return form === "map" || form === "record" || mapOf(value) !== undefined; },
    get(value) {
      const form = sharedFormKind(value);
      if (form === "map" || form === "record") return sharedMapEntries(sharedFormOf(value));
      return undefined;
    },
    kinds: {
      has: value => { const form = sharedFormKind(value); return form === "map" || form === "record" || mapOf(value) !== undefined; },
      get(value) {
        const form = sharedFormKind(value);
        if (form === "map") return __rt.formIsArrayMap(sharedFormOf(value)) ? "array-map" : "hash-map";
        if (form === "record") return "record";
        const kind = mapOf(value)?.cljs$lang$kind$$LKEY;
        return kind === 28 ? "array-map" : kind === 29 ? "hash-map" : kind === 30 ? "sorted-map" : undefined;
      }
    }
  };
  //@unit mapKinds requires: maps
  // The V8 profile's reads only the classes; an engine map's kind is its
  // form's (`maps.kinds`).
  const mapKinds = maps.kinds;
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
  //@unit sets requires: kindHooks, setCellOf, sharedFormKind, sharedFormOf, sharedSetMembers,
  //@ treeSetOf
  // A set is an engine set form (an imported one through its occurrence),
  // read as a positional set of its members (`sharedSetMembers`, in the
  // engine's order; the verbatim units take their positional paths:
  // `set?`, `count`, `contains?`, `get`, `seq`, printing, equality and
  // hashing, `conj` and `disj`, which build an engine set again,
  // `makeSet`), of the `array-map` kind when it is positional
  // (`formLiteralSet`) and the `hash-map` kind when the engine hashes it
  // (`formSet`); or a runtime-owned set of the V8 profile's classes
  // (light-cljs-1038): a sorted set (`TreeSetCell`, light-cljs-666), or a
  // hash set a public constructor or NaN members keep (`sharedOwnedSet`).
  const sets = {
    has: value => sharedFormKind(value) === "set" || setCellOf(value) !== undefined || treeSetOf(value) !== undefined,
    get: value => sharedFormKind(value) === "set" ? sharedSetMembers(sharedFormOf(value))
      : setCellOf(value)?.entries$LKEY ?? (treeSetOf(value) !== undefined ? kindHooks.treeSetMembers(value) : undefined),
    kinds: {
      has: value => sharedFormKind(value) === "set" || setCellOf(value) !== undefined || treeSetOf(value) !== undefined,
      get: value => sharedFormKind(value) === "set" ? (__rt.formIsArrayMap(sharedFormOf(value)) ? "array-map" : "hash-map")
        : setCellOf(value)?.skind$LKEY ?? (treeSetOf(value) !== undefined ? "sorted-set" : undefined)
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
  //@unit vectorCellCopy requires: sharedFormKind, sharedFormOf, sharedVectorStore, VectorViewCell
  // The V8 profile's, and for an engine vector, which `with-meta` of nil
  // wraps (`preserveMeta`, light-cljs-699), a view (`VectorViewCell`) of all
  // of the form's store (`sharedVectorStore`), which `sharedVectorForm`
  // gives back as the form.
  function vectorCellCopy(value) {
    if (sharedFormKind(value) === "vector") {
      const store = sharedVectorStore(sharedFormOf(value));
      return new VectorViewCell(null, store.cnt$LKEY, store, 0, null);
    }
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
  //@unit mapEntryValue requires: isEntryCell
  const mapEntryValue = value => __rt.cljIsMapEntry(value) || isEntryCell(value);
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
  //@unit vectorStore requires: sharedFormKind, sharedFormOf, sharedVectorStore
  // A vector is an engine vector form (an engine map entry is one), whose
  // store is read from the form; a vector the runtime builds as a class
  // value (a subvector's `SubvecCell`, a sorted map's `sharedTreeEntry`)
  // is its own store, as the V8 profile's are.
  function vectorStore(value) {
    if (sharedFormKind(value) === "vector") return sharedVectorStore(sharedFormOf(value));
    if (value == null) return undefined;
    const kind = value.cljs$lang$kind$$LKEY;
    return (kind === 1 || kind === 2 || kind === 3) && (value.cnt$LKEY !== undefined || "cnt$LKEY" in value) ? value : undefined;
  }
  //@unit stamped compile-time requires: sharedEngineVar, sharedFormKind, stampBrand
  // An engine vector form is the runtime's own value too: the compiler form
  // bridge reads it (`vectorStore`) without reading a property of it; so
  // are its keywords and symbols, and the engine's lazy seqs, lists and
  // conses, chunked ones among them, which `lazyOf`, `cellOf` and
  // `chunkedConsOf` read, and its maps, which `maps` reads (light-cljs-1012),
  // and its sets, reduced markers and atoms, which `sets`, `isReduced` and
  // `atoms` read (light-cljs-1013), and its Vars, which `vars` reads
  // (light-cljs-1038).
  function stamped(value) {
    const kind = sharedFormKind(value);
    return kind === "vector" || kind === "keyword" || kind === "symbol" || kind === "lazy-seq" || kind === "cons" || kind === "list" || kind === "chunked-cons" || kind === "map" || kind === "set" || __rt.cljIsReduced(value) || __rt.cljIsAtom(value) || sharedEngineVar(value) || stampBrand.has(value);
  }
  //@unit symbolOf requires: sharedFormKind, sharedFormOf
  // A symbol is an engine symbol form, whose fields are read from the form
  // as `keywordOf` reads a keyword's, or one of the symbols the engine's
  // forms do not carry (`sharedSymbol`), a symbol cell of the V8 profile
  // with its own fields (light-cljs-666).
  function symbolOf(value) {
    if (sharedFormKind(value) === "symbol") {
      const form = sharedFormOf(value), ns = __rt.formNamespace(form), name = __rt.formName(form);
      return {ns$LKEY: ns, name$LKEY: name, str$LKEY: ns == null ? name : ns + "/" + name, hash$LKEY: __rt.formHash(value)};
    }
    return value != null && value.cljs$lang$kind$$LKEY === 21 && "str$LKEY" in value ? value : undefined;
  }
  //@unit sharedNamedSplits
  // Whether an engine keyword or symbol can carry the pair: its spelling
  // splits back into the namespace and name at its first `/` (the engine's
  // `FormName::splits_back`; `formSymbol` of another pair is an error).
  const sharedNamedSplits = (namespace, name) => namespace == null ? name === "/" || !name.includes("/") : !namespace.includes("/") && (namespace !== "" || name !== "");
  //@unit sharedSymbol requires: sharedFormKind, sharedFormOf, sharedNamedSplits, SymbolCell
  // A fresh engine symbol, with its metadata when that is an engine map
  // (`formWithMeta`). The engine's forms carry neither a pair that does not
  // split back (a `/` inside an unqualified name, as `(symbol nil "a/b")`
  // asks for) nor runtime-built metadata (a map of the verbatim `maps`,
  // until the maps view): such a symbol is a runtime-owned symbol cell of
  // the V8 profile, with its own fields, its metadata among them
  // (light-cljs-1031), which `symbolOf`, `meta`, equality, hashing and
  // printing read alike, and its task's own.
  function sharedSymbol(namespace, name, metadata = null, cachedHash = null, spelling = null) {
    const canonical = namespace == null ? name : namespace + "/" + name;
    if ((spelling === null || spelling === canonical) && sharedNamedSplits(namespace, name) && (metadata == null || sharedFormKind(metadata) === "map")) {
      const value = __rt.formSymbol(namespace, name);
      if (cachedHash === null || cachedHash === __rt.formHash(value)) return metadata == null ? value : __rt.formWithMeta(value, sharedFormOf(metadata));
    }
    return new SymbolCell(namespace, name, spelling === null ? canonical : spelling, cachedHash, metadata);
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
  //@ requires: collectionWithMeta, sharedFormKind, sharedSymbol, symbolOf
  // `with-meta` of a symbol builds a symbol of the metadata's kind
  // (`sharedSymbol`): an engine symbol, or a runtime-owned one for
  // runtime-built metadata; a runtime-owned symbol given engine metadata or
  // none is an engine symbol again, unless its spelling or cached hash is
  // its own (the positional constructor's), which the copy keeps.
  collectionWithMeta = (operation => (value, metadata) => {
    const own = __rt.formKind(value) === null;
    const data = !own || (symbolOf(value) !== undefined && (metadata == null || sharedFormKind(metadata) === "map")) ? symbolOf(value) : undefined;
    return data === undefined ? operation(value, metadata)
      : sharedSymbol(data.ns$LKEY, data.name$LKEY, metadata, own ? data.hash$LKEY : null, own ? data.str$LKEY : null);
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
  //@unit str requires: kindHooks, maps, recordTypeOf, sequential, sets, sharedNamedString, sharedScalar
  function str(...values) { return values.map(value => { value = sharedScalar(value); return value == null ? "" : typeof value === "string" ? value : sharedNamedString(value) ?? (sequential(value) || (maps.has(value) && recordTypeOf(value) === undefined) || sets.has(value) ? kindHooks.strCollection(value) : String(value)); }).join(""); }
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
  //@unit preserveMeta requires: collectionWithMeta, meta, ownMeta, setMeta,
  //@ sharedFormKind, sharedFormOf
  // An operation's engine result (a vector, a list) takes its input's
  // metadata as a form (`formWithMeta`; the form itself when it is the same
  // map already), since a form's metadata is given where the form is built
  // and `setMeta` on one throws; runtime-built metadata (a map
  // of the verbatim `maps`, until the maps view) takes the wrapper
  // `with-meta` builds over the views' fallbacks. An input without
  // metadata gives a result without: the input may be the wrapper
  // `with-meta` of nil builds over an engine form that carries metadata,
  // which the operation's engine result keeps (light-cljs-699). A
  // runtime-owned result keeps the V8 profile's store path (light-cljs-693).
  function preserveMeta(original, result) {
    if (original === result) return original;
    const metadata = meta(original);
    if (__rt.formKind(result) !== null) {
      if (metadata == null) return __rt.formMeta(result) == null ? result : __rt.formWithMeta(result, null);
      return sharedFormKind(metadata) === "map" ? __rt.formWithMeta(result, sharedFormOf(metadata)) : collectionWithMeta(result, metadata);
    }
    if (metadata != null || metadata === undefined && ownMeta(original)) setMeta(result, metadata);
    return result;
  }
  //@unit varPrototype requires: ifnApply, ifnPrototype, meta, str, Var, vars
  // The V8 profile's, with `toString` through `str`: a Var's name is an
  // engine symbol, which `String` cannot take (light-cljs-692).
  const varPrototype = Object.freeze({
    constructor: Var,
    isMacro() { return vars.get(this).val$LKEY.call(null).cljs$lang$macro; },
    get val() { return vars.get(this).val$LKEY; },
    get sym() { return vars.get(this).sym$LKEY; },
    get _meta() { return meta(this); },
    apply(_self, args) { return ifnApply(this, args); },
    call(_self, ...args) { return ifnPrototype.call.call(this, _self, ...args); },
    toString() { return "#'" + str(vars.get(this).sym$LKEY); }
  });
  //@unit cellOf requires: sharedChunked, sharedFormKind, sharedFormOf
  // The engine's lists and conses, read through a view of the V8 profile's
  // keyed fields (`cellOf`): a list's `first`, `rest` and `count`, a cons's
  // `first` and `rest`; a chunked one is a chunked cons (`chunkedConsOf`),
  // as the V8 profile's chunked conses are no cells. Else the V8 profile's
  // test, which a runtime-built cell (`ListCell`) answers.
  function cellOf(value) {
    const kind = sharedFormKind(value);
    if ((kind === "cons" || kind === "list") && !sharedChunked(value)) {
      const form = sharedFormOf(value);
      return kind === "list" ? {cell$LKEY: kind, first$LKEY: __rt.cljFirst(form), rest$LKEY: __rt.cljRest(form), count$LKEY: __rt.cljCount(form)}
        : {cell$LKEY: kind, first$LKEY: __rt.cljFirst(form), rest$LKEY: __rt.cljRest(form)};
    }
    if (value == null) return undefined;
    return value.cljs$lang$kind$$LKEY !== undefined && value.cell$LKEY !== undefined ? value : undefined;
  }
  //@unit sharedChunked requires: sharedFormKind, sharedFormOf
  // Whether a value is one of the engine's chunked conses (a `chunk-cons`,
  // or a chunked `concat` or `map`).
  const sharedChunked = value => {
    const kind = sharedFormKind(value);
    return (kind === "cons" || kind === "chunked-cons") && __rt.cljChunkedSeq(sharedFormOf(value));
  };
  //@unit chunkedConsOf requires: arrayChunk, sharedChunked, sharedFormHashes, sharedFormOf,
  //@ sharedVectorItems
  // An engine chunked cons (`sharedChunked`), read through a view of the V8
  // profile's keyed fields (`ChunkedConsCell`): its chunk as a runtime
  // chunk, the seq after it, and its hash in the task's cache
  // (`sharedFormHashes`); else the V8 profile's test.
  function chunkedConsOf(value) {
    if (sharedChunked(value)) {
      const form = sharedFormOf(value), chunk = __rt.cljChunkFirst(form);
      return {chunk$LKEY: arrayChunk(sharedVectorItems(chunk, 0, __rt.formCount(chunk))), more$LKEY: __rt.cljChunkRest(form),
        get hash$LKEY() { return sharedFormHashes.get(form); }, set hash$LKEY(hash) { sharedFormHashes.set(form, hash); }};
    }
    if (value == null) return undefined;
    return value.cljs$lang$kind$$LKEY === 17 && "chunk$LKEY" in value ? value : undefined;
  }
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
  //@unit heldHash requires: sharedFormHashes, sharedFormKind, sharedFormOf
  // A value's cached hash: an engine collection's is the runtime's, cached
  // per task (`sharedFormHashes`, keyed by the form behind an imported
  // form's occurrence); any other engine form's is computed and cached by
  // the engine (`formHash`); a runtime-built value keeps its own in its
  // `hash` field (light-cljs-1053), where a store held it before.
  function heldHash(value) {
    const kind = sharedFormKind(value);
    return kind === null ? value.hash$LKEY : sharedFormHashes.is(kind) ? sharedFormHashes.get(sharedFormOf(value)) : __rt.formHash(value);
  }
  //@unit holdHash requires: sharedFormHashes, sharedFormKind, sharedFormOf
  // Caches a hash as `heldHash` reads it, and gives it: a hash given for an
  // engine form the engine hashes is dropped, and a value the boot sealed
  // keeps none, so its hash is computed again.
  function holdHash(value, hash) {
    const kind = sharedFormKind(value);
    if (kind === null) { if (!Object.isFrozen(value)) value.hash$LKEY = hash; }
    else if (sharedFormHashes.is(kind)) sharedFormHashes.set(sharedFormOf(value), hash);
    return hash;
  }
  //@unit meta requires: atoms, deferredVarMetadata, kindHooks, methodKeys, ownMeta, protocolHooks,
  //@ sharedEngineVar, sharedFormOf, sharedVarMeta
  // The V8 profile's, but an engine Var's metadata is its own
  // (`sharedVarMeta`) and an engine form's the engine's (`formMeta`, of an
  // imported form through its occurrence), as the `metadataValues` store
  // answered them before light-cljs-1053.
  function meta(value) {
    if (value == null) return null;
    const method = protocolHooks.programMethod?.(value, methodKeys[35], 1);
    if (method) return method.call(value, value);
    if (sharedEngineVar(value)) return sharedVarMeta(value);
    if (__rt.formKind(value) !== null) return __rt.formMeta(sharedFormOf(value)) ?? null;
    if (ownMeta(value)) {
      if (value.cljs$lang$kind$$LKEY === 45) { const deferred = deferredVarMetadata.get(value); if (deferred) kindHooks.metaDeferred(value, deferred); }
      return value.meta$LKEY;
    }
    return atoms.has(value) ? value.meta : null;
  }
  //@unit setMeta
  // The V8 profile's, but a form's metadata is given where the form is
  // built (`formWithMeta`), so giving an engine form metadata throws.
  function setMeta(value, metadata) {
    if (__rt.formKind(value) !== null) throw new TypeError("shared profile: setMeta on an engine form");
    if ("meta$LKEY" in value) value.meta$LKEY = metadata;
    else Object.defineProperty(value, "meta$LKEY", {value: metadata, writable: true, configurable: true});
  }
  //@unit deferredVarMetadata requires: sharedStore
  const deferredVarMetadata = sharedStore();
  //@unit identityHashes
  // The identity hashes of host objects are the engine's (`identityHash`:
  // the same in every task for the object's lifetime, never reused after
  // its collection; design point 6), so every object has one and the
  // verbatim `hash` unit's counter is never reached.
  const identityHashes = {has: () => true, get: value => __rt.identityHash(value), set() { return this; }};
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
  //@unit vars requires: sharedEngineVar, sharedFormOf
  // An engine Var, whose fields `get` gives in a fresh object of the V8
  // profile's keyed names, or a Var of the V8 profile's class (`VarCell`,
  // light-cljs-1038).
  const vars = {
    has: value => sharedEngineVar(value) || value != null && value.cljs$lang$kind$$LKEY === 45 && (value.sym$LKEY !== undefined || "sym$LKEY" in value),
    get(value) {
      if (sharedEngineVar(value)) return {val$LKEY: __rt.structField(sharedFormOf(value), "val"), sym$LKEY: __rt.structField(sharedFormOf(value), "sym")};
      if (vars.has(value)) return value;
    }
  };
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
  }))(sharedStore()),
    // A volatile is the V8 profile's class (light-cljs-1013).
    volatiles = {has: value => value != null && value.cljs$lang$kind$$LKEY === 39 && "state" in value};
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
  //@unit lazyOf requires: sharedFormHashes, sharedFormKind, sharedFormOf
  // A lazy seq over the engine's once-cell, read through a view of the V8
  // profile's keyed fields (`LazySeqCell`): `fn` is null once the cell is
  // realized (`cljRealized`, which realizes nothing), else a function that
  // realizes the cell, so the verbatim `lazyStep` and `realized?` read the
  // cell's state; `s` is the cell's seq (realized or parked on by
  // `cljSeq`). The writes `lazyStep` makes after a call are dropped: the
  // cell holds the value. Its `hash` is the task's cache of the runtime's
  // hash (`sharedFormHashes`), which `hashSequential` fills. Any other
  // value takes the V8 profile's test.
  function lazyOf(value) {
    if (sharedFormKind(value) === "lazy-seq") {
      const form = sharedFormOf(value);
      return {
        get fn$LKEY() { return __rt.cljRealized(form) ? null : () => __rt.cljSeq(form); }, set fn$LKEY(_) {},
        get s$LKEY() { return __rt.cljSeq(form); }, set s$LKEY(_) {},
        get hash$LKEY() { return sharedFormHashes.get(form); }, set hash$LKEY(hash) { sharedFormHashes.set(form, hash); }
      };
    }
    if (value == null) return undefined;
    return value.cljs$lang$kind$$LKEY === 16 && "fn$LKEY" in value ? value : undefined;
  }
  //@unit delays requires: sharedView
  // A delay over the engine's once-cell. Its `value` is read without
  // running its body (`cljDelayState`): its seed until realized.
  // A delay's `thunk` is `false`, not null, while the cell is realized but
  // prints as pending (`cljDelayState` reads `[ready value]`): one made
  // with a `false` thunk, whose `realized?` is true (`!truth(false)`) and
  // whose printer shows `:pending` (`thunk == null`), as the V8 profile's
  // state has it. Its `value` is the state's in every case: the seed while
  // pending or after a failed realization, else the realized value.
  const delays = sharedView("delays", kind => kind === "delay",
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
  //@unit isReduced requires: reducedValues
  function isReduced(value) { return typeof value === "object" && value !== null && reducedValues.has(value); }
  //@unit ifnValues requires: sharedStore
  const ifnValues = sharedStore(), ifnVariadics = sharedStore();
  //@unit typeNames requires: sharedRegistryStore
  // A registry on the task's cells: a brand on a type's function would keep
  // its record from sealing (`sharedTaskDefaults`).
  const typeNames = sharedRegistryStore("typeNames");
  //@unit recordTypeOf requires: sharedFormKind, sharedFormOf
  // The V8 profile's, which also answers an engine record form with its
  // struct type (light-cljs-1037).
  function recordTypeOf(value) {
    if (sharedFormKind(value) === "record") return __rt.structTypeOf(sharedFormOf(value));
    return value != null && value.cljs$lang$kind$$LKEY === 65 ? value.cljs$lang$record$$LKEY : undefined;
  }
  //@unit recordCells requires: sharedFormKind
  // The V8 profile's, which also answers an engine record form: the native
  // tests that exclude records from the maps (`ifnValue`, `findValue`,
  // `emptyableValue`, ...) exclude it, as `maps` holds it.
  const recordCells = Object.freeze({has: value => sharedFormKind(value) === "record" || value != null && value.cljs$lang$kind$$LKEY === 65});
  //@unit isTypeValue requires: sharedFormKind
  // The V8 profile's, which also answers an engine struct or record form.
  const isTypeValue = value => ["struct", "record"].includes(sharedFormKind(value)) || value != null && value.cljs$lang$kind$$LKEY >= 64;
  //@unit isRecord requires: methodKeys, protocolHooks, recordTypeOf, sharedFormKind
  const isRecord = value => sharedFormKind(value) === "record" || (protocolHooks.satisfies?.(methodKeys[42], value) ?? recordTypeOf(value) !== undefined);
  //@unit isInstance requires: Atom, Delay, MultiFn, sharedEngineVar, sharedFormKind, sharedFormOf, Var
  const isInstance = (type, value) => sharedFormKind(type) === "type" ? __rt.structTypeOf(sharedFormOf(value)) === sharedFormOf(type)
    : type === Var && sharedEngineVar(value) || type === Atom && __rt.cljIsAtom(value)
    || type === Delay && sharedFormKind(value) === "delay"
    || type === MultiFn && sharedFormKind(value) === "struct" && __rt.formName(__rt.structTypeOf(sharedFormOf(value))) === "cljs.core/MultiFn"
    || value instanceof type;
  //@unit sharedRecordNativeKey requires: sharedUpdates
  // Records need the same key policy as maps. Check stored keys too: a native
  // query can match a runtime-owned key, and an ordinary update can promote an
  // extension map containing repeated NaN occurrences under native equality.
  const sharedRecordNativeKey = (entries, key) => sharedUpdates.key(key) && entries.every(entry => sharedUpdates.key(entry.key));
  //@unit sharedRecordFromEntries requires: sharedFormOf, sharedMapForm
  // These entries already occupy distinct positions. Rehashing them on a
  // record update or field removal could merge identical NaN occurrences.
  function sharedRecordFromEntries(value, entries, keepRecord = true) {
    const map = sharedMapForm(entries, "array-map");
    return __rt.cljWithMeta(keepRecord ? __rt.recordFromMap(__rt.structTypeOf(sharedFormOf(value)), map) : map, __rt.cljMeta(value));
  }
  //@unit sharedRecordAssoc requires: equalPair, sharedFormOf, sharedMapEntries,
  //@ sharedRecordFromEntries, sharedRecordNativeKey
  function sharedRecordAssoc(value, key, item) {
    const entries = sharedMapEntries(sharedFormOf(value));
    if (sharedRecordNativeKey(entries, key)) return __rt.cljAssoc(value, key, item);
    const index = entries.findIndex(entry => equalPair(key, entry.key));
    if (index >= 0 && entries[index].value === item) return value;
    const updated = entries.slice(), entry = {key: index < 0 ? key : entries[index].key, value: item};
    if (index < 0) updated.push(entry); else updated[index] = entry;
    return sharedRecordFromEntries(value, updated);
  }
  //@unit sharedRecordDissoc requires: equalPair, sharedFormOf, sharedMapEntries,
  //@ sharedRecordFromEntries, sharedRecordNativeKey
  function sharedRecordDissoc(value, key) {
    const form = sharedFormOf(value), entries = sharedMapEntries(form);
    if (sharedRecordNativeKey(entries, key)) return __rt.cljDissoc(value, key);
    const index = entries.findIndex(entry => equalPair(key, entry.key));
    if (index < 0) return value;
    const fields = sharedMapEntries(__rt.structNew(__rt.structTypeOf(form)));
    const keepRecord = !fields.some(entry => equalPair(key, entry.key));
    const updated = entries.slice();
    updated.splice(index, 1);
    return sharedRecordFromEntries(value, updated, keepRecord);
  }
  //@unit sharedRecordOperations effect: recordMethods
  //@ requires: assoc, collectionWithMeta, dissoc, equalPair, get, seq, sharedFormKind, sharedFormOf,
  //@ sharedMapEntries, sharedRecordAssoc, sharedRecordDissoc, sharedRecordNativeKey
  // Engine records already own their fields, extension map and metadata.
  // Preserve that representation on updates, leaving dispatch registration
  // (including user methods on structs) to the frontier tables of 649.
  get = (operation => function (value, key, fallback) {
    if (sharedFormKind(value) !== "record" || (arguments.length !== 2 && arguments.length !== 3)) return operation(...arguments);
    const entries = sharedMapEntries(sharedFormOf(value)), notFound = arguments.length === 2 ? null : fallback;
    if (sharedRecordNativeKey(entries, key)) return __rt.cljGet(value, key, notFound);
    const index = entries.findIndex(entry => equalPair(key, entry.key));
    return index < 0 ? notFound : entries[index].value;
  })(get);
  assoc = (operation => function (value, key, item, ...pairs) {
    if (sharedFormKind(value) !== "record") return operation(...arguments);
    let result = sharedRecordAssoc(value, key, item);
    for (let i = 0; i < pairs.length; i += 2) result = sharedRecordAssoc(result, pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null);
    return result;
  })(assoc);
  dissoc = (operation => function (value, ...keys) {
    if (sharedFormKind(value) !== "record") return operation(...arguments);
    let result = value;
    for (const key of keys) result = sharedFormKind(result) === "record" ? sharedRecordDissoc(result, key) : operation(result, key);
    return result;
  })(dissoc);
  seq = (operation => value => sharedFormKind(value) === "record" ? __rt.cljSeq(sharedFormOf(value)) : operation(value))(seq);
  collectionWithMeta = (operation => (value, metadata) => sharedFormKind(value) === "record"
    ? __rt.cljWithMeta(value, metadata) : operation(value, metadata))(collectionWithMeta);
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
  //@unit cellFields requires: cellOf, ConsCell, EmptyListCell, ListCell
  // The V8 profile's cell copies (`cellCopy`), and an engine list's or
  // cons's, whose `cellOf` is a view of its fields rather than the value: a
  // runtime list or cons of those fields.
  const cellFields = Object.freeze(["cell$LKEY", "first$LKEY", "rest$LKEY", "count$LKEY", "end$LKEY", "step$LKEY", "integer$LKEY", "remaining$LKEY", "next$LKEY",
    "f$LKEY", "prev$LKEY", "seed$LKEY", "realized$LKEY", "all$LKEY", "current$LKEY", "mseq$LKEY", "stack$LKEY", "ascending$LKEY", "cnt$LKEY",
    "pairs$LKEY", "i$LKEY", "root$LKEY", "entries$LKEY"]);
  function cellCopy(value) {
    const cell = cellOf(value);
    if (cell !== value) return cell.cell$LKEY === "cons" ? new ConsCell(cell.first$LKEY, cell.rest$LKEY)
      : cell.count$LKEY === 0 ? new EmptyListCell() : new ListCell(cell.first$LKEY, cell.rest$LKEY, cell.count$LKEY);
    const Type = function () {};
    Type.prototype = Object.getPrototypeOf(value);
    return Reflect.construct(CellCopy, [value], Type);
  }
  function CellCopy(value) { for (const name of Object.keys(value)) if (cellFields.includes(name)) this[name] = value[name]; /*@brand*/ }
  //@unit cons requires: count, emptyList, ListCell, methodKeys, protocolHooks, seq,
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
      return new ListCell(head, tail, count(tail) + 1);
    }
    return __rt.cljCons(head, sharedSeqForm(tail));
  }
  function listCons(head, tail) {
    return cons(head, tail, "list");
  }
  //@unit subvec requires: count, isVector, makeVector, methodKeys, nth, protocolHooks,
  //@ vectorFromArray, vectorStore, vectorView
  // An engine vector of the items: the engine has no view kind, so the
  // result is a vector of its own, not upstream's `Subvec` with its source.
  function subvec(value, start, end = count(value)) {
    if (start == null || end == null) throw new Error("Assert failed: subvec indices cannot be nil");
    start = start | 0; end = end | 0;
    if (start < 0 || end < start || end > count(value)) throw new Error("Index out of bounds");
    if (!isVector(value)) throw new Error("subvec requires a vector");
    const store = !protocolHooks.programMethod?.(value, methodKeys[25], 2, true, true) && vectorStore(value);
    return store ? makeVector(null, vectorView(store, start, end - start))
      : vectorFromArray(Array.from({length: end - start}, (_, i) => nth(value, start + i)));
  }
  //@unit hashRoots requires: sharedStore
  const hashRoots = sharedStore();
  //@unit traversalPairs requires: sharedStore
  // The V8 profile's cache of a hash map's traversal pairs
  // (light-cljs-1034), as a store that publishes.
  const traversalPairs = sharedStore();
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
  //@unit vectorLeaf requires: sharedVectorItems, vectorSlice, vectorTailOffset
  // The leaf, or the tail, that holds index `i` of a store; of an engine
  // store, a new array of its items from index `i & -32`; of a view
  // (`vectorView`), a new array of its items from index `i & -32`.
  function vectorLeaf(store, i) {
    if (store.sub$LKEY) {
      const from = store.begin$LKEY + (i & -32);
      return vectorSlice(store.sub$LKEY, from, Math.min(from + 32, store.begin$LKEY + store.cnt$LKEY));
    }
    if (store.form$LKEY !== undefined) return sharedVectorItems(store.form$LKEY, i & -32, Math.min((i & -32) + 32, store.cnt$LKEY));
    if (i >= vectorTailOffset(store.cnt$LKEY)) return store.tail$LKEY;
    let node = store.root$LKEY;
    for (let level = store.shift$LKEY; level > 0; level -= 5) node = node[(i >>> level) & 31];
    return node;
  }
  //@unit vectorSlice requires: sharedVectorItems, vectorLeaf
  // A new array of a store's items from index `from` to `to`.
  function vectorSlice(store, from, to) {
    if (store.form$LKEY !== undefined) return sharedVectorItems(store.form$LKEY, from, to);
    const items = [];
    for (let i = from; i < to; i = (i | 31) + 1) items.push(...vectorLeaf(store, i).slice(i & 31, (i & 31) + to - i));
    return items;
  }
  //@unit vectorConj requires: sharedVectorForm, sharedVectorStore
  // `store` with `item` added: the engine's `conj`.
  function vectorConj(store, item) {
    return sharedVectorStore(__rt.formConj(sharedVectorForm(store), item));
  }
  //@unit vectorAssoc requires: sharedVectorForm, sharedVectorStore, VectorViewCell
  // `store` with `item` at its index `i`: the engine's `assoc`. The callers
  // check the bounds; a fractional index truncates, as the V8 profile's
  // bitwise indexing of its trie has it, where the engine wants an integer.
  function vectorAssoc(store, i, item) {
    if (store.sub$LKEY) return new VectorViewCell(null, store.cnt$LKEY, vectorAssoc(store.sub$LKEY, store.begin$LKEY + i, item), store.begin$LKEY, null);
    return sharedVectorStore(__rt.formAssoc(sharedVectorForm(store), Math.trunc(i), item));
  }
  //@unit vectorPop requires: sharedVectorForm, sharedVectorStore, VectorViewCell
  // `store`, of at least one item, without its last: the engine's `pop`.
  function vectorPop(store) {
    if (store.sub$LKEY) return new VectorViewCell(null, store.cnt$LKEY - 1, store.sub$LKEY, store.begin$LKEY, null);
    return sharedVectorStore(__rt.cljPop(sharedVectorForm(store)));
  }
  //@unit vectorNth requires: missing, outOfBounds, vectorLeaf
  // The item of a vector's store at a number `index`, truncated once in
  // bounds (a negative fraction keeps its fallback); a view's is that of
  // the store it views.
  function vectorNth(store, index, notFound) {
    if (index >= 0 && index < store.cnt$LKEY) {
      if (store.sub$LKEY) return vectorNth(store.sub$LKEY, store.begin$LKEY + index, notFound);
      return store.form$LKEY !== undefined ? __rt.formNth(store.form$LKEY, Math.trunc(index)) : vectorLeaf(store, index)[index & 31];
    }
    if (notFound === missing) return outOfBounds();
    return notFound;
  }
  //@unit makeVector.transient effect: makeVector, mapEntry when: asTransient requires: kindHooks,
  //@ TransientVectorCell, vectorSlice, vectorTrie, viewTrie
  // A transient vector edits a trie of the vector's items, a task-owned
  // value (design point 8), as the V8 profile's edits a copy of a non-empty
  // tail and marks owned trie nodes with a lazy `edit` token (`vectorEditor`).
  // An engine vector's freshly materialized trie owns its tail already.
  kindHooks.transientVector = store => {
    const engine = store.form$LKEY !== undefined;
    const trie = engine ? vectorTrie(vectorSlice(store, 0, store.cnt$LKEY)) : viewTrie(store);
    const cnt = trie.cnt$LKEY;
    const tail = engine ? trie.tail$LKEY : cnt ? trie.tail$LKEY.slice() : [];
    return new TransientVectorCell(cnt, trie.shift$LKEY, trie.root$LKEY, tail);
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
  //@unit ClassBrand compile-time
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
  //@unit ownedProtocolValue requires: protocolStores, storesPredicate
  // The V8 profile's reads the keyed kind (`ownedKind`, light-cljs-1046),
  // which this profile's engine forms do not have: here the stores of
  // `protocolStores` are probed in turn, as before.
  const ownedProtocolValue = storesPredicate(...protocolStores);
  //@unit seqableValue requires: chunkedConsCells, iteratorSeqs, lazyCells, maps, queues,
  //@ recordCells, seqCells, sets, storesPredicate, vectorCells, viewCells
  // The V8 profile's store inventories test the keyed kind (`kindPredicate`,
  // light-cljs-1079), as `ownedProtocolValue`: here their stores are probed
  // in turn, as before.
  const seqableValue = storesPredicate(vectorCells, maps, sets, seqCells, chunkedConsCells, viewCells, queues, lazyCells, iteratorSeqs, recordCells);
  //@unit collectionValue requires: chunkedConsCells, lazyCells, maps, queues, recordCells,
  //@ seqCells, sets, storesPredicate, vectorCells, viewCells
  const collectionValue = storesPredicate(vectorCells, maps, sets, seqCells, chunkedConsCells, viewCells, queues, lazyCells, recordCells);
  //@unit metaValue requires: atoms, chunkedConsCells, lazyCells, maps, metaFunctions, queues,
  //@ recordCells, reifyCells, seqCells, sets, storesPredicate, symbolCells, vars, vectorCells,
  //@ viewCells
  const metaValue = storesPredicate(symbolCells, vectorCells, maps, sets, seqCells, chunkedConsCells, viewCells, queues, lazyCells, vars, atoms, metaFunctions, reifyCells, recordCells);
  //@unit withMetaValue requires: chunkedConsCells, lazyCells, maps, metaFunctions, queues,
  //@ recordCells, reifyCells, seqCells, sets, storesPredicate, symbolCells, vars, vectorCells,
  //@ viewCells
  const withMetaValue = storesPredicate(symbolCells, vectorCells, maps, sets, seqCells, chunkedConsCells, viewCells, queues, lazyCells, vars, metaFunctions, reifyCells, recordCells);
  //@unit fnValue requires: metaFunctions, vars
  // This profile's `vars` also hold the engine's vars, which have no kind.
  const fnValue = value => metaFunctions.has(value) || vars.has(value);
  //@unit withMetaKeeps requires: chunkedConsCells, lazyCells, maps, queues, seqCells, sets,
  //@ storesPredicate, vectorCells, viewCells
  // The V8 profile's `copyWithMeta` tests these by kind (light-cljs-1079):
  // here the stores are probed in turn, as before.
  const withMetaKeeps = storesPredicate(vectorCells, maps, sets, seqCells, chunkedConsCells, viewCells, queues, lazyCells);
  //@unit withMetaCopies requires: maps, queues, reifyCells, seqCells, sets, storesPredicate,
  //@ symbolCells, vars, viewCells
  const withMetaCopies = storesPredicate(symbolCells, maps, sets, seqCells, viewCells, queues, vars, reifyCells);
  //@unit cloneablePayload requires: cellOf, holdsVector, isEntryCell, maps, queues, recordCells,
  //@ sets, viewOf
  // The V8 profile reads these native tests' mask bits (`maskNative`,
  // light-cljs-1079), which this profile's engine forms do not have: here
  // they test the stores and the recognisers, as before.
  function cloneablePayload(value) { return (holdsVector(value) && !isEntryCell(value)) || (maps.has(value) && !recordCells.has(value)) || sets.has(value) || queues.has(value) || ["list", "cons", "range"].includes(cellOf(value)?.cell$LKEY) || ["indexed", "reverse"].includes(viewOf(value)?.view$LKEY); }
  //@unit stackValue requires: cellOf, holdsVector, isCell, queues
  const stackValue = value => !["cycle", "iterate", "range", "tree-seq", "repeat", "array-map"].includes(cellOf(value)?.cell$LKEY) && (holdsVector(value) || isCell(value) || queues.has(value));
  //@unit reversibleValue requires: comparatorOf, holdsVector, viewOf
  const reversibleValue = value => holdsVector(value) || viewOf(value)?.view$LKEY === "indexed" || comparatorOf(value) !== undefined;
  //@unit aseqValue requires: cellOf, isChunkedCons, viewOf
  function aseqValue(value) {
    const cell = cellOf(value), kind = viewOf(value)?.view$LKEY;
    return kind === "chunked" || cell?.cell$LKEY === "cons" || (cell?.cell$LKEY === "list" && (cell.count$LKEY > 0 || cell.cljs$lang$kind$$LKEY === 12)) || isChunkedCons(value) || kind === "indexed" || kind === "hash-map";
  }
  //@unit comparableValue requires: holdsVector, namedOf, uuids
  const comparableValue = value => holdsVector(value) || uuids.has(value) || namedOf(value) !== undefined || value instanceof Date;
  //@unit pendingValue requires: cellOf, delays, isLazy
  const pendingValue = value => ["repeat", "cycle", "iterate"].includes(cellOf(value)?.cell$LKEY) || delays.has(value) || isLazy(value);
  //@unit demungePattern requires: demungeCharacters
  // The source of the V8 profile's global `RegExp`, a string: a regex
  // object is exotic data a seal cannot take, and an engine regex form
  // takes no `g` flag (its `lastIndex` would be state), so `demunge` builds
  // the global regex when it is called (light-cljs-666).
  const demungePattern = Object.keys(demungeCharacters).sort((a, b) => b.length - a.length).join("|") + "|\\$";
  //@unit demunge requires: demungeCharacters, demungePattern, str, symbolCore, symbolOf
  function demunge(value) { let result = str(value); result = result === "_DOT__DOT_" ? ".." : result.replace(/\$$/, "").replace(new RegExp(demungePattern, "g"), key => key === "$" ? "/" : demungeCharacters[key]); return symbolOf(value) !== undefined ? symbolCore(result) : result; }
  //@unit stringEscapes
  // A regex literal is exotic data a seal cannot take (light-cljs-666): keep
  // `stringEscapeTest` a sealable object delegating to a per-call regex test
  // so `printWriterImpl` publishes cleanly after `ba3efd525`.
  const stringEscapes = {'"': '\\"', '\\': '\\\\', '\b': '\\b', '\f': '\\f', '\n': '\\n', '\r': '\\r', '\t': '\\t'}, stringEscapeTest = {test: value => /[\\"\b\f\n\r\t]/.test(value)};
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
    const source = sharedFormOf(pattern);
    if (source instanceof RegExp) return pattern;
    if (typeof source !== "string") throw new TypeError("regex pattern requires a string");
    const prefix = /^\(\?([dgimsuxy]*)\)/.exec(source);
    const value = new RegExp(prefix ? source.slice(prefix[0].length) : source, prefix ? prefix[1] : "");
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
  //@unit multiFns requires: sharedFormOf
  // Only the brand view belongs here (engine structs plus class-layout
  // `MultiFn` instances, kind 41). Mutable method/preference registries and
  // dispatch frontier installation remain light-cljs-649's.
  const multiFns = {
    has(value) {
      const type = __rt.structTypeOf(sharedFormOf(value));
      return type !== null && __rt.formName(type) === "cljs.core/MultiFn"
        || value != null && value.cljs$lang$kind$$LKEY === 41 && "dispatch_fn" in value;
    }
  };
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
  // once. The engine caches the hash (`heldHash`), so `cachedHash`
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
  //@unit exceptionMessages compile-time requires: sharedStore
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
  //@unit getMethodProtocol effect requires: coreMethod, getMethod, multiFnValue, multiHooks,
  //@ protocolHooks, sharedMethodKeys
  // Shareable once booted (as `coreMethod` above): the resolved key is
  // registered in `sharedMethodKeys` so `sharedProfileBoot` fills its memos
  // and seals it, and the hook assigns no captured binding.
  const getMethodProtocol = coreMethod("IMultiFn/-get-method", [2], getMethod, multiFnValue);
  if (multiHooks.getMethodProtocol) {
    const resolved = protocolHooks.methodKey?.("IMultiFn/-get-method");
    if (resolved !== undefined) sharedMethodKeys.push(resolved);
    sharedMethodKeys.maxArity = Math.max(sharedMethodKeys.maxArity, 2);
    multiHooks.getMethodProtocol = (mf, dispatch) => {
      const method = protocolHooks.builtin?.(mf, resolved ?? protocolHooks.methodKey("IMultiFn/-get-method"), 2);
      return method != null ? method.call(mf, mf, dispatch) : multiFnValue(mf) ? getMethod(mf, dispatch) : getMethodProtocol(mf, dispatch);
    };
  }
  //@unit protocolMasks requires: protocolMaskNames, sharedMap
  // Membership checks are on every generic collection operation. Resolve the
  // fixed ABI once as [property, bit], including the last occurrence of
  // duplicate protocol names.
  const protocolMasks = sharedMap(protocolMaskNames.map((name, index) => [name,
    ["cljs$lang$protocol_mask$partition" + (index >> 5) + "$$LKEY", 1 << (index % 32)]]));
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
  //@unit makeMap.print effect: ArrayMapCell, makeMap, recordMethods, treeMapConstructor when: printWriterImpl
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
  //@unit PersistentArrayMapValue.threshold effect: PersistentArrayMapValue when: member_HASHMAP_THRESHOLD
  //@ requires: PersistentArrayMapValue
  PersistentArrayMapValue.HASHMAP_THRESHOLD = 8;
  //@unit PersistentHashMapValue.statics effect: PersistentHashMapValue when: typeStatics requires: makeMap,
  //@ PersistentHashMapValue
  // The V8 profile's, without the hash tries: the engine hashes the entries.
  PersistentHashMapValue.EMPTY = makeMap(PersistentHashMapValue, [], "hash-map");
  //@unit PersistentHashMapValue.fromArrays effect: PersistentHashMapValue when: member_fromArray,
  //@ member_fromArrays requires: aclone, displayStr, makeMap, PersistentHashMapValue, putEntry
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
  //@ requires: keywordOf, kindHooks, printKind, sharedNamedString, symbolOf, vars, write
  // A keyword or a symbol prints from its fields; a Var through its
  // `toString`, as in the V8 profile.
  kindHooks.print = printKind(kindHooks.print, value => keywordOf(value) !== undefined || symbolOf(value) !== undefined || vars.has(value),
    (value, writer) => write(writer, keywordOf(value) !== undefined ? ":" + keywordOf(value).fqn$LKEY : symbolOf(value) !== undefined ? symbolOf(value).str$LKEY : sharedNamedString(value) ?? String(value)));
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
