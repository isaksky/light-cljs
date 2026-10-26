// First-party bounded native-form runtime. Mutable JS belongs to this one task.
// Protocols and types (light-cljs-478; docs/native-macros.md, "Protocols and
// types"). A protocol method's implementations are an engine dispatch table:
// a SameValueZero persistent map from a value's dispatch key (`__rt.cljDispatch`:
// a struct's type identity, a form kind, "string", "number", ..., "_" for
// `default`) to the method, which `__rt.cljDispatch` reads without property
// lookup or user equality. A table holds the extensions installed in source
// order up to the compiler's frontier: an extension's order is its
// declaration bundle's, and code run outside a bundle extends at order 0.
const decl = (() => {
  const tables = new Map(), extensions = new Map(), views = new Map(), frontiers = [];
  let order = 0, frontier = Infinity, version = 0;
  const table = (key, protocol) => {
    let t = tables.get(key);
    if (t === undefined) tables.set(key, t = {protocol, entries: [], map: null, version: -1, frontier: NaN});
    return t;
  };
  const protocolOf = (name, arity) => arity === 0 ? name : name.slice(0, name.lastIndexOf('/'));
  const methodTable = (name, arity) => table(name + '\u0000' + arity, protocolOf(name, arity));
  // Core types that an extension names: the native form kinds.
  const coreKeys = new Map();
  const keyOf = target => {
    if (typeof target === 'string') return target;
    if (__rt.formKind(target) === 'type') return target;
    if (typeof target === 'function' && target.__lightNativeType !== undefined) return target.__lightNativeType;
    const core = coreKeys.get(target);
    if (core !== undefined) return core;
    throw new Error('unsupported operation: native protocol extension of ' + String(target));
  };
  // An extension a bundle installs has the bundle's order; one that macro
  // code runs outside any bundle is an overlay after every bundle's, in the
  // order run, visible at every frontier.
  let overlays = 0;
  // A type's `Object` methods (`Object/m`, with the munged JavaScript
  // member name as the entry's sixth item) are tables too: one per fixed
  // arity, and arity -1 for a variadic overload. `objectArities` holds each
  // method's fixed arities.
  const objectArities = new Map();
  const extend = (target, specs) => {
    const key = keyOf(target), overlay = order === 0 ? ++overlays : 0, info = views.get(key);
    for (const [name, arity, fn, , variadic, member] of specs) {
      const object = name.startsWith('Object/');
      const n = object && variadic != null ? -1 : arity;
      methodTable(name, n).entries.push({order, overlay, key, fn});
      if (!object) continue;
      let arities = objectArities.get(name);
      if (arities === undefined) objectArities.set(name, arities = new Set());
      if (n >= 0) arities.add(n);
      if (info !== undefined) objectMember(info, name, member);
    }
    version++;
    return null;
  };
  const bundles = [];
  const force = b => {
    if (b.state === 2) return;
    if (b.state === 3) throw b.error;
    if (b.state === 1) throw new Error('bundle initialization cycle: ' + (b.names[0] ?? b.location));
    const savedOrder = order, savedProducts = globalThis.__lightCompilerBundleProducts;
    b.state = 1; order = b.order;
    b.products = globalThis.__lightCompilerBundleProducts = {};
    try {
      __rt.need(b.unit, 2);
      const factory = __rt.need(b.unit, 3);
      factory(globalThis.__lightNativeRuntime);
      b.state = 2;
    } catch (e) {
      b.state = 3; b.error = e;
      if (e instanceof Error) e.message = b.location + ': ' + (b.names[0] ?? 'extension') + ': ' + e.message;
      throw e;
    } finally {
      order = savedOrder; globalThis.__lightCompilerBundleProducts = savedProducts;
    }
  };
  // The table of `t` at the current frontier, after installing every
  // extension of its protocol registered up to the frontier.
  const build = t => {
    const pending = extensions.get(t.protocol);
    if (pending !== undefined) for (const b of pending) { if (b.order > frontier) break; force(b); }
    let map = __rt.map();
    const visible = t.entries.filter(e => e.overlay > 0 || e.order <= frontier)
      .sort((a, b) => a.overlay - b.overlay || a.order - b.order);
    for (const e of visible) map = __rt.assoc(map, e.key, e.fn);
    t.map = map; t.version = version; t.frontier = frontier;
  };
  // A struct of a type this task has not declared yet (a published value
  // another task made): demand the type's declaration, its product of the
  // same name, so that its implementations and view exist here too.
  const declare = x => {
    const ty = __rt.structTypeOf(x);
    if (ty === null || views.has(ty)) return false;
    const name = __rt.formName(ty);
    if (!(name in globalThis.__lightCompilerRoots)) return false;
    globalThis.__lightCompilerRoots[name];
    return true;
  };
  const lookup = (t, x) => {
    if (t.version !== version || t.frontier !== frontier) build(t);
    let m = __rt.cljDispatch(t.map, x);
    if (m === undefined) {
      // A JavaScript object: a mutable type's instance, or a host object.
      const ty = x.__lightNativeType;
      if (ty !== undefined) m = __rt.getOr(t.map, ty, undefined);
      if (m === undefined) m = __rt.getOr(t.map, 'object', undefined);
      if (m === undefined) m = __rt.getOr(t.map, '_', null);
    }
    return m;
  };
  const resolve = (t, x) => {
    declare(x);
    return lookup(t, x);
  };
  const missing = (name, x) => new Error('No protocol method ' + name + ' defined for type ' + (x === null || x === undefined ? 'null' : __rt.formKind(x) ?? typeof x));
  // `:extend-via-metadata`: the receiver's metadata entry for the method's
  // qualified symbol comes first.
  const viaMeta = (name, x) => {
    const meta = __rt.cljMeta(x);
    const f = meta === null ? null : __rt.cljGet(meta, __rt.cljSymbol(name), null);
    return __rt.cljTruth(f) ? f : null;
  };
  const method = (key, name, arities, viaMetadata) => {
    const ts = arities.map(n => methodTable(key, n));
    const find = (t, x) => {
      if (viaMetadata) { const f = viaMeta(name, x); if (f !== null) return [f, false]; }
      const m = resolve(t, x);
      if (m == null) throw missing(name, x);
      return [m, true];
    };
    if (arities.length === 1 && !viaMetadata) {
      const t = ts[0], n = arities[0];
      switch (n) {
        case 1: return x => { const m = resolve(t, x); if (m == null) throw missing(name, x); return m.call(x, x); };
        case 2: return (x, a) => { const m = resolve(t, x); if (m == null) throw missing(name, x); return m.call(x, x, a); };
        case 3: return (x, a, b) => { const m = resolve(t, x); if (m == null) throw missing(name, x); return m.call(x, x, a, b); };
      }
    }
    return function (x, ...args) {
      let i = arities.indexOf(args.length + 1);
      // One signature: its arity, padded or truncated as a JavaScript
      // call would be.
      if (arities.length === 1) { i = 0; args.length = arities[0] - 1; }
      if (i < 0) throw new Error('Invalid arity: ' + (args.length + 1));
      const [m, own] = find(ts[i], x);
      return own ? m.call(x, x, ...args) : m(x, ...args);
    };
  };
  const satisfies = (key, x) => resolve(methodTable(key, 0), x) != null;
  // `implements?`: the value's own type implements the protocol, not a
  // host-type or default extension.
  // The key of a direct implementation for `x`: its struct type, a mutable
  // type instance's type, or a core collection's native kind (`extend-type
  // PersistentVector`); never a host tag or `default`.
  const directKinds = new Set(['keyword', 'symbol', 'vector', 'map', 'set']);
  const directKey = x => {
    const ty = __rt.structTypeOf(x);
    if (ty !== null) return ty;
    const value = __rt.formValue(x), kind = __rt.formKind(value);
    if (kind === 'list') return __rt.formCount(value) === 0 ? 'empty-list' : 'list';
    if (kind !== null) return directKinds.has(kind) ? kind : null;
    return value !== null && typeof value === 'object' ? value.__lightNativeType ?? null : null;
  };
  const implementsDirectly = (key, x) => {
    const t = methodTable(key, 0);
    resolve(t, x);
    const direct = directKey(x);
    return direct !== null && __rt.getOr(t.map, direct, null) !== null;
  };
  // The implementation for the type `ty` (a struct type, or a mutable
  // type's key) in the table of method `name` and arity `n`, at the current
  // frontier. An `Object` method selects by its declaring type, so the
  // receiver need not be a `ty` (`(.call f other)`).
  const implementation = (name, n, ty) => {
    const t = methodTable(name, n);
    if (t.version !== version || t.frontier !== frontier) build(t);
    return __rt.getOr(t.map, ty, null);
  };
  // The tables of the `Object` methods installed as `member` of the type
  // `info` describes, built at the current frontier: building one installs
  // the visible extension bundles, which may add methods to `member`.
  const settle = (info, member) => {
    const names = info.members.get(member);
    let seen;
    do {
      seen = names.length;
      for (const name of names) {
        for (const n of objectArities.get(name) ?? []) implementation(name, n, info.ty);
        implementation(name, -1, info.ty);
      }
    } while (names.length !== seen);
    return names;
  };
  // A call of the JavaScript member `member` of the type `info` describes,
  // with receiver `x`: the method of the last group (by source name, in the
  // order first extended) with an implementation visible at the frontier,
  // whose overload is selected as the legacy `installMethods` does.
  const objectInvoke = (info, member, x, args) => {
    const names = settle(info, member);
    for (let i = names.length - 1; i >= 0; i--) {
      const name = names[i];
      let only = null, count = 0, exact = null;
      for (const n of objectArities.get(name) ?? []) {
        const m = implementation(name, n, info.ty);
        if (m == null) continue;
        count++; only = m;
        if (n === args.length + 1) exact = m;
      }
      const rest = implementation(name, -1, info.ty);
      if (count === 0 && rest == null) continue;
      const m = rest == null && count === 1 ? only : exact ?? rest;
      if (m == null) throw new Error('Invalid arity: ' + args.length);
      return m.call(x, x, ...args);
    }
    throw new TypeError(member + ' is not a function');
  };
  // The `Object` method `name`, installed as the JavaScript member
  // `member` of the type `info` describes from now on: a mutable type's
  // instances inherit it, and struct views get it. Methods whose names
  // munge alike share the member ([`objectInvoke`]).
  const objectMember = (info, name, member) => {
    if (info.names.has(name)) return;
    info.names.add(name);
    const names = info.members.get(member);
    if (names !== undefined) { names.push(name); return; }
    info.members.set(member, [name]);
    if (info.mutable) info.view.prototype[member] = function (...a) { return objectInvoke(info, member, this, a); };
  };
  // A type: a native struct type, or with a mutable field the constructor
  // of task-owned JavaScript instances. `view` constructs the JavaScript
  // object a struct's interop reads.
  const type = (fields, specs, view, mutable, ty) => {
    views.set(ty, {ty, view, fields, names: new Set(), members: new Map(), mutable, record: __rt.formKind(__rt.structNew(ty)) === 'record'});
    if (mutable) {
      view.__lightNativeType = ty;
      view.prototype.__lightNativeType = ty;
    }
    extend(ty, specs);
    return mutable ? view : ty;
  };
  // The struct a struct view, or an object inheriting from one, stands for.
  const viewStructs = new WeakMap();
  const structOf = o => {
    for (let p = o; p !== null && typeof p === 'object'; p = Object.getPrototypeOf(p)) {
      const s = viewStructs.get(p);
      if (s !== undefined) return s;
    }
    return o;
  };
  const viewOf = s => {
    declare(s);
    const info = views.get(__rt.structTypeOf(s));
    if (info === undefined) throw new Error('unsupported operation: JavaScript interop with a native struct of an undeclared type');
    const args = info.fields.map(f => __rt.structField(s, f));
    if (info.record) args.push(__rt.cljMeta(s), null, null);
    const v = new info.view(...args);
    viewStructs.set(v, s);
    // The receiver is the call's `this`; a view stands for its struct.
    for (const member of info.members.keys()) v[member] = function (...a) { return objectInvoke(info, member, structOf(this), a); };
    return v;
  };
  const register = (names, indices, location, once, unit) => {
    const b = {order: bundles.length + 1, names, unit, location, state: 0, error: null, products: null};
    bundles.push(b);
    if (once && names.length && names.every(name => name in globalThis.__lightCompilerRoots)) return null;
    for (const name of names) {
      // A preloaded root (a data property) stays; a product replaces an
      // earlier definition's.
      const old = Object.getOwnPropertyDescriptor(globalThis.__lightCompilerRoots, name);
      if (old !== undefined && 'value' in old) continue;
      Object.defineProperty(globalThis.__lightCompilerRoots, name, {configurable: true, get: () => { force(b); return b.products[name]; }});
    }
    for (const [protocol] of indices) {
      let list = extensions.get(protocol);
      if (list === undefined) extensions.set(protocol, list = []);
      list.push(b);
    }
    return null;
  };
  const enter = next => { frontiers.push(frontier); frontier = next; return null; };
  const leave = () => { frontier = frontiers.pop() ?? Infinity; return null; };
  const isRecord = x => { const ty = __rt.structTypeOf(x); return ty !== null && views.get(ty)?.record === true; };
  return {extend, method, satisfies, implementsDirectly, type, viewOf, register, enter, leave, coreKeys, isRecord};
})();
const native = (() => {
  let gensymScope = 0, gensymPrefix = '';
  const val = x => __rt.formValue(x);
  const interop = x => {
    const value = val(x), kind = __rt.formKind(value);
    if (kind === 'struct' || kind === 'record') return decl.viewOf(value);
    if (kind !== null) throw new Error('unsupported operation: JavaScript interop with native ' + kind + ' form');
    return value;
  };
  // Core operations are the engine's `__rt.clj*` intrinsics (light-cljs-477):
  // Rust over native forms, nil-punning and transparent to occurrences.
  // Macro units call them directly; these entries serve the core values a
  // unit passes around (`(map first xs)`) and variadic calls.
  const truth = x => __rt.cljTruth(x);
  // The elements of a collection or sequence; a lazy sequence is realized
  // to its end (light-cljs-479).
  const arr = x => {
    if (Array.isArray(x)) return x;
    let value = val(x);
    const kind = __rt.formKind(value);
    if (kind !== 'list' && kind !== 'vector') value = __rt.cljList(x);
    const out = [];
    for (let i = 0, n = __rt.formCount(value); i < n; i++) out.push(__rt.formNth(value, i));
    return out;
  };
  const list = a => { let v = __rt.formList(); for(let i=a.length-1;i>=0;i--) v=__rt.formConj(v,a[i]); return v; };
  const vector = a => { let v=__rt.formVector();for(const x of a)v=__rt.formConj(v,x);return v; };
  const map = a => { let v=__rt.formMap();for(let i=0;i<a.length;i+=2)v=__rt.formAssoc(v,a[i],a[i+1]);return v; };
  const set = a => { let v=__rt.formSet();for(const x of a)v=__rt.formConj(v,x);return v; };
  // A set literal with members that are not constants rejects duplicates.
  // Reader-distinct constants retain every position after lowering.
  const counted = (v, n, message) => { if (__rt.formCount(v) !== n) throw new Error(message); return v; };
  const literalSet = a => counted(set(a), a.length, 'Duplicate key');
  const constantSet = a => __rt.formLiteralSet(vector(a));
  const constantMap = a => __rt.formLiteralMap(vector(a));
  const arity = n => { throw new Error('Invalid arity: ' + n); };
  const gensym = (...args) => {
    if (args.length > 1) return arity(args.length);
    const base = core.str(args.length === 0 ? 'G__' : args[0]);
    let scope=gensymScope;
    const initializer=initializerStack[initializerStack.length-1];
    if(initializer!==undefined){
      const cell=__rt.effect(65532,0);
      if(cell!==0)__rt.need(cell,5);
      scope=__rt.effect(65531,'s'+initializer+':'+scope);
    }
    let name;
    if (scope !== 0) name = base + (scope===gensymScope?gensymPrefix:__rt.need(scope,6)) + __rt.effect(65531, 'g' + scope);
    else do { name = base + __rt.effect(65531, 'n'); } while (!__rt.effect(65531, 'a' + name));
    // The compiler's captured log decides whether recording is enabled.
    // A nested helper producer has a fresh task and no task-local log flag.
    __rt.effect(65531, initializer===undefined?'L'+name:'C'+scope+':'+name);
    return __rt.cljSymbol(name);
  };
  const seq = x => __rt.cljSeq(x);
  const get = (m,k,n=null) => __rt.cljGet(m,k,n);
  // Read `.call` before evaluating nested callback arguments. A custom
  // callable's getter and receiver have the same order as `rf.call(null, ...)`.
  const prepareInvoke = f => {
    const value = val(f), kind = __rt.formKind(value);
    if (kind === 'vector' || kind === 'map' || kind === 'set' || kind === 'keyword' || kind === 'symbol') return (...args) => {
      if (kind !== 'vector' && (args.length < 1 || args.length > 2)) arity(args.length);
      return __rt.cljInvoke(f, args[0], args.length > 1 ? args[1] : null, args.length);
    };
    const call = value.call;
    return (...args) => Reflect.apply(call, value, [null, ...args]);
  };
  const invoke = (f,...args) => {
    const value=val(f),kind=__rt.formKind(value);
    if(kind!=='vector'&&kind!=='map'&&kind!=='set'&&kind!=='keyword'&&kind!=='symbol')return value.call(null,...args);
    if(kind!=='vector'&&(args.length<1||args.length>2))arity(args.length);
    return __rt.cljInvoke(f,args[0],args.length>1?args[1]:null,args.length);
  };
  const unsupported = name => function(){throw new Error('unsupported operation: native macro runtime '+name);};
  // A comparison of adjacent operands, true for one; as the legacy runtimes.
  const chain = (xs, test) => { if (xs.length === 0) arity(0); for (let i = 1; i < xs.length; i++) if (!test(val(xs[i - 1]), val(xs[i]))) return false; return true; };
  const fix = x => x >= 0 ? Math.floor(x) : Math.ceil(x);
  const quot = (n,d) => fix((n-n%d)/d);
  const each = (op, x, xs) => { for (const y of xs) x = op(x, y); return x; };
  // Compile-time atoms are engine atoms (light-cljs-480): one shared atom
  // per Var, since an initializer's atom publishes. Before a task touches an
  // atom another activity made, it asks the compilation's effect lanes
  // (host effect 65532), and parks on the lane cell (kind 5) they give
  // until every earlier unit and pass has finished: atom effects keep their
  // `--jobs 1` order. An atom holds shareable values only: one that would
  // hold a mutable JavaScript object fails with the engine's NotShareable
  // error, which no handler catches.
  const atomOwners = new WeakMap();
  const lane = a => {
    if (!__rt.cljIsAtom(a)) throw new Error('unsupported operation: native macro runtime atom operation on ' + typeof val(a));
    const cell = __rt.effect(65532, atomOwners.get(a) ?? 0);
    if (cell !== 0) __rt.need(cell, 5);
  };
  const atom = (state, ...options) => {
    if (options.length) throw new Error('unsupported operation: native macro runtime atom options');
    const a = __rt.atomNew(state), owner = __rt.effect(65532, -1);
    if (owner !== 0) atomOwners.set(a, owner);
    return a;
  };
  // Store `v` whatever the atom holds, as the legacy runtimes' reset, and
  // give the state it replaced: an identity compare-and-set that repeats
  // only if another task wrote between the read and the set, never running
  // user code again. A swap's function runs once, before the store, so one
  // that changes the same atom behaves as in cljs.core.
  const store = (a, v) => {
    for (;;) {
      const current = __rt.atomLoad(a);
      if (Object.is(__rt.atomCas(a, current, v), current)) return current;
    }
  };
  const deref = a => { if (__rt.cljIsReduced(a)) return __rt.cljUnreduced(a); lane(a); return __rt.atomLoad(a); };
  const reset = (a, v) => { lane(a); store(a, v); return v; };
  const swapPair = (a, f, args) => {
    lane(a);
    const next = invoke(f, __rt.atomLoad(a), ...args);
    return [store(a, next), next];
  };
  // By `=`, as both pinned cljs.core runtimes compare.
  const compareAndSet = (a, o, n) => {
    lane(a);
    if (!__rt.formEquals(__rt.atomLoad(a), o)) return false;
    store(a, n);
    return true;
  };
  // Lazy sequences (light-cljs-479; docs/native-macros.md, "Lazy
  // sequences"): engine once-realized cells. Each step function takes
  // itself as its first argument instead of naming itself, so its thunks
  // capture only their arguments and publish whenever those do: a lazy
  // sequence a helper initializer returns is shared by every task. Chunked
  // sources (a vector's seq, a range and `chunk-cons` sequences) are read
  // 32 elements at a time, as in ClojureScript; lists one at a time.
  const lazy = f => __rt.lazySeq(f);
  // Match now, advance the same RegExp only when the tail is demanded.
  // Offsets, empty matches and slicing all use JavaScript UTF-16 units.
  const reSeq = (self, re, text) => {
    text = val(text);
    if (typeof text !== 'string') throw new TypeError('regex matching requires a string');
    const match = re.exec(text);
    if (match === null) return null;
    const value = match.length === 1 ? match[0] : vector(Array.from(match));
    return __rt.cljCons(value, lazy(() => {
      const offset = match.index + Math.max(1, match[0].length);
      return offset <= text.length ? self(self, re, text.slice(offset)) : null;
    }));
  };
  const rePattern = pattern => {
    pattern = val(pattern);
    if (pattern instanceof RegExp) return pattern;
    if (typeof pattern !== 'string') throw new TypeError('regex pattern requires a string');
    const prefix = /^\(\?([dgimsuxy]*)\)/.exec(pattern);
    return new RegExp(prefix ? pattern.slice(prefix[0].length) : pattern, prefix ? prefix[1] : '');
  };
  const reMatch = (re, text, full) => {
    text = val(text);
    if (typeof text !== 'string') throw new TypeError('regex matching requires a string');
    const result = re.exec(text);
    if (result === null || (full && result[0] !== text)) return null;
    return result.length === 1 ? result[0] : vector(Array.from(result));
  };
  const seqOf = x => __rt.cljSeq(x);
  const chunked = (s, step) => {
    const ch = __rt.cljChunkFirst(s), b = __rt.vectorBuilder();
    for (let i = 0, n = __rt.cljCount(ch); i < n; i++) step(b, __rt.cljNth(ch, i));
    return __rt.freeze(b);
  };
  const mapOne = (self, f, c) => lazy(() => {
    const s = seqOf(c);
    if (s === null) return null;
    if (__rt.cljChunkedSeq(s)) return __rt.cljChunkCons(chunked(s, (b, x) => __rt.builderConj(b, invoke(f, x))), self(self, f, __rt.cljChunkRest(s)));
    return __rt.cljCons(invoke(f, __rt.cljFirst(s)), self(self, f, __rt.cljRest(s)));
  });
  // `cs`: a list of the collections.
  const seqsOf = cs => { const ss = []; for (const c of arr(cs)) { const s = seqOf(c); if (s === null) return null; ss.push(s); } return ss; };
  const mapMany = (self, f, cs) => lazy(() => {
    const ss = seqsOf(cs);
    if (ss === null) return null;
    return __rt.cljCons(invoke(f, ...ss.map(s => __rt.cljFirst(s))), self(self, f, list(ss.map(s => __rt.cljRest(s)))));
  });
  const mapIndexedFrom = (self, f, c, i) => lazy(() => {
    const s = seqOf(c);
    if (s === null) return null;
    if (__rt.cljChunkedSeq(s)) {
      let k = i;
      const ch = chunked(s, (b, x) => __rt.builderConj(b, invoke(f, k++, x)));
      return __rt.cljChunkCons(ch, self(self, f, __rt.cljChunkRest(s), k));
    }
    return __rt.cljCons(invoke(f, i, __rt.cljFirst(s)), self(self, f, __rt.cljRest(s), i + 1));
  });
  const keepIndexedFrom = (self, f, c, i) => lazy(() => {
    for (let s=seqOf(c);s!==null;s=seqOf(__rt.cljRest(s)),i++) {
      if (__rt.cljChunkedSeq(s)) {
        const ch=__rt.cljChunkFirst(s), b=__rt.vectorBuilder(), n=__rt.cljCount(ch);
        for(let k=0;k<n;k++){const v=invoke(f,i+k,__rt.cljNth(ch,k));if(val(v)!=null)__rt.builderConj(b,v);}
        return __rt.cljChunkCons(__rt.freeze(b),self(self,f,__rt.cljChunkRest(s),i+n));
      }
      const v=invoke(f,i,__rt.cljFirst(s));
      if(val(v)!=null)return __rt.cljCons(v,self(self,f,__rt.cljRest(s),i+1));
    }
    return null;
  });
  // `filter` (keep when `keep`), `remove` (keep when not), `keep` (keep
  // non-nil values of `f`).
  const select = (self, f, c, mode) => lazy(() => {
    for (let s = seqOf(c); s !== null; s = seqOf(__rt.cljRest(s))) {
      if (__rt.cljChunkedSeq(s)) {
        const kept = chunked(s, (b, x) => {
          const v = invoke(f, x);
          if (mode === 2) { if (val(v) != null) __rt.builderConj(b, v); }
          else if (__rt.cljTruth(v) === (mode === 0)) __rt.builderConj(b, x);
        });
        return __rt.cljChunkCons(kept, self(self, f, __rt.cljChunkRest(s), mode));
      }
      const x = __rt.cljFirst(s), v = invoke(f, x);
      if (mode === 2 ? val(v) != null : __rt.cljTruth(v) === (mode === 0)) return __rt.cljCons(mode === 2 ? v : x, self(self, f, __rt.cljRest(s), mode));
    }
    return null;
  });
  const takeN = (self, n, c) => lazy(() => {
    if (!(n > 0)) return null;
    const s = seqOf(c);
    return s === null ? null : __rt.cljCons(__rt.cljFirst(s), self(self, n - 1, __rt.cljRest(s)));
  });
  const dropN = (n, c) => lazy(() => {
    let s = seqOf(c);
    for (let i = 0; i < n && s !== null; i++) s = __rt.cljNext(s);
    return s;
  });
  const takeWhileFrom = (self, f, c) => lazy(() => {
    const s = seqOf(c);
    if (s === null) return null;
    const x = __rt.cljFirst(s);
    return __rt.cljTruth(invoke(f, x)) ? __rt.cljCons(x, self(self, f, __rt.cljRest(s))) : null;
  });
  const dropWhileFrom = (f, c) => lazy(() => {
    let s = seqOf(c);
    while (s !== null && __rt.cljTruth(invoke(f, __rt.cljFirst(s)))) s = __rt.cljNext(s);
    return s;
  });
  // The concatenation of the collections in the sequence `cs`.
  const catSeqs = (self, cs) => lazy(() => {
    for (let s = seqOf(cs); s !== null; s = __rt.cljNext(s)) {
      const c = seqOf(__rt.cljFirst(s));
      if (c !== null) return __rt.cljConcat(c, self(self, __rt.cljRest(s)));
    }
    return null;
  });
  const interleaveAll = (self, cs) => lazy(() => {
    const ss = seqsOf(cs);
    if (ss === null) return null;
    return __rt.cljConcat(list(ss.map(s => __rt.cljFirst(s))), self(self, list(ss.map(s => __rt.cljRest(s)))));
  });
  const iterateFrom = (self, f, x) => __rt.cljCons(x, lazy(() => self(self, f, invoke(f, x))));
  const repeatForever = (self, x) => lazy(() => __rt.cljCons(x, self(self, x)));
  // A range: its first chunk of 32 numbers, then the rest lazily, as the
  // runtime's `IntegerRange` (a chunked sequence without metadata); `()`
  // when empty. A thunk never assigns what it captured: that would make the
  // binding a task-owned cell and the sequence unpublishable.
  const rangeFrom = (self, start, end, step) => {
    const b = __rt.vectorBuilder();
    let k = 0, x = start;
    for (; k < 32 && (step > 0 ? x < end : step < 0 ? x > end : true); k++) { __rt.builderConj(b, x); x += step; }
    if (k === 0) return __rt.formList();
    const next = x;
    return __rt.cljRangeCons(__rt.freeze(b), lazy(() => self(self, next, end, step)));
  };
  const partitionFrom = (self, n, step, c) => lazy(() => {
    const s = seqOf(c);
    if (s === null) return null;
    const p = takeN(takeN, n, s);
    return __rt.cljCount(p) === n ? __rt.cljCons(p, self(self, n, step, dropN(step, s))) : null;
  });
  // Reducer callbacks run on the task VM and can suspend without replay.
  // Stop before reading the next source element and unwrap one marker only;
  // nested markers let a flattening reducer propagate termination outward.
  const reduceSeq = (f, init, c) => {
    let acc = init;
    for (let s = seqOf(c); s !== null; s = __rt.cljNext(s)) {
      acc = invoke(f, acc, __rt.cljFirst(s));
      if (__rt.cljIsReduced(acc)) return __rt.cljUnreduced(acc);
    }
    return acc;
  };
  const reducing = (rf, step, complete = result => invoke(rf, result)) => (...a) => {
    if (a.length === 0) return invoke(rf);
    if (a.length === 1) return complete(a[0]);
    if (a.length === 2) return step(a[0], a[1]);
    return arity(a.length);
  };
  const mapTransform = f => rf => (...a) => a.length < 2 ? invoke(rf, ...a) : prepareInvoke(rf)(a[0], invoke(f, ...a.slice(1)));
  const filterTransform = f => rf => reducing(rf, (result, x) => truth(invoke(f, x)) ? invoke(rf, result, x) : result);
  const mapcatTransform = f => rf => (...a) => {
    if (a.length < 2) return invoke(rf, ...a);
    let result = a[0];
    for (let s = seqOf(invoke(f, ...a.slice(1))); s !== null; s = __rt.cljNext(s)) {
      result = invoke(rf, result, __rt.cljFirst(s));
      if (__rt.cljIsReduced(result)) return result;
    }
    return result;
  };
  const partitionByTransform = f => rf => {
    const none = __rt.formKeyword('cljs.core', 'none');
    let buffer = __rt.formVector(), prior = none;
    const flush = result => { const group = buffer; buffer = __rt.formVector(); return invoke(rf, result, group); };
    return reducing(rf, (result, x) => {
      const previous = prior, key = invoke(f, x); prior = key;
      if (__rt.formEquals(previous, none) || __rt.formEquals(previous, key)) { buffer = __rt.formConj(buffer, x); return result; }
      const ret = flush(result);
      if (!__rt.cljIsReduced(ret)) buffer = __rt.formConj(buffer, x);
      return ret;
    }, result => {
      if (__rt.formCount(buffer)) result = __rt.cljUnreduced(flush(result));
      return invoke(rf, result);
    });
  };
  // The collection overload keeps the legacy lazy partition/run shape and
  // predicate order, including re-reading the boundary to start the next run.
  const partitionByFrom = (self, f, c) => lazy(() => {
    const s = seqOf(c);
    if (s === null) return null;
    const x = __rt.cljFirst(s), key = invoke(f, x);
    const run = __rt.cljCons(x, takeWhileFrom(takeWhileFrom, y => __rt.formEquals(key, invoke(f, y)), __rt.cljNext(s)));
    return __rt.cljCons(run, self(self, f, lazy(() => dropN(__rt.cljCount(run), s))));
  });
  const transduce = (...a) => {
    if (a.length !== 3 && a.length !== 4) return arity(a.length);
    const xf = a[0], rf = a[1], init = a.length === 3 ? invoke(rf) : a[2], coll = a[a.length - 1];
    const transformed = invoke(xf, rf);
    return prepareInvoke(transformed)(reduceSeq(transformed, init, coll));
  };
  const intoReducer = (...a) => a.length === 1 ? a[0] : a.length === 2 ? __rt.cljConj(a[0], a[1]) : arity(a.length);
  const core = {
    assertions:true,
    gensym, rePattern, reSeq:(re,text)=>reSeq(reSeq,re,text),
    reFind:(re,text)=>reMatch(re,text,false), reMatches:(re,text)=>reMatch(re,text,true),
    isRegex:x=>val(x) instanceof RegExp,
    invoke, truth, get, identity:x=>x, macroReaderFunction:f=>f,
    sequence:x=>{const items=__rt.cljSeq(x);return items===null?__rt.formList():items;},
    list:(...a)=>list(a), vector:(...a)=>vector(a), hashMap:(...a)=>map(a), hashSet:(...a)=>set(a), set:x=>set(arr(x)), vec:x=>vector(arr(x)),
    arrayMap:(...a)=>{ if (a.length % 2) throw new Error('No value supplied for key'); return map(a); },
    seqToMap:value=>{
      if (__rt.cljNext(value) === null) return seq(value) === null ? __rt.formMap() : __rt.cljFirst(value);
      const values = arr(value).slice(), trailing = values.length % 2 ? values.pop() : null;
      const base = map(values);
      return trailing == null ? base : core.into(base, trailing);
    },
    keyword:(...a)=>a.length===1?__rt.cljKeyword(a[0]):a.length===2?__rt.cljKeywordNs(a[0],a[1]):arity(a.length),
    symbol:(...a)=>a.length===1?__rt.cljSymbol(a[0]):a.length===2?__rt.cljSymbolNs(a[0],a[1]):arity(a.length),
    first:x=>__rt.cljFirst(x), second:x=>__rt.cljFirst(__rt.cljNext(x)),
    nth:(x,n,...d)=>d.length?__rt.cljNthOr(x,n,d[0]):__rt.cljNth(x,n),
    count:x=>__rt.cljCount(x),
    seq, isEmpty:x=>__rt.cljSeq(x)===null,
    rest:x=>__rt.cljRest(x), next:x=>__rt.cljNext(x),
    cons:(x,xs)=>__rt.cljCons(x,xs), concat:(...xs)=>__rt.cljConcatAll(list(xs)),
    listStar:(...xs)=>list([...xs.slice(0,-1),...arr(xs[xs.length-1])]),
    apply:(f,...xs)=>invoke(f,...xs.slice(0,-1),...arr(xs[xs.length-1])),
    assoc:(m,...xs)=>{for(let i=0;i<xs.length;i+=2)m=__rt.cljAssoc(m,xs[i],xs[i+1]);return m;},
    dissoc:(m,...ks)=>each((m,k)=>__rt.cljDissoc(m,k),m,ks),
    conj:(...a)=>a.length===0?__rt.formVector():each((m,x)=>__rt.cljConj(m,x),a[0],a.slice(1)),
    disj:(s,...xs)=>each((s,x)=>__rt.cljDisj(s,x),s,xs),
    contains:(c,k)=>__rt.cljContains(c,k),
    keys:m=>{if(__rt.formKind(val(m))==='record'){const s=__rt.cljSeq(m);return s===null?null:list(arr(s).map(e=>__rt.formNth(e,0)));}
      const k=__rt.formValue(m)==null?null:__rt.formKeys(__rt.formValue(m));return k===null||__rt.formCount(k)===0?null:list(arr(k));},
    vals:m=>{const s=__rt.cljSeq(m);return s===null?null:list(arr(s).map(e=>__rt.formNth(e,1)));},
    meta:x=>__rt.cljMeta(x), withMeta:(x,m)=>__rt.cljWithMeta(x,m),
    isKeyword:x=>__rt.formKind(val(x))==='keyword', isSymbol:x=>__rt.formKind(val(x))==='symbol',
    isVector:x=>__rt.formKind(val(x))==='vector', isMap:x=>{const k=__rt.formKind(val(x));return k==='map'||k==='record';}, isSeq:x=>{const k=__rt.formKind(val(x));return k==='list'||k==='lazy-seq'||k==='cons';},
    isSet:x=>__rt.formKind(val(x))==='set',
    isNil:x=>val(x)==null, isSome:x=>val(x)!=null, isFalse:x=>__rt.cljIsFalse(x), isString:x=>typeof val(x)==='string', not:x=>!truth(x), boolean:truth,
    reduced:x=>__rt.cljReduced(x), isReduced:x=>__rt.cljIsReduced(x), unreduced:x=>__rt.cljUnreduced(x),
    ensureReduced:x=>__rt.cljIsReduced(x)?x:__rt.cljReduced(x),
    equal:(...xs)=>xs.every((x,i)=>i===0||__rt.formEquals(xs[i-1],x)), hash:x=>__rt.formHash(x),
    notEqual:(...xs)=>!core.equal(...xs),
    name:x=>{if(__rt.formKind(val(x))==='type')throw new Error('Doesn\'t support name: '+__rt.formName(val(x)));return __rt.formName(val(x));},
    namespace:x=>{if(__rt.formKind(val(x))==='type')throw new Error('Doesn\'t support namespace: '+__rt.formName(val(x)));return __rt.formNamespace(val(x));},
    inc:x=>val(x)+1, dec:x=>val(x)-1, multiply:(...xs)=>xs.reduce((a,b)=>a*val(b),1),
    less:(...xs)=>chain(xs,(a,b)=>a<b), greater:(...xs)=>chain(xs,(a,b)=>a>b), lessEqual:(...xs)=>chain(xs,(a,b)=>a<=b), greaterEqual:(...xs)=>chain(xs,(a,b)=>a>=b),
    str:(...xs)=>xs.map(x=>{x=val(x);if(x==null)return '';const kind=__rt.formKind(x);
      if(kind==='symbol'||kind==='keyword'){const ns=__rt.formNamespace(x);return (kind==='keyword'?':':'')+(ns===null?'':ns+'/')+__rt.formName(x);}
      if(kind!==null)throw new Error('unsupported operation: native macro runtime str of a native '+kind+' form');return String(x);}).join(''),
    identical:(a,b)=>val(a)===val(b), isInstance:(c,x)=>__rt.formKind(c)==='type'?__rt.structTypeOf(x)===c:val(x) instanceof c, isRecord:x=>__rt.formKind(val(x))==='record',
    add:(...xs)=>xs.reduce((a,b)=>a+val(b),0), subtract:(a,...xs)=>xs.length?xs.reduce((a,b)=>a-val(b),val(a)):-val(a),
    divide:(...xs)=>xs.length===0?NaN:xs.length===1?1/val(xs[0]):xs.slice(1).reduce((a,b)=>a/val(b),val(xs[0])),
    int:x=>val(x)|0, long:x=>fix(val(x)), byte:x=>x, short:x=>x, float:x=>x, double:x=>x,
    quot:(n,d)=>quot(val(n),val(d)), rem:(n,d)=>{n=val(n);d=val(d);return n-d*quot(n,d);},
    mod:(n,d)=>{n=val(n);d=val(d);return ((n%d)+d)%d;},
    numericEqual:(...xs)=>xs.length!==0&&core.equal(...xs),
    isNumber:x=>typeof val(x)==='number', isFloat:x=>typeof val(x)==='number', isDouble:x=>typeof val(x)==='number',
    isInfinite:x=>val(x)===Infinity||val(x)===-Infinity,
    exInfo:(m,d)=>new Error(val(m)),
    atom, deref, reset, compareAndSet,
    swap:(a,f,...args)=>swapPair(a,f,args)[1],
    swapVals:(a,f,...args)=>{const [o,n]=swapPair(a,f,args);return __rt.formVector(o,n);},
    resetVals:(a,v)=>{lane(a);return __rt.formVector(store(a,v),v);},
    // Lazy and chunked sequences (light-cljs-479).
    // An explicit `(LazySeq. meta fn s hash)`: a realized one (no `fn`) holds `s`.
    LazySeq:function(m,f,s){const x=f==null?__rt.lazySeq(()=>s):__rt.lazySeq(f);return m==null?x:__rt.cljWithMeta(x,m);},
    realized:x=>__rt.cljRealized(x),
    mapSeq:(...a)=>a.length===0?lazy(()=>null):a.length===1?mapTransform(a[0]):a.length===2?mapOne(mapOne,a[0],a[1]):mapMany(mapMany,a[0],list(a.slice(1))),
    mapIndexed:(f,c)=>mapIndexedFrom(mapIndexedFrom,f,c,0),
    keepIndexed:(...a)=>{
      if(a.length===2)return keepIndexedFrom(keepIndexedFrom,a[0],a[1],0);
      if(a.length!==1)return arity(a.length);
      const f=a[0];
      return rf=>{let i=-1;return reducing(rf,(result,input)=>{const v=invoke(f,++i,input);return val(v)==null?result:invoke(rf,result,v);});};
    },
    filter:(...a)=>a.length===1?filterTransform(a[0]):a.length===2?select(select,a[0],a[1],0):arity(a.length), remove:(f,c)=>select(select,f,c,1), keep:(f,c)=>select(select,f,c,2),
    mapcat:(...a)=>a.length===0?arity(0):a.length===1?mapcatTransform(a[0]):catSeqs(catSeqs,core.mapSeq(...a)),
    partitionBy:(...a)=>a.length===1?partitionByTransform(a[0]):a.length===2?partitionByFrom(partitionByFrom,a[0],a[1]):arity(a.length),
    take:(n,c)=>takeN(takeN,val(n),c), drop:(n,c)=>dropN(val(n),c),
    takeWhile:(f,c)=>takeWhileFrom(takeWhileFrom,f,c), dropWhile:(f,c)=>dropWhileFrom(f,c),
    interleave:(...cs)=>cs.length===0?__rt.formList():cs.length===1?((c)=>lazy(()=>seqOf(c)))(cs[0]):interleaveAll(interleaveAll,list(cs)),
    interpose:(...a)=>{
      if(a.length===2)return dropN(1,interleaveAll(interleaveAll,__rt.formList(repeatForever(repeatForever,a[0]),a[1])));
      if(a.length!==1)return arity(a.length);
      const sep=a[0];
      return rf=>{let started=false;return reducing(rf,(result,input)=>{
        if(started){result=invoke(rf,result,sep);if(__rt.cljIsReduced(result))return result;}else started=true;
        return invoke(rf,result,input);
      });};
    },
    iterate:(f,x)=>iterateFrom(iterateFrom,f,x),
    repeat:(...a)=>a.length===1?repeatForever(repeatForever,a[0]):a.length===2?takeN(takeN,val(a[0]),repeatForever(repeatForever,a[1])):arity(a.length),
    range:(...a)=>a.length===0?rangeFrom(rangeFrom,0,Infinity,1):a.length===1?rangeFrom(rangeFrom,0,val(a[0]),1)
      :a.length<=3?rangeFrom(rangeFrom,val(a[0]),val(a[1]),a.length===3?val(a[2]):1):arity(a.length),
    partition:(...a)=>a.length===2?partitionFrom(partitionFrom,val(a[0]),val(a[0]),a[1]):a.length===3?partitionFrom(partitionFrom,val(a[0]),val(a[1]),a[2]):arity(a.length),
    reduce:(f,...a)=>{if(a.length===2)return reduceSeq(f,a[0],a[1]);if(a.length!==1)arity(a.length+1);const s=seqOf(a[0]);
      return s===null?invoke(f):reduceSeq(f,__rt.cljFirst(s),__rt.cljRest(s));},
    transduce,
    completing:(...a)=>a.length===1||a.length===2?reducing(a[0],(result,x)=>invoke(a[0],result,x),a.length===1?result=>result:result=>invoke(a[1],result)):arity(a.length),
    into:(...a)=>{
      if (a.length === 0) return __rt.formVector();
      if (a.length === 1) return a[0];
      if (a.length === 2) return reduceSeq(intoReducer, a[0], a[1]);
      if (a.length !== 3) return arity(a.length);
      // Legacy editable targets use a step/completion adapter. Other targets
      // receive conj itself, whose initialization and variadic arities a
      // custom transducer may call even though transduce has an explicit init.
      const rf = __rt.cljIsEditable(a[0]) ? intoReducer : core.conj;
      return transduce(a[1], rf, a[0], a[2]);
    },
    every:(f,c)=>{for(let s=seqOf(c);s!==null;s=__rt.cljNext(s))if(!truth(invoke(f,__rt.cljFirst(s))))return false;return true;},
    some:(f,c)=>{for(let s=seqOf(c);s!==null;s=__rt.cljNext(s)){const v=invoke(f,__rt.cljFirst(s));if(truth(v))return v;}return null;},
    notEvery:(f,c)=>!core.every(f,c), notAny:(f,c)=>!truth(core.some(f,c)),
    last:c=>{const a=arr(c);return a.length?a[a.length-1]:null;}, butlast:c=>{const a=arr(c);return a.length>1?list(a.slice(0,-1)):null;},
    reverse:c=>list(arr(c).reverse()),
    mapv:(f,...cs)=>vector(arr(core.mapSeq(f,...cs))), filterv:(f,c)=>vector(arr(core.filter(f,c))),
    replace:(...a)=>{
      if (a.length !== 1 && a.length !== 2) return arity(a.length);
      const substitutions=a[0], replaceOne=x=>__rt.cljContains(substitutions,x)?get(substitutions,x):x;
      if (a.length===1) return mapTransform(replaceOne);
      const coll=a[1];
      if (__rt.formKind(val(coll))!=='vector') return mapOne(mapOne,replaceOne,coll);
      let result=coll;
      for(let i=0,n=__rt.cljCount(coll);i<n;i++){
        const x=__rt.cljNth(result,i);
        if(__rt.cljContains(substitutions,x)) result=__rt.cljAssoc(result,i,get(substitutions,x));
      }
      return result;
    },
    // `(doall n coll)` and `(dorun n coll)` realize the first `n` elements.
    doall:(...a)=>a.length===1?(__rt.cljCount(a[0]),a[0]):a.length===2?(core.dorun(a[0],a[1]),a[1]):arity(a.length),
    dorun:(...a)=>{if(a.length===1){__rt.cljCount(a[0]);return null;}if(a.length!==2)arity(a.length);
      for(let n=val(a[0]),s=seqOf(a[1]);s!==null&&n>0;n--)s=__rt.cljNext(s);return null;},
    isChunkedSeq:x=>__rt.cljChunkedSeq(x), chunkFirst:s=>__rt.cljChunkFirst(s), chunkRest:s=>__rt.cljChunkRest(s),
    chunkNext:s=>__rt.cljChunkNext(s), chunkCons:(c,r)=>__rt.cljChunkCons(c,r),
    chunkBuffer:n=>__rt.vectorBuilder(), chunkAppend:(b,x)=>{__rt.builderConj(b,x);return null;}, chunk:b=>__rt.freeze(b),
    nthProtocol:(c,n,...d)=>d.length?__rt.cljNthOr(c,n,d[0]):__rt.cljNth(c,n),
    // Small helpers the sequence functions are used with.
    isEven:n=>{n=val(n);if(!Number.isInteger(n))throw new Error('Argument must be an integer: '+n);return (n&1)===0;},
    isOdd:n=>{n=val(n);if(!Number.isInteger(n))throw new Error('Argument must be an integer: '+n);return (n&1)!==0;},
    isZero:n=>__rt.cljIsZero(n), isPositive:n=>val(n)>0, isNegative:n=>val(n)<0,
    // Keep the pure constructors in emitter.rs equivalent: the returned
    // closures capture native vectors, so a composed helper can publish.
    comp:(...fs)=>{
      const n=fs.length;
      if(n===0)return core.identity;
      if(n===1)return fs[0];
      const functions=vector(fs);
      return (...a)=>{let r=invoke(__rt.formNth(functions,n-1),...a);for(let i=n-2;i>=0;i--)r=invoke(__rt.formNth(functions,i),r);return r;};
    },
    partial:(...supplied)=>{
      const f=supplied[0];
      if(supplied.length===1)return f;
      const prefix=supplied.length===0?__rt.formVector(undefined,undefined,undefined):vector(supplied.slice(1));
      return (...a)=>{const args=arr(prefix);return invoke(f,...args,...a);};
    },
    complement:f=>(...a)=>!truth(invoke(f,...a)),
    constantly:x=>()=>x,
    // `exists?` demands a definition's value, while `resolve` only tests
    // whether its binding was registered (a declaration counts).
    nativeRootExists:(name,binding)=>{
      const bindings=globalThis.__lightCompilerRoots;
      if(!(name in bindings))return false;
      if(binding)return true;
      if(roots.has(name)&&roots.get(name).unit===null)return false;
      return bindings[name]!==undefined;
    },
    juxt:(...fs)=>{
      const functions=fs.length===0?__rt.formVector(undefined,undefined,undefined):vector(fs);
      return (...a)=>{let result=__rt.formVector();for(let i=0,n=__rt.formCount(functions);i<n;i++)result=__rt.formConj(result,invoke(__rt.formNth(functions,i),...a));return result;};
    },
    fnil:(f,...ds)=>{
      const n=ds.length;
      if(n<1||n>3)arity(n+1);
      const defaults=vector(ds);
      return (...a)=>{
        if(a.length<(n===1?1:2))a.length=3;
        for(let i=0;i<n&&i<a.length;i++)if(val(a[i])==null)a[i]=__rt.formNth(defaults,i);
        return invoke(f,...a);
      };
    },
    registerReaderNamespace:()=>null, addReaderDefinitions:()=>null, setReaderNamespace:()=>null,
    PersistentVector:{EMPTY:__rt.formVector()}, PersistentArrayMap:{EMPTY:__rt.formMap()}, List:{EMPTY:__rt.formList()},
    PersistentHashSet:{EMPTY:__rt.formSet()},
    Keyword:{}, Symbol:{}, PersistentHashMap:{EMPTY:__rt.formMap()}, EmptyList:{},
  };
  // `extend-type` of a core type extends the native form kind it is; array
  // and hash maps are one native kind (light-cljs-478).
  for (const [name, key] of [['Keyword','keyword'],['Symbol','symbol'],['PersistentVector','vector'],
    ['PersistentArrayMap','map'],['PersistentHashMap','map'],['PersistentHashSet','set'],['List','list'],['EmptyList','empty-list']])
    decl.coreKeys.set(core[name], key);
  for (const name of [/* CORE_KEYS */]) if (!(name in core)) Object.defineProperty(core,name,{get:unsupported('core.'+name)});
  const rt={core,
    nativeInterop:interop,declarationReceiver:x=>x,
    protocol:name=>Object.freeze({name}),
    protocolMethod:(key,name,arities,viaMetadata)=>decl.method(key,name,arities,viaMetadata===true),
    protocolPredicate:(protocol,key,x,direct)=>direct?decl.implementsDirectly(key,x):decl.satisfies(key,x),
    extendType:(target,specs)=>decl.extend(target,specs),
    extendInstance:unsupported('extendInstance'),
    nativeType:(fields,specs,record,name,view,mutable,ty)=>decl.type(fields,specs,view,mutable,ty),
    keyword:(ns,n)=>__rt.formKeyword(ns,n),symbol:(ns,n)=>__rt.formSymbol(ns,n),get,
    vector:(...xs)=>vector(xs),vectorFromArray:vector,listFromArray:list,mapFromArray:map,arrayMapFromArray:map,constantMapFromArray:constantMap,
    mapFromArrays:(ks,vs)=>map(ks.flatMap((k,i)=>[k,vs[i]])),
    mapShape:keys=>keys,mapFromShape:(ks,vs)=>map(ks.flatMap((k,i)=>[k,vs[i]])),
    setFromArray:set,arraySetFromArray:set,constantSetFromArray:constantSet,literalSetFromArray:literalSet,
    fnRest:(args,n)=>list(Array.prototype.slice.call(args,n)),
    makeIFn:f=>f,
    compilerForms:{gensymScope:(serial,prefix)=>{gensymScope=serial;gensymPrefix=prefix;return null;},takeEncodePeak:()=>0,
      takeGensyms:()=>__rt.formVector(__rt.effect(65531,'t')),
      raiseGensyms:n=>{__rt.effect(65531,'r'+n);return null;}},
  };
  return rt;
})();
globalThis.__lightNativeRuntime=native;
const roots=new Map();
let initializerSerial=0;
const initializerStack=[];
globalThis.__lightInitializerUnit=null;
globalThis.__lightInitializerParent=null;
globalThis.__lightInitializerResult=null;
globalThis.__lightInitializerFailure=null;
globalThis.__lightRegisterUnit=(name,unit,once=false)=>{
  if(!roots.has(name)&&(name in globalThis.__lightCompilerRoots))return null;
  if(roots.has(name)&&roots.get(name).unit===unit)return null;
  // A defonce keeps an earlier definition (not a declaration), as the
  // legacy runtimes' registry does.
  if(once&&roots.has(name)&&roots.get(name).unit!==null)return null;
  const slot={unit,state:0,value:null};roots.set(name,slot);
  Object.defineProperty(globalThis.__lightCompilerRoots,name,{configurable:true,get:()=>{
    if(unit===null)throw new Error('unavailable definition: '+name);
    __rt.need(unit,2);
    const factory=__rt.need(unit,3);
    if(slot.state===2){__rt.need(unit,4);return slot.value;}
    if(slot.state===1)throw new Error('root initialization cycle: '+name);
    if(slot.state===3)throw slot.value;
    slot.state=1;
    try{
      const serial=++initializerSerial;
      globalThis.__lightInitializerUnit=unit;
      globalThis.__lightInitializerParent=initializerStack[initializerStack.length-1]??null;
      const producer=__rt.need(serial,beginKind);
      if(producer){
        let failed=false,failure;
        initializerStack.push(unit);
        try{slot.value=factory(native);}catch(e){failed=true;failure=e;}finally{initializerStack.pop();}
        globalThis.__lightInitializerUnit=unit;
        globalThis.__lightInitializerResult=slot.value;
        globalThis.__lightInitializerFailure=failed?String(failure):null;
        __rt.need(serial,finishKind);
        if(failed)throw failure;
        __rt.need(unit,4);
      }else{
        const initialized=__rt.need(unit,4);
        if(__rt.nth(initialized,0))slot.value=__rt.nth(initialized,1);
        else {slot.value=factory(native);__rt.effect(65535,unit);}
      }
      slot.state=2;return slot.value;
    }
    catch(e){slot.value=e;slot.state=3;throw e;}
  }});return null;
};
// Declaration bundles advertise their products without demanding their code.
// Keep preloaded roots and the same explicit unsupported-demand getters as units.
globalThis.__lightRegisterBundle=(names,code,indices=[],location='',once=false,group=false,unit=-1)=>{
  // A bundle without a unit (a test's, or an embedder's registration) keeps
  // preloaded roots and reports demand as unsupported.
  if(unit<0){for(const name of names)if(!(name in globalThis.__lightCompilerRoots))Object.defineProperty(globalThis.__lightCompilerRoots,name,{configurable:true,get:()=>{throw new Error('unsupported operation: helper not preloaded: '+name);}});return null;}
  return decl.register(names,indices,location,once,unit);
};
globalThis.__lightEnterCompilerFrontier=next=>decl.enter(next);
globalThis.__lightLeaveCompilerFrontier=()=>decl.leave();
globalThis.__lightCompilerRoots={};
const values=new Map();
globalThis.__lightResult=null;
globalThis.__lightFailure=null;
// A command's value enters `values` only once the next command arrives:
// nothing allocates between a macro's return and the park at `need`, so
// data a macro released after catching a heap byte denial is collected when
// the next run starts, before this loop allocates again. A denied entry is
// retried with the following command. A release allocates nothing: it
// drops the held value or deletes an entered one, keeping any other held.
let held=false,heldId=0,heldValue=null;
for(let serial=1;;serial++){
  const command=__rt.need(serial,commandKind);
  const op=__rt.nth(command,0), id=__rt.nth(command,1), input=__rt.nth(command,2);
  globalThis.__lightFailure=null;
  try {
    let result=null;
    if(op===3){if(held&&heldId===id){held=false;heldValue=null;}else values.delete(id);}
    else {
      if(held){values.set(heldId,heldValue);held=false;heldValue=null;}
      if(op===0) result=(0,eval)(input);
      else if(op===1) result=input;
      else {const args=[];for(let i=1;i<__rt.count(input);i++)args.push(values.get(__rt.nth(input,i)));
        // A call result is exported as finite forms: its lazy sequences are
        // realized into lists here, in the task (light-cljs-479).
        result=__rt.cljRealizeAll(values.get(__rt.nth(input,0))(...args));}
      held=true;heldId=id;heldValue=result;
    }
    globalThis.__lightResult=result;
  }catch(e){globalThis.__lightFailure=String(e);globalThis.__lightResult=null;}
}
