// First-party bounded native-form runtime. Mutable JS belongs to this one task.
// Protocols and types (light-cljs-478; docs/native-macros.md, "Protocols and
// types"). A protocol method's implementations are an engine dispatch table:
// a SameValueZero persistent map from a value's dispatch key (`__rt.cljDispatch`:
// a struct's type identity, a form kind, "string", "number", ..., "_" for
// `default`) to the method, which `__rt.cljDispatch` reads without property
// lookup or user equality. A table holds the extensions installed in source
// order up to the compiler's frontier: an extension's order is its
// declaration bundle's, and code run outside a bundle extends at order 0.
// Internal identity bookkeeping uses SameValueZero maps and engine CAS only;
// it never invokes user equality, effects, or publication-time task captures.
const canonicalMember = (key, make) => {
  let candidate;
  for (;;) {
    const current = __rt.atomLoad(rawMembersCell);
    const found = current === null ? undefined : __rt.getOr(current,key,undefined);
    if (found !== undefined) return found;
    if (candidate === undefined) candidate = make();
    const next = __rt.assoc(current === null ? __rt.map() : current,key,candidate);
    if (Object.is(__rt.atomCas(rawMembersCell,current,next),current)) return candidate;
  }
};
// The type tag of a mutable type's host instance, or of a transparent Proxy
// of one or of a mutable-field struct, read once as `__lightNativeType`. A
// struct may declare a field of that name, which its Proxy then answers; only
// then is the type read again under TYPE_KEY, which no compiler-munged member
// name can equal, so internal classification never takes a user's field.
const TYPE_KEY = '@lightNativeType';
/* MEMBER_MUNGING */
const typeTag = x => {
  const ty = x.__lightNativeType;
  return ty === undefined || __rt.formKind(ty) === 'type' ? ty : x[TYPE_KEY];
};
const decl = (() => {
  const tables = new Map(), extensions = new Map(), views = new Map(), frontiers = [];
  const instances=new WeakMap();
  const ownMethod=(x,name,n)=>instances.get(x)?.get(name)?.get(n);
  const extendInstance=(target,specs,returnTarget=false)=>{
    if(target!=null&&(typeof target==='object'||typeof target==='function')){
      const methods=new Map(Array.from(instances.get(target)??[],([name,arities])=>[name,new Map(arities)]));
      instances.set(target,methods);
      for(const [name,n,fn] of specs){
        let arities=methods.get(name);if(arities===undefined)methods.set(name,arities=new Map());
        arities.set(n,n===0?true:(x,...args)=>fn.call(x,x,...args));
      }
      for(const [name,arities] of methods)if(name.startsWith('Object/'))
        Object.defineProperty(target,mungeMember(name.slice(7)),{configurable:true,writable:true,value:function(...args){
          const fn=arities.size===1?arities.values().next().value:arities.get(args.length+1);
          if(fn===undefined)throw new Error('Invalid arity: '+args.length);
          return fn(this,...args);
        }});
      if(methods.has('IFn/-invoke')){
        Object.defineProperty(target,'call',{configurable:true,writable:true,value:function(_, ...args){
          const arities=instances.get(this).get('IFn/-invoke');
          const fn=arities.size===1?arities.values().next().value:arities.get(args.length+1);
          if(fn===undefined)throw new Error('Invalid arity: '+args.length);
          return fn(this,...args);
        }});
        Object.defineProperty(target,'apply',{configurable:true,writable:true,value:function(_,args){return this.call(null,...Array.from(args));}});
      }
    }
    return returnTarget?target:specs.length?specs[specs.length-1][1]===0?true:specs[specs.length-1][2]:null;
  };
  // Native lookup can resolve installed structs without repeating JavaScript
  // declaration probes. This map belongs to the task, just like views/tables.
  let declared = __rt.map();
  // All six collection predicates use partition zero. A declared mask in
  // that partition blocks host/default fallback for a missing core marker,
  // as protocolMasks/satisfies in the shared runtime do.
  const partition0 = new Set('IFn ICounted IEmptyableCollection ICollection IIndexed ASeq ISeq INext ILookup IAssociative IMap IMapEntry ISet IStack IVector IDeref IDerefWithTimeout IMeta IWithMeta IReduce IKVReduce IEquiv IHash ISeqable ISequential IList IRecord IReversible ISorted IPrintWithWriter IWriter'.split(' '));
  let order = 0, frontier = Infinity, version = 0;
  const table = (key, name, arity) => {
    let t = tables.get(key);
    if (t === undefined) tables.set(key, t = {name, arity, marker: arity===0, protocol: protocolOf(name, arity), entries: [], map: null, snapshot: null, directSnapshot: null, version: -1, frontier: NaN});
    return t;
  };
  const protocolOf = (name, arity) => arity === 0 ? name : name.slice(0, name.lastIndexOf('/'));
  const methodTable = (name, arity) => table(name + '\u0000' + arity, name, arity);
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
    // An installation into a declared type after its declaration: it
    // rewrites the type's Object methods over its IFn adapters (`objectWins`).
    // Installed by a bundle that only a protocol table build forced (for
    // every type at once), it stays inactive until the legacy runtime would
    // have installed the bundle (`activate`): a demand of one of its
    // protocols for any value one of its selectors matches (`demandFor`),
    // a product demand or an Object demand. Activation installs the whole
    // bundle, for every target. The bundle records the dispatch keys it
    // extends per protocol, to match its `@type` selectors. A bundle is
    // pending, and its protocols' dispatch skips the lookup cache so that
    // every dispatch can demand, only while it holds an installation that
    // can change a member: Object methods, or a type whose IFn adapters
    // compete with an Object call/apply.
    const bundle = installing[installing.length - 1];
    const forced = bundle !== undefined && !bundle.active;
    if (forced) for (const [name, arity] of specs) {
      const protocol = name.startsWith('Object/') ? 'Object' : protocolOf(name, arity);
      let keys = (bundle.keys ??= new Map()).get(protocol);
      if (keys === undefined) bundle.keys.set(protocol, keys = new Set());
      keys.add(key);
    }
    const lazy = forced && info !== undefined && !info.mutable && !info.declaring;
    if (lazy && !pendingBundles.has(bundle) && (specs.some(spec => spec[0].startsWith('Object/'))
        || info.ifn.length > 0 && (info.members.has('call') || info.members.has('apply')))) {
      pendingBundles.add(bundle);
      for (const protocol of new Set(bundle.indices.map(([p]) => p)))
        pendingProtocols.set(protocol, (pendingProtocols.get(protocol) ?? 0) + 1);
    }
    const from = lazy ? bundle : null;
    if (info !== undefined && !info.declaring) info.installs.push({order, overlay, bundle: from});
    for (const [name, arity, fn, , variadic, member, direct] of specs) {
      const object = name.startsWith('Object/');
      const n = object && variadic != null ? -1 : arity;
      // Native fixed entries take their hidden receiver from the first
      // operand. Adapt scope-sensitive legacy entries once at installation;
      // their raw JavaScript and `this-as` still see the original receiver.
      const entry = object || direct || arity === 0 ? fn : (x, ...args) => fn.call(x, x, ...args);
      methodTable(name, n).entries.push({order, overlay, key, fn: entry, bundle: object ? from : null});
      if (!object) continue;
      let arities = objectArities.get(name);
      if (arities === undefined) objectArities.set(name, arities = new Set());
      if (n >= 0) arities.add(n);
      if (info !== undefined) objectMember(info, name, member);
    }
    version++;
    return null;
  };
  const installing = [];
  // Inactive bundles whose installations can change a member, and how many
  // of them index each protocol: that protocol's tables keep no lookup
  // cache meanwhile (`build`).
  const pendingBundles = new Set(), pendingProtocols = new Map();
  // Mutable types (their instances are tagged host objects), and the Object
  // extension selectors that name them.
  const mutableTypes = new Set(), mutableSelectors = new Set();
  // A bundle's installations become active (visible to member reads and
  // precedence) once the legacy runtime would have installed it.
  const activate = b => {
    if (b.active) return;
    b.active = true;
    if (!pendingBundles.delete(b)) return;
    for (const protocol of new Set(b.indices.map(([p]) => p))) {
      const n = pendingProtocols.get(protocol) - 1;
      if (n === 0) pendingProtocols.delete(protocol); else pendingProtocols.set(protocol, n);
    }
    version++;
  };
  // `lazy`: forced for a protocol table build, not demanded for a type.
  const force = (b, lazy = false) => {
    if (!lazy) activate(b);
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
    const pending = protocolBundles(t.protocol);
    if (pending !== undefined) for (const b of pending) { if (b.order > frontier) break; force(b, true); }
    let map = __rt.map();
    const visible = t.entries.filter(e => (e.overlay > 0 || e.order <= frontier) && (e.bundle == null || e.bundle.active))
      .sort((a, b) => a.overlay - b.overlay || a.order - b.order);
    for (const e of visible) map = __rt.assoc(map, e.key, e.fn);
    t.map = map; t.promise=__rt.getOr(map,'js/Promise',undefined);t.version = version; t.frontier = frontier;
    t.mutable = visible.some(e => mutableTypes.has(e.key));
    // A pending bundle of this protocol may need any dispatch to demand it.
    const cached = !pendingProtocols.has(t.protocol);
    t.snapshot = cached ? __rt.vector(version, frontier, map, declared) : null;
    t.directSnapshot = cached ? __rt.vector(version, frontier, map, declared, true) : null;
  };
  // A struct of a type this task has not declared yet (a published value
  // another task made): demand the type's declaration, its product of the
  // same name, so that its implementations and view exist here too.
  const declare = x => {
    const ty = __rt.structTypeOf(x);
    if (ty === null || views.has(ty)) return false;
    const name = __rt.formName(ty);
    if (!(name in registry())) return false;
    registry()[name];
    return true;
  };
  const resolve = (t, x) => {
    // Tables carry their fixed arity independently of installed type entries.
    const instance=ownMethod(__rt.formValue(x),t.name,t.arity);
    if(instance!==undefined)return instance;
    const cached = __rt.cljProtocolLookup(t.snapshot, version, frontier, x);
    if (cached !== undefined) return cached;
    // Demand an unknown type before building the protocol's pending bundles:
    // declaration effects, errors and dependency waits keep their order.
    declare(x);
    if (t.version !== version || t.frontier !== frontier) build(t);
    demandFor(t.protocol, x);
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
      // Its tag is read only when the table extends a mutable type, the one
      // case it can select; a host array (a Proxy of one too, which the
      // engine gives no key) takes the array entry, as legacy `Array.isArray`
      // does. No other host property is read.
      const ty = struct ?? (__rt.cljIsAtom(x) || !t.mutable ? undefined : typeTag(x));
      if (ty !== undefined && struct === null) demandFor(t.protocol, x, ty);
      if (ty !== undefined) m = __rt.getOr(t.map, ty, undefined);
      if (m === undefined && struct === null && Array.isArray(__rt.formValue(x))) m = __rt.getOr(t.map, 'array', undefined);
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
  const sequenceKinds = __rt.map('list','list','empty-list','empty-list','sequence','sequence','uncounted-sequence','sequence');
  const vectorKinds = __rt.map('vector','vector','map-entry','map-entry');
  // The direct key of a native form that is not a struct, from the
  // engine's form kind alone: no host object property is read.
  const formKey = (value, kind) => {
    if (kind === 'list') return __rt.cljDispatch(sequenceKinds,value);
    if (kind === 'vector') return __rt.cljDispatch(vectorKinds,value);
    return directKinds.has(kind) ? kind : null;
  };
  const directKey = x => {
    const ty = __rt.structTypeOf(x);
    if (ty !== null) return ty;
    const value = __rt.formValue(x), kind = __rt.formKind(value);
    if (kind !== null) return formKey(value, kind);
    if (value instanceof Promise) return 'js/Promise';
    return value !== null && typeof value === 'object' ? typeTag(value) ?? null : null;
  };
  const implementsDirectly = (key, x) => {
    if(ownMethod(__rt.formValue(x),key,0)!==undefined)return true;
    const t = methodTable(key, 0);
    resolve(t, x);
    const direct = directKey(x);
    return direct !== null && __rt.getOr(t.map, direct, null) !== null;
  };
  const ownedKinds = new Set(['keyword','symbol','list','vector','map','record','set','lazy-seq','cons','reduced']);
  const predicate = (key, x, owned = false) => {
    if (implementsDirectly(key,x)) return true;
    const value=__rt.formValue(x),kind=__rt.formKind(value);
    if (owned || ownedKinds.has(kind) || __rt.cljIsAtom(x)
        || kind==='compiler-literal' && __rt.formName(value)==='queue') return false;
    const ty=directKey(x);
    // Only the original declaration installs a mask. Later extend-type
    // bundles and overlays add direct markers without changing that mask.
    if (views.get(ty)?.mask0) return false;
    return satisfies(key,x);
  };
  // Owned core implementations precede object/default extensions. Explicit
  // extensions of the receiver's native type still replace that implementation.
  const coreLookup = (t, x, builtin, directOnly = false) => {
    const own=ownMethod(__rt.formValue(x),t.name,t.arity);
    if(own!==undefined)return own;
    if (directOnly && !builtin && !implementsDirectly(t.protocol, x)) return null;
    let m;
    if (builtin) {
      m = __rt.cljProtocolLookup(t.directSnapshot, version, frontier, x);
      if (m === undefined) {
        declare(x);
        if (t.version !== version || t.frontier !== frontier) build(t);
        const key = directKey(x);
        demandFor(t.protocol, x, key);
        m = key === null ? null : __rt.getOr(t.map, key, null);
      }
    } else m = resolve(t, x);
    // Membership without this arity is an error, unless the engine owns
    // the implementation (notably the built-in methods of records).
    if (m === null && !builtin && satisfies(t.protocol, x))
      throw new Error('No protocol method ' + t.name + ' defined for this value');
    return m;
  };
  const resolveCore = (name, arity, x, builtin, directOnly = false) =>
    coreLookup(methodTable(name, arity), x, builtin, directOnly);
  // The implementation for the type `ty` (a struct type, or a mutable
  // type's key) in the table of method `name` and arity `n`, at the current
  // frontier. An `Object` method selects by its declaring type, so the
  // receiver need not be a `ty` (`(.call f other)`).
  const implementation = (name, n, ty) => {
    const t = methodTable(name, n);
    if (t.version !== version || t.frontier !== frontier) build(t);
    return __rt.getOr(t.map, ty, null);
  };
  // A call of the JavaScript member `member` of the type `info` describes,
  // with receiver `x`: the method of the last group (by source name, in the
  // order first extended) with an implementation visible at the frontier,
  // whose overload is selected as the legacy `installMethods` does.
  const objectInvoke = (info, member, x, args) => {
    const names = info.members.get(member);
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
  // instances inherit it, and a struct answers it through `memberFunction`.
  // Methods whose names munge alike share the member ([`objectInvoke`]).
  const objectMember = (info, name, member) => {
    if (info.names.has(name)) return;
    info.names.add(name);
    const names = info.members.get(member);
    if (names !== undefined) { names.push(name); return; }
    info.members.set(member, [name]);
    if (info.mutable) info.view.prototype[member] = function (...a) { return objectInvoke(info, member, this, a); };
  };
  // A type: a native struct type, or for raw bodies and mutable records the
  // constructor of task-owned JavaScript instances (`view`, unused for a struct type:
  // a struct is its own JavaScript object, see `jsMember`).
  const type = (fields, specs, view, mutable, ty, fieldMembers) => {
    const sample = __rt.structNew(ty), record = __rt.formKind(sample) === 'record';
    const mutableFields = __rt.structState(sample) !== 0;
    const mask0 = record || specs.some(([name, arity]) => arity === 0 && partition0.has(name));
    // Keep one compiler-munged member per declared field: multiple fields
    // may share a member, and property enumeration may reorder names.
    // Raw call/apply use the type's declared IFn entries. Protocol extensions
    // do not replace those entries or install JavaScript callable members.
    // `installs` are the later installations into the type (`extend`).
    const ifn = specs.filter(s=>s[0]==='IFn/-invoke').map(s=>[s[1],s[2],s[4]!=null,s[3]??s[2]]);
    const info = {ty, view, fields, fieldMembers, names: new Set(), members: new Map(), memberFunctions: new Map(), ifn, installs: [], declaring: true, mutable, mutableFields, record, mask0};
    views.set(ty, info);
    declared = __rt.assoc(declared, ty, true);
    if (mutable) {
      view.__lightNativeType = ty;
      view.prototype.__lightNativeType = ty;
      view.prototype[TYPE_KEY] = ty;
    }
    if (mutable || mutableFields) {
      mutableTypes.add(ty);
      mutableSelectors.add('@' + __rt.formName(ty));
    }
    try { extend(ty, specs); } finally { info.declaring = false; }
    return mutable ? view : ty;
  };
  const ifnMethods = x => {
    declare(x);
    return views.get(__rt.structTypeOf(x))?.ifn ?? [];
  };
  const callable = x => ifnMethods(x).length > 0;
  const callMember = x => {
    const methods = ifnMethods(x), rest = methods.find(m=>m[2]);
    if (rest === undefined) return null;
    const ty = __rt.structTypeOf(x), overloaded = methods.some(m=>!m[2]);
    return canonicalMember(ty,()=>__rt.fnApplyNull(function(...args) {
      // The single variadic method is the legacy `call` member itself and
      // runs with any receiver; an overloaded dispatcher needs the
      // receiver's IFn methods.
      if (overloaded && __rt.structTypeOf(this) === null) throw new TypeError('undefined is not a function');
      return globalThis.__lightNativeRuntime.core.borrowedIFn(ty,overloaded,this,args);
    }));
  };
  const borrowedIFn = (ty,overloaded,receiver,args) => {
    if (overloaded) return invokeIFn(receiver,args.slice(1));
    declare(__rt.structNew(ty));
    const method = views.get(ty).ifn.find(m=>m[2])[1];
    return method.call(receiver,...args);
  };
  const invokeIFn = (x, args) => {
    const methods=ifnMethods(x);
    if(methods.length===0)throw new TypeError('undefined is not a function');
    let fixed = null, rest = null, exact = null, count = 0;
    for (const [n, fn, variadic] of methods) {
      if (variadic) rest = fn;
      else { count++; fixed = fn; if (n === args.length + 1) exact = fn; }
    }
    const method = rest ? exact ?? rest : count === 1 ? fixed : exact;
    if (method == null) throw new Error('Invalid arity: ' + args.length);
    const receiver = rest && count > 0 && exact === null ? rest : x;
    return method.call(receiver, receiver, ...args);
  };
  // Clojure apply reads the direct invoke slot first. Its receiver differs
  // from an overloaded .call's variadic fallback, and recur retains it.
  // Without one it calls the `call` member the precedence rule selects as
  // legacy apply does, `f.call(f, ...args)`: the leading operand is the
  // callable itself, not null.
  const applyIFn = (x, args) => {
    if (args.length <= 20) {
      const entry = ifnMethods(x).findLast(([n]) => n === args.length + 1);
      if (entry !== undefined) return entry[3].call(x, x, ...args);
      const call = x.call;
      if (typeof call !== 'function') throw new TypeError('undefined is not a function');
      return Reflect.apply(call, x, [x, ...args]);
    }
    return x.apply(null,args);
  };
  // The declaration of a struct's type in this task, demanded if cold.
  const structInfo = s => {
    declare(s);
    const info = views.get(__rt.structTypeOf(s));
    if (info === undefined) throw new Error('unsupported operation: JavaScript interop with a native struct of an undeclared type');
    return info;
  };
  // Whether the type `info` describes has an Object method installed as
  // the member `member` and visible at the frontier.
  const hasObjectMember = (info, member) => {
    const names = info.members.get(member);
    return names !== undefined && names.some(name => implementation(name, -1, info.ty) != null
      || [...(objectArities.get(name) ?? [])].some(n => implementation(name, n, info.ty) != null));
  };
  // Rule 2 of docs/native-macros.md, "Member precedence: one rule": whether
  // a struct's member `member` is its type's Object method rather than its
  // IFn adapter. Every installation into a type writes the Object methods
  // it has so far, and only the declaration writes IFn adapters after them,
  // so the adapters win only while no later installation is visible.
  const objectWins = (info, member) => hasObjectMember(info, member)
    && (info.ifn.length === 0 || member !== 'call' && member !== 'apply'
      || info.installs.some(e => (e.overlay > 0 || e.order <= frontier) && (e.bundle === null || e.bundle.active)));
  // The legacy runtime's compiled-interop demand (`declarationReceiver`):
  // install the Object extension bundles registered up to the frontier for
  // the type of `x` (a struct, an object inheriting from one, or a mutable
  // type's instance), in source order. Nothing else installs them, so a raw
  // member read, a call or a presence check sees only what is installed.
  const objectBundles = [];
  // The legacy demand of `protocol` for the value `x`: activate every
  // pending bundle registered up to the frontier with an index of that
  // protocol whose selector matches `x` as the legacy runtime's does (none,
  // default, the value's JavaScript type, or a type the bundle extends for
  // the protocol that `x` belongs to).
  // It reads no host object's properties: a struct's type and a form's kind
  // come from the engine; a mutable type instance's tag only from a caller
  // that has read it already (`hostKey`), as dispatch does.
  const demandFor = (protocol, x, hostKey = null) => {
    if (pendingBundles.size === 0) return;
    const value = __rt.formValue(x), ty = __rt.structTypeOf(x), kind = __rt.formKind(value);
    const key = ty !== null ? ty : kind !== null ? formKey(value, kind) : hostKey;
    const native = value == null ? 'null' : Array.isArray(value) ? 'array' : typeof value;
    for (const b of [...pendingBundles])
      if (b.order <= frontier && b.indices.some(([p, s]) => p === protocol
          && (s === null || s === '_' || s === native || s.startsWith('@') && key !== null && b.keys?.get(protocol)?.has(key) === true)))
        activate(b);
  };
  const demandObject = x => {
    if (lazyCatalog !== null) catalogProtocol('Object');
    // Once every Object bundle up to the frontier is installed there is
    // nothing to demand, and the receiver is not inspected at all.
    if (!objectBundles.some(({b}) => b.order <= frontier && (b.state !== 2 || !b.active))) return;
    // As the legacy demand does for every receiver, a bundle with no
    // selector (an alias or other unnamed target), `default` or the
    // receiver's JavaScript type is installed whatever the receiver is.
    // Only a `@type` selector needs the receiver's own type, found on first
    // need: a struct's (or the struct an object inherits from), or a
    // mutable type instance's tag, read only for a selector naming a
    // mutable type.
    const value = __rt.formValue(x);
    const native = value == null ? 'null' : Array.isArray(value) ? 'array' : typeof value;
    let s, structType, mutableSelector;
    const owns = selector => {
      if (s === undefined) {
        s = structOf(x); structType = __rt.structTypeOf(s);
        if (structType !== null) declare(s);
      }
      if (structType !== null) return selector === '@' + __rt.formName(structType);
      if (!mutableSelectors.has(selector) || s === null || typeof s !== 'object'
          || __rt.formKind(s) !== null || __rt.cljIsAtom(s)) return false;
      if (mutableSelector === undefined) {
        // Snapshot even a missing tag: getters and Proxy traps run at most
        // once for this demand, regardless of the number of selectors.
        const ty = typeTag(s);
        const info = views.get(ty);
        mutableSelector = info !== undefined && (info.mutable || info.mutableFields) ? '@' + __rt.formName(ty) : null;
      }
      return selector === mutableSelector;
    };
    for (const {b, selector} of objectBundles) {
      if (b.order > frontier) break;
      if (installing.includes(b)) continue;
      if (selector === null || selector === '_' || selector === native || owns(selector)) force(b);
    }
  };
  // The struct an object inheriting from one (`Object.create`) stands for.
  // The struct an object inheriting from one (`Object.create`) stands for,
  // for its Object methods; the callable adapters take their receiver as
  // is, like the legacy ones.
  const structOf = o => {
    for (let p = __rt.formValue(o); p !== null && typeof p === 'object'; p = Object.getPrototypeOf(p))
      if (__rt.formKind(p) !== null) return p;
    return o;
  };
  // The function a struct's JavaScript member `member` reads: one per type
  // and member, dispatching on the declaring type with the call's `this`
  // (the struct itself, an object inheriting from it, or a borrowed
  // receiver) as the method's receiver.
  const memberFunction = (info, member) => {
    let f = info.memberFunctions.get(member);
    // A mutable-field type keeps the call's receiver, as its legacy
    // prototype member does: an inheriting object's stores stay its own.
    if (f === undefined) info.memberFunctions.set(member, f = function (...a) { return objectInvoke(info, member, info.mutableFields ? this : structOf(this), a); });
    return f;
  };
  // A bundle's declaration order is its position among every bundle the
  // task registered, a catalog prefix it adopted included (`lazyAdopt`).
  let bundleCount = 0;
  const makeBundle = (names, indices, location, unit, order) => ({order, names, unit, location, state: 0, error: null, products: null, active: false,
    indices: indices.map(([protocol, selector = null]) => [protocol, selector]),
    multi: indices.length > 0 && indices.every(index => index[0].startsWith('multifn:'))});
  const productGetter = (b, name) => () => { force(b); return b.products[name]; };
  // Index `b`'s extensions of `protocol` (every one when it is null).
  const index = (b, protocol = null) => {
    for (const [p, selector] of b.indices) {
      if (protocol !== null && p !== protocol) continue;
      // Object extensions are installed by demand only (`demandObject`).
      if (p === 'Object') { objectBundles.push({b, selector}); continue; }
      let list = extensions.get(p);
      if (list === undefined) extensions.set(p, list = []);
      list.push(b);
    }
  };
  // The bundles extending `protocol` in order, an adopted catalog's included.
  const protocolBundles = protocol => {
    if (lazyCatalog !== null) catalogProtocol(protocol);
    return extensions.get(protocol);
  };
  const register = (names, indices, location, once, unit) => {
    const b = makeBundle(names, indices, location, unit, ++bundleCount);
    if (once && names.length && names.every(name => globalThis.__lightCompilerRootKept(name))) return null;
    for (const name of names) {
      // A preloaded root (a data property) stays; a product replaces an
      // earlier definition's.
      lazyResolve(name);
      const old = Object.getOwnPropertyDescriptor(registry(), name);
      if (old !== undefined && 'value' in old) continue;
      globalThis.__lightInstallCompilerProduct(name, productGetter(b, name), b);
    }
    if (lazyCatalog !== null) for (const [protocol] of b.indices) catalogProtocol(protocol);
    index(b);
    return null;
  };
  const enter = next => { frontiers.push(frontier); frontier = next; return null; };
  const leave = () => { frontier = frontiers.pop() ?? Infinity; return null; };
  // Multimethod declarations use the same source frontier as protocols.
  // Installing a method must not recursively demand its own bundle.
  const demandMulti = name => {
    const pending = [...(protocolBundles('multifn:' + name) ?? []), ...(protocolBundles('multifn:*') ?? [])]
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
    const info = views.get(typeTag(x));
    return info?.mutable && info.record ? info : null;
  };
  return {makeBundle, productGetter, index, reserve: n => { const base = bundleCount; bundleCount += n; return base; },
    extend, extendInstance, inheritInstance:(x,parent)=>instances.set(x,instances.get(parent)), method, satisfies, implementsDirectly, predicate, type, structInfo, objectWins, demandObject, memberFunction, register, enter, leave, coreKeys, mutableRecord,
    callable, invokeIFn, applyIFn, callMember, borrowedIFn,
    demandMulti, keptProduct, productOwner, defineProduct,
    multiOrder:()=>order, multiFrontier:()=>frontier, installingMulti:()=>installing.some(b=>b.multi),
    coreMethod:resolveCore,
    coreTable:methodTable, coreLookup,
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
  let gensymScope = 0, gensymPrefix = '', gensymNative = true;
  // Explicit data readers count independently of macro expansion, as in the
  // separate reader heap of the V8 build. This state follows the reader task.
  const readerGensymScopes = new Map();
  const val = x => __rt.formValue(x);
  // Compiled interop (`(.-m x)`, `(.m x a)`, host calls, `instance?`): a
  // native form is the same object in JavaScript, so nothing is converted;
  // only occurrence provenance wrappers come off. Raw JavaScript reads a
  // form's members through `jsMember` below. Explicit compiled interop with
  // a non-struct form stays unsupported, as its legacy object layout is not
  // modelled, except `.call`/`.apply`, which apply a function to operands
  // that pass through unchanged.
  const interop = (x, member) => {
    const value = val(x), kind = __rt.formKind(value);
    if (kind === 'compiler-literal' && __rt.formName(value) === 'regex') return value;
    if (kind !== null && kind !== 'struct' && kind !== 'record' && member !== 'call' && member !== 'apply')
      throw new Error('unsupported operation: JavaScript interop with native ' + kind + ' form');
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
  // Macro traversal is observable in generated operand and property order.
  // Select the legacy hash paths while retaining native persistent storage.
  const ordered = x => __rt.formReaderCollection(x);
  const map = a => { let v=ordered(__rt.formMap());for(let i=0;i<a.length;i+=2)v=__rt.formAssoc(v,a[i],a[i+1]);return v; };
  const arrayMap = a => ordered(__rt.formArrayMap(vector(a)));
  const literalMap = a => a.length<=16?arrayMap(a):map(a);
  // Native bootstrap for the calls the small Rust js-obj builder declines.
  // Match compiler_core_macros.cljs: partition drops an odd trailing form;
  // small inputs replace whole pairs, wide inputs assoc into an array map
  // which promotes only after its ninth distinct key. Inspect the legacy
  // traversal explicitly: ordinary native map seq has its own trie order.
  const bootstrapJsObj = form => {
    // Read operands from the rooted form instead of spreading them into a
    // VM call frame: generated serializer objects can exceed its stack limit.
    const entries=arr(form).slice(1);
    let pairs=[];
    if(Math.floor(entries.length/2)<=8){
      for(let i=0;i+1<entries.length;i+=2){
        const pair=[entries[i],entries[i+1]];
        let found=false;
        for(let j=0;j<pairs.length;j++)if(__rt.formEquals(pairs[j][0],pair[0])){pairs[j]=pair;found=true;}
        if(!found)pairs.push(pair);
      }
    }else{
      let object=arrayMap([]);
      for(let i=0;i+1<entries.length;i+=2)object=__rt.cljAssoc(object,entries[i],entries[i+1]);
      pairs=arr(__rt.formPrintItems(object)).map(arr);
    }
    const strings=[],symbols=[],expressions=[];
    for(const pair of pairs){
      const key=val(pair[0]);
      if(typeof key==='string')strings.push(pair);
      else if(__rt.formKind(key)==='symbol')symbols.push(pair);
      else expressions.push(pair);
    }
    const bindings=[],bound=[];
    for(const pair of expressions){const name=gensym('key');bindings.push(name,pair[0]);bound.push([name,pair[1]]);}
    const object=gensym('object'),sym=name=>__rt.cljSymbol(name);
    const initial=[sym('js*'),'({'+strings.map(()=>'~{}:~{}').join(',')+'})'];
    for(const pair of strings)initial.push(pair[0],pair[1]);
    bindings.push(object,list(initial));
    const result=[sym('cljs.core/let'),vector(bindings)];
    for(const pair of symbols)result.push(list([sym('cljs.core/unchecked-set'),object,pair[0],pair[1]]));
    for(const pair of bound)result.push(list([sym('cljs.core/unchecked-set'),object,pair[0],pair[1]]));
    result.push(object);
    return list(result);
  };
  const set = a => { let v=ordered(__rt.formSet());for(const x of a)v=__rt.formConj(v,x);return v; };
  // A set literal with members that are not constants rejects duplicates.
  // Reader-distinct constants retain every position after lowering.
  const counted = (v, n, message) => { if (__rt.formCount(v) !== n) throw new Error(message); return v; };
  const literalSet = a => counted(set(a), a.length, 'Duplicate key');
  const constantSet = a => ordered(__rt.formLiteralSet(vector(a)));
  const constantMap = a => ordered(__rt.formLiteralMap(vector(a)));
  const arity = n => { throw new Error('Invalid arity: ' + n); };
  const reifySites=new Map(),reifyMetadata=new WeakMap();
  const reify=(site,fields,values,methods,metadata=null)=>{
    let prototype=reifySites.get(site);
    if(prototype===undefined){
      prototype={};
      decl.extendInstance(prototype,[
        ['IMeta',0,null],['IWithMeta',0,null],
        ['IMeta/-meta',1,x=>reifyMetadata.get(x)??null],
        ['IWithMeta/-with-meta',2,(x,m)=>{const result=Object.create(Object.getPrototypeOf(x));
          for(const key of Object.keys(x))Object.defineProperty(result,key,{value:x[key],writable:true,enumerable:true,configurable:true});
          decl.inheritInstance(result,x);reifyMetadata.set(result,m);return result;}],...methods()],true);
      reifySites.set(site,prototype);
    }
    const result=Object.create(prototype);
    decl.inheritInstance(result,prototype);
    fields.forEach((field,i)=>Object.defineProperty(result,mungeMember(field),{value:values[i],writable:true,enumerable:true,configurable:true}));
    reifyMetadata.set(result,metadata);return result;
  };
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
    if (!gensymNative) {
      do {
        const count=(readerGensymScopes.get(scope)??0)+1;
        readerGensymScopes.set(scope,count);
        name=base+(scope===0?'':gensymPrefix)+count;
      } while (scope===0&&!__rt.effect(65531,'a'+name));
    }
    else if (scope !== 0) name = base + (scope===gensymScope?gensymPrefix:__rt.need(scope,6)) + __rt.effect(65531, 'g' + scope);
    else do { name = base + __rt.effect(65531, 'n'); } while (!__rt.effect(65531, 'a' + name));
    // The compiler's captured log decides whether recording is enabled.
    // A nested helper producer has a fresh task and no task-local log flag.
    __rt.effect(65531, initializer===undefined?'L'+name:'C'+scope+':'+name);
    return __rt.cljSymbol(name);
  };
  const seq = x => __rt.cljSeq(x);
  const get = function(m,k,n) { return arguments.length === 2 ? __rt.cljGet2(m,k) : __rt.cljGet(m,k,n); };
  // Vars (light-cljs-589; docs/native-macros.md, "Vars"): an immutable
  // struct with the legacy Var's slots. `val` is the thunk that reads the
  // binding, `sym` its symbol and `__metadata` its metadata; `__deferred`,
  // of a definition's Var only, gives the executable :test metadata when
  // the metadata is first read, so taking a Var never demands its
  // definition. The type is keyed by `sym` (field 1): a Var is its symbol,
  // for `=` and `hash` and inside collections, and the thunk never runs
  // for them. Like a MultiFn, a Var another task made is recognized by its
  // type's name.
  const varType = __rt.structType('cljs.core/Var',__rt.formVector('val','sym','__metadata','__deferred'),1,null);
  const isVar = x => { const t = __rt.structTypeOf(val(x)); return t !== null && __rt.formName(t) === 'cljs.core/Var'; };
  const varField = (v, k) => __rt.structField(val(v), k);
  // A Var's metadata with its deferred :test, made once in each task, as
  // the legacy Var materializes it once: every read, `meta` or `._meta`,
  // gives the same map. The :test may be a value only this task holds.
  const varMetadata = new WeakMap();
  const varMeta = v => {
    const m = varField(v,'__metadata'), deferred = varField(v,'__deferred');
    if (deferred === null) return m;
    const key = val(v);
    if (varMetadata.has(key)) return varMetadata.get(key);
    const materialized = __rt.cljAssoc(m, __rt.cljKeyword('test'), deferred());
    varMetadata.set(key, materialized);
    return materialized;
  };
  const symText = s => { s = val(s); const ns = __rt.formNamespace(s); return (ns === null ? '' : ns + '/') + __rt.formName(s); };
  const varText = v => "#'" + symText(varField(v,'sym'));
  const makeVar = (get, sym, metadata = null) => __rt.structNew(varType, get, sym, metadata, null);
  // The metadata the legacy runtime's `definitionVar` gives: its base keys
  // in their order, the definition's metadata, then :ns, :name and :test.
  // Without a deferred :test the root is read, as the legacy runtime does.
  const varBase = (sym, entries) => {
    const ns = __rt.formNamespace(val(sym));
    return [__rt.cljKeyword('ns'), ns === null ? null : __rt.formSymbol(null, ns), __rt.cljKeyword('name'), __rt.formSymbol(null, __rt.formName(val(sym))), ...entries];
  };
  const definitionVar = (get, sym, metadata = null, deferred = null) => {
    const root = deferred === null ? val(get()) : null;
    const k = name => __rt.cljKeyword(name), base = varBase(sym, []);
    let m = arrayMap(varBase(sym, [k('doc'), null, k('file'), null, k('line'), null, k('column'), null, k('arglists'), __rt.formList()]));
    if (val(metadata) != null) m = __rt.cljConj(m, metadata);
    // A root without :test gives undefined, as in the legacy runtime.
    const test = root != null && root !== false ? root.cljs$lang$test : null;
    m = __rt.cljAssoc(__rt.cljAssoc(__rt.cljAssoc(m, base[0], base[1]), base[2], base[3]), k('test'), test);
    return __rt.structNew(varType, get, sym, m, deferred);
  };
  const compilerMacroVar = (...a) => {
    if (a.length !== 2) arity(a.length);
    const [sym, metadata] = a;
    let m = arrayMap(varBase(sym, [__rt.cljKeyword('macro'), true]));
    if (val(metadata) != null) m = __rt.cljConj(m, metadata);
    const text = symText(sym);
    return makeVar(() => { throw new Error('compile-time macro cannot be consumed as a runtime value: ' + text); }, sym, m);
  };
  const varMethods = {
    formEquals: (x, y) => isVar(y) && __rt.formEquals(varField(x,'sym'), varField(y,'sym')),
    formHash: x => __rt.formHash(varField(x,'sym')),
    cljMeta: varMeta,
    cljWithMeta: (x, m) => __rt.structNew(__rt.structTypeOf(val(x)), varField(x,'val'), varField(x,'sym'), m, null),
  };
  // The engine calls this task-local resolver only for native structs. The
  // selected entry runs through a rooted native callback frame, so declaration
  // demand and user methods can yield/park without replaying effects.
  // Binding table identities does not force any declarations. The lookup still
  // checks the live version/frontier, while avoiding per-call helper closures
  // and construction/resolution of the method's composite table key.
  const coreTables = {
    formEquals:decl.coreTable('IEquiv/-equiv',2), formHash:decl.coreTable('IHash/-hash',1),
    cljGet2:decl.coreTable('ILookup/-lookup',2), cljGet:decl.coreTable('ILookup/-lookup',3),
    cljContains:decl.coreTable('IAssociative/-contains-key?',2), cljAssoc:decl.coreTable('IAssociative/-assoc',3),
    cljDissoc:decl.coreTable('IMap/-dissoc',2), cljConj:decl.coreTable('ICollection/-conj',2),
    cljDisj:decl.coreTable('ISet/-disjoin',2), cljMeta:decl.coreTable('IMeta/-meta',1),
    cljWithMeta:decl.coreTable('IWithMeta/-with-meta',2), cljSeq:decl.coreTable('ISeqable/-seq',1),
    cljPeek:decl.coreTable('IStack/-peek',1), cljCount:decl.coreTable('ICounted/-count',1), cljNext:decl.coreTable('INext/-next',1),
  };
  __rt.cljCoreResolver((op, x, throughLazy, name) => {
    if (op === 'jsMember') return jsMember(x, name);
    if (op === 'structSet') return setMutableMember;
    if(op==='cljWithExportedMeta')op='cljWithMeta';
    const kind = __rt.formKind(x);
    if(__rt.cljIsAtom(x)&&op==='cljMeta')return a=>{lane(a);const slot=optionsOf(a);return slot===null?null:__rt.formNth(__rt.atomLoad(slot),0);};
    if (kind === 'struct' && isVar(x) && varMethods[op] !== undefined) return varMethods[op];
    // The engine also offers sorted-map-by maps (and `=` with one, also
    // nested in another comparison) here, whatever their comparator, nil too.
    const sorted = sortedBy.methods[op];
    if (sorted !== undefined && kind === 'map' && __rt.formIsSortedBy(x)) return sorted;
    const record = kind === 'record', table = coreTables[op];
    const cursor = arraySeqs.get(x), array = Array.isArray(val(x));
    if (cursor !== undefined || array) {
      switch (op) {
        case 'formEquals': if (cursor !== undefined) return equalArraySeq; break;
        case 'formHash': if (cursor !== undefined) return x => __rt.formHash(list(arr(x))); break;
        case 'cljSeq': return reduceSeqOf;
        case 'cljFirst': return firstOf;
        case 'cljRest': return restOf;
        case 'cljNext': return nextOf;
        case 'cljCount': return () => cursor === undefined ? val(x).length : Math.max(0,cursor.array.length-cursor.index);
        case 'cljMeta': return () => cursor?.meta ?? null;
        case 'cljWithMeta': if (cursor !== undefined) return (x,m) =>
          val(cursor.meta) === val(m) ? x : copyArraySeq(cursor.array,cursor.index,m); break;
      }
    }
    if ((op==='formEquals'||op==='formHash') && __rt.formKind(x)===null) return null;
    switch (op) {
      case 'cljPeek': return decl.coreLookup(table,x,stackValue(x));
      case 'cljIsList': return x => decl.predicate('IList',x,cursor!==undefined||multi.is(x));
      case 'cljIsSeq': return x => cursor!==undefined || val(x)!=null && decl.predicate('ISeq',x,multi.is(x));
      case 'cljIsSequential': return x => cursor!==undefined || decl.predicate('ISequential',x,multi.is(x));
      case 'cljIsColl': return x => cursor!==undefined || val(x)!=null && decl.predicate('ICollection',x,multi.is(x));
      case 'cljIsVector': return x => decl.predicate('IVector',x,cursor!==undefined||multi.is(x));
      case 'cljIsMapEntry': return x => decl.implementsDirectly('IMapEntry',x);
      case 'formEquals': {
        const equiv = decl.coreLookup(table, x, record);
        return equiv === null ? null : (x,y) => !!val(equiv(x,y));
      }
      case 'formHash': {
        const hash = decl.coreLookup(table, x, record);
        return hash === null ? null : x => val(hash(x)) | 0;
      }
      case 'cljGet2': return decl.coreLookup(table, x, record) ?? (record ? null : () => null);
      case 'cljGet': return decl.coreLookup(table, x, record) ?? (record ? null : (x,k,d) => d);
      case 'cljContains': {
        const contains = decl.coreLookup(table, x, record);
        if (contains !== null) return contains;
        if (record) return null;
        return (x,k) => { const missing = {}; return get(x,k,missing) !== missing; };
      }
      case 'cljAssoc': case 'cljDissoc': case 'cljConj': case 'cljDisj':
      case 'cljMeta': case 'cljWithMeta': return decl.coreLookup(table, x, record);
      case 'cljSeq': return throughLazy ? () => __rt.cljSeq(x) : decl.coreLookup(table, x, record);
      case 'cljList': return x => {
        const out = [];
        for (let s = reduceSeqOf(x); s != null; s = nextOf(s)) out.push(firstOf(s));
        return list(out);
      };
      case 'cljCount': {
        if (!throughLazy) {
          const count = decl.coreLookup(table, x, record);
          if (count !== null || record) return count;
          if (decl.coreLookup(coreTables.cljSeq, x, record) === null) return null;
        }
        return x => {
          let n = 0;
          for (let s = reduceSeqOf(x); s != null; s = nextOf(s)) {
            const kind = __rt.formKind(val(s));
            const builtin = nativeSeq(s) && kind !== 'lazy-seq' && (kind !== 'cons' || __rt.cljDispatch(countedKinds,s))
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
        : decl.coreLookup(table, x, false, true) ?? (x => __rt.cljSeq(__rt.cljRest(x)));
      case 'cljNth': return (x,i) => nthOf(x,i,null,false,throughLazy);
      case 'cljNthOr': return (x,i,d) => nthOf(x,i,d,true,throughLazy);
    }
    return null;
  });
  // Read `.call` before evaluating nested callback arguments. A custom
  // callable's getter and receiver have the same order as `rf.call(null, ...)`.
  const prepareInvoke = f => {
    const value = val(f), kind = __rt.formKind(value);
    // The RegExp constructor as a value makes shareable regexes
    // (light-cljs-563), through the task global for the reason below.
    if (value === globalThis.__lightNativeRuntime.intrinsicRegExp)
      return globalThis.__lightNativeRuntime.nativeRegExpCall(value);
    // Resolve through the task global rather than capturing `multi` before
    // its initialization: that forward capture would box ordinary lazy
    // callbacks in a task-owned binding cell and prevent their publication.
    if(kind==='struct'&&__rt.formName(__rt.structTypeOf(value))==='cljs.core/MultiFn')
      return (...args)=>globalThis.__lightNativeRuntime.core.invokeMulti(value,args);
    if(kind==='struct'&&isVar(value))
      return (...args)=>globalThis.__lightNativeRuntime.core.invokeVar(value,args);
    if (kind === 'vector' || kind === 'map' || kind === 'set' || kind === 'keyword' || kind === 'symbol') return (...args) => {
      if (kind !== 'vector' && (args.length < 1 || args.length > 2)) arity(args.length);
      return __rt.cljInvoke(f, args[0], args.length > 1 ? args[1] : null, args.length);
    };
    const call = value.call;
    return (...args) => Reflect.apply(call, value, [null, ...args]);
  };
  const invoke = (f,...args) => {
    const value=val(f),kind=__rt.formKind(value);
    if(value===globalThis.__lightNativeRuntime.intrinsicRegExp)return globalThis.__lightNativeRuntime.nativeRegExpCall(value)(...args);
    if(kind==='struct'&&__rt.formName(__rt.structTypeOf(value))==='cljs.core/MultiFn')
      return globalThis.__lightNativeRuntime.core.invokeMulti(value,args);
    if(kind==='struct'&&isVar(value))return globalThis.__lightNativeRuntime.core.invokeVar(value,args);
    if(kind==='struct'||kind==='record')return globalThis.__lightNativeRuntime.core.invokeStruct(value,args);
    if(kind!=='vector'&&kind!=='map'&&kind!=='set'&&kind!=='keyword'&&kind!=='symbol')return value.call(null,...args);
    if(kind!=='vector'&&(args.length<1||args.length>2))arity(args.length);
    return __rt.cljInvoke(f,args[0],args.length>1?args[1]:null,args.length);
  };
  // Match legacy collection members, including their ignored receiver operand.
  // These methods capture no task-owned state and retain the native receiver.
  const rawMember = expected => function(_, ...args) {
    if (__rt.formKind(__rt.formValue(this)) !== expected) throw new Error('Expected ' + expected);
    if (arguments.length === 0 && (expected === 'keyword' || expected === 'symbol' || expected === 'map' || expected === 'set'
        || expected === 'vector' && __rt.cljIsMapEntry(this))) throw new Error('Invalid arity: -1');
    return globalThis.__lightNativeRuntime.core.invoke(this,...args);
  };
  const families = ['keyword','symbol','vector','map','set'];
  const rawCalls = new Map(families.map(k=>[k,canonicalMember(k,()=>rawMember(k))]));
  // The IFn and MultiFn adapters take a struct receiver (their own or a
  // borrowed one): any other receiver is a catchable TypeError, as a legacy
  // adapter's failure is.
  const rawMultiCall = canonicalMember('multi-call',()=>function(_, ...args) {
    if (__rt.structTypeOf(this) === null) throw new TypeError('undefined is not a function');
    return globalThis.__lightNativeRuntime.core.invokeMulti(this,args);
  });
  rawCalls.set('struct',rawMultiCall);
  const rawIFnCall = canonicalMember('ifn-call',()=>function(_, ...args) {
    if (__rt.structTypeOf(this) === null) throw new TypeError('undefined is not a function');
    return globalThis.__lightNativeRuntime.core.invokeIFn(this,args);
  });
  // The legacy collection and MultiFn adapters read `this.call` before
  // realizing the arguments; the IFn adapter realizes them first.
  const rawApplyMember = ifn => function(_, args) {
    const call = ifn ? undefined : this.call;
    const values = globalThis.__lightNativeRuntime.core.nativeApplyArgs(args);
    if (ifn) return this.call(null,...(values.length > 20 ? [...values.slice(0,20),values.slice(20)] : values));
    return Reflect.apply(call, this, [null, ...values]);
  };
  const rawApplies = new Map([...families,'struct'].map(k=>[k,canonicalMember(k+'-apply',()=>rawApplyMember(false))]));
  const rawIFnApply = canonicalMember('ifn-apply',()=>rawApplyMember(true));
  // A Var's `call`, `apply`, `isMacro` and `toString`, as the legacy Var
  // prototype's.
  const rawVarMembers = new Map([
    ['call', canonicalMember('var-call',()=>function(_, ...args) {
      return globalThis.__lightNativeRuntime.core.invokeVar(this,args);
    })],
    ['apply', canonicalMember('var-apply',()=>function(_, args) {
      // Past 20 arguments the root's apply, as the legacy Var's apply.
      return globalThis.__lightNativeRuntime.core.applyVar(this,globalThis.__lightNativeRuntime.core.nativeApplyArgs(args));
    })],
    // The root's macro flag, read through the thunk, so the read demands
    // the root as the legacy Var's does (light-cljs-603).
    ['isMacro', canonicalMember('var-is-macro',()=>function() {
      return globalThis.__lightNativeRuntime.core.varIsMacro(this);
    })],
    ['toString', canonicalMember('var-string',()=>function() {
      return globalThis.__lightNativeRuntime.core.str(this);
    })]]);
  // A native form's named member, as raw JavaScript reads it: a form is one
  // object in both worlds and no call boundary converts anything. The
  // engine itself answers a struct's fields and record slots (its own data
  // properties, so reflection agrees with reads) and defers every other
  // named read here with the form as the receiver, including ToPrimitive's
  // valueOf/toString. A struct answers its Object methods, then its IFn
  // call/apply adapters, then what a legacy instance inherits from
  // Object.prototype; the collection families, keywords, symbols and
  // multimethods answer call/apply only.
  // What a legacy instance inherits from Object.prototype (toString,
  // valueOf, ...): every struct, record and multimethod has them.
  const inherited = name => {
    const member = name === 'constructor' ? undefined : Object.prototype[name];
    return typeof member === 'function' ? member : undefined;
  };
  const jsMember = (x, name) => {
    const value = val(x), kind = __rt.formKind(value);
    if (kind !== 'struct' && kind !== 'record')
      return name === 'call' ? rawCalls.get(kind) : name === 'apply' ? rawApplies.get(kind) : undefined;
    if (multi.is(value)) return name === 'call' ? rawMultiCall : name === 'apply' ? rawApplies.get('struct') : inherited(name);
    // A Var's `_meta` is its materialized metadata, as the legacy getter's.
    if (isVar(value)) return name === '_meta' ? varMeta(value) : rawVarMembers.get(name) ?? inherited(name);
    if (__rt.structState(value) !== 0) {
      const info = decl.structInfo(value), index = info.fieldMembers.lastIndexOf(name);
      if (index >= 0) {
        mutableLane(value);
        return __rt.structField(value, info.fields[index]);
      }
      // A transparent Proxy must classify like the task-owned representation
      // did; a declared field of that name shadows the tag. Direct structs
      // use engine type identity without reading it.
      if (name === '__lightNativeType' || name === TYPE_KEY) return __rt.structTypeOf(value);
    }
    return structMember(value, name);
  };
  const mutableLane = x => {
    if (__rt.structState(x) !== 2) return;
    const cell = __rt.effect(65532, 0);
    if (cell !== 0) __rt.need(cell, 5);
  };
  const setMutableMember = (x, name, value) => {
    const info = decl.structInfo(x), index = info.fieldMembers.lastIndexOf(name);
    if (index < 0) throw new TypeError('unknown mutable field: ' + name);
    mutableLane(x);
    return __rt.structSet(x, info.fields[index], value);
  };
  // Rules 2 and 3 of docs/native-macros.md, "Member precedence: one rule",
  // for a struct or record (the engine answers rule 1, its own properties):
  // its type's Object method, unless the IFn call/apply adapters win
  // (`decl.objectWins`), then what a legacy instance inherits from
  // Object.prototype. Every consumer of a struct's member reads it here.
  const structMember = (value, name) => {
    const info = decl.structInfo(value);
    if (decl.objectWins(info, name)) return decl.memberFunction(info, name);
    if (info.ifn.length > 0 && name === 'call') return decl.callMember(value) ?? rawIFnCall;
    if (info.ifn.length > 0 && name === 'apply') return rawIFnApply;
    return inherited(name);
  };
  // `String(struct)` converts through the engine: the struct's Object
  // toString method, else Object.prototype's (a legacy record has no
  // toString either).
  const structString = x => String(val(x));
  // sorted-map-by (light-cljs-561). The engine retains the comparator and
  // sorted entries; ordering runs here, on the task VM, so a comparator can
  // demand a helper and park. Calls follow the legacy runtime: fnComparator
  // takes a number result as the order, otherwise a truthy result is less
  // and the reversed call decides greater; a binary search compares
  // (entry key, key), then one more call tests the found position. Defined
  // after `invoke`: capturing it before its initialization would box it in a
  // task-owned binding cell and keep lazy callbacks from publishing.
  const sortedBy = (() => {
    const order = (f, x, y) => {
      const r = invoke(f, x, y), n = val(r);
      if (typeof n === 'number') return n;
      return truth(r) ? -1 : truth(invoke(f, y, x)) ? 1 : 0;
    };
    const key = (m, i) => __rt.formSortedEntry(m, i, 0);
    const position = (m, f, k) => {
      let lo = 0, hi = __rt.formCount(m);
      while (lo < hi) {
        const mid = (lo + hi) >>> 1;
        if (order(f, key(m, mid), k) < 0) lo = mid + 1; else hi = mid;
      }
      return lo;
    };
    const found = (m, f, i, k) => i < __rt.formCount(m) && order(f, key(m, i), k) === 0;
    const index = (m, k) => {
      const f = __rt.formSortedComparator(m), i = position(m, f, k);
      return found(m, f, i, k) ? i : -1;
    };
    // An equal key keeps the stored key; an equal value keeps the map.
    const assoc = (m, k, v) => {
      const f = __rt.formSortedComparator(m), i = position(m, f, k);
      if (!found(m, f, i, k)) return __rt.formSortedInsert(m, i, k, v);
      return __rt.formEquals(__rt.formSortedEntry(m, i, 1), v) ? m : __rt.formSortedReplace(m, i, v);
    };
    const lookup = (m, k, d) => { const i = index(m, k); return i < 0 ? d : __rt.formSortedEntry(m, i, 1); };
    // Legacy mapConj: nil leaves the map, a pair is one entry, and any other
    // value is a sequence of vector entries (a map, a record, a list, a lazy
    // sequence), each added in order. Other vectors keep the leaf's report.
    const conj = (m, x) => {
      if (x === null || x === undefined) return m;
      const item = val(x), kind = __rt.formKind(item);
      if (kind === 'vector' && __rt.formCount(item) === 2) return assoc(m, __rt.formNth(item, 0), __rt.formNth(item, 1));
      if (kind === 'vector') return __rt.cljConj(__rt.formArrayMap(__rt.formVector()), x);
      for (let s = __rt.cljSeq(x); s !== null; s = __rt.cljNext(s)) {
        const e = __rt.cljFirst(s);
        if (__rt.formKind(val(e)) !== 'vector') throw new Error('Map conj requires vector entries');
        m = assoc(m, __rt.formNth(e, 0), __rt.formNth(e, 1));
      }
      return m;
    };
    // Legacy map equality: equal counts, then each left entry, in its seq
    // order, is found in the right map (by its comparator when it has one).
    // An array map compares its own value first, a hash or sorted map the
    // found value, as in legacy equiv-map. A struct's -equiv on the left
    // never reaches this method. The engine asks it for nested comparisons
    // with a sorted-map-by map too, and runs the instruction again.
    const equals = (a, b) => {
      const left = val(a), right = val(b);
      if (__rt.formKind(left) !== 'map' || __rt.formKind(right) !== 'map') return false;
      if (__rt.formCount(left) !== __rt.formCount(right)) return false;
      const custom = __rt.formIsSortedBy(right), ownFirst = __rt.formIsArrayMap(left), missing = {};
      for (let s = __rt.cljSeq(left); s !== null; s = __rt.cljNext(s)) {
        const e = __rt.cljFirst(s), own = __rt.formNth(e, 1);
        const other = custom ? lookup(right, __rt.formNth(e, 0), missing) : __rt.cljGet(right, __rt.formNth(e, 0), missing);
        if (other === missing || !(ownFirst ? __rt.formEquals(own, other) : __rt.formEquals(other, own))) return false;
      }
      return true;
    };
    const methods = {
      cljGet2: (m, k) => lookup(m, k, null), cljGet: lookup,
      cljContains: (m, k) => index(m, k) >= 0, cljAssoc: assoc,
      cljDissoc: (m, k) => { const i = index(m, k); return i < 0 ? m : __rt.formSortedRemove(m, i); },
      cljConj: conj, formEquals: equals,
    };
    const create = (f, kvs) => {
      let m = __rt.formSortedMapBy(f);
      for (let i = 0; i < kvs.length; i += 2) m = assoc(m, kvs[i], i + 1 < kvs.length ? kvs[i + 1] : null);
      return m;
    };
    return {create, methods};
  })();
  // memoize calls its callback through Clojure apply, whose fixed function
  // receiver is the function itself. Native variadic dispatchers preserve
  // applyTo's null receiver in immutable engine metadata, so they can publish
  // without a mutable JavaScript arity property record.
  const applyArgs = (f,args) => {
    if(isVar(f))return globalThis.__lightNativeRuntime.core.applyVar(f,args);
    const value=val(f),kind=__rt.formKind(value);
    if(kind!==null)return (kind==='struct'||kind==='record')
      && __rt.formName(__rt.structTypeOf(value))!=='cljs.core/MultiFn' ? decl.applyIFn(value,args) : invoke(f,...args);
    if(value.cljs$lang$applyTo){
      return args.length<=value.cljs$lang$maxFixedArity
        ?value.call(null,...args):value.cljs$lang$applyTo(list(args));
    }
    const receiver=__rt.fnApplyReceiver(value);
    if(args.length>20)return value.apply(receiver,args);
    const entry=value['cljs$core$IFn$_invoke$arity$'+args.length];
    return entry?entry.call(receiver,...args):value.call(receiver,...args);
  };
  const unsupported = name => function(){throw new Error('unsupported operation: native macro runtime '+name);};
  const ExceptionInfo=function(message,data,cause){
    const error=new Error(val(message)),target=this==null?globalThis:Object(this);
    target.message=message;target.data=data;target.cause=cause;
    for(const key of ['name','description','number','fileName','lineNumber','columnNumber','stack'])target[key]=error[key];
    return target;
  };
  Object.setPrototypeOf(ExceptionInfo.prototype,Error.prototype);
  ExceptionInfo.prototype.toString=function(){return '#error {:message '+globalThis.__lightNativeRuntime.core.prStr(this.message)
    +(truth(this.data)?', :data '+globalThis.__lightNativeRuntime.core.prStr(this.data):'')+(truth(this.cause)?', :cause '+globalThis.__lightNativeRuntime.core.prStr(this.cause):'')+'}';};
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
  function StringBufferWriter(sb){const writer=Object.create(StringBufferWriter.prototype);writer.sb=sb;writers.add(writer);return writer;}
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
  const literalObjectString = x => {
    const object={},pairs=literalObjectFields(x);
    for(let i=0;i<pairs.length;i+=2)Object.defineProperty(object,pairs[i],{value:val(pairs[i+1]),writable:true,enumerable:true,configurable:true});
    return String(object);
  };
  const literalIsObject = x => {
    if(__rt.formName(x)!=='js-object')return false;
    const pairs=literalObjectFields(x);
    for(let i=0;i<pairs.length;i+=2)if(pairs[i]==='constructor')return val(pairs[i+1])===Object;
    return true;
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
      if(tag==='regex')return write(writer,'#\"'+x.source+'\"');
      if(tag==='js-array')return printSequential(writer,printWriter,'#js [',' ',']',options,payload);
      if(tag==='js-object'){
        if(!literalIsObject(x))return write(writer,'#object[Object '+literalObjectString(x)+']');
        write(writer,'#js ');
        const pairs=literalObjectFields(x);
        return printSequential(writer,(entry,w,o)=>{
          const key=val(__rt.cljNth(entry,0));
          printWriter(/^[A-Za-z_*+?!\-'][\w*+?!\-']*$/.test(key)?__rt.cljKeyword(key):key,w,o);
          write(w,' ');return printWriter(__rt.cljNth(entry,1),w,o);
        },'{',', ','}',options,__rt.formPrintItems(constantMap(pairs)));
      }
      if(tag==='queue')return printSequential(writer,printWriter,'#queue [',' ',']',options,payload);
      if(tag==='uuid')return write(writer,'#uuid "'+val(payload).toLowerCase()+'"');
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
    // The legacy MultiFn constructor is unqualified; user types print their qualified name.
    if(kind==='struct'&&isVar(x))return write(writer,varText(x));
    if(kind==='struct')return write(writer,'#object['+(multi.is(x)?'MultiFn':__rt.formName(__rt.structTypeOf(x)).replaceAll('/','.'))+' '+structString(x)+']');
    if(kind===null&&(typeof x==='object'||typeof x==='function')){
      const ty=typeTag(x);
      if(__rt.formKind(ty)==='type')return write(writer,typeof x==='function'?__rt.formName(ty):'#object['+__rt.formName(ty).replaceAll('/','.')+' '+String(x)+']');
    }
    if(x instanceof Namespace)return write(writer,'#object[Namespace '+String(x)+']');
    if(x instanceof RegExp)return write(writer,'#"'+x.source+'"');
    if(x instanceof ExceptionInfo)return write(writer,x.toString());
    if(x instanceof Error)return write(writer,'#object['+(x.constructor?.name||'Object')+' '+String(x)+']');
    if(x instanceof Date)return write(writer,'#inst "'+x.toISOString().replace(/Z$/,'-00:00')+'"');
    if(arraySeqs.has(x))return printSequential(writer,printWriter,'(',' ',')',options,x);
    if(Array.isArray(x))return printSequential(writer,printWriter,'#js [',' ',']',options,x);
    if(typeof x==='function')return write(writer,'#object['+(x.name||'Function')+(truth(core.printFnBodies)?' "'+String(x)+'"':'')+']');
    if(typeof x==='object'&&Object.getPrototypeOf(x)===null)return write(writer,'#object[Object]');
    if(typeof x==='object'&&x.constructor===Object){
      write(writer,'#js ');
      return printSequential(writer,(entry,w,o)=>{
        const key=val(__rt.cljNth(entry,0));
        printWriter(/^[A-Za-z_*+?!\-'][\w*+?!\-']*$/.test(key)?__rt.cljKeyword(key):key,w,o);
        write(w,' ');return printWriter(__rt.cljNth(entry,1),w,o);
      },'{',', ','}',options,vector(Object.keys(x).map(k=>__rt.formVector(k,x[k]))));
    }
    if(kind===null)return write(writer,'#object['+(x.constructor?.cljs$lang$ctorStr?.replaceAll('/','.')||x.constructor?.name||'Object')+' '+String(x)+']');
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
  // The registry is the compilation's (host effect 65529, `src/bbp_evaluator/
  // namespaces.rs`, light-cljs-589): a creation, including each macro call's
  // namespace, is recorded at the task's place in the effect lanes without
  // waiting; a reader waits for its lane, then sees what the activities
  // before it created, and its own, in one-slot order. `namespaces` keeps
  // this task's Namespace object of each name, so one name is one object.
  const localNs=(key,name)=>{
    let ns=namespaces.get(key);
    if(ns===undefined){ns=new Namespace(null,name??__rt.formSymbol(null,key));namespaces.set(key,ns);}
    return ns;
  };
  const registryLane=()=>{const cell=__rt.effect(65532,0);if(cell!==0)__rt.need(cell,5);};
  const registryNames=()=>{
    registryLane();
    const text=__rt.effect(65529,'v'),names=[];
    for(let at=0;at<text.length;){
      const colon=text.indexOf(':',at),end=colon+1+Number(text.slice(at,colon));
      names.push(text.slice(colon+1,end));at=end;
    }
    return names;
  };
  const createNs = (...a) => {
    if(a.length!==1&&a.length!==2)return arity(a.length);
    const name=a[0];if(__rt.formKind(val(name))!=='symbol')throw new TypeError('create-ns requires a symbol');
    const key=namedText(name),obj=a.length===2?val(a[1]):null;
    __rt.effect(65529,'r'+key);
    const ns=localNs(key,name);if(obj!=null)ns.obj=obj;
    return ns;
  };
  const findNs = (...a) => {
    if(a.length!==1)return arity(a.length);
    const name=val(a[0]);if(name==null)return null;if(name instanceof Namespace)return name;
    if(__rt.formKind(name)!=='symbol')throw new TypeError('find-ns requires a symbol or Namespace');
    const key=namedText(name);registryLane();
    return __rt.effect(65529,'f'+key)?localNs(key):null;
  };
  const macroReaderFunction = fn => Object.assign(function(...args){
    const env=get(args[1],__rt.cljKeyword('ns')),name=get(env,__rt.cljKeyword('name'));
    if(val(name)==null)return Reflect.apply(fn,null,args);
    const saved=core.currentNs,ns=createNs(name),previous=ns.readerContext;
    const context=previous??{aliases:new Map(),refers:new Map(),excludes:new Set(),bindings:new Set()};
    const aliases=new Map(),refers=new Map(),excludes=new Set(),bindings=new Set();
    const text=x=>val(x), nth=(x,n)=>__rt.cljNth(x,n), key=n=>__rt.cljKeyword(n);
    for(const pair of arr(get(env,key('requires'))))aliases.set(core.str(nth(pair,0)),core.str(nth(pair,1)));
    const view=get(args[1],__rt.formKeyword('light-cljs','reader-context'));
    let declared=context.declared,visible=context.visible??0;
    if(val(view)!=null){
      for(const entry of arr(get(view,key('refers'))))refers.set(text(nth(entry,0)),[text(nth(entry,1)),text(nth(entry,2))]);
      for(const name of arr(get(view,key('excludes'))))excludes.add(text(name));
      for(const name of arr(get(view,key('bindings'))))bindings.add(text(name));
      const added=get(view,key('declared'));
      if(val(added)!=null){
        const generation=val(get(added,key('generation')));
        if(ns.readerDeclared?.generation!==generation)ns.readerDeclared={generation,index:new Map()};
        let position=val(get(added,key('from')));
        for(const name of arr(get(added,key('names'))))ns.readerDeclared.index.set(text(name),position++);
        declared=ns.readerDeclared.index;visible=val(get(view,key('visible')));
      }
    }
    const overlay=(base,own)=>own.size===0?base:{has:key=>own.has(key)||base.has(key),get:key=>own.has(key)?own.get(key):base.get(key)};
    ns.readerContext={aliases:overlay(context.aliases,aliases),refers:overlay(context.refers,refers),
      excludes:overlay(context.excludes,excludes),bindings:overlay(context.bindings,bindings),declared,visible};
    core.currentNs=ns;
    try{return Reflect.apply(fn,null,args);}finally{ns.readerContext=previous;core.currentNs=saved;}
  },fn);
  // A comparison of adjacent operands, true for one; as the legacy runtimes.
  const chain = (xs, test) => { if (xs.length === 0) arity(0); for (let i = 1; i < xs.length; i++) if (!test(val(xs[i - 1]), val(xs[i]))) return false; return true; };
  const fix = x => x >= 0 ? Math.floor(x) : Math.ceil(x);
  const multiplyInt = Math.imul;
  const bitOp = op => (x,y,...more) => {
    let result=op(val(x),val(y));
    for(const value of more)result=op(result,val(value));
    return result;
  };
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
  // One private SameValueZero registry per shared atom. Runtime bookkeeping
  // keys never invoke language equality; watch keys remain language values.
  const auxiliary=(a,key,create)=>{
    const slot=__rt.atomAux(a);let candidate;
    for(;;){
      const current=__rt.atomLoad(slot),found=current===null?undefined:__rt.getOr(current,key,undefined);
      if(found!==undefined)return found;
      if(candidate===undefined)candidate=create();
      const next=__rt.assoc(current===null?__rt.map():current,key,candidate);
      if(Object.is(__rt.atomCas(slot,current,next),current))return candidate;
    }
  };
  const optionsOf=a=>{const registry=__rt.atomLoad(__rt.atomAux(a));return registry===null?null:__rt.getOr(registry,'atom',null);};
  const atomOptions=(a,metadata=null,validator=null,watches=null)=>auxiliary(a,'atom',()=>__rt.atomNew(__rt.formVector(metadata,validator,watches)));
  const lane = a => {
    if (!__rt.cljIsAtom(a)) throw new Error('unsupported operation: native macro runtime atom operation on ' + typeof val(a));
    const cell = __rt.effect(65532, atomOwners.get(a) ?? 0);
    if (cell !== 0) __rt.need(cell, 5);
  };
  const atom = (state, ...options) => {
    const a = __rt.atomNew(state), owner = __rt.effect(65532, -1);
    if (owner !== 0) atomOwners.set(a, owner);
    if(options.length){
      const opts=core.seqToMap(list(options));
      atomOptions(a,get(opts,__rt.cljKeyword('meta')),get(opts,__rt.cljKeyword('validator')));
    }
    return a;
  };
  const Atom=function(state,metadata,validator,watches){const a=atom(state);atomOptions(a,metadata,validator,watches);return a;};
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
  const deref = a => { if (__rt.cljIsReduced(a)) return __rt.cljUnreduced(a); if (isVar(a)) return varField(a,'val')(); lane(a); return __rt.atomLoad(a); };
  const notifyWatches=(a,old,next)=>{
    const slot=optionsOf(a),watches=slot===null?null:__rt.formNth(__rt.atomLoad(slot),2);
    for(let s=__rt.cljSeq(watches);s!==null;s=__rt.cljNext(s)){
      const entry=__rt.cljFirst(s);invoke(__rt.cljNth(entry,1),__rt.cljNth(entry,0),a,old,next);
    }
    return null;
  };
  const resetPair=(a,v)=>{
    lane(a);const slot=optionsOf(a),validator=slot===null?null:__rt.formNth(__rt.atomLoad(slot),1);
    if(val(validator)!=null&&!truth(invoke(validator,v)))throw new Error('Validator rejected reference state');
    const old=store(a,v);notifyWatches(a,old,v);return [old,v];
  };
  const reset = (a, v) => resetPair(a,v)[1];
  const swapPair = (a, f, args) => {
    lane(a);
    const next = invoke(f, __rt.atomLoad(a), ...args);
    return resetPair(a,next);
  };
  // By `=`, as both pinned cljs.core runtimes compare.
  const compareAndSet = (a, o, n) => {
    lane(a);
    if (!__rt.formEquals(__rt.atomLoad(a), o)) return false;
    reset(a, n);
    return true;
  };
  // Native multimethods carry only persistent fields and engine atom handles.
  // In particular their identity can publish without retaining a task's JS
  // runtime. Calls enter the consuming task's runtime through its global.
  const multi = (() => {
    const ty=__rt.structType('cljs.core/MultiFn',__rt.formVector('name','dispatch_fn','default_dispatch_val','hierarchy','method_table','prefer_table','method_cache','cached_hierarchy','__method_state','__prefer_state'),false,null);
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
      const slot=auxiliary(ref,'multifn',()=>__rt.atomNew(null)),existing=__rt.atomLoad(slot);
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
  const fnCompare=(f,x,y)=>{
    const result=invoke(f,x,y),value=val(result);
    return typeof value==='number'?value:truth(result)?-1:truth(invoke(f,y,x))?1:0;
  };
  // Match now, advance the same RegExp only when the tail is demanded.
  // Offsets, empty matches and slicing all use JavaScript UTF-16 units.
  const reSeq = (self, re, text) => {
    text = val(text);
    if (typeof text !== 'string') throw new TypeError('regex matching requires a string');
    re=val(re);
    const match = re.exec(text);
    if (match === null) return null;
    const value = match.length === 1 ? match[0] : vector(Array.from(match));
    return __rt.cljCons(value, lazy(() => {
      const offset = match.index + Math.max(1, match[0].length);
      return offset <= text.length ? self(self, re, text.slice(offset)) : null;
    }));
  };
  // Regex values (light-cljs-563) are shareable forms that JavaScript uses
  // as RegExps (`__rt.formRegex`, the engine's `crate::regexp::shared`): one
  // object in both worlds, so atoms and published helpers hold them and no
  // boundary converts them. Their `lastIndex` stays 0, so a global or
  // sticky regex, whose matching moves it, stays an ordinary task-owned
  // RegExp.
  const shareRegExp = re => re.global || re.sticky ? re : __rt.formRegex(re.source, re.flags);
  const rePattern = pattern => {
    const x = val(pattern);
    if (x instanceof RegExp) return x;
    if (typeof x !== 'string') throw new TypeError('regex pattern requires a string');
    const prefix = /^\(\?([dgimsuxy]*)\)/.exec(x);
    // The current global RegExp, as in QuickJS; only the intrinsic one
    // makes a shareable regex (`nativeRegExp`, below).
    return nativeRegExp(RegExp)(prefix ? x.slice(prefix[0].length) : x, prefix ? prefix[1] : '');
  };
  // `(js/RegExp. …)` and `(js/RegExp …)` take the constructor as it was
  // evaluated, before the operands: the intrinsic RegExp makes shareable
  // regexes, keeping `RegExp(r)`'s identity for a regex `r`; any other value
  // (a replaced global) is constructed or called as it is. Operands lose
  // occurrence wrappers first, so `RegExp(r)` returns `r`.
  const intrinsicRegExp = RegExp;
  const nativeRegExp = C => (...a) => C !== intrinsicRegExp ? new C(...a) : shareRegExp(new C(...a.map(val)));
  const nativeRegExpCall = C => (...a) => {
    if (C !== intrinsicRegExp) return C(...a);
    a = a.map(val);
    const re = C(...a);
    return a.length > 0 && re === a[0] ? re : shareRegExp(re);
  };
  const reMatch = (re, text, full) => {
    text = val(text);
    if (typeof text !== 'string') throw new TypeError('regex matching requires a string');
    const result = val(re).exec(text);
    if (result === null || (full && result[0] !== text)) return null;
    return result.length === 1 ? result[0] : vector(Array.from(result));
  };
  const seqOf = x => __rt.cljSeq(x);
  const distinctFrom=(self,coll,seen)=>lazy(()=>{
    for(let s=seqOf(coll);s!==null;s=__rt.cljNext(s)){
      const x=__rt.cljFirst(s);
      if(!__rt.cljContains(seen,x))return __rt.cljCons(x,self(self,__rt.cljRest(s),__rt.cljConj(seen,x)));
    }
    return null;
  });
  const repeatedlyFrom=(self,f)=>lazy(()=>__rt.cljCons(invoke(f),self(self,f)));
  const treeFrom=(self,branch,children,node)=>lazy(()=>__rt.cljCons(node,truth(invoke(branch,node))
    ?globalThis.__lightNativeRuntime.core.mapcat(x=>self(self,branch,children,x),invoke(children,node)):null));
  const nativeSeq = x => {
    const kind = __rt.formKind(val(x));
    return kind === 'list' || kind === 'cons' || kind === 'lazy-seq';
  };
  // Reduction's protocol tables are task-owned. Keep this adapter out of the
  // lazy sequence constructors, whose intrinsic-only closures can publish.
  // Array cursors retain the original storage and check its current length
  // on advancement. Reducers can replace, append or remove unvisited items.
  const arraySeqs = new WeakMap();
  // Metadata copying preserves an existing view even if its storage shrank.
  const copyArraySeq = (array, index, meta) => {
    const cursor = {};
    arraySeqs.set(cursor, {array, index, meta});
    return cursor;
  };
  const arraySeq = (array, index) => index < array.length ? copyArraySeq(array,index,null) : null;
  const typedArray=(...a)=>{
    if(a.length===1)return typeof val(a[0])==='number'?typedArray(a[0],null):arr(a[0]).slice();
    if(a.length!==2)return arity(a.length);
    const size=val(a[0]),init=a[1],result=new Array(size);
    if(__rt.cljIsSeq(init)){
      let s=seqOf(init);for(let i=0;s!==null&&i<size;i++,s=__rt.cljNext(s))result[i]=__rt.cljFirst(s);
    }else for(let i=0;i<size;i++)result[i]=init;
    return result;
  };
  const countedKinds = __rt.map('list',true,'empty-list',true,'sequence',true,'vector',true,'map-entry',true,'map',true,'set',true);
  const equalArraySeq = (a,b) => {
    // Mutable records are host objects: when the right operand is a cursor,
    // the engine selects its adapter before resolving the left record.
    if (decl.mutableRecord(val(a)) !== null) {
      const equiv = decl.coreLookup(coreTables.formEquals,a,true);
      if (equiv !== null) return !!val(equiv(a,b));
    }
    // Records' builtin equality compares types before sequential traversal.
    const record = x => __rt.formKind(val(x)) === 'record' || decl.mutableRecord(val(x)) !== null;
    if (record(a) || record(b)) return false;
    if (!core.isSequential(a) || !core.isSequential(b)) return false;
    // Queue compiler literals retain a vector payload, including when an
    // imported source occurrence wraps the descriptor.
    const sequence = x => __rt.formKind(val(x)) === 'compiler-literal' && __rt.formName(val(x)) === 'queue'
      ? __rt.formLiteralPayload(val(x)) : x;
    a = sequence(a); b = sequence(b);
    const counted = x => {
      return arraySeqs.has(x) || __rt.cljDispatch(countedKinds,x)
        || decl.predicate('ICounted',x);
    };
    if (counted(a) && counted(b) && val(__rt.cljCount(a)) !== val(__rt.cljCount(b))) return false;
    let left = seqOf(a), right = seqOf(b);
    while (left !== null) {
      if (right === null || !__rt.formEquals(firstOf(left),firstOf(right))) return false;
      left = nextOf(left); right = nextOf(right);
    }
    return right === null;
  };
  const stackValue = x => {
    const kind = __rt.formKind(val(x));
    return kind === 'vector' || (kind === 'list' || kind === 'cons') && __rt.cljIsList(x)
      || kind === 'compiler-literal' && __rt.formName(val(x)) === 'queue';
  };
  const peekProtocol = x => {
    const builtin = stackValue(x), method = decl.coreLookup(coreTables.cljPeek,x,builtin);
    if (method !== null) return method(x);
    if (builtin) return __rt.cljPeek(x);
    throw new Error('No protocol method IStack/-peek defined for this value');
  };
  // An IStack/-pop extension, including one of a core stack, wins; the
  // builtin leaf handles vectors and lists without realizing other sequences.
  const popTable = decl.coreTable('IStack/-pop',1);
  const popExtension = x => decl.coreLookup(popTable,x,stackValue(x));
  const popProtocol = x => {
    const method = popExtension(x);
    if (method !== null) return method(x);
    if (stackValue(x)) return __rt.cljPop(x);
    throw new Error('No protocol method IStack/-pop defined for this value');
  };
  // Public pop leaves nil alone; the leaf reports other unsupported values.
  const pop = x => {
    if (val(x) == null) return null;
    const method = popExtension(x);
    return method !== null ? method(x) : __rt.cljPop(x);
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
  // tree-seq's mapcat builds lazy walks of the children with map, then
  // applies concat. Its bounded argument count reads four mapped children
  // (or a whole source chunk) before returning, without walking any child.
  // Keep that observable lookahead, including when flatten removes the root.
  const flattenChildren = (walk, node) => {
    const children = mapOne(mapOne, x => walk(walk, x), __rt.cljSeq(node));
    let s = __rt.cljSeq(children);
    for (let n = 0; n < 3 && s !== null; n++) s = __rt.cljNext(s);
    return children;
  };
  // Cache each successful tree-seq node before descending. If a descendant
  // throws, retry its cell without replaying the completed parents' callbacks.
  const flattenWalk = (self, node) => lazy(() => __rt.cljCons(node,
    __rt.cljIsSequential(node) ? flattenChildren(self, node) : null));
  // Linked cons cells keep pushes constant-time at every depth. Each entry
  // contains sibling walk cells, so traversal and retries stay iterative.
  // Captures are persistent values and pure functions, safe to publish/park.
  const flattenFrom = (self, stack) => lazy(() => {
    let pending = stack;
    while (__rt.cljSeq(pending) !== null) {
      const s = __rt.cljSeq(__rt.cljFirst(pending));
      pending = __rt.cljRest(pending);
      if (s === null) continue;
      const walk = __rt.cljSeq(__rt.cljFirst(s));
      const node = __rt.cljFirst(walk);
      pending = __rt.cljCons(__rt.cljRest(s), pending);
      pending = __rt.cljCons(__rt.cljRest(walk), pending);
      // tree-seq tests a node before obtaining its children; filter tests it
      // again afterwards. In particular, declaration demand stays resumable.
      if (!__rt.cljIsSequential(node)) return __rt.cljCons(node, self(self, pending));
    }
    return null;
  });
  const interleaveAll = (self, cs) => lazy(() => {
    const ss = seqsOf(cs);
    if (ss === null) return null;
    return __rt.cljConcat(list(ss.map(s => __rt.cljFirst(s))), self(self, list(ss.map(s => __rt.cljRest(s)))));
  });
  const iterateFrom = (self, f, x) => __rt.formSequence(__rt.cljCons(x, lazy(() => self(self, f, invoke(f, x)))));
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
    const counted = step !== 0 && Number.isInteger(start) && Number.isInteger(end) && Number.isInteger(step);
    const result = __rt.cljRangeCons(__rt.freeze(b), lazy(() => self(self, next, end, step)),counted ? end : null,step);
    return counted ? result : __rt.formSequence(result);
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
  // keep and keep-indexed drop nil results only (false is kept). The index
  // cell belongs to the reducer, so every application counts from zero.
  const keepTransform = (f, indexed) => rf => {
    let i = -1;
    return reducing(rf, (result, x) => {
      const v = indexed ? invoke(f, ++i, x) : invoke(f, x);
      return val(v) == null ? result : invoke(rf, result, v);
    });
  };
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
    isFn:x=>typeof val(x)==='function'||__rt.formKind(val(x))==='type'||isVar(x)||decl.satisfies('Fn',x),
    isVar, compilerMacroVar,
    varIsMacro:v=>val(varField(v,'val')()).cljs$lang$macro,
    // The legacy Var's call entries: arities 0-20 call the root, arity 21
    // applies it with the last argument as the sequence tail, and a larger
    // arity fails before the root is read (light-cljs-602).
    invokeVar:(v,args)=>{
      if(args.length>21)return arity(args.length);
      const f=varField(v,'val')();
      return args.length<=20?invoke(f,...args):core.apply(f,...args);
    },
    // The legacy Var's apply: up to 20 arguments it calls the value its
    // thunk reads (through that value's `call`, as a direct call does);
    // past 20 it applies the value.
    applyVar:(v,args)=>{const f=varField(v,'val')();return args.length<=20?invoke(f,...args):applyArgs(f,args);},
    test:v=>{
      const fn=isVar(v)?get(__rt.cljMeta(v),__rt.cljKeyword('test')):val(v)==null?null:val(v).cljs$lang$test;
      if(truth(fn)){invoke(fn);return __rt.cljKeyword('ok');}
      return __rt.cljKeyword('no-test');
    },
    isIFn:x=>core.isFn(x)||multi.is(x)||['keyword','symbol','vector','map','set'].includes(__rt.formKind(val(x)))||decl.satisfies('IFn',x),
    invokeMulti:(m,args)=>multi.dispatch(m,args),
    invokeIFn:(x,args)=>decl.invokeIFn(x,args),
    // Legacy `(f x)` is `f.call(null, x)`: a `call` field or the Object
    // method the precedence rule selects shadows the declared IFn methods,
    // which are otherwise entered directly (as their adapter would).
    invokeStruct:(x,args)=>{
      const info=decl.structInfo(x);
      if(!info.fieldMembers.includes('call')&&!decl.objectWins(info,'call')){
        if(decl.callable(x))return decl.invokeIFn(x,args);
        throw new TypeError('undefined is not a function');
      }
      const call=x.call;
      if(typeof call!=='function')throw new TypeError('undefined is not a function');
      return call.call(x,null,...args);
    },
    borrowedIFn:(ty,overloaded,x,args)=>decl.borrowedIFn(ty,overloaded,x,args),
    nativeApplyArgs:args=>{
      args=val(args);
      const kind=__rt.formKind(args);
      // A record is an array-like object to the legacy adapters (no
      // Symbol.iterator): Array.from reads its length member, if any.
      return kind==='vector'||kind==='list'||kind==='cons'||kind==='lazy-seq'||kind==='map'||kind==='set'
        ?arr(args):Array.from(args);
    },
    makeHierarchy:multi.makeHierarchy,getGlobalHierarchy:multi.getGlobalHierarchy,isa:multi.isa,
    parents:(...a)=>multi.relation('parents',...a),ancestors:(...a)=>multi.relation('ancestors',...a),descendants:(...a)=>multi.relation('descendants',...a),
    derive:multi.derive,underive:multi.underive,
    addMethod:(m,k,f)=>multi.change(m,'add',k,f),removeMethod:(m,k)=>multi.change(m,'remove',k),removeAllMethods:m=>multi.change(m,'reset'),
    preferMethod:multi.prefer,getMethod:multi.getMethod,methods:m=>multi.tables(m).methods,prefers:m=>multi.tables(m).prefs,
    dispatchFn:m=>multi.field(m,'dispatch_fn'),defaultDispatchVal:m=>multi.field(m,'default_dispatch_val'),
    assertions:true,
    gensym, rePattern, reSeq:(re,text)=>reSeq(reSeq,re,text),
    reFind:(re,text)=>reMatch(re,text,false), reMatches:(re,text)=>reMatch(re,text,true),
    isRegex:x=>val(x) instanceof RegExp,
    invoke, truth, get, identity:x=>x, macroReaderFunction,
    printReadably:true, printLength:null, printLevel:null, printNamespaceMaps:false,
    printMeta:false, printDup:false, printFnBodies:false, printNewline:true, flushOnNewline:true,
    printFn:null, printErrFn:null,
    setPrintFn:(...a)=>a.length===1?core.printFn=a[0]:arity(a.length),
    setPrintErrFn:(...a)=>a.length===1?core.printErrFn=a[0]:arity(a.length),
    stringPrint:(...a)=>{
      if(a.length!==1)return arity(a.length);
      if(core.printFn==null)throw new Error('No *print-fn* fn set for evaluation environment');
      invoke(core.printFn,a[0]);return null;
    },
    enableConsolePrint:(...a)=>{if(a.length)return arity(a.length);core.printNewline=false;
      core.printFn=(...xs)=>globalThis.console.log(...xs);core.printErrFn=(...xs)=>globalThis.console.error(...xs);return null;},
    newline:(...a)=>{if(a.length>1)return arity(a.length);core.stringPrint('\n');return truth(printOption(a[0],'flush-on-newline','flushOnNewline'))?core.flush():null;},
    flush:(...a)=>a.length?arity(a.length):null,
    printCore:(...xs)=>{const saved=core.printReadably;core.printReadably=false;try{return core.stringPrint(prStr(...xs));}finally{core.printReadably=saved;}},
    pr:(...xs)=>core.stringPrint(prStr(...xs)),
    println:(...xs)=>{core.printCore(...xs);return truth(core.printNewline)?core.newline():null;},
    prn:(...xs)=>{core.pr(...xs);return truth(core.printNewline)?core.newline():null;},
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
    getIn:(...a)=>{
      if(a.length!==2&&a.length!==3)return arity(a.length);
      let value=a[0];const missing={};
      for(let path=__rt.cljSeq(a[1]);path!==null;path=__rt.cljNext(path)){
        value=a.length===3?get(value,__rt.cljFirst(path),missing):get(value,__rt.cljFirst(path));
        if(value===missing)return a[2];
      }
      return value;
    },
    createNs, findNs, Namespace,
    allNs:(...a)=>a.length?arity(a.length):list(registryNames().map(key=>localNs(key))),
    theNs:name=>{const ns=findNs(name);if(ns===null)throw new Error('No namespace: '+core.str(name));return ns;},
    nsName:(...a)=>a.length!==1?arity(a.length):__rt.formKind(val(a[0]))==='symbol'?__rt.formName(val(a[0])):val(a[0]).name,
    sequence:x=>{const items=__rt.cljSeq(x);return items===null?__rt.formList():items;},
    array:(...a)=>a,
    intArray:typedArray,longArray:typedArray,doubleArray:typedArray,objectArray:typedArray,
    alength:a=>val(a).length,
    primSeq:(...a)=>a.length===1||a.length===2?arraySeq(val(a[0]),a.length===1?0:val(a[1])):arity(a.length),
    arraySeq:(a,i=0)=>val(a)==null?null:arraySeq(val(a),val(i)),
    IndexedSeq:function(a,i,m){return copyArraySeq(val(a),val(i),m);},
    list:(...a)=>list(a), vector:(...a)=>vector(a), hashMap:(...a)=>map(a), hashSet:(...a)=>set(a), set:x=>set(arr(x)), vec:x=>vector(arr(x)),
    arrayMap:(...a)=>{ if (a.length % 2) throw new Error('No value supplied for key'); return arrayMap(a); },
    sortedMap:(...a)=>__rt.formSortedMap(vector(a)),
    sortedMapBy:(f,...a)=>sortedBy.create(f,a),
    compare:(...a)=>a.length===2?__rt.formCompare(a[0],a[1]):arity(a.length),
    // Keep emitter.rs's publishable `comparator` constructor equivalent.
    comparator:f=>(x,y)=>truth(invoke(f,x,y))?-1:truth(invoke(f,y,x))?1:0,
    // Vectors and sorted maps reverse their elements or entries; an
    // IReversible extension wins, as in the legacy runtime.
    rseq:x=>{
      const v=val(x),kind=__rt.formKind(v),builtin=kind==='vector'||kind==='map'&&!__rt.cljIsEditable(v);
      const method=decl.coreMethod('IReversible/-rseq',1,x,builtin);
      if(method!==null)return method(x);
      if(!builtin)throw new Error('No protocol method IReversible.-rseq defined for this value');
      const items=arr(x).slice().reverse();
      return items.length?__rt.formSequence(vector(items)):null;
    },
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
    rest:x=>__rt.cljRest(x), next:x=>__rt.cljNext(x), peek:x=>__rt.cljPeek(x), peekProtocol,
    // The doseq definition steps its bindings in pairs (light-cljs-611).
    nnext:x=>__rt.cljNext(__rt.cljNext(x)),
    pop, popProtocol,
    cons:(x,xs)=>__rt.cljCons(x,xs), concat:(...xs)=>__rt.cljConcatAll(list(xs)),
    listStar:(...xs)=>list([...xs.slice(0,-1),...arr(xs[xs.length-1])]),
    apply:(f,...xs)=>{
      const args=[...xs.slice(0,-1),...arr(xs[xs.length-1])];
      if(isVar(f))return core.applyVar(f,args);
      const kind=__rt.formKind(val(f));
      return (kind==='struct'||kind==='record') && !multi.is(f)
        ? decl.applyIFn(val(f),args) : invoke(f,...args);
    },
    applyTo:(f,n,args)=>{
      args=seqOf(args);n=val(n);
      if(!Number.isInteger(n)||n<0||n>20)throw new Error('Only up to 20 arguments supported on functions');
      const values=[];for(let i=0;i<n;i++,args=__rt.cljRest(args))values.push(__rt.cljFirst(args));
      return invoke(f,...values);
    },
    jsInvoke:(object,method,...args)=>{object=val(object);return object[val(method)](...args);},
    update:(value,key,f,...args)=>{const old=core.get(value,key);return core.assoc(value,key,invoke(f,old,...args));},
    assoc:(m,...xs)=>{for(let i=0;i<xs.length;i+=2)m=__rt.cljAssoc(m,xs[i],xs[i+1]);return m;},
    dissoc:(m,...ks)=>each((m,k)=>__rt.cljDissoc(m,k),m,ks),
    conj:(...a)=>a.length===0?__rt.formVector():each((m,x)=>__rt.cljConj(m,x),a[0],a.slice(1)),
    merge:(...xs)=>xs.some(truth)?xs.reduce((left,right)=>val(right)==null?left:__rt.cljConj(truth(left)?left:arrayMap([]),right)):null,
    zipmap:(keys,values)=>{
      let result=arrayMap([]), ks=__rt.cljSeq(keys), vs=__rt.cljSeq(values);
      while(ks!==null&&vs!==null){
        result=__rt.cljAssoc(result,__rt.cljFirst(ks),__rt.cljFirst(vs));
        // Advance both, in order, even when the keys have just ended.
        ks=__rt.cljNext(ks);vs=__rt.cljNext(vs);
      }
      return result;
    },
    disj:(s,...xs)=>each((s,x)=>__rt.cljDisj(s,x),s,xs),
    contains:(c,k)=>__rt.cljContains(c,k),
    empty:x=>{
      const kind=__rt.formKind(val(x)),builtin=['vector','list','cons','lazy-seq','map','set','record'].includes(kind)||arraySeqs.has(x);
      const method=decl.coreMethod('IEmptyableCollection/-empty',1,x,builtin);
      if(method!==null)return method(x);
      let result=null;
      if(kind==='vector')result=__rt.formVector();
      else if(kind==='map'||kind==='set'){result=x;for(const entry of arr(x))result=kind==='map'?__rt.cljDissoc(result,__rt.cljNth(entry,0)):__rt.cljDisj(result,entry);}
      else if(['list','cons','lazy-seq'].includes(kind)||arraySeqs.has(x))result=__rt.formList();
      return result===null?null:__rt.cljWithMeta(result,__rt.cljMeta(x));
    },
    find:(x,k)=>{const method=decl.coreMethod('IFind/-find',2,x,['map','record','vector'].includes(__rt.formKind(val(x))));
      if(method!==null)return method(x,k);
      if(!(['map','record','vector'].includes(__rt.formKind(val(x)))||decl.satisfies('IAssociative',x))||!__rt.cljContains(x,k))return null;
      if(__rt.formKind(val(x))==='map')for(const entry of arr(x))if(__rt.formEquals(__rt.cljNth(entry,0),k))return entry;
      return __rt.formVector(k,get(x,k));},
    keys:m=>{if(__rt.formKind(val(m))==='record'){const s=__rt.cljSeq(m);return s===null?null:__rt.formSequence(vector(arr(s).map(e=>__rt.formNth(e,0))));}
      const k=__rt.formValue(m)==null?null:__rt.formKeys(__rt.formValue(m));return k===null||__rt.formCount(k)===0?null:__rt.formSequence(k);},
    vals:m=>{const s=__rt.cljSeq(m);return s===null?null:__rt.formSequence(vector(arr(s).map(e=>__rt.formNth(e,1))));},
    meta:x=>__rt.cljMeta(x), withMeta:(x,m)=>__rt.cljWithExportedMeta(x,m),
    varyMeta:(x,f,...args)=>{const m=invoke(f,__rt.cljMeta(x),...args);return val(x)==null?null:__rt.cljWithExportedMeta(x,m);},
    isKeyword:x=>__rt.formKind(val(x))==='keyword', isSymbol:x=>__rt.formKind(val(x))==='symbol',
    isQualifiedKeyword:x=>__rt.formKind(val(x))==='keyword'&&__rt.formNamespace(val(x))!==null,
    isList:x=>__rt.cljIsList(x), isColl:x=>__rt.cljIsColl(x), isMapEntry:x=>__rt.cljIsMapEntry(x),
    isSequential:x=>__rt.cljIsSequential(x), isVector:x=>__rt.cljIsVector(x),
    isMap:x=>{const k=__rt.formKind(val(x));return k==='map'||k==='record';}, isSeq:x=>__rt.cljIsSeq(x),
    isSet:x=>__rt.formKind(val(x))==='set',
    isNil:x=>val(x)==null, isUndefined:x=>x===undefined, isSome:x=>val(x)!=null, isFalse:x=>__rt.cljIsFalse(x), isTrue:x=>val(x)===true, isString:x=>typeof val(x)==='string', not:x=>!truth(x), boolean:truth,
    reduced:x=>__rt.cljReduced(x), isReduced:x=>__rt.cljIsReduced(x), unreduced:x=>__rt.cljUnreduced(x),
    Reduced:function(x){return __rt.cljReduced(x);},
    Cons:function(metadata,head,tail){return __rt.cljWithMeta(__rt.cljCons(head,tail),metadata);},
    ensureReduced:x=>__rt.cljIsReduced(x)?x:__rt.cljReduced(x),
    equal:(...xs)=>xs.every((x,i)=>i===0||__rt.formEquals(xs[i-1],x)), hash:x=>__rt.formHash(x),
    imul:(a,b)=>multiplyInt(val(a),val(b)),
    rotateLeft:(x,d)=>(val(x)<<val(d))|(val(x)>>>(32-val(d))),
    bitOr:bitOp((x,y)=>x|y),bitAnd:bitOp((x,y)=>x&y),bitXor:bitOp((x,y)=>x^y),bitAndNot:bitOp((x,y)=>x&~y),
    bitNot:x=>~val(x),bitClear:(x,n)=>val(x)&~(1<<val(n)),bitFlip:(x,n)=>val(x)^(1<<val(n)),
    bitSet:(x,n)=>val(x)|(1<<val(n)),bitTest:(x,n)=>(val(x)&(1<<val(n)))!==0,
    bitShiftLeft:(x,n)=>val(x)<<val(n),bitShiftRight:(x,n)=>val(x)>>val(n),
    bitShiftRightZeroFill:(x,n)=>val(x)>>>val(n),unsignedBitShiftRight:(x,n)=>val(x)>>>val(n),
    bitCount:x=>{let v=val(x);v=v-((v>>1)&0x55555555);v=(v&0x33333333)+((v>>2)&0x33333333);return (((v+(v>>4))&0x0f0f0f0f)*0x01010101)>>24;},
    notEqual:(...xs)=>!core.equal(...xs),
    name:x=>{if(typeof val(x)==='string')return x;if(__rt.formKind(val(x))==='compiler-literal'||isVar(x))throw new Error("Doesn't support name on this type");if(__rt.formKind(val(x))==='type')throw new Error('Doesn\'t support name: '+__rt.formName(val(x)));return __rt.formName(val(x));},
    namespace:x=>{if(__rt.formKind(val(x))==='compiler-literal'||isVar(x))throw new Error("Doesn't support namespace on this type");if(__rt.formKind(val(x))==='type')throw new Error('Doesn\'t support namespace: '+__rt.formName(val(x)));return __rt.formNamespace(val(x));},
    subs:(...a)=>a.length===2?val(a[0]).substring(val(a[1])):a.length===3?val(a[0]).substring(val(a[1]),val(a[2])):arity(a.length),
    inc:x=>val(x)+1, dec:x=>val(x)-1, multiply:(...xs)=>xs.reduce((a,b)=>a*val(b),1),
    less:(...xs)=>chain(xs,(a,b)=>a<b), greater:(...xs)=>chain(xs,(a,b)=>a>b), lessEqual:(...xs)=>chain(xs,(a,b)=>a<=b), greaterEqual:(...xs)=>chain(xs,(a,b)=>a>=b),
    str:function str(...xs){return xs.map(x=>{x=val(x);if(x==null)return '';const kind=__rt.formKind(x);
      if(kind==='compiler-literal'){
        const tag=__rt.formName(x),payload=__rt.formLiteralPayload(x);
        if(tag==='js-array')return arr(payload).map(v=>val(v)==null?'':str(v)).join(',');
        if(tag==='js-object')return literalObjectString(x);
        if(tag==='regex')return String(x);
        if(tag==='instant')return String(new Date(val(payload)));
        if(tag==='uuid')return val(payload).toLowerCase();
      }
      if(kind==='reduced'||__rt.cljIsAtom(x))return '[object Object]';
      if(kind==='symbol'||kind==='keyword'){const ns=__rt.formNamespace(x);return (kind==='keyword'?':':'')+(ns===null?'':ns+'/')+__rt.formName(x);}
      if(kind==='type')return __rt.formName(x);
      if(kind==='struct'&&isVar(x))return varText(x);
      if(kind==='struct'||kind==='record')return structString(x);
      if(kind!==null){const saved=core.printReadably;core.printReadably=true;try{const {buffer,writer}=stringWriter();printWriterImpl(x,writer,null,true);flushWriter(writer);return buffer.text;}finally{core.printReadably=saved;}}return String(x);}).join('');},
    identical:(a,b)=>val(a)===val(b), isInstance:(c,x)=>c===multi.MultiFn?multi.is(x):__rt.formKind(c)==='type'?__rt.structTypeOf(x)===c:val(x) instanceof c, isRecord:x=>__rt.formKind(val(x))==='record',
    add:(...xs)=>xs.reduce((a,b)=>a+val(b),0), subtract:(a,...xs)=>xs.length?xs.reduce((a,b)=>a-val(b),val(a)):-val(a),
    divide:(...xs)=>xs.length===0?NaN:xs.length===1?1/val(xs[0]):xs.slice(1).reduce((a,b)=>a/val(b),val(xs[0])),
    int:x=>val(x)|0, long:x=>fix(val(x)), byte:x=>x, short:x=>x, float:x=>x, double:x=>x,
    uncheckedInt:x=>fix(val(x)),
    quot:(n,d)=>quot(val(n),val(d)), rem:(n,d)=>{n=val(n);d=val(d);return n-d*quot(n,d);},
    mod:(n,d)=>{n=val(n);d=val(d);return ((n%d)+d)%d;},
    numericEqual:(...xs)=>xs.length!==0&&core.equal(...xs),
    isArray:x=>{x=val(x);return __rt.formKind(x)==='compiler-literal'?__rt.formName(x)==='js-array':Array.isArray(x);},
    isObject:x=>{x=val(x);const kind=__rt.formKind(x);return kind==='compiler-literal'?literalIsObject(x):kind===null&&!__rt.cljIsAtom(x)&&x!=null&&x.constructor===Object;},
    isNumber:x=>typeof val(x)==='number', isFloat:x=>typeof val(x)==='number', isDouble:x=>typeof val(x)==='number',
    isBoolean:x=>typeof val(x)==='boolean',
    isInteger:x=>typeof val(x)==='number'&&!Number.isNaN(val(x))&&val(x)!==Infinity&&parseFloat(val(x))===parseInt(val(x),10),
    isInt:x=>core.isInteger(x),
    keywordIdentical:(x,y)=>val(x)===val(y)||__rt.formKind(val(x))==='keyword'&&__rt.formKind(val(y))==='keyword'&&__rt.formEquals(x,y),
    isInfinite:x=>val(x)===Infinity||val(x)===-Infinity,
    uuid:x=>{x=val(x);if(typeof x!=='string')throw new Error('UUID expects a string');return __rt.formCompilerLiteral('uuid',x.toLowerCase());},
    isUuid:x=>__rt.formKind(val(x))==='compiler-literal'&&__rt.formName(val(x))==='uuid',
    parseUuid:x=>{x=val(x);if(typeof x!=='string')throw new Error('Expected string, got: '+(x==null?'nil':Array.isArray(x)?'array':typeof x));
      return x.length===36&&/^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(x)?core.uuid(x):null;},
    rand:(...a)=>a.length===0?Math.random():a.length===1?val(a[0])*Math.random():arity(a.length),
    randInt:n=>Math.floor(Math.random()*val(n)),
    ExceptionInfo,
    exInfo:(...a)=>a.length===2||a.length===3?new ExceptionInfo(a[0],a[1],a.length===2?null:a[2]):arity(a.length),
    exData:x=>val(x) instanceof ExceptionInfo?val(x).data:null,
    exMessage:x=>val(x) instanceof Error?val(x).message:null,
    exCause:x=>val(x) instanceof ExceptionInfo?val(x).cause:null,
    throwableToMap:error=>{
      const k=n=>__rt.cljKeyword(n),via=[];let root=null;
      for(let current=error;truth(current);current=core.exCause(current)){
        root=current;const value=val(current),pairs=[k('type'),value instanceof ExceptionInfo?__rt.cljSymbol('cljs.core/ExceptionInfo'):value instanceof Error?__rt.formSymbol('js',value.name):null];
        const message=core.exMessage(current),data=core.exData(current);
        if(truth(message))pairs.push(k('message'),message);if(truth(data))pairs.push(k('data'),data);via.push(arrayMap(pairs));
      }
      const pairs=[k('via'),vector(via),k('trace'),null],message=core.exMessage(root),data=core.exData(root),phase=get(core.exData(error),__rt.formKeyword('clojure.error','phase'));
      if(truth(message))pairs.push(k('cause'),message);if(truth(data))pairs.push(k('data'),data);if(truth(phase))pairs.push(k('phase'),phase);return arrayMap(pairs);
    },
    atom, Atom, deref, reset, compareAndSet,
    resetVals:(a,v)=>vector(resetPair(a,v)),
    resetMeta:(a,m)=>{lane(a);const slot=atomOptions(a);store(slot,__rt.formAssoc(__rt.atomLoad(slot),0,m));return m;},
    alterMeta:(a,f,...args)=>core.resetMeta(a,invoke(f,__rt.cljMeta(a),...args)),
    notifyWatches,
    getValidator:a=>{lane(a);const slot=optionsOf(a);return slot===null?null:__rt.formNth(__rt.atomLoad(slot),1);},
    setValidator:(a,f)=>{lane(a);if(val(f)!=null&&!truth(invoke(f,deref(a))))throw new Error('Validator rejected reference state');
      const slot=atomOptions(a);store(slot,__rt.formAssoc(__rt.atomLoad(slot),1,f));return f;},
    addWatch:(a,key,f)=>{lane(a);const slot=atomOptions(a),old=__rt.atomLoad(slot);
      store(slot,__rt.formAssoc(old,2,__rt.cljAssoc(__rt.formNth(old,2),key,f)));return a;},
    removeWatch:(a,key)=>{lane(a);const slot=optionsOf(a);if(slot!==null){const old=__rt.atomLoad(slot);store(slot,__rt.formAssoc(old,2,__rt.cljDissoc(__rt.formNth(old,2),key)));}return a;},
    addWatchProtocol:(a,k,f)=>core.addWatch(a,k,f),
    removeWatchProtocol:(a,k)=>{core.removeWatch(a,k);const slot=optionsOf(a);return slot===null?null:__rt.formNth(__rt.atomLoad(slot),2);},
    swap:(a,f,...args)=>swapPair(a,f,args)[1],
    swapVals:(a,f,...args)=>{const [o,n]=swapPair(a,f,args);return __rt.formVector(o,n);},
    resetVals:(a,v)=>{lane(a);return __rt.formVector(store(a,v),v);},
    // The wrapper captures only its shared atom. Resolve
    // operations in the calling task's runtime, as with published MultiFns.
    memoize:f=>{
      // Original core closures belong to a task (gensym's current scope,
      // namespace state, etc.). Keep their identity as a descriptor, resolved
      // against the consuming task's original bindings, not live replacements.
      // Custom callable properties and user callbacks still undergo capture validation.
      const descriptor=memoCoreCallback(f), adapted=descriptor!==undefined;
      const cache=atom(__rt.formVector(adapted?descriptor:f,arrayMap([]),adapted));
      return (...args)=>globalThis.__lightNativeRuntime.core.invokeMemoized(cache,args);
    },
    invokeMemoized:(cache,args)=>{
      lane(cache);
      const key=vector(args), state=__rt.atomLoad(cache), callback=__rt.formNth(state,0), table=__rt.formNth(state,1), adapted=__rt.formNth(state,2);
      // Presence is independent of the result: nil, false and undefined
      // are cached too. Keys use the native Clojure equality/hash contract.
      if(__rt.cljContains(table,key))return get(table,key);
      // A cold helper can park here. The saved VM continuation resumes
      // after it; exceptions leave the cache untouched. Read the table
      // again after the call to preserve entries added by recursive calls.
      const f=adapted?memoCoreCallbacks[callback]:callback, result=applyArgs(f,args);
      store(cache,__rt.formVector(callback,__rt.cljAssoc(__rt.formNth(__rt.atomLoad(cache),1),key,result),adapted));
      return result;
    },
    // Lazy and chunked sequences (light-cljs-479).
    // An explicit `(LazySeq. meta fn s hash)`: a realized one (no `fn`) holds `s`.
    LazySeq:function(m,f,s){const x=f==null?__rt.lazySeq(()=>s):__rt.lazySeq(f);return m==null?x:__rt.cljWithMeta(x,m);},
    realized:x=>__rt.cljRealized(x),
    mapSeq:(...a)=>a.length===0?lazy(()=>null):a.length===1?mapTransform(a[0]):a.length===2?mapOne(mapOne,a[0],a[1]):mapMany(mapMany,a[0],list(a.slice(1))),
    mapIndexed:(f,c)=>mapIndexedFrom(mapIndexedFrom,f,c,0),
    keepIndexed:(...a)=>a.length===1?keepTransform(a[0],true):a.length===2?keepIndexedFrom(keepIndexedFrom,a[0],a[1],0):arity(a.length),
    filter:(...a)=>a.length===1?filterTransform(a[0]):a.length===2?select(select,a[0],a[1],0):arity(a.length),
    remove:(...a)=>a.length===1?filterTransform(x=>!truth(invoke(a[0],x))):a.length===2?select(select,a[0],a[1],1):arity(a.length),
    keep:(...a)=>a.length===1?keepTransform(a[0],false):a.length===2?select(select,a[0],a[1],2):arity(a.length),
    mapcat:(...a)=>a.length===0?arity(0):a.length===1?mapcatTransform(a[0]):catSeqs(catSeqs,core.mapSeq(...a)),
    flatten:x=>flattenFrom(flattenFrom,__rt.cljIsSequential(x)?__rt.cljCons(flattenChildren(flattenWalk,x),null):null),
    partitionBy:(...a)=>a.length===1?partitionByTransform(a[0]):a.length===2?partitionByFrom(partitionByFrom,a[0],a[1]):arity(a.length),
    partitionAll:(...a)=>a.length===1?partitionAllTransform(val(a[0])):a.length===2?partitionAllFrom(partitionAllFrom,val(a[0]),val(a[0]),a[1]):a.length===3?partitionAllFrom(partitionAllFrom,val(a[0]),val(a[1]),a[2]):arity(a.length),
    take:(n,c)=>takeN(takeN,val(n),c), drop:(n,c)=>dropN(val(n),c),
    nthrest:(x,n)=>{
      for(n=val(n);n>0;n--){const s=seqOf(x);if(s===null)break;x=restOf(s);}
      return x;
    },
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
    sort:(...a)=>{
      if(a.length!==1&&a.length!==2)return arity(a.length);
      const coll=a[a.length-1],f=a.length===1?core.compare:a[0];
      if(seqOf(coll)===null)return __rt.formList();
      const items=arr(coll).slice();items.sort((x,y)=>fnCompare(f,x,y));
      return __rt.cljWithMeta(__rt.formSequence(vector(items)),__rt.cljMeta(coll));
    },
    sortBy:(...a)=>a.length===2?core.sort((x,y)=>core.compare(invoke(a[0],x),invoke(a[0],y)),a[1])
      :a.length===3?core.sort((x,y)=>fnCompare(a[1],invoke(a[0],x),invoke(a[0],y)),a[2]):arity(a.length),
    maxKey:(f,x,...xs)=>xs.reduce((best,value)=>val(invoke(f,best))>val(invoke(f,value))?best:value,x),
    minKey:(f,x,...xs)=>xs.reduce((best,value)=>val(invoke(f,best))<val(invoke(f,value))?best:value,x),
    isDistinct:(...xs)=>{if(xs.length===0)return false;if(xs.length===1)return true;let seen=set([]);for(const x of xs){if(__rt.cljContains(seen,x))return false;seen=__rt.cljConj(seen,x);}return true;},
    distinct:(...a)=>{
      if(a.length===1)return distinctFrom(distinctFrom,a[0],set([]));
      if(a.length)return arity(a.length);
      return rf=>{let seen=set([]);return reducing(rf,(result,x)=>{
        if(__rt.cljContains(seen,x))return result;
        seen=__rt.cljConj(seen,x);return invoke(rf,result,x);
      });};
    },
    haltWhen:(...a)=>{
      if(a.length!==1&&a.length!==2)return arity(a.length);
      const pred=a[0],retf=a[1],halt=__rt.formKeyword('cljs.core','halt');
      return rf=>reducing(rf,(result,x)=>truth(invoke(pred,x))
        ?__rt.cljReduced(arrayMap([halt,truth(retf)?invoke(retf,invoke(rf,result),x):x])):invoke(rf,result,x),
        result=>core.isMap(result)&&__rt.cljContains(result,halt)?get(result,halt):invoke(rf,result));
    },
    repeatedly:(...a)=>a.length===1?repeatedlyFrom(repeatedlyFrom,a[0]):a.length===2?core.take(a[0],repeatedlyFrom(repeatedlyFrom,a[1])):arity(a.length),
    treeSeq:(branch,children,node)=>treeFrom(treeFrom,branch,children,node),
    trampoline:(f,...args)=>{let result=args.length?invoke(f,...args):invoke(f);while(core.isFn(result))result=invoke(result);return result;},
    hashOrderedColl:c=>__rt.formHash(list(arr(c))),
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
      lazyResolve(name);
      const bindings=registry();
      if(bindingOverlays.has(name))return binding||bindingOverlays.get(name)!==undefined;
      if(!(name in bindings))return false;
      if(binding)return true;
      if(roots.has(name)&&__rt.initializerSlotGet(roots.get(name),0)===null)return false;
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
    registerReaderNamespace:(name,context={},added=false)=>{
      const ns=createNs(__rt.cljSymbol(val(name)));
      if(added&&ns.readerContext){for(const b of context.bindings)ns.readerContext.bindings.add(b);return ns;}
      ns.readerContext={aliases:new Map(context.aliases??[]),refers:new Map((context.refers??[]).map(([local,ns,name])=>[local,[ns,name]])),
        excludes:new Set(context.excludes??[]),bindings:new Set(context.bindings??[])};
      return ns;
    }, addReaderDefinitions:()=>null,
    PersistentVector:{EMPTY:__rt.formVector()}, PersistentArrayMap:{EMPTY:arrayMap([])}, List:{EMPTY:__rt.formList()},
    PersistentHashSet:{EMPTY:ordered(__rt.formSet())},
    Keyword:{}, Symbol:{}, PersistentHashMap:{EMPTY:ordered(__rt.formMap()), fromArrays:(ks,vs)=>{
      let m=ordered(__rt.formMap());for(let i=0;i<ks.length;i++)m=__rt.cljAssoc(m,ks[i],vs[i]);return m;
    }}, EmptyList:{},
  };
  // As the legacy runtimes: `cljs.core` follows `cljs.user`, both before
  // anything (the registry starts with them).
  currentNamespace=localNs('cljs.user',__rt.cljSymbol('cljs.user'));
  localNs('cljs.core',__rt.cljSymbol('cljs.core'));
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
  // Compact identity snapshot before user core writes. Bootstrap data entries
  // have the same order in every task; live/unsupported getters are nonenumerable.
  // Descriptors store an index, never a task-owned function or property table.
  const memoCoreCallbacks=Object.values(core);
  const memoCoreCallback=f=>{
    const index=typeof f==='function'?memoCoreCallbacks.indexOf(f):-1;
    if(index===-1)return undefined;
    for(const key of Reflect.ownKeys(f)){
      const d=Object.getOwnPropertyDescriptor(f,key);
      if(d.get!==undefined||d.set!==undefined)return undefined;
      if(key==='name'&&typeof d.value==='string'||key==='length'&&typeof d.value==='number')continue;
      if(key==='prototype'&&d.value!==null&&typeof d.value==='object'){
        const keys=Reflect.ownKeys(d.value), constructor=Object.getOwnPropertyDescriptor(d.value,'constructor');
        if(keys.length===1&&keys[0]==='constructor'&&constructor?.value===f)continue;
      }
      return undefined;
    }
    return index;
  };
  const rt={core,
    jsObject:pairs=>{
      const result={};
      for(let i=0;i<pairs.length;i+=2){
        const key=val(pairs[i]);
        if(key==='__proto__'){
          const prototype=val(pairs[i+1]);
          if(prototype===null||typeof prototype==='object'||typeof prototype==='function')Object.setPrototypeOf(result,prototype);
        }else Object.defineProperty(result,key,{value:pairs[i+1],writable:true,enumerable:true,configurable:true});
      }
      return result;
    },
    // Inspect data properties without running an unsupported entry's getter.
    // Units keep that getter's access at the original expression site.
    nativeCoreBinding:name=>Object.getOwnPropertyDescriptor(core,name),
    nativeInterop:interop,intrinsicRegExp,nativeRegExp,nativeRegExpCall,nativeRegExpMember:function(_,...a){return nativeRegExpCall(intrinsicRegExp)(...a);},nativeConstruct:(C,...a)=>nativeRegExp(C)(...a),declarationReceiver:x=>{decl.demandObject(x);return x;},
    nativeStoreValue:(x,v)=>__rt.structState(x)!==0?v:interop(v),
    protocol:name=>Object.freeze({name}),
    protocolMethod:(key,name,arities,viaMetadata)=>decl.method(key,name,arities,viaMetadata===true),
    protocolPredicate:(protocol,key,x,direct)=>direct?decl.implementsDirectly(key,x):decl.satisfies(key,x),
    extendType:(target,specs)=>decl.extend(target,specs),
    extendInstance:decl.extendInstance, reify,
    nativeType:(fields,specs,record,name,view,mutable,ty,fieldMembers)=>decl.type(fields,specs,view,mutable,ty,fieldMembers),
    keyword:(ns,n)=>__rt.formKeyword(ns,n),symbol:(ns,n)=>__rt.formSymbol(ns,n),get,
    vector:(...xs)=>vector(xs),vectorFromArray:vector,listFromArray:list,mapFromArray:literalMap,arrayMapFromArray:arrayMap,constantMapFromArray:constantMap,
    mapFromArrays:(ks,vs)=>map(ks.flatMap((k,i)=>[k,vs[i]])),
    mapShape:keys=>keys,mapFromShape:(ks,vs)=>literalMap(ks.flatMap((k,i)=>[k,vs[i]])),
    setFromArray:set,arraySetFromArray:set,constantSetFromArray:constantSet,literalSetFromArray:literalSet,
    fnRest:(args,n)=>list(Array.prototype.slice.call(args,n)),
    makeIFn:f=>f, makeVar, definitionVar,
    compilerForms:{
      jsObj:bootstrapJsObj,
      jsArray:items=>__rt.formCompilerLiteral('js-array',vector(items)),
      jsObject:pairs=>__rt.formCompilerLiteral('js-object',constantMap(literalObjectPairs(pairs))),
      withMeta:(value,meta)=>__rt.cljWithExportedMeta(value,meta),
      gensymScope:(serial,prefix,retired='',native=true)=>{
        if(retired)for(const ended of retired.split(','))readerGensymScopes.delete(+ended);
        gensymScope=serial;gensymPrefix=prefix;gensymNative=native;return null;
      },takeEncodePeak:()=>0,
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
const productReaders=new Map();
// Of each root, the unit of its definition's executable :test metadata;
// of each unit, its root's slot.
const rootTests=new Map(), unitSlots=new Map();
// Private task-owned slots: unit/name/state/value/defined are fields 0..4.
// Full JS values stay traced; public roots and saved getters remain ordinary.
// A bundle replaces an unavailable declaration's getter. Remember that it
// defines the name, retaining the unit record for later ordinary registrations.
globalThis.__lightInstallCompilerProduct=(name,read,bundle)=>{
  lazyResolve(name);
  const previous=roots.get(name);
  if(previous!==undefined)__rt.initializerSlotSet(previous,4,true);
  else {const slot=__rt.initializerSlot(null,name);__rt.initializerSlotSet(slot,4,true);roots.set(name,slot);}
  Object.defineProperty(registry(),name,{configurable:true,get:read});
  if(bundle!==undefined)productReaders.set(read,__rt.vector(bundle,name));
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
  lazyResolve(name);
  if(bindingOverlays.has(name))bindingOverlays.set(name,value);
  else registry()[name]=value;
  return value;
};
globalThis.__lightCompilerRootKept=name=>{
  lazyResolve(name);
  const bound=bindingOverlays.has(name)&&bindingOverlays.get(name)!==undefined;
  if(decl.productOwner(name)!==null)return decl.keptProduct(name)||bound;
  if(roots.has(name)){const root=roots.get(name);return __rt.initializerSlotGet(root,0)!==null||__rt.initializerSlotGet(root,4)===true||bound;}
  return Object.getOwnPropertyDescriptor(registry(),name)!==undefined||bound;
};
globalThis.__lightDefineCompilerRoot=(name,initialize,once)=>{
  lazyResolve(name);
  if(once&&globalThis.__lightCompilerRootKept(name))return null;
  if(decl.defineProduct(name,initialize))return globalThis.__lightCompilerBundleProducts[name];
  if(initialize===null)return undefined;
  const value=initialize();
  if(bindingOverlays.has(name))bindingOverlays.set(name,value);
  else {const previous=roots.get(name);if(previous!==undefined)__rt.initializerSlotSet(previous,4,true);Object.defineProperty(registry(),name,{configurable:true,writable:true,value});}
  return value;
};
let initializerSerial=0;
const initializerStack=[];
// Helper compilation uses a separate task. Initializers on this task remain
// the innermost code; the catalog supplies the analysis ancestry in Rust.
globalThis.__lightBuildCaller=()=>(initializerStack.length?'i':'e')+'\u0000'+initializerStack.join(',');
let stringServiceSerial=0;
globalThis.__lightStringServiceName=null;
globalThis.__lightStringServiceRequest=null;
globalThis.__lightCallStringService=(name,request)=>{
  if(typeof request!=='string')throw new TypeError('compiler service expected a string');
  globalThis.__lightStringServiceName=name;
  globalThis.__lightStringServiceRequest=request;
  try{return __rt.nth(__rt.need(++stringServiceSerial,stringKind),0);}
  finally{globalThis.__lightStringServiceName=null;globalThis.__lightStringServiceRequest=null;}
};
globalThis.__lightInitializerUnit=null;
globalThis.__lightInitializerParent=null;
globalThis.__lightInitializerResult=null;
globalThis.__lightInitializerFailure=null;
globalThis.__lightInitializerRequireShared=false;
globalThis.__lightInitializerSelect=false;
globalThis.__lightRegisterUnit=(name,unit,once=false,location='',test=null)=>{
  lazyResolve(name);
  if(!roots.has(name)&&(name in registry()))return null;
  if(roots.has(name)&&__rt.initializerSlotGet(roots.get(name),0)===unit)return null;
  // A defonce keeps an earlier definition (not a declaration), as the
  // legacy runtimes' registry does.
  if(once&&roots.has(name)&&(__rt.initializerSlotGet(roots.get(name),0)!==null||__rt.initializerSlotGet(roots.get(name),4)===true))return null;
  const slot=__rt.initializerSlot(unit,name);roots.set(name,slot);
  // The unit of the definition's executable :test metadata, whose own root
  // a Var reflects (`__lightCompilerVarMetadata`).
  rootTests.set(name,test);
  if(unit!==null)unitSlots.set(unit,slot);
  Object.defineProperty(registry(),name,{configurable:true,get:rootGetter(name,unit,slot),set:value=>{
    __rt.initializerSlotSet(slot,4,true);
    Object.defineProperty(rootTarget,name,{configurable:true,writable:true,value});
  }});return null;
};
const rootGetter=(name,unit,slot)=>()=>{
  if(unit===null)throw new Error('unavailable definition: '+name);
  __rt.need(unit,2);
  const factory=__rt.need(unit,3);
  return initializeValue(slot,factory);
};
const demandSlot=slot=>{
  const unit=__rt.initializerSlotGet(slot,0);
  if(unit===null)throw new Error('unavailable definition: '+__rt.initializerSlotGet(slot,1));
  __rt.need(unit,2);
  const factory=__rt.need(unit,3);
  return initializeValue(slot,factory);
};
// A definition's executable :test metadata, from its own unit: the unit's
// root initializes once, and a failure stays. A lazily adopted catalog
// installs the unit's root first.
globalThis.__lightCompilerTest=unit=>{
  if(!unitSlots.has(unit)&&lazyCatalog!==null)materializeAll();
  const slot=unitSlots.get(unit);
  if(slot===undefined)throw new Error('unavailable definition: executable :test metadata');
  return demandSlot(slot);
};
// A Var's deferred :test metadata. The source generation is captured
// without demanding the root: a root already initialized gives its
// function's :test, any other the :test unit of the definition registered
// now, which reflection demands only when it reads the metadata.
globalThis.__lightCompilerVarMetadata=name=>{
  lazyResolve(name);
  let ready=false,root;
  const descriptor=Object.getOwnPropertyDescriptor(rootTarget,name);
  if(bindingOverlays.has(name)){ready=true;root=bindingOverlays.get(name);}
  else if(descriptor!==undefined&&'value' in descriptor){ready=true;root=descriptor.value;}
  else if(descriptor!==undefined&&productReaders.has(descriptor.get)){
    const bundle=__rt.nth(productReaders.get(descriptor.get),0);
    if(bundle.state===2&&name in bundle.products){ready=true;root=bundle.products[name];}
  }else if(roots.has(name)&&__rt.initializerSlotGet(roots.get(name),2)===2){ready=true;root=__rt.initializerSlotGet(roots.get(name),3);}
  if(ready){
    const value=__rt.formValue(root),test=value!=null&&value!==false?value.cljs$lang$test:null;
    return ()=>test;
  }
  const test=productReaders.has(descriptor?.get)?null:rootTests.get(name)??null;
  return ()=>test===null?null:globalThis.__lightCompilerTest(test);
};
const initializeValue=(slot,factory)=>{
    const unit=__rt.initializerSlotGet(slot,0),state=__rt.initializerSlotGet(slot,2);
    if(state===2){__rt.need(unit,4);return __rt.initializerSlotGet(slot,3);}
    if(state===1)throw new Error('root initialization cycle: '+(__rt.initializerSlotGet(slot,1)??unit));
    if(state===3)throw __rt.initializerSlotGet(slot,3);
    __rt.initializerSlotSet(slot,2,1);
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
        try{__rt.initializerSlotSet(slot,3,factory(native));}catch(e){failed=true;failure=e;}finally{native.compilerForms.leaveNamespace();initializerStack.pop();}
        globalThis.__lightInitializerUnit=unit;
        globalThis.__lightInitializerResult=__rt.initializerSlotGet(slot,3);
        globalThis.__lightInitializerFailure=failed?String(failure):null;
        const type=__rt.structTypeOf(__rt.initializerSlotGet(slot,3));
        globalThis.__lightInitializerRequireShared=type!==null&&__rt.formName(type)==='cljs.core/MultiFn';
        __rt.need(serial,finishKind);
        if(failed)throw failure;
        __rt.need(unit,4);
      }else{
        const initialized=__rt.need(unit,4);
        if(__rt.nth(initialized,0))__rt.initializerSlotSet(slot,3,__rt.nth(initialized,1));
        else {__rt.initializerSlotSet(slot,3,factory(native));__rt.effect(65535,unit);}
      }
      __rt.initializerSlotSet(slot,2,2);return __rt.initializerSlotGet(slot,3);
    }
    catch(e){__rt.initializerSlotSet(slot,3,e);__rt.initializerSlotSet(slot,2,3);throw e;}
};
globalThis.__lightInitializeCompilerValue=(unit,factory)=>initializeValue(__rt.initializerSlot(unit,undefined),factory);
// Declaration bundles advertise their products without demanding their code.
// Keep preloaded roots and the same explicit unsupported-demand getters as units.
globalThis.__lightRegisterBundle=(names,code,indices=[],location='',once=false,group=false,unit=-1)=>{
  // A bundle without a unit (a test's, or an embedder's registration) keeps
  // preloaded roots and reports demand as unsupported.
  if(unit<0){for(const name of names)if(lazyResolve(name),!(name in registry()))Object.defineProperty(registry(),name,{configurable:true,get:()=>{throw new Error('unsupported operation: helper not preloaded: '+name);}});return null;}
  return decl.register(names,indices,location,once,unit);
};
globalThis.__lightEnterCompilerFrontier=next=>decl.enter(next);
globalThis.__lightLeaveCompilerFrontier=()=>decl.leave();
const rootTarget={};
globalThis.__lightCompilerRoots=new Proxy(rootTarget, {
  get:(target,name,receiver)=>bindingOverlays.has(name)?bindingOverlays.get(name):(lazyCatalog!==null&&lazyResolve(name),Reflect.get(target,name,receiver)),
  has:(target,name)=>bindingOverlays.has(name)||(lazyCatalog!==null&&lazyResolve(name),Reflect.has(target,name)),
});
// The compiler's bounded read intrinsic checks this live overlay Map and
// target on every read. Ordinary interop retains the Proxy and its receiver.
globalThis.__lightCompilerRootContext=__rt.vector(globalThis.__lightCompilerRoots,rootTarget,bindingOverlays,productReaders);
// A producer task adopts its parent's catalog snapshot instead of applying
// it (light-cljs-590): a name gets its catalog registrations when the task
// first uses it, and a protocol its catalog bundles, each with exactly the
// state applying every registration at adoption would have given it. That
// state is a function of the task's state at adoption and the registrations
// before it: a registration's effect on a name depends only on that name's
// state and, for a defonce bundle, its other products' states before it.
// Every internal reader and writer of a name resolves it first
// (`lazyResolve`), so an unresolved name still has its state at adoption.
// The Proxy's supported traps (get, has) resolve too; its other operations
// forward straight to the target, so code that reads the public global
// `__lightCompilerRoots` gets a fully applied catalog in registration order
// (`materializeAll`), while compiler code reads the registry through
// `__lightCompilerRootReceiver` (src/emitter.rs) and `registry()`.
const rootsProxy=globalThis.__lightCompilerRoots;
// Declarations executed directly by a macro have no enclosing declaration
// bundle. Their products are ordinary roots, just as a def executed by that
// macro is. A bundle temporarily supplies its private product table instead.
globalThis.__lightCompilerBundleProducts=rootTarget;
globalThis.__lightCompilerRootReceiver=null;
// Catalog entries this task applied on first use: the producer counters.
globalThis.__lightCatalogApplied=0;
function registry(){return globalThis.__lightCompilerRootReceiver??globalThis.__lightCompilerRoots;}
let lazyCatalog=null;
const lazyAdopt=snapshot=>{
  const entries=__rt.nth(snapshot,0),end=__rt.count(entries);
  const global=Object.getOwnPropertyDescriptor(globalThis,'__lightCompilerRoots');
  // Anything unusual applies the catalog as before.
  if(end===0||bindingOverlays.size!==0||global===undefined||global.value!==rootsProxy||!global.writable
     ||!global.configurable||!Reflect.isExtensible(rootTarget))return false;
  const pre=Reflect.ownKeys(rootTarget);
  if(pre.some(key=>!Reflect.getOwnPropertyDescriptor(rootTarget,key).configurable))return false;
  const c={entries,end,names:__rt.nth(snapshot,1),protocols:__rt.nth(snapshot,2),
    // Its bundles' declaration orders follow those registered before it.
    base:decl.reserve(__rt.nth(snapshot,3)),
    // Each resolved name's state at adoption, each bundle's whether it is
    // registered (a defonce bundle may not be), and its task object.
    resolved:new Map(),prefixes:new Map(),effective:new Map(),bundles:new Map(),protocolsDone:new Set(),
    // Names the catalog created, at the position that created each, and
    // the target's keys before it: `materializeAll` restores key order.
    created:new Map(),pre:new Set(pre),global};
  lazyCatalog=c;
  globalThis.__lightCompilerRootReceiver=rootsProxy;
  Object.defineProperty(globalThis,'__lightCompilerRoots',{configurable:true,enumerable:global.enumerable,
    get:()=>{materializeAll();return globalThis.__lightCompilerRoots;},
    set:value=>{materializeAll();globalThis.__lightCompilerRoots=value;}});
  // The compiler's read intrinsic reads a name the target already has
  // without asking the Proxy: resolve those now.
  for(const key of pre)if(typeof key==='string')lazyResolve(key);
  return true;
};
// A name's state as the registration functions see it.
const liveState=name=>{
  const slot=roots.get(name),d=Reflect.getOwnPropertyDescriptor(rootTarget,name);
  return {slot:slot!==undefined,unit:slot===undefined?null:__rt.initializerSlotGet(slot,0),
    defined:slot!==undefined&&__rt.initializerSlotGet(slot,4)===true,
    own:d!==undefined,data:d!==undefined&&'value' in d,has:d!==undefined||Reflect.has(rootTarget,name),
    // A new slot, the defined mark on the slot it had, the entry that owns
    // its property and the one that created it.
    fresh:false,pre:false,owner:-1,first:-1};
};
// Catalog entry `p` applied to a name's state `s`, as `__lightRegisterUnit`
// and `decl.register` (through `__lightInstallCompilerProduct`) apply it.
const lazyStep=(c,s,p)=>{
  globalThis.__lightCatalogApplied++;
  const e=__rt.nth(c.entries,p);
  if(__rt.nth(e,0)===0){
    const unit=__rt.nth(e,2);
    if(!s.slot&&s.has||s.slot&&s.unit===unit||__rt.nth(e,3)&&s.slot&&(s.unit!==null||s.defined))return s;
    return {slot:true,unit,defined:false,own:true,data:false,has:true,fresh:true,pre:s.pre,owner:p,first:s.own?s.first:p};
  }
  if(!lazyEffective(c,p)||s.own&&s.data)return s;
  return {slot:true,unit:s.slot?s.unit:null,defined:true,own:true,data:false,has:true,
    fresh:s.slot?s.fresh:true,pre:s.pre||s.slot&&!s.fresh,owner:p,first:s.own?s.first:p};
};
const lazyStateBefore=(c,name,p)=>{
  const at=__rt.getOr(c.names,name,null);
  let states=c.prefixes.get(name);
  // Keep the adoption state even after this name is installed or written.
  // states[i] includes exactly the first i registrations of this name.
  if(states===undefined){states=[c.resolved.get(name)??liveState(name)];c.prefixes.set(name,states);}
  if(at===null)return states[0];
  let i=states.length-1;
  // Dependencies of an earlier defonce may ask for an already cached prefix.
  if(i>0&&__rt.nth(at,i-1)>=p){
    let lo=0,hi=i;
    while(lo<hi){const mid=Math.floor((lo+hi)/2);if(__rt.nth(at,mid)<p)lo=mid+1;else hi=mid;}
    return states[lo];
  }
  for(const n=__rt.count(at);i<n;i++){
    const q=__rt.nth(at,i);if(q>=p)break;
    states.push(lazyStep(c,states[i],q));
  }
  return states[i];
};
// Whether bundle `p` registers: a defonce bundle does not when every one
// of its products is kept (`__lightCompilerRootKept`) before it.
const lazyEffective=(c,p)=>{
  let r=c.effective.get(p);
  if(r===undefined){
    const e=__rt.nth(c.entries,p),names=__rt.nth(e,1),n=__rt.count(names);
    r=!__rt.nth(e,4)||n===0;
    for(let i=0;!r&&i<n;i++){
      const s=lazyStateBefore(c,__rt.nth(names,i),p);
      if(!(s.slot?s.unit!==null||s.defined:s.own))r=true;
    }
    c.effective.set(p,r);
  }
  return r;
};
const lazyBundle=(c,p)=>{
  let b=c.bundles.get(p);
  if(b===undefined){
    const e=__rt.nth(c.entries,p),products=__rt.nth(e,1),extensions=__rt.nth(e,2),names=[],indices=[];
    for(let i=0;i<__rt.count(products);i++)names.push(__rt.nth(products,i));
    for(let i=0;i<__rt.count(extensions);i++)indices.push([__rt.nth(__rt.nth(extensions,i),0),__rt.nth(__rt.nth(extensions,i),1)]);
    b=decl.makeBundle(names,indices,__rt.nth(e,3),__rt.nth(e,5),c.base+__rt.nth(e,6)+1);
    c.bundles.set(p,b);
  }
  return b;
};
// Install a name's catalog registrations, once, before its first use.
function lazyResolve(name){
  const c=lazyCatalog;
  if(c===null||typeof name!=='string'||c.resolved.has(name))return;
  const at=__rt.getOr(c.names,name,null);
  if(at===null)return;
  const start=liveState(name);
  c.resolved.set(name,start);
  const s=lazyStateBefore(c,name,c.end);
  if(s.owner<0)return;
  const previous=roots.get(name);
  if(s.pre)__rt.initializerSlotSet(previous,4,true);
  let slot=previous;
  if(s.fresh){slot=__rt.initializerSlot(s.unit,name);if(s.defined)__rt.initializerSlotSet(slot,4,true);roots.set(name,slot);}
  const owner=__rt.nth(c.entries,s.owner);
  if(__rt.nth(owner,0)===0){
    // As `__lightRegisterUnit`: the :test unit Var reflection reads.
    rootTests.set(name,__rt.nth(owner,4));
    if(s.unit!==null)unitSlots.set(s.unit,slot);
    Object.defineProperty(rootTarget,name,{configurable:true,get:rootGetter(name,s.unit,slot)});
  }else{
    const b=lazyBundle(c,s.owner),read=decl.productGetter(b,name);
    Object.defineProperty(rootTarget,name,{configurable:true,get:read});
    productReaders.set(read,__rt.vector(b,name));
  }
  if(!start.own){
    // A bundle creates its products in order.
    const e=__rt.nth(c.entries,s.first);
    let at=s.first;
    if(__rt.nth(e,0)===1){
      const products=__rt.nth(e,1),n=__rt.count(products);
      for(let i=0;i<n;i++)if(__rt.nth(products,i)===name){at+=i/(n+1);break;}
    }
    c.created.set(name,at);
  }
}
// Index a protocol's catalog bundles, once, before its extensions are read
// or another bundle extends it.
function catalogProtocol(protocol){
  const c=lazyCatalog;
  if(c===null||c.protocolsDone.has(protocol))return;
  c.protocolsDone.add(protocol);
  const at=__rt.getOr(c.protocols,protocol,null);
  if(at===null)return;
  for(let i=0,n=__rt.count(at);i<n;i++){
    const q=__rt.nth(at,i);
    if(q>=c.end)break;
    globalThis.__lightCatalogApplied++;
    if(lazyEffective(c,q))decl.index(lazyBundle(c,q),protocol);
  }
}
// Apply the rest of the adopted catalog and return the public global.
function materializeAll(){
  const c=lazyCatalog;
  if(c===null)return;
  for(let p=0;p<c.end;p++){
    const e=__rt.nth(c.entries,p);
    if(__rt.nth(e,0)===0){lazyResolve(__rt.nth(e,1));continue;}
    const names=__rt.nth(e,1),extensions=__rt.nth(e,2);
    for(let i=0;i<__rt.count(names);i++)lazyResolve(__rt.nth(names,i));
    for(let i=0;i<__rt.count(extensions);i++)catalogProtocol(__rt.nth(__rt.nth(extensions,i),0));
  }
  // Names the catalog created follow the keys before it, in the order the
  // catalog created them, and precede every key added since.
  const keys=Reflect.ownKeys(rootTarget).filter(key=>!c.pre.has(key)),present=new Set(keys);
  const order=[...c.created.keys()].filter(key=>present.has(key)).sort((a,b)=>c.created.get(a)-c.created.get(b));
  for(const key of keys)if(!c.created.has(key))order.push(key);
  if(order.some((key,i)=>key!==keys[i])){
    const saved=order.map(key=>[key,Reflect.getOwnPropertyDescriptor(rootTarget,key)]);
    for(const [key] of saved)Reflect.deleteProperty(rootTarget,key);
    for(const [key,d] of saved)Reflect.defineProperty(rootTarget,key,d);
  }
  lazyCatalog=null;
  globalThis.__lightCompilerRootReceiver=null;
  Object.defineProperty(globalThis,'__lightCompilerRoots',{value:rootsProxy,writable:c.global.writable,
    enumerable:c.global.enumerable,configurable:c.global.configurable});
}
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
      // A producer's whole catalog is adopted, a suffix applied.
      const snapshot=__rt.nth(input,0),catalog=__rt.nth(snapshot,0);
      if(!(__rt.nth(input,2)&&__rt.nth(input,1)===0&&lazyCatalog===null&&lazyAdopt(snapshot)))
      for(let at=__rt.nth(input,1);at<__rt.count(catalog);at++){
        const entry=__rt.nth(catalog,at);
        if(__rt.nth(entry,0)===0){
          globalThis.__lightRegisterUnit(__rt.nth(entry,1),__rt.nth(entry,2),__rt.nth(entry,3),'',__rt.nth(entry,4));
        }else{
          const products=__rt.nth(entry,1), extensions=__rt.nth(entry,2), names=[], indices=[];
          for(let i=0;i<__rt.count(products);i++)names.push(__rt.nth(products,i));
          for(let i=0;i<__rt.count(extensions);i++)indices.push([__rt.nth(__rt.nth(extensions,i),0),__rt.nth(__rt.nth(extensions,i),1)]);
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
