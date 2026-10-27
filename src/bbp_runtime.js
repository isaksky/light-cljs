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
    if (target === Promise) return 'js/Promise';
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
    const key = keyOf(target);
    // Promise has protocol dispatch, but no native instance/member view.
    // Validate the entire specification before installing any entries.
    if (key === 'js/Promise' && specs.some(spec => spec[0].startsWith('Object/')))
      throw new Error('unsupported operation: native Promise Object extension');
    const overlay = order === 0 ? ++overlays : 0, info = views.get(key);
    for (const [name, arity, fn, , variadic, member, direct] of specs) {
      const object = name.startsWith('Object/');
      const n = object && variadic != null ? -1 : arity;
      // Native fixed entries take their hidden receiver from the first
      // operand. Adapt scope-sensitive legacy entries once at installation;
      // their raw JavaScript and `this-as` still see the original receiver.
      const entry = object || direct || arity === 0 ? fn : (x, ...args) => fn.call(x, x, ...args);
      methodTable(name, n).entries.push({order, overlay, key, fn: entry});
      if (!object) continue;
      let arities = objectArities.get(name);
      if (arities === undefined) objectArities.set(name, arities = new Set());
      if (n >= 0) arities.add(n);
      if (info !== undefined) objectMember(info, name, member);
    }
    version++;
    return null;
  };
  const bundles = [], installing = [];
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
      installing.push(b);
      try {
        if (b.multi) globalThis.__lightInitializeCompilerValue(b.unit, () => { factory(globalThis.__lightNativeRuntime); return null; });
        else factory(globalThis.__lightNativeRuntime);
      } finally { installing.pop(); }
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
    t.map = map; t.promise=__rt.getOr(map,'js/Promise',undefined);t.version = version; t.frontier = frontier;
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
  const resolve = (t, x) => {
    declare(x);
    if (t.version !== version || t.frontier !== frontier) build(t);
    const value=t.promise===undefined?null:__rt.formValue(x);
    const promise=value!==null&&typeof value==='object'&&__rt.formKind(value)===null&&value instanceof Promise;
    let m=promise?t.promise:undefined;
    // Native structs represent JavaScript objects in the legacy runtime:
    // their type entry precedes Object, which precedes default. The engine's
    // dispatch intrinsic deliberately knows only the type and default keys.
    const struct = __rt.structTypeOf(x);
    if(m===undefined)m = struct === null ? __rt.cljDispatch(t.map, x) : __rt.getOr(t.map, struct, undefined);
    if (m === undefined) {
      // A JavaScript object: a mutable type's instance, or a host object.
      const ty = struct ?? (__rt.cljIsAtom(x)?undefined:x.__lightNativeType);
      if (ty !== undefined) m = __rt.getOr(t.map, ty, undefined);
      if (m === undefined) m = __rt.getOr(t.map, 'object', undefined);
      if (m === undefined) m = __rt.getOr(t.map, '_', null);
    }
    return m;
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
    if (arities.length === 1 && !viaMetadata) {
      const t = ts[0], n = arities[0];
      switch (n) {
        case 1: return x => { const m = resolve(t, x); if (m == null) throw missing(name, x); return m(x); };
        case 2: return (x, a) => { const m = resolve(t, x); if (m == null) throw missing(name, x); return m(x, a); };
        case 3: return (x, a, b) => { const m = resolve(t, x); if (m == null) throw missing(name, x); return m(x, a, b); };
      }
    }
    return function (x, ...args) {
      let i = arities.indexOf(args.length + 1);
      // One signature: its arity, padded or truncated as a JavaScript
      // call would be.
      if (arities.length === 1) { i = 0; args.length = arities[0] - 1; }
      if (i < 0) throw new Error('Invalid arity: ' + (args.length + 1));
      // Metadata functions are ordinary callables, not the table's adapted
      // explicit-receiver entries. Keep the legacy null receiver.
      if (viaMetadata) { const f = viaMeta(name, x); if (f !== null) return f.call(null, x, ...args); }
      const m = resolve(ts[i], x);
      if (m == null) throw missing(name, x);
      return m(x, ...args);
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
    if (value instanceof Promise) return 'js/Promise';
    return value !== null && typeof value === 'object' ? value.__lightNativeType ?? null : null;
  };
  const implementsDirectly = (key, x) => {
    const t = methodTable(key, 0);
    resolve(t, x);
    const direct = directKey(x);
    return direct !== null && __rt.getOr(t.map, direct, null) !== null;
  };
  // Owned core implementations precede object/default extensions. Explicit
  // extensions of the receiver's native type still replace that implementation.
  const resolveCore = (name, arity, x, builtin, directOnly = false) => {
    const t = methodTable(name, arity);
    if (directOnly && !builtin && !implementsDirectly(t.protocol, x)) return null;
    let m;
    if (builtin) {
      declare(x);
      if (t.version !== version || t.frontier !== frontier) build(t);
      const key = directKey(x);
      m = key === null ? null : __rt.getOr(t.map, key, null);
    } else m = resolve(t, x);
    // Membership without this arity is an error, unless the engine owns
    // the implementation (notably the built-in methods of records).
    if (m === null && !builtin && satisfies(t.protocol, x))
      throw new Error('No protocol method ' + name + ' defined for this value');
    return m;
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
  const type = (fields, specs, view, mutable, ty, fieldMembers) => {
    const record = __rt.formKind(__rt.structNew(ty)) === 'record';
    // Keep one compiler-munged member per declared field: multiple fields
    // may share a member, and property enumeration may reorder names.
    views.set(ty, {ty, view, fields, fieldMembers, names: new Set(), members: new Map(), mutable, record});
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
    const b = {order: bundles.length + 1, names, unit, location, state: 0, error: null, products: null,
      multi: indices.length > 0 && indices.every(index => index[0].startsWith('multifn:'))};
    bundles.push(b);
    if (once && names.length && names.every(name => globalThis.__lightCompilerRootKept(name))) return null;
    for (const name of names) {
      // A preloaded root (a data property) stays; a product replaces an
      // earlier definition's.
      const old = Object.getOwnPropertyDescriptor(globalThis.__lightCompilerRoots, name);
      if (old !== undefined && 'value' in old) continue;
      globalThis.__lightInstallCompilerProduct(name, () => { force(b); return b.products[name]; });
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
  // Multimethod declarations use the same source frontier as protocols.
  // Installing a method must not recursively demand its own bundle.
  const demandMulti = name => {
    const pending = [...(extensions.get('multifn:' + name) ?? []), ...(extensions.get('multifn:*') ?? [])]
      .sort((a,b) => a.order-b.order);
    for (const b of pending) if (b.order <= frontier && !installing.includes(b)) force(b);
  };
  const productOwner = name => {
    const b = installing[installing.length-1];
    return b !== undefined && b.names.includes(name) ? b : null;
  };
  const keptProduct = name => {
    const b = productOwner(name);
    return b !== null && name in b.products;
  };
  const defineProduct = (name, initialize) => {
    const b = productOwner(name);
    if (b === null) return false;
    if (initialize !== null) {
      // A defmulti's defonce is a single-product bundle. Its initializer
      // joins the existing task-owned initializer protocol, so dispatch
      // function/hierarchy effects run once even across suspended demands.
      b.products[name] = b.names.length === 1
        ? globalThis.__lightInitializeCompilerValue(b.unit, initialize)
        : initialize();
    }
    return true;
  };
  const mutableRecord = x => {
    if (x === null || typeof x !== 'object' || __rt.formKind(x) !== null) return null;
    const info = views.get(x.__lightNativeType);
    return info?.mutable && info.record ? info : null;
  };
  return {extend, method, satisfies, implementsDirectly, type, viewOf, register, enter, leave, coreKeys, mutableRecord,
    demandMulti, keptProduct, productOwner, defineProduct,
    multiOrder:()=>order, multiFrontier:()=>frontier, installingMulti:()=>installing.some(b=>b.multi),
    coreMethod:resolveCore,
    printer:x=>resolve(methodTable('IPrintWithWriter/-pr-writer',3),x),
    writer:x=>resolve(methodTable('IWriter/-write',2),x),
    flusher:x=>resolve(methodTable('IWriter/-flush',1),x),
    kvReducer:(x,builtin)=>resolveCore('IKVReduce/-kv-reduce',3,x,builtin),
    entryKey:(x,builtin)=>resolveCore('IMapEntry/-key',1,x,builtin),
    entryVal:(x,builtin)=>resolveCore('IMapEntry/-val',1,x,builtin),
    seq:(x,builtin)=>resolveCore('ISeqable/-seq',1,x,builtin),
    first:(x,builtin)=>resolveCore('ISeq/-first',1,x,builtin,true),
    rest:(x,builtin)=>resolveCore('ISeq/-rest',1,x,builtin,true),
    next:(x,builtin)=>resolveCore('INext/-next',1,x,builtin,true)};
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
  const arrayMap = a => __rt.formArrayMap(vector(a));
  const literalMap = a => a.length<=16?arrayMap(a):map(a);
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
  const get = function(m,k,n) { return arguments.length === 2 ? __rt.cljGet2(m,k) : __rt.cljGet(m,k,n); };
  // The engine calls this task-local resolver only for native structs. The
  // selected entry runs through a rooted native callback frame, so declaration
  // demand and user methods can yield/park without replaying effects.
  __rt.cljCoreResolver((op, x, throughLazy) => {
    const record = __rt.formKind(x) === 'record';
    const method = (name, n, builtin = record, directOnly = false) => decl.coreMethod(name, n, x, builtin, directOnly);
    switch (op) {
      case 'formEquals': {
        const equiv = method('IEquiv/-equiv', 2);
        return equiv === null ? null : (x,y) => !!val(equiv(x,y));
      }
      case 'formHash': {
        const hash = method('IHash/-hash', 1);
        return hash === null ? null : x => val(hash(x)) | 0;
      }
      case 'cljGet2': return method('ILookup/-lookup', 2) ?? (record ? null : () => null);
      case 'cljGet': return method('ILookup/-lookup', 3) ?? (record ? null : (x,k,d) => d);
      case 'cljContains': {
        const contains = method('IAssociative/-contains-key?', 2);
        if (contains !== null) return contains;
        if (record) return null;
        return (x,k) => { const missing = {}; return get(x,k,missing) !== missing; };
      }
      case 'cljAssoc': return method('IAssociative/-assoc', 3);
      case 'cljDissoc': return method('IMap/-dissoc', 2);
      case 'cljConj': return method('ICollection/-conj', 2);
      case 'cljDisj': return method('ISet/-disjoin', 2);
      case 'cljMeta': return method('IMeta/-meta', 1);
      case 'cljWithMeta': return method('IWithMeta/-with-meta', 2);
      case 'cljSeq': return throughLazy ? () => __rt.cljSeq(x) : method('ISeqable/-seq', 1);
      case 'cljList': return x => {
        const out = [];
        for (let s = reduceSeqOf(x); s != null; s = nextOf(s)) out.push(firstOf(s));
        return list(out);
      };
      case 'cljCount': {
        if (!throughLazy) {
          const count = method('ICounted/-count', 1);
          if (count !== null || record) return count;
          if (method('ISeqable/-seq', 1) === null) return null;
        }
        return x => {
          let n = 0;
          for (let s = reduceSeqOf(x); s != null; s = nextOf(s)) {
            const kind = __rt.formKind(val(s));
            const builtin = nativeSeq(s) && kind !== 'lazy-seq' && kind !== 'cons'
              || kind === 'vector' || kind === 'map' || kind === 'record' || kind === 'set'
              || typeof val(s) === 'string' || Array.isArray(s);
            const count = decl.coreMethod('ICounted/-count', 1, s, builtin);
            if (count !== null) return n + val(count(s));
            if (builtin) return n + __rt.cljCount(s);
            n++;
          }
          return n;
        };
      }
      // LazySeq first/rest/next first take the seq of the realized value,
      // then perform the operation on that value, matching the legacy order.
      case 'cljFirst': return throughLazy ? () => { __rt.cljSeq(x); return __rt.cljFirst(x); }
        : firstOf;
      case 'cljRest': return throughLazy ? () => { __rt.cljSeq(x); return __rt.cljRest(x); }
        : restOf;
      case 'cljNext': return throughLazy ? () => { __rt.cljSeq(x); return __rt.cljNext(x); }
        : method('INext/-next', 1, false, true) ?? (x => __rt.cljSeq(__rt.cljRest(x)));
      case 'cljNth': return (x,i) => nthOf(x,i,null,false,throughLazy);
      case 'cljNthOr': return (x,i,d) => nthOf(x,i,d,true,throughLazy);
    }
    return null;
  });
  // Read `.call` before evaluating nested callback arguments. A custom
  // callable's getter and receiver have the same order as `rf.call(null, ...)`.
  const prepareInvoke = f => {
    const value = val(f), kind = __rt.formKind(value);
    // Resolve through the task global rather than capturing `multi` before
    // its initialization: that forward capture would box ordinary lazy
    // callbacks in a task-owned binding cell and prevent their publication.
    if(kind==='struct'&&__rt.formName(__rt.structTypeOf(value))==='cljs.core/MultiFn')
      return (...args)=>globalThis.__lightNativeRuntime.core.invokeMulti(value,args);
    if (kind === 'vector' || kind === 'map' || kind === 'set' || kind === 'keyword' || kind === 'symbol') return (...args) => {
      if (kind !== 'vector' && (args.length < 1 || args.length > 2)) arity(args.length);
      return __rt.cljInvoke(f, args[0], args.length > 1 ? args[1] : null, args.length);
    };
    const call = value.call;
    return (...args) => Reflect.apply(call, value, [null, ...args]);
  };
  const invoke = (f,...args) => {
    const value=val(f),kind=__rt.formKind(value);
    if(kind==='struct'&&__rt.formName(__rt.structTypeOf(value))==='cljs.core/MultiFn')
      return globalThis.__lightNativeRuntime.core.invokeMulti(value,args);
    if(kind!=='vector'&&kind!=='map'&&kind!=='set'&&kind!=='keyword'&&kind!=='symbol')return value.call(null,...args);
    if(kind!=='vector'&&(args.length<1||args.length>2))arity(args.length);
    return __rt.cljInvoke(f,args[0],args.length>1?args[1]:null,args.length);
  };
  const unsupported = name => function(){throw new Error('unsupported operation: native macro runtime '+name);};
  // Printing walks native seqs incrementally. In particular, a bounded print
  // must not realize the whole lazy input. All strings remain exact UTF-16.
  const namedText = x => {
    x=val(x);
    const ns=__rt.formNamespace(x);
    return (__rt.formKind(x)==='keyword'?':':'')+(ns===null?'':ns+'/')+__rt.formName(x);
  };
  const printOption = (options,name,root) => options==null?core[root]:get(options,__rt.cljKeyword(name));
  const writers=new WeakSet();
  const stringWriter=()=>{
    const buffer={text:'',append(text){this.text+=text;return this;}};
    return {buffer,writer:new StringBufferWriter(buffer)};
  };
  function StringBufferWriter(sb){this.sb=sb;writers.add(this);}
  const write = (writer,text) => {
    if(writers.has(writer))return writer.sb.append(val(text));
    const method=decl.writer(writer);
    if(method!=null)return method.call(writer,writer,text);
    throw new Error('No protocol method IWriter/-write defined for this value');
  };
  const flushWriter = writer => {
    if(writers.has(writer))return null;
    const method=decl.flusher(writer);
    if(method!=null)return method.call(writer,writer);
    throw new Error('No protocol method IWriter/-flush defined for this value');
  };
  const printSequential = (writer,one,begin,separator,end,options,coll) => {
    const saved=core.printLevel;
    core.printLevel=val(saved)==null?null:val(saved)-1;
    try {
      if(core.printLevel!==null&&core.printLevel<0)return write(writer,'#');
      write(writer,begin);
      let s=seq(coll),remaining=val(printOption(options,'print-length','printLength')),printed=false;
      while(s!==null&&(remaining===null||remaining!==0)){
        if(printed)write(writer,separator);
        invoke(one,__rt.cljFirst(s),writer,options);printed=true;
        s=__rt.cljNext(s);if(remaining!==null)remaining--;
      }
      if(s!==null){if(printed)write(writer,separator);const marker=options==null?null:get(options,__rt.cljKeyword('more-marker'));write(writer,truth(marker)?marker:'...');}
      return write(writer,end);
    } finally {core.printLevel=saved;}
  };
  const printWriter = (value,writer,options) => {
    if(options!=null){
      const alt=get(options,__rt.cljKeyword('alt-impl'));
      if(truth(alt))return invoke(alt,value,writer,__rt.cljAssoc(options,__rt.cljKeyword('fallback-impl'),printWriterImpl));
    }
    return printWriterImpl(value,writer,options);
  };
  // Explicit object constructors keep JS own-property order and last writes.
  // The temporary host container never crosses the form bridge.
  const literalObjectPairs = pairs => {
    const object={};
    for(let i=0;i<pairs.length;i+=2){
      const key=val(pairs[i]),name=__rt.formKind(key)==='keyword'?__rt.formName(key):key;
      if(name!=='__proto__')Object.defineProperty(object,name,{value:pairs[i+1],enumerable:true,configurable:true});
    }
    return Object.keys(object).flatMap(key=>[key,object[key]]);
  };
  const literalObjectFields = x => {
    const payload=__rt.formLiteralPayload(x);
    let pairs=arr(__rt.formPrintItems(val(payload))).flatMap(arr);
    // Legacy bridge input decodes the compiler's map with mapFromArray before
    // constructing the JS object. Keep that traversal view separate from the
    // original positional payload and its occurrence/provenance identity.
    if(__rt.formKind(payload)==='occurrence')pairs=arr(__rt.formPrintItems(literalMap(pairs))).flatMap(arr);
    return literalObjectPairs(pairs);
  };
  const printWriterImpl = (value,writer,options,skipMetadata=false) => {
    const x=val(value);
    if(x==null)return write(writer,'nil');
    const record=decl.mutableRecord(x);
    if(!skipMetadata&&truth(printOption(options,'meta','printMeta'))){
      const m=record===null?__rt.cljMeta(value):x.__meta;
      if(m!=null){write(writer,'^');printWriter(m,writer,options);write(writer,' ');}
    }
    const custom=decl.printer(x);
    if(custom!=null)return custom.call(x,x,writer,options);
    const kind=__rt.formKind(x);
    if(__rt.cljIsAtom(x)){write(writer,'#object[cljs.core.Atom {:val ');printWriter(core.deref(x),writer,options);return write(writer,'}]');}
    if(kind==='reduced')return write(writer,'#object[Reduced [object Object]]');
    if(kind==='keyword'||kind==='symbol')return write(writer,namedText(x));
    if(kind==='type')return write(writer,__rt.formName(x));
    if(kind==='compiler-literal'){
      const tag=__rt.formName(x),payload=__rt.formLiteralPayload(x);
      if(tag==='regex')return write(writer,'#\"'+regexValue(x).source+'\"');
      if(tag==='js-array')return printSequential(writer,printWriter,'#js [',' ',']',options,payload);
      if(tag==='js-object'){
        if(!core.isObject(x))return write(writer,'#object[Object '+core.str(x)+']');
        write(writer,'#js ');
        const pairs=literalObjectFields(x);
        return printSequential(writer,(entry,w,o)=>{
          const key=val(__rt.cljNth(entry,0));
          printWriter(/^[A-Za-z_*+?!\-'][\w*+?!\-']*$/.test(key)?__rt.cljKeyword(key):key,w,o);
          write(w,' ');return printWriter(__rt.cljNth(entry,1),w,o);
        },'{',', ','}',options,__rt.formPrintItems(constantMap(pairs)));
      }
      if(tag==='queue')return printSequential(writer,printWriter,'#queue [',' ',']',options,payload);
      if(tag==='uuid')return write(writer,'#uuid "'+val(payload)+'"');
      if(tag==='instant')return write(writer,'#inst "'+new Date(val(payload)).toISOString().replace(/Z$/,'-00:00')+'"');
    }
    if(typeof x==='string'){
      const escapes={'"':'\\"','\\':'\\\\','\b':'\\b','\f':'\\f','\n':'\\n','\r':'\\r','\t':'\\t'};
      return write(writer,truth(printOption(options,'readably','printReadably'))?'"'+x.replace(/[\\"\b\f\n\r\t]/g,ch=>escapes[ch])+'"':x);
    }
    if(typeof x==='number')return write(writer,Number.isNaN(x)?'##NaN':x===Infinity?'##Inf':x===-Infinity?'##-Inf':Object.is(x,-0)?'-0.0':String(x));
    if(typeof x==='boolean')return write(writer,String(x));
    if(kind==='vector')return printSequential(writer,printWriter,'[',' ',']',options,x);
    if(kind==='list'||kind==='lazy-seq'||kind==='cons')return printSequential(writer,printWriter,'(',' ',')',options,x);
    if(kind==='set')return printSequential(writer,printWriter,'#{',' ','}',options,__rt.formPrintItems(x));
    if(kind==='map'){
      let ns=null;
      if(truth(core.printNamespaceMaps)){
        const keys=arr(__rt.formKeys(x));
        if(keys.length&&keys.every(k=>['keyword','symbol'].includes(__rt.formKind(val(k))))){
          ns=__rt.formNamespace(val(keys[0]));
          if(!keys.every(k=>__rt.formNamespace(val(k))===ns))ns=null;
        }
      }
      return printSequential(writer,(e,w,o)=>{
        let k=__rt.cljNth(e,0);
        if(ns)k=__rt.formKind(val(k))==='keyword'?__rt.cljKeyword(__rt.formName(val(k))):__rt.cljSymbol(__rt.formName(val(k)));
        printWriter(k,w,o);write(w,' ');return printWriter(__rt.cljNth(e,1),w,o);
      },(ns?'#:'+ns:'')+'{',', ','}',options,__rt.formPrintItems(x));
    }
    if(kind==='record')return printSequential(writer,(e,w,o)=>printSequential(w,printWriter,'',' ','',o,e),'#'+__rt.formName(__rt.structTypeOf(x)).replaceAll('/','.')+'{',', ','}',options,__rt.formPrintItems(x));
    if(record!==null){
      let entries=vector(record.fields.map((f,i)=>__rt.formVector(__rt.cljKeyword(f),x[record.fieldMembers[i]])));
      if(x.__extmap!=null)for(let s=seq(__rt.formPrintItems(x.__extmap));s!==null;s=__rt.cljNext(s))entries=__rt.formConj(entries,__rt.cljFirst(s));
      return printSequential(writer,(e,w,o)=>printSequential(w,printWriter,'',' ','',o,e),'#'+__rt.formName(record.ty).replaceAll('/','.')+'{',', ','}',options,entries);
    }
    if(kind==='struct')return write(writer,'#object['+__rt.formName(__rt.structTypeOf(x)).replaceAll('/','.')+' '+String(interop(x))+']');
    if(kind===null&&(typeof x==='object'||typeof x==='function')){
      const ty=x.__lightNativeType;
      if(__rt.formKind(ty)==='type')return write(writer,typeof x==='function'?__rt.formName(ty):'#object['+__rt.formName(ty).replaceAll('/','.')+' '+String(x)+']');
    }
    if(x instanceof Namespace)return write(writer,'#object[Namespace '+String(x)+']');
    if(x instanceof RegExp)return write(writer,'#"'+x.source+'"');
    // Host objects with a printer protocol work above. Other unimplemented
    // kinds fail explicitly rather than silently stringifying native data.
    throw new Error('unsupported operation: native macro runtime pr-str of '+(kind??typeof x));
  };
  const printValues = (values,options) => {
    const {buffer,writer}=stringWriter();let printed=false;
    for(let s=seq(values);s!==null;s=__rt.cljNext(s)){
      if(printed)write(writer,' ');printWriter(__rt.cljFirst(s),writer,options);printed=true;
    }
    flushWriter(writer);return buffer.text;
  };
  const prStr = (...values) => printValues(vector(values),null);
  const prWriter = (x,w,o) => {
    const custom=decl.printer(val(x));
    if(custom!=null)return custom.call(val(x),x,w,o);
    const kind=__rt.formKind(val(x));
    if(!['keyword','symbol','list','vector','map','set','lazy-seq','cons','record','reduced'].includes(kind)&&!__rt.cljIsAtom(val(x))&&decl.mutableRecord(val(x))===null)
      throw new Error('No protocol method IPrintWithWriter/-pr-writer defined for this value');
    return printWriterImpl(x,w,o,true);
  };
  const printStr = (...values) => {const saved=core.printReadably;core.printReadably=false;try{return prStr(...values);}finally{core.printReadably=saved;}};
  const namespaces=new Map();
  // Explicit Namespace values are not registry entries. A snapshot contains
  // their original name occurrence; each task restores the same local object
  // for repeated reads of that snapshot, without interning it by name. Only
  // an observable snapshot keeps the cached object and its name alive.
  const explicitNamespaces=new WeakMap();
  let currentNamespace;
  const namespaceFrames=[];
  function Namespace(obj,name){if(!(this instanceof Namespace))return new Namespace(obj,name);this.obj=obj;this.name=name;}
  Namespace.prototype.toString=function(){const value=val(this.name),kind=__rt.formKind(value);return kind==='symbol'||kind==='keyword'?namedText(value):String(value);};
  Namespace.prototype.getName=function(){return this.name;};
  const createNs = (...a) => {
    if(a.length!==1&&a.length!==2)return arity(a.length);
    const name=a[0];if(__rt.formKind(val(name))!=='symbol')throw new TypeError('create-ns requires a symbol');
    const key=namedText(name),obj=a.length===2?val(a[1]):null;
    if(!namespaces.has(key))namespaces.set(key,new Namespace(obj,name));else if(obj!=null)namespaces.get(key).obj=obj;
    return namespaces.get(key);
  };
  const findNs = (...a) => {
    if(a.length!==1)return arity(a.length);
    const name=val(a[0]);if(name==null)return null;if(name instanceof Namespace)return name;
    if(__rt.formKind(name)!=='symbol')throw new TypeError('find-ns requires a symbol or Namespace');
    return namespaces.get(namedText(name))??null;
  };
  const macroReaderFunction = fn => Object.assign(function(...args){
    const env=get(args[1],__rt.cljKeyword('ns')),name=get(env,__rt.cljKeyword('name'));
    if(val(name)==null)return Reflect.apply(fn,null,args);
    const saved=core.currentNs;core.currentNs=createNs(name);
    try{return Reflect.apply(fn,null,args);}finally{core.currentNs=saved;}
  },fn);
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
  // Native multimethods carry only persistent fields and engine atom handles.
  // In particular their identity can publish without retaining a task's JS
  // runtime. Calls enter the consuming task's runtime through its global.
  const multi = (() => {
    const ty=__rt.structType('cljs.core/MultiFn',__rt.formVector('name','dispatch_fn','default_dispatch_val','hierarchy','method_table','prefer_table','method_cache','cached_hierarchy','__method_state','__prefer_state'),false);
    const is=x=>{const t=__rt.structTypeOf(val(x));return t!==null&&__rt.formName(t)==='cljs.core/MultiFn';};
    const field=(m,k)=>__rt.structField(val(m),k);
    const text=x=>printStr(x);
    const empty=()=>arrayMap([]), kw=x=>__rt.cljKeyword(x);
    const ident=x=>{const k=__rt.formKind(val(x));return k==='symbol'||k==='keyword';};
    const constructor=x=>typeof val(x)==='function'||__rt.formKind(val(x))==='type';
    // Immutable native types stand for the same constructor functions as
    // deftype/defrecord in emitted JavaScript (light-cljs-478).
    const constructorValue=x=>__rt.formKind(val(x))==='type'?MultiFn:val(x);
    const part=(h,k)=>get(h,kw(k));
    const makeHierarchy=(...a)=>a.length?arity(a.length):arrayMap([kw('parents'),empty(),kw('ancestors'),empty(),kw('descendants'),empty()]);
    let globalHierarchy;
    // Outside the catalog's unit range, but still inside JavaScript's exact
    // integer range: one initializer cell for the compilation's hierarchy.
    const getGlobalHierarchy=(...a)=>a.length?arity(a.length):(globalHierarchy??=globalThis.__lightInitializeCompilerValue(4503599627370495,()=>atom(makeHierarchy())));
    const isaIn=(h,child,parent)=>{
      if(__rt.formEquals(child,parent))return true;
      child=val(child);parent=val(parent);
      if(constructor(child)&&typeof parent==='function'&&constructorValue(child) instanceof parent)return true;
      if(__rt.cljContains(get(part(h,'ancestors'),child),parent))return true;
      if(constructor(child))for(let base=Object.getPrototypeOf(constructorValue(child));base!==null;base=Object.getPrototypeOf(base))
        if(__rt.cljContains(get(part(h,'ancestors'),base),parent))return true;
      return __rt.formKind(child)==='vector'&&__rt.formKind(parent)==='vector'&&__rt.formCount(child)===__rt.formCount(parent)
        &&arr(child).every((x,i)=>isaIn(h,x,__rt.formNth(parent,i)));
    };
    const relation=(kind,...a)=>{
      if(a.length===1)a=[deref(getGlobalHierarchy()),a[0]];
      if(a.length!==2)return arity(a.length);
      let result=get(part(a[0],kind),a[1]);
      if(constructor(a[1])){
        if(kind==='descendants')throw new Error("Can't get descendants of constructors");
        let base=Object.getPrototypeOf(constructorValue(a[1]));
        while(base!==null){result=__rt.cljConj(result??set([]),base);if(kind==='parents')break;
          for(const x of arr(get(part(a[0],kind),base)))result=__rt.cljConj(result,x);
          base=Object.getPrototypeOf(base);}
      }
      return __rt.cljCount(result)===0?null:result;
    };
    const derive=(...a)=>{
      if(a.length===2){const [tag,parent]=a;
        if(!ident(parent)||core.namespace(parent)===null||!(constructor(tag)||ident(tag)&&core.namespace(tag)!==null))throw new Error('Assert failed: qualified derive tags');
        const ref=getGlobalHierarchy();reset(ref,derive(deref(ref),tag,parent));return null;}
      if(a.length!==3)return arity(a.length);
      const [h,tag,parent]=a;
      if(__rt.formEquals(tag,parent)||!(constructor(tag)||ident(tag))||!ident(parent))throw new Error('Assert failed: derive tags');
      const p=part(h,'parents'),anc=part(h,'ancestors'),desc=part(h,'descendants');
      if(__rt.cljContains(get(p,tag),parent))return h;
      if(__rt.cljContains(get(anc,tag),parent))throw new Error(text(tag)+'already has'+text(parent)+'as ancestor');
      if(__rt.cljContains(get(anc,parent),tag))throw new Error('Cyclic derivation:'+text(parent)+'has'+text(tag)+'as ancestor');
      const closure=(table,source,sources,target,targets)=>{
        for(const k of [source,...arr(get(sources,source))])table=__rt.cljAssoc(table,k,set([...arr(get(targets,k)),target,...arr(get(targets,target))]));
        return table;
      };
      return arrayMap([kw('parents'),__rt.cljAssoc(p,tag,__rt.cljConj(get(p,tag,set([])),parent)),
        kw('ancestors'),closure(anc,tag,desc,parent,anc),kw('descendants'),closure(desc,parent,anc,tag,desc)]);
    };
    const underive=(...a)=>{
      if(a.length===2){const ref=getGlobalHierarchy();reset(ref,underive(deref(ref),...a));return null;}
      if(a.length!==3)return arity(a.length);
      const [h,tag,parent]=a,p=part(h,'parents');
      if(!__rt.cljContains(get(p,tag),parent))return h;
      const remaining=__rt.cljDisj(get(p,tag),parent),updated=__rt.cljCount(remaining)?__rt.cljAssoc(p,tag,remaining):__rt.cljDissoc(p,tag);
      let result=makeHierarchy();
      for(const e of arr(updated))for(const x of arr(__rt.formNth(e,1)))result=derive(result,__rt.formNth(e,0),x);
      return result;
    };
    // Histories follow the supplied atom's lifetime, including across tasks
    // and after all its MultiFns have been discarded. The engine's private
    // auxiliary slot keeps bookkeeping out of the public map and introduces
    // no compilation-wide root. CAS publishes only one state, without user
    // equality, constructor replay or declaration effects.
    const tableState=ref=>{
      const slot=__rt.atomAux(ref),existing=__rt.atomLoad(slot);
      if(existing!==null){reconcile(ref,existing);return existing;}
      const initial=deref(ref),candidate=vector([initial,atom(empty()),atom(initial)]);
      const previous=__rt.atomCas(slot,null,candidate);
      if(previous===null)return candidate;
      reconcile(ref,previous);return previous;
    };
    function MultiFn(name,dispatch,defaultValue,hierarchy,methods,prefs,cache,cached){
      return __rt.structNew(ty,name,dispatch,defaultValue,hierarchy,methods,prefs,cache,cached,
        tableState(methods),tableState(prefs));
    }
    const demand=m=>decl.demandMulti(text(field(m,'name')));
    // A persistent map of declaration order to operations. Materializing at
    // the current frontier never reexecutes a declaration or its effects.
    // Runtime mutations follow every source declaration, in lane order.
    const record=(state,op,x,y)=>{
      const ref=__rt.formNth(state,1),history=deref(ref),order=decl.multiOrder();
      const key=order===0?-(__rt.cljCount(history)+1):order;
      reset(ref,__rt.cljAssoc(history,key,__rt.cljConj(get(history,key,vector([])),vector([op,x,y]))));
    };
    // The public table may have been reset/swapped independently of a
    // MultiFn. Remember the last observed/published identity in shared state,
    // so frontier materialization writes never become replacement operations.
    const reconcile=(ref,state)=>{
      const current=deref(ref),published=__rt.formNth(state,2);
      if(current!==deref(published)){record(state,'replace',current,null);reset(published,current);}
    };
    const change=(m,op,x=null,y=null)=>{
      if(!decl.installingMulti())demand(m);
      reconcile(field(m,'method_table'),field(m,'__method_state'));
      reconcile(field(m,'prefer_table'),field(m,'__prefer_state'));
      if(op!=='prefer')record(field(m,'__method_state'),op,x,y);
      if(op==='prefer'||op==='reset')record(field(m,'__prefer_state'),op,x,y);
      // Retained constructor atoms remain language-level tables. Installation
      // must not recursively demand the bundle (or any later declarations).
      const view=tables(m,false);
      reset(field(m,'method_cache'),view.methods);
      reset(field(m,'cached_hierarchy'),op==='reset'?null:deref(field(m,'hierarchy')));
      return m;
    };
    const views=new WeakMap();
    const materialize=(table,state)=>{
      reconcile(table,state);
      const ref=__rt.formNth(state,1),history=deref(ref),frontier=decl.multiFrontier(),old=views.get(ref);
      if(old!==undefined&&old.history===history&&old.frontier===frontier)return old.value;
      const entries=arr(history).filter(e=>__rt.formNth(e,0)<=frontier)
        .sort((a,b)=>{const x=__rt.formNth(a,0),y=__rt.formNth(b,0);return (x<0)-(y<0)||(x<0?y-x:x-y);});
      let value=__rt.formNth(state,0);
      for(const entry of entries)for(const e of arr(__rt.formNth(entry,1))){
        const op=__rt.formNth(e,0),x=__rt.formNth(e,1),y=__rt.formNth(e,2);
        if(op==='add')value=__rt.cljAssoc(value,x,y);
        else if(op==='remove')value=__rt.cljDissoc(value,x);
        else if(op==='reset')value=empty();
        else if(op==='replace')value=x;
        else value=__rt.cljAssoc(value,x,__rt.cljConj(get(value,x,set([])),y));
      }
      views.set(ref,{value,history,frontier});return value;
    };
    const tables=(m,force=true)=>{
      if(force)demand(m);
      const view={methods:materialize(field(m,'method_table'),field(m,'__method_state')),prefs:materialize(field(m,'prefer_table'),field(m,'__prefer_state'))};
      publish(m,view);return view;
    };
    const publish=(m,view)=>{
      for(const [key,state,value] of [['method_table','__method_state',view.methods],['prefer_table','__prefer_state',view.prefs]]){
        const ref=field(m,key);if(deref(ref)!==value)reset(ref,value);
        reset(__rt.formNth(field(m,state),2),value);
      }
    };
    const getMethod=(m,dispatch)=>{
      const {methods,prefs}=tables(m),h=deref(field(m,'hierarchy'));
      // CLJS seeds its method cache with the table itself: an exact method
      // wins before preference/hierarchy search, even over a preferred parent.
      const exact=get(methods,dispatch);
      if(truth(exact))return exact;
      const dominates=(x,y)=>truth(get(get(prefs,x),y))||isaIn(h,x,y);
      let best=null;
      for(const entry of arr(methods)){
        const key=__rt.formNth(entry,0);
        if(!isaIn(h,dispatch,key))continue;
        const candidate=best===null||dominates(key,__rt.formNth(best,0))?entry:best;
        if(!dominates(__rt.formNth(candidate,0),key))throw new Error("Multiple methods in multimethod '"+text(field(m,'name'))+"' match dispatch value: "+text(dispatch)+' -> '+text(key)+' and '+text(__rt.formNth(candidate,0))+', and neither is preferred');
        best=candidate;
      }
      return best===null?get(methods,field(m,'default_dispatch_val')):__rt.formNth(best,1);
    };
    const dispatch=(m,args)=>{
      const value=invoke(field(m,'dispatch_fn'),...args),target=getMethod(m,value);
      if(!truth(target))throw new Error("No method in multimethod '"+text(field(m,'name'))+"' for dispatch value: "+text(value));
      return invoke(target,...args);
    };
    const prefer=(m,x,y)=>{
      const {prefs}=tables(m);
      if(truth(get(get(prefs,y),x)))throw new Error("Preference conflict in multimethod '"+text(field(m,'name'))+"': "+text(y)+' is already preferred to '+text(x));
      change(m,'prefer',x,y);
      return deref(field(m,'hierarchy'));
    };
    return {is,field,MultiFn,dispatch,getMethod,change,tables,prefer,makeHierarchy,getGlobalHierarchy,derive,underive,relation,
      isa:(...a)=>a.length===2?isaIn(deref(getGlobalHierarchy()),...a):a.length===3?isaIn(...a):arity(a.length)};
  })();
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
    re=regexValue(re);
    const match = re.exec(text);
    if (match === null) return null;
    const value = match.length === 1 ? match[0] : vector(Array.from(match));
    return __rt.cljCons(value, lazy(() => {
      const offset = match.index + Math.max(1, match[0].length);
      return offset <= text.length ? self(self, re, text.slice(offset)) : null;
    }));
  };
  const literalViews=new WeakMap();
  const regexValue = value => {
    const x=val(value);
    if(__rt.formKind(x)!=='compiler-literal'||__rt.formName(x)!=='regex')return x;
    let re=literalViews.get(x);
    if(re===undefined){re=rePattern(__rt.formLiteralPayload(x));literalViews.set(x,re);}
    return re;
  };
  const rePattern = pattern => {
    pattern = regexValue(pattern);
    if (pattern instanceof RegExp) return pattern;
    if (typeof pattern !== 'string') throw new TypeError('regex pattern requires a string');
    const prefix = /^\(\?([dgimsuxy]*)\)/.exec(pattern);
    return new RegExp(prefix ? pattern.slice(prefix[0].length) : pattern, prefix ? prefix[1] : '');
  };
  const reMatch = (re, text, full) => {
    text = val(text);
    if (typeof text !== 'string') throw new TypeError('regex matching requires a string');
    const result = regexValue(re).exec(text);
    if (result === null || (full && result[0] !== text)) return null;
    return result.length === 1 ? result[0] : vector(Array.from(result));
  };
  const seqOf = x => __rt.cljSeq(x);
  const nativeSeq = x => {
    const kind = __rt.formKind(val(x));
    return kind === 'list' || kind === 'cons' || kind === 'lazy-seq';
  };
  // Reduction's protocol tables are task-owned. Keep this adapter out of the
  // lazy sequence constructors, whose intrinsic-only closures can publish.
  // Array cursors retain the original storage and check its current length
  // on advancement. Reducers can replace, append or remove unvisited items.
  const arraySeqs = new WeakMap();
  const arraySeq = (array, index) => {
    if (index >= array.length) return null;
    const cursor = {};
    arraySeqs.set(cursor, {array, index});
    return cursor;
  };
  const reduceSeqOf = x => {
    const cursor = arraySeqs.get(x);
    if (cursor !== undefined) return cursor.index < cursor.array.length ? x : null;
    const value = val(x), kind = __rt.formKind(value);
    const builtin = value == null || typeof value === 'string' || Array.isArray(value) || nativeSeq(x)
      || kind === 'vector' || kind === 'map' || kind === 'record' || kind === 'set';
    const method = decl.seq(x,builtin);
    if (method !== null) {
      const result = method.call(x,x);
      // Imported nil carries a source occurrence; traversal still ends here.
      return val(result) == null ? null : result;
    }
    if (!builtin) throw new Error(core.str(x) + ' is not ISeqable');
    return Array.isArray(value) ? arraySeq(value, 0) : __rt.cljSeq(x);
  };
  const firstOf = (x, stored = false) => {
    const cursor = arraySeqs.get(x);
    // Stored reductions have already checked the cursor on advancement.
    // Sequence -first checks again, after any reducer call getter effects.
    if (cursor !== undefined) return stored || cursor.index < cursor.array.length ? cursor.array[cursor.index] : null;
    const method = decl.first(x,nativeSeq(x) || val(x) == null);
    if (method !== null) return method.call(x,x);
    if (nativeSeq(x) || val(x) == null) return __rt.cljFirst(x);
    const s = reduceSeqOf(x);
    if (s == null) return null;
    if (arraySeqs.has(s)) return firstOf(s);
    // Once seq has selected the receiver, ISeq dispatch also accepts an
    // Object/default extension. Taking seq again can return this same value.
    const first = decl.coreMethod('ISeq/-first',1,s,nativeSeq(s));
    if (first !== null) return first.call(s,s);
    if (nativeSeq(s)) return __rt.cljFirst(s);
    throw new Error('ISeq.-first not supported on this type');
  };
  const restOf = x => {
    const cursor = arraySeqs.get(x);
    if (cursor !== undefined) return cursor.index < cursor.array.length
      ? arraySeq(cursor.array, cursor.index + 1) ?? __rt.formList() : __rt.formList();
    const method = decl.rest(x,nativeSeq(x) || val(x) == null);
    if (method !== null) return method.call(x,x);
    if (nativeSeq(x) || val(x) == null) return __rt.cljRest(x);
    const s = reduceSeqOf(x);
    if (s == null) return __rt.formList();
    if (arraySeqs.has(s)) return restOf(s);
    const rest = decl.coreMethod('ISeq/-rest',1,s,nativeSeq(s));
    if (rest !== null) return rest.call(s,s);
    if (nativeSeq(s)) return __rt.cljRest(s);
    throw new Error('ISeq.-rest not supported on this type');
  };
  const nextOf = x => {
    const cursor = arraySeqs.get(x);
    if (cursor !== undefined) return arraySeq(cursor.array, cursor.index + 1);
    const builtin = nativeSeq(x) || val(x) == null;
    const method = decl.next(x,builtin);
    if (method !== null) return method.call(x,x);
    return builtin ? __rt.cljNext(x) : reduceSeqOf(restOf(x));
  };
  const nthOf = (x, i, fallback, supplied, throughLazy = false) => {
    if (typeof val(i) !== 'number') throw new Error('Index argument to nth must be a number');
    const indexed = decl.coreMethod('IIndexed/-nth', supplied ? 3 : 2, x, false);
    if (indexed !== null) return supplied ? indexed(x,i,fallback) : indexed(x,i);
    if (!throughLazy && !decl.implementsDirectly('ISeq', x) && !decl.implementsDirectly('ISequential', x))
      throw new Error('nth not supported on this type');
    let remaining = val(i);
    for (let s = x; s != null && remaining >= 0; s = nextOf(s), remaining--) {
      if (remaining === 0) {
        if (reduceSeqOf(s) != null) return firstOf(s);
        break;
      }
      // A heterogeneous sequence can end in an indexed collection. Keep
      // nth's original arity and pass the remaining index to that tail.
      const indexed = decl.coreMethod('IIndexed/-nth', supplied ? 3 : 2, s, false);
      if (indexed !== null) return supplied ? indexed(s,remaining,fallback) : indexed(s,remaining);
      if (reduceSeqOf(s) == null) break;
    }
    if (supplied) return fallback;
    throw new Error('Index out of bounds');
  };
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
  const partitionAllFrom = (self, n, step, c) => lazy(() => {
    const s = seqOf(c);
    return s === null ? null : __rt.cljCons(takeN(takeN, n, s), self(self, n, step, dropN(step, s)));
  });
  // Reducer callbacks run on the task VM and can suspend without replay.
  // Stop before reading the next source element and unwrap one marker only;
  // nested markers let a flattening reducer propagate termination outward.
  const storedReduction = c => {
    const value = val(c), kind = __rt.formKind(value);
    return arraySeqs.has(c) || Array.isArray(value) || typeof value === 'string'
      || kind === 'vector' || kind === 'map';
  };
  const reduceSeq = (f, init, c, initialized = true) => {
    let acc = init;
    const headBeforeCall = storedReduction(c);
    for (let s = reduceSeqOf(c); s != null; s = nextOf(s)) {
      // Seed stored two-argument reductions in the same traversal so the
      // tail's -seq is not called again before its first head is read.
      if (!initialized) { acc = firstOf(s, true); initialized = true; continue; }
      // Legacy stored collections read the item before `.call`, even with
      // a custom -seq. Sequence traversal reads `.call` before -first.
      acc = headBeforeCall ? invoke(f, acc, firstOf(s, true)) : prepareInvoke(f)(acc, firstOf(s));
      if (__rt.cljIsReduced(acc)) return __rt.cljUnreduced(acc);
    }
    return initialized ? acc : invoke(f);
  };
  const entryPart = (x, value) => {
    const builtin = __rt.formKind(val(x)) === 'vector' && !__rt.cljIsEditable(x);
    const method = value ? decl.entryVal(x,builtin) : decl.entryKey(x,builtin);
    if (method !== null) return method.call(x, x);
    // Map entries have vector storage, but are not editable vectors.
    if (builtin) return __rt.cljNth(x, value ? 1 : 0);
    throw new Error('No protocol method IMapEntry/-' + (value ? 'val' : 'key') + ' defined for this value');
  };
  const kvBuiltin = c => {
    const value = val(c), kind = __rt.formKind(value);
    return kind === 'map' || kind === 'record' || decl.mutableRecord(value) !== null
      || kind === 'vector' && __rt.cljIsEditable(c);
  };
  const reduceKv = (f, init, c) => {
    const method = decl.kvReducer(c,kvBuiltin(c));
    if (method !== null) return method.call(c, c, f, init);
    const value = val(c), kind = __rt.formKind(value);
    if (value == null) return init;
    const record = decl.mutableRecord(value);
    let entries;
    // The printer traversal already retains positional duplicates and key
    // identity, and orders hash paths (including record extensions) as the
    // legacy runtime does. No transient or callback runs while preparing it.
    if (kind === 'map' || kind === 'record') entries = __rt.formPrintItems(value);
    else if (record !== null) {
      entries = vector(record.fields.map((name,i)=>__rt.formVector(__rt.cljKeyword(name),value[record.fieldMembers[i]])));
      if (value.__extmap != null) for (const entry of arr(__rt.formPrintItems(value.__extmap))) entries = __rt.formConj(entries,entry);
    } else if (kind === 'vector' && __rt.cljIsEditable(c)) {
      for (let i = 0, n = __rt.cljCount(c); i < n; i++) {
        init = prepareInvoke(f)(init, i, __rt.cljNth(c, i));
        if (__rt.cljIsReduced(init)) return __rt.cljUnreduced(init);
      }
      return init;
    } else {
      return reduceSeq((result,entry)=>prepareInvoke(f)(result,entryPart(entry,false),entryPart(entry,true)),init,c);
    }
    for (let i = 0, n = __rt.formCount(entries); i < n; i++) {
      const entry = __rt.formNth(entries,i);
      init = prepareInvoke(f)(init,__rt.formNth(entry,0),__rt.formNth(entry,1));
      if (__rt.cljIsReduced(init)) return __rt.cljUnreduced(init);
    }
    return init;
  };
  const kvReduceProtocol = (c,f,init) => {
    const builtin = kvBuiltin(c), method = decl.kvReducer(c,builtin);
    if (method !== null) return method.call(c,c,f,init);
    if (builtin) return reduceKv(f,init,c);
    throw new Error('No protocol method IKVReduce/-kv-reduce defined for this value');
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
  const partitionAllTransform = n => rf => {
    let buffer = __rt.formVector();
    const flush = result => { const group = buffer; buffer = __rt.formVector(); return invoke(rf, result, group); };
    return reducing(rf, (result, x) => {
      buffer = __rt.formConj(buffer, x);
      return __rt.formCount(buffer) === n ? flush(result) : result;
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
    MultiFn:multi.MultiFn, MultiFnFactory:(...a)=>new multi.MultiFn(...a),
    isFn:x=>typeof val(x)==='function'||__rt.formKind(val(x))==='type'||decl.satisfies('Fn',x),
    isIFn:x=>core.isFn(x)||multi.is(x)||['keyword','symbol','vector','map','set'].includes(__rt.formKind(val(x)))||decl.satisfies('IFn',x),
    invokeMulti:(m,args)=>multi.dispatch(m,args),
    makeHierarchy:multi.makeHierarchy,getGlobalHierarchy:multi.getGlobalHierarchy,isa:multi.isa,
    parents:(...a)=>multi.relation('parents',...a),ancestors:(...a)=>multi.relation('ancestors',...a),descendants:(...a)=>multi.relation('descendants',...a),
    derive:multi.derive,underive:multi.underive,
    addMethod:(m,k,f)=>multi.change(m,'add',k,f),removeMethod:(m,k)=>multi.change(m,'remove',k),removeAllMethods:m=>multi.change(m,'reset'),
    preferMethod:multi.prefer,getMethod:multi.getMethod,methods:m=>multi.tables(m).methods,prefers:m=>multi.tables(m).prefs,
    dispatchFn:m=>multi.field(m,'dispatch_fn'),defaultDispatchVal:m=>multi.field(m,'default_dispatch_val'),
    assertions:true,
    gensym, rePattern, reSeq:(re,text)=>reSeq(reSeq,re,text),
    reFind:(re,text)=>reMatch(re,text,false), reMatches:(re,text)=>reMatch(re,text,true),
    isRegex:x=>regexValue(x) instanceof RegExp,
    invoke, truth, get, identity:x=>x, macroReaderFunction,
    printReadably:true, printLength:null, printLevel:null, printNamespaceMaps:false,
    printMeta:false, printDup:false, printFnBodies:false, printNewline:true, flushOnNewline:true,
    prStr, printStr, prnStr:(...xs)=>prStr(...xs)+'\n', printlnStr:(...xs)=>printStr(...xs)+'\n',
    prStrWithOpts:(...a)=>a.length===2?printValues(a[0],a[1]):arity(a.length),
    prnStrWithOpts:(...a)=>a.length===2?printValues(a[0],a[1])+'\n':arity(a.length),
    prWriter, prStrStar:x=>{const {buffer,writer}=stringWriter();prWriter(x,writer,null);flushWriter(writer);return buffer.text;},
    printMetaQ:(...a)=>{
      if(a.length!==2)return arity(a.length);
      if(!truth(printOption(a[0],'meta','printMeta')))return false;
      const x=val(a[1]),m=decl.mutableRecord(x)===null?__rt.cljMeta(a[1]):x.__meta;
      return m!=null;
    },
    write, flushWriter, StringBufferWriter,
    writeAll:(w,...xs)=>{for(const x of xs)write(w,x);return null;},
    prSequentialWriter:(...a)=>a.length===7?printSequential(...a):arity(a.length),
    prSeqWriter:(...a)=>{if(a.length!==3)return arity(a.length);const [xs,w,o]=a;printWriter(__rt.cljFirst(xs),w,o);for(let s=__rt.cljNext(xs);s!==null;s=__rt.cljNext(s)){write(w,' ');printWriter(__rt.cljFirst(s),w,o);}return null;},
    createNs, findNs, Namespace,
    theNs:name=>{const ns=findNs(name);if(ns===null)throw new Error('No namespace: '+core.str(name));return ns;},
    nsName:(...a)=>a.length!==1?arity(a.length):__rt.formKind(val(a[0]))==='symbol'?__rt.formName(val(a[0])):val(a[0]).name,
    sequence:x=>{const items=__rt.cljSeq(x);return items===null?__rt.formList():items;},
    array:(...a)=>a,
    list:(...a)=>list(a), vector:(...a)=>vector(a), hashMap:(...a)=>map(a), hashSet:(...a)=>set(a), set:x=>set(arr(x)), vec:x=>vector(arr(x)),
    arrayMap:(...a)=>{ if (a.length % 2) throw new Error('No value supplied for key'); return arrayMap(a); },
    sortedMap:(...a)=>__rt.formSortedMap(vector(a)),
    isSorted:x=>__rt.formKind(val(x))==='map'&&!__rt.cljIsEditable(x),
    seqToMap:value=>{
      if (__rt.cljNext(value) === null) return seq(value) === null ? arrayMap([]) : __rt.cljFirst(value);
      const values = arr(value).slice(), trailing = values.length % 2 ? values.pop() : null;
      const base = arrayMap(values);
      return trailing == null ? base : core.into(base, trailing);
    },
    keyword:(...a)=>a.length===1?__rt.cljKeyword(a[0]):a.length===2?__rt.cljKeywordNs(a[0],a[1]):arity(a.length),
    symbol:(...a)=>a.length===1?__rt.cljSymbol(a[0]):a.length===2?__rt.cljSymbolNs(a[0],a[1]):arity(a.length),
    ffirst:x=>core.first(core.first(x)),
    first:x=>__rt.cljFirst(x), second:x=>__rt.cljFirst(__rt.cljNext(x)),
    nth:(x,n,...d)=>d.length?__rt.cljNthOr(x,n,d[0]):__rt.cljNth(x,n),
    count:x=>__rt.cljCount(x),
    seq, isEmpty:x=>__rt.cljSeq(x)===null,
    rest:x=>__rt.cljRest(x), next:x=>__rt.cljNext(x),
    cons:(x,xs)=>__rt.cljCons(x,xs), concat:(...xs)=>__rt.cljConcatAll(list(xs)),
    listStar:(...xs)=>list([...xs.slice(0,-1),...arr(xs[xs.length-1])]),
    apply:(f,...xs)=>invoke(f,...xs.slice(0,-1),...arr(xs[xs.length-1])),
    update:(value,key,f,...args)=>{const old=core.get(value,key);return core.assoc(value,key,invoke(f,old,...args));},
    assoc:(m,...xs)=>{for(let i=0;i<xs.length;i+=2)m=__rt.cljAssoc(m,xs[i],xs[i+1]);return m;},
    dissoc:(m,...ks)=>each((m,k)=>__rt.cljDissoc(m,k),m,ks),
    conj:(...a)=>a.length===0?__rt.formVector():each((m,x)=>__rt.cljConj(m,x),a[0],a.slice(1)),
    merge:(...xs)=>xs.some(truth)?xs.reduce((left,right)=>val(right)==null?left:__rt.cljConj(truth(left)?left:arrayMap([]),right)):null,
    disj:(s,...xs)=>each((s,x)=>__rt.cljDisj(s,x),s,xs),
    contains:(c,k)=>__rt.cljContains(c,k),
    keys:m=>{if(__rt.formKind(val(m))==='record'){const s=__rt.cljSeq(m);return s===null?null:list(arr(s).map(e=>__rt.formNth(e,0)));}
      const k=__rt.formValue(m)==null?null:__rt.formKeys(__rt.formValue(m));return k===null||__rt.formCount(k)===0?null:list(arr(k));},
    vals:m=>{const s=__rt.cljSeq(m);return s===null?null:list(arr(s).map(e=>__rt.formNth(e,1)));},
    meta:x=>__rt.cljMeta(x), withMeta:(x,m)=>__rt.cljWithMeta(x,m),
    varyMeta:(x,f,...args)=>{const m=invoke(f,__rt.cljMeta(x),...args);return val(x)==null?null:__rt.cljWithMeta(x,m);},
    isKeyword:x=>__rt.formKind(val(x))==='keyword', isSymbol:x=>__rt.formKind(val(x))==='symbol',
    isQualifiedKeyword:x=>__rt.formKind(val(x))==='keyword'&&__rt.formNamespace(val(x))!==null,
    isSequential:x=>{const v=val(x),k=__rt.formKind(v);return ['list','vector','lazy-seq','cons'].includes(k)||(k==='compiler-literal'&&__rt.formName(v)==='queue')||decl.satisfies('ISequential',x);},
    isVector:x=>__rt.formKind(val(x))==='vector', isMap:x=>{const k=__rt.formKind(val(x));return k==='map'||k==='record';}, isSeq:x=>{const k=__rt.formKind(val(x));return k==='list'||k==='lazy-seq'||k==='cons';},
    isSet:x=>__rt.formKind(val(x))==='set',
    isNil:x=>val(x)==null, isSome:x=>val(x)!=null, isFalse:x=>__rt.cljIsFalse(x), isTrue:x=>val(x)===true, isString:x=>typeof val(x)==='string', not:x=>!truth(x), boolean:truth,
    reduced:x=>__rt.cljReduced(x), isReduced:x=>__rt.cljIsReduced(x), unreduced:x=>__rt.cljUnreduced(x),
    ensureReduced:x=>__rt.cljIsReduced(x)?x:__rt.cljReduced(x),
    equal:(...xs)=>xs.every((x,i)=>i===0||__rt.formEquals(xs[i-1],x)), hash:x=>__rt.formHash(x),
    notEqual:(...xs)=>!core.equal(...xs),
    name:x=>{if(typeof val(x)==='string')return x;if(__rt.formKind(val(x))==='compiler-literal')throw new Error("Doesn't support name on this type");if(__rt.formKind(val(x))==='type')throw new Error('Doesn\'t support name: '+__rt.formName(val(x)));return __rt.formName(val(x));},
    namespace:x=>{if(__rt.formKind(val(x))==='compiler-literal')throw new Error("Doesn't support namespace on this type");if(__rt.formKind(val(x))==='type')throw new Error('Doesn\'t support namespace: '+__rt.formName(val(x)));return __rt.formNamespace(val(x));},
    subs:(...a)=>a.length===2?val(a[0]).substring(val(a[1])):a.length===3?val(a[0]).substring(val(a[1]),val(a[2])):arity(a.length),
    inc:x=>val(x)+1, dec:x=>val(x)-1, multiply:(...xs)=>xs.reduce((a,b)=>a*val(b),1),
    less:(...xs)=>chain(xs,(a,b)=>a<b), greater:(...xs)=>chain(xs,(a,b)=>a>b), lessEqual:(...xs)=>chain(xs,(a,b)=>a<=b), greaterEqual:(...xs)=>chain(xs,(a,b)=>a>=b),
    str:(...xs)=>xs.map(x=>{x=val(x);if(x==null)return '';const kind=__rt.formKind(x);
      if(kind==='compiler-literal'){
        const tag=__rt.formName(x),payload=__rt.formLiteralPayload(x);
        if(tag==='js-array')return arr(payload).map(v=>val(v)==null?'':core.str(v)).join(',');
        if(tag==='js-object'){
          const object={},pairs=literalObjectFields(x);
          for(let i=0;i<pairs.length;i+=2)Object.defineProperty(object,pairs[i],{value:val(pairs[i+1]),enumerable:true});
          return String(object);
        }
        if(tag==='regex')return String(regexValue(x));
        if(tag==='instant')return String(new Date(val(payload)));
        if(tag==='uuid')return val(payload);
      }
      if(kind==='reduced'||__rt.cljIsAtom(x))return '[object Object]';
      if(kind==='symbol'||kind==='keyword'){const ns=__rt.formNamespace(x);return (kind==='keyword'?':':'')+(ns===null?'':ns+'/')+__rt.formName(x);}
      if(kind==='type')return __rt.formName(x);
      if(kind==='struct'||kind==='record')return String(interop(x));
      if(kind!==null){const saved=core.printReadably;core.printReadably=true;try{const {buffer,writer}=stringWriter();printWriterImpl(x,writer,null,true);flushWriter(writer);return buffer.text;}finally{core.printReadably=saved;}}return String(x);}).join(''),
    identical:(a,b)=>val(a)===val(b), isInstance:(c,x)=>c===multi.MultiFn?multi.is(x):__rt.formKind(c)==='type'?__rt.structTypeOf(x)===c:val(x) instanceof c, isRecord:x=>__rt.formKind(val(x))==='record',
    add:(...xs)=>xs.reduce((a,b)=>a+val(b),0), subtract:(a,...xs)=>xs.length?xs.reduce((a,b)=>a-val(b),val(a)):-val(a),
    divide:(...xs)=>xs.length===0?NaN:xs.length===1?1/val(xs[0]):xs.slice(1).reduce((a,b)=>a/val(b),val(xs[0])),
    int:x=>val(x)|0, long:x=>fix(val(x)), byte:x=>x, short:x=>x, float:x=>x, double:x=>x,
    quot:(n,d)=>quot(val(n),val(d)), rem:(n,d)=>{n=val(n);d=val(d);return n-d*quot(n,d);},
    mod:(n,d)=>{n=val(n);d=val(d);return ((n%d)+d)%d;},
    numericEqual:(...xs)=>xs.length!==0&&core.equal(...xs),
    isArray:x=>{x=val(x);return __rt.formKind(x)==='compiler-literal'?__rt.formName(x)==='js-array':Array.isArray(x);},
    isObject:x=>{x=val(x);const kind=__rt.formKind(x);if(kind==='compiler-literal'){
      if(__rt.formName(x)!=='js-object')return false;
      const pairs=literalObjectFields(x);
      for(let i=0;i<pairs.length;i+=2)if(pairs[i]==='constructor')return val(pairs[i+1])===Object;
      return true;
    }return kind===null&&!__rt.cljIsAtom(x)&&x!=null&&x.constructor===Object;},
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
    filter:(...a)=>a.length===1?filterTransform(a[0]):a.length===2?select(select,a[0],a[1],0):arity(a.length),
    remove:(...a)=>a.length===1?filterTransform(x=>!truth(invoke(a[0],x))):a.length===2?select(select,a[0],a[1],1):arity(a.length), keep:(f,c)=>select(select,f,c,2),
    mapcat:(...a)=>a.length===0?arity(0):a.length===1?mapcatTransform(a[0]):catSeqs(catSeqs,core.mapSeq(...a)),
    partitionBy:(...a)=>a.length===1?partitionByTransform(a[0]):a.length===2?partitionByFrom(partitionByFrom,a[0],a[1]):arity(a.length),
    partitionAll:(...a)=>a.length===1?partitionAllTransform(val(a[0])):a.length===2?partitionAllFrom(partitionAllFrom,val(a[0]),val(a[0]),a[1]):a.length===3?partitionAllFrom(partitionAllFrom,val(a[0]),val(a[1]),a[2]):arity(a.length),
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
    reduce:(f,...a)=>{if(a.length===2)return reduceSeq(f,a[0],a[1]);if(a.length!==1)arity(a.length+1);
      if(storedReduction(a[0]))return reduceSeq(f,undefined,a[0],false);
      const s=reduceSeqOf(a[0]);return s==null?invoke(f):reduceSeq(f,firstOf(s),nextOf(s));},
    reduceKv, kvReduceProtocol,
    keyProtocol:x=>entryPart(x,false), valProtocol:x=>entryPart(x,true),
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
      if(bindingOverlays.has(name))return binding||bindingOverlays.get(name)!==undefined;
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
    registerReaderNamespace:name=>createNs(__rt.cljSymbol(val(name))), addReaderDefinitions:()=>null,
    PersistentVector:{EMPTY:__rt.formVector()}, PersistentArrayMap:{EMPTY:arrayMap([])}, List:{EMPTY:__rt.formList()},
    PersistentHashSet:{EMPTY:__rt.formSet()},
    Keyword:{}, Symbol:{}, PersistentHashMap:{EMPTY:__rt.formMap(), fromArrays:(ks,vs)=>{
      let m=__rt.formMap();for(let i=0;i<ks.length;i++)m=__rt.cljAssoc(m,ks[i],vs[i]);return m;
    }}, EmptyList:{},
  };
  currentNamespace=createNs(__rt.cljSymbol('cljs.user'));
  Object.defineProperty(core,'currentNs',{
    get:()=>{
      const frame=namespaceFrames[namespaceFrames.length-1];
      if(frame===undefined||frame.bound)return currentNamespace;
      const cell=__rt.effect(65532,0);if(cell!==0)__rt.need(cell,5);
      const context=__rt.need(__rt.effect(65530,frame.unit),7),value=__rt.formNth(context,1);
      const kind=__rt.formNth(context,0);
      if(kind===0)return value;
      if(kind===1)return createNs(value);
      if(!explicitNamespaces.has(context))explicitNamespaces.set(context,new Namespace(null,value));
      return explicitNamespaces.get(context);
    },
    set:value=>{currentNamespace=value;const frame=namespaceFrames[namespaceFrames.length-1];if(frame!==undefined)frame.bound=true;}
  });
  // `extend-type` of a core type extends the native form kind it is; array
  // and hash maps are one native kind (light-cljs-478).
  for (const [name, key] of [['Keyword','keyword'],['Symbol','symbol'],['PersistentVector','vector'],
    ['PersistentArrayMap','map'],['PersistentHashMap','map'],['PersistentHashSet','set'],['List','list'],['EmptyList','empty-list']])
    decl.coreKeys.set(core[name], key);
  /* NATIVE_READER */
  for (const name of [/* CORE_KEYS */]) if (!(name in core)) Object.defineProperty(core,name,{get:unsupported('core.'+name)});
  const rt={core,
    // Inspect data properties without running an unsupported entry's getter.
    // Units keep that getter's access at the original expression site.
    nativeCoreBinding:name=>Object.getOwnPropertyDescriptor(core,name),
    nativeInterop:interop,declarationReceiver:x=>x,
    protocol:name=>Object.freeze({name}),
    protocolMethod:(key,name,arities,viaMetadata)=>decl.method(key,name,arities,viaMetadata===true),
    protocolPredicate:(protocol,key,x,direct)=>direct?decl.implementsDirectly(key,x):decl.satisfies(key,x),
    extendType:(target,specs)=>decl.extend(target,specs),
    extendInstance:unsupported('extendInstance'),
    nativeType:(fields,specs,record,name,view,mutable,ty,fieldMembers)=>decl.type(fields,specs,view,mutable,ty,fieldMembers),
    keyword:(ns,n)=>__rt.formKeyword(ns,n),symbol:(ns,n)=>__rt.formSymbol(ns,n),get,
    vector:(...xs)=>vector(xs),vectorFromArray:vector,listFromArray:list,mapFromArray:literalMap,arrayMapFromArray:arrayMap,constantMapFromArray:constantMap,
    mapFromArrays:(ks,vs)=>map(ks.flatMap((k,i)=>[k,vs[i]])),
    mapShape:keys=>keys,mapFromShape:(ks,vs)=>literalMap(ks.flatMap((k,i)=>[k,vs[i]])),
    setFromArray:set,arraySetFromArray:set,constantSetFromArray:constantSet,literalSetFromArray:literalSet,
    fnRest:(args,n)=>list(Array.prototype.slice.call(args,n)),
    makeIFn:f=>f,
    compilerForms:{
      jsArray:items=>__rt.formCompilerLiteral('js-array',vector(items)),
      jsObject:pairs=>__rt.formCompilerLiteral('js-object',constantMap(literalObjectPairs(pairs))),
      withMeta:(value,meta)=>__rt.cljWithMeta(value,meta),
      gensymScope:(serial,prefix)=>{gensymScope=serial;gensymPrefix=prefix;return null;},takeEncodePeak:()=>0,
      namespaceContext:()=>{
        if(!(currentNamespace instanceof Namespace))return __rt.formVector(0,currentNamespace);
        const name=currentNamespace.name,kind=__rt.formKind(val(name));
        const interned=kind==='symbol'&&namespaces.get(namedText(name))===currentNamespace;
        const context=__rt.formVector(interned?1:2,name);
        if(!interned)explicitNamespaces.set(context,currentNamespace);
        return context;
      },
      namespaceBound:()=>namespaceFrames.some(frame=>frame.bound),
      enterNamespace:unit=>{namespaceFrames.push({unit,saved:currentNamespace,bound:false});return null;},
      leaveNamespace:()=>{currentNamespace=namespaceFrames.pop().saved;return null;},
      takeGensyms:()=>__rt.formVector(__rt.effect(65531,'t')),
      raiseGensyms:n=>{__rt.effect(65531,'r'+n);return null;}},
  };
  return rt;
})();
globalThis.__lightNativeRuntime=native;
const roots=new Map();
// A bundle replaces an unavailable declaration's getter. Remember that it
// defines the name, retaining the unit record for later ordinary registrations.
globalThis.__lightInstallCompilerProduct=(name,read)=>{
  const previous=roots.get(name);
  if(previous!==undefined)previous.defined=true;
  else roots.set(name,{unit:null,defined:true});
  Object.defineProperty(globalThis.__lightCompilerRoots,name,{configurable:true,get:read});
};
// Each invocation of this runtime belongs to one VM task. Overlay snapshots
// stay in its saved frames across need/producer waits and context lending;
// saving a binding must never force, replace, or reset a root's slot.
const bindingOverlays=new Map();
globalThis.__lightSaveCompilerBinding=name=>[bindingOverlays.has(name),bindingOverlays.get(name)];
globalThis.__lightBindCompilerRoot=(name,value)=>{
  // A bound read can initialize a shared root without touching an atom.
  // Enter the caller's existing effect lane before installing its context,
  // so the first source-order demand determines that root's value too.
  const cell=__rt.effect(65532,0);if(cell!==0)__rt.need(cell,5);
  bindingOverlays.set(name,value);
};
globalThis.__lightRestoreCompilerBinding=(name,saved)=>{
  if(saved[0])bindingOverlays.set(name,saved[1]);else bindingOverlays.delete(name);
};
// Bound set! writes the overlay, including when its saved value is nil or
// false. Keep ordinary writes on the existing property path; the engine's
// Proxy set trap is not needed for compiler-owned bindings.
globalThis.__lightSetCompilerRoot=(name,value)=>{
  if(bindingOverlays.has(name))bindingOverlays.set(name,value);
  else globalThis.__lightCompilerRoots[name]=value;
  return value;
};
globalThis.__lightCompilerRootKept=name=>{
  const bound=bindingOverlays.has(name)&&bindingOverlays.get(name)!==undefined;
  if(decl.productOwner(name)!==null)return decl.keptProduct(name)||bound;
  if(roots.has(name)){const root=roots.get(name);return root.unit!==null||root.defined===true||bound;}
  return Object.getOwnPropertyDescriptor(globalThis.__lightCompilerRoots,name)!==undefined||bound;
};
globalThis.__lightDefineCompilerRoot=(name,initialize,once)=>{
  if(once&&globalThis.__lightCompilerRootKept(name))return null;
  if(decl.defineProduct(name,initialize))return globalThis.__lightCompilerBundleProducts[name];
  if(initialize===null)return undefined;
  const value=initialize();
  if(bindingOverlays.has(name))bindingOverlays.set(name,value);
  else {const previous=roots.get(name);if(previous!==undefined)previous.defined=true;Object.defineProperty(globalThis.__lightCompilerRoots,name,{configurable:true,writable:true,value});}
  return value;
};
let initializerSerial=0;
const initializerStack=[];
globalThis.__lightInitializerUnit=null;
globalThis.__lightInitializerParent=null;
globalThis.__lightInitializerResult=null;
globalThis.__lightInitializerFailure=null;
globalThis.__lightInitializerRequireShared=false;
globalThis.__lightInitializerSelect=false;
globalThis.__lightRegisterUnit=(name,unit,once=false)=>{
  if(!roots.has(name)&&(name in globalThis.__lightCompilerRoots))return null;
  if(roots.has(name)&&roots.get(name).unit===unit)return null;
  // A defonce keeps an earlier definition (not a declaration), as the
  // legacy runtimes' registry does.
  if(once&&roots.has(name)&&(roots.get(name).unit!==null||roots.get(name).defined===true))return null;
  const slot={unit,name,state:0,value:null};roots.set(name,slot);
  Object.defineProperty(globalThis.__lightCompilerRoots,name,{configurable:true,get:()=>{
    if(unit===null)throw new Error('unavailable definition: '+name);
    __rt.need(unit,2);
    const factory=__rt.need(unit,3);
    return initializeValue(slot,factory);
  }});return null;
};
const initializeValue=(slot,factory)=>{
    const unit=slot.unit;
    if(slot.state===2){__rt.need(unit,4);return slot.value;}
    if(slot.state===1)throw new Error('root initialization cycle: '+(slot.name??unit));
    if(slot.state===3)throw slot.value;
    slot.state=1;
    try{
      const serial=++initializerSerial;
      globalThis.__lightInitializerUnit=unit;
      globalThis.__lightInitializerParent=initializerStack[initializerStack.length-1]??null;
      // Capture without reading the dynamic Var. Rust retains snapshots for
      // direct callers and nested initializer ancestry.
      globalThis.__lightInitializerNamespace=native.compilerForms.namespaceContext();
      globalThis.__lightInitializerNamespaceBound=native.compilerForms.namespaceBound();
      // Reserve before waiting so earlier callers can lend their lane.
      // No factory code runs until the lane selects the earliest caller's
      // task, retaining its overlays without sharing task-owned objects.
      const candidate=__rt.need(serial,beginKind);
      let producer=false;
      if(candidate){
        const cell=__rt.effect(65532,0);if(cell!==0)__rt.need(cell,5);
        globalThis.__lightInitializerUnit=unit;
        globalThis.__lightInitializerSelect=true;
        try{producer=__rt.need(++initializerSerial,beginKind);}
        finally{globalThis.__lightInitializerSelect=false;}
      }
      if(producer){
        let failed=false,failure;
        initializerStack.push(unit);
        native.compilerForms.enterNamespace(unit);
        try{slot.value=factory(native);}catch(e){failed=true;failure=e;}finally{native.compilerForms.leaveNamespace();initializerStack.pop();}
        globalThis.__lightInitializerUnit=unit;
        globalThis.__lightInitializerResult=slot.value;
        globalThis.__lightInitializerFailure=failed?String(failure):null;
        const type=__rt.structTypeOf(slot.value);
        globalThis.__lightInitializerRequireShared=type!==null&&__rt.formName(type)==='cljs.core/MultiFn';
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
};
globalThis.__lightInitializeCompilerValue=(unit,factory)=>initializeValue({unit,state:0,value:null},factory);
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
globalThis.__lightCompilerRoots=new Proxy({}, {
  get:(target,name,receiver)=>bindingOverlays.has(name)?bindingOverlays.get(name):Reflect.get(target,name,receiver),
  has:(target,name)=>bindingOverlays.has(name)||Reflect.has(target,name),
});
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
    if(op===4){
      const catalog=__rt.nth(input,0);
      for(let at=__rt.nth(input,1);at<__rt.count(catalog);at++){
        const entry=__rt.nth(catalog,at);
        if(__rt.nth(entry,0)===0){
          globalThis.__lightRegisterUnit(__rt.nth(entry,1),__rt.nth(entry,2),__rt.nth(entry,3));
        }else{
          const products=__rt.nth(entry,1), extensions=__rt.nth(entry,2), names=[], indices=[];
          for(let i=0;i<__rt.count(products);i++)names.push(__rt.nth(products,i));
          for(let i=0;i<__rt.count(extensions);i++)indices.push([__rt.nth(__rt.nth(extensions,i),0)]);
          decl.register(names,indices,__rt.nth(entry,3),__rt.nth(entry,4),__rt.nth(entry,5));
        }
      }
    }
    else if(op===3){if(held&&heldId===id){held=false;heldValue=null;}else values.delete(id);}
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
