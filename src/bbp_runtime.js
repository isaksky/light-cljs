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
  const extend = (target, specs) => {
    const key = keyOf(target);
    for (const [name, arity, fn] of specs) methodTable(name, arity).entries.push({order, key, fn});
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
    const visible = t.entries.filter(e => e.order <= frontier).sort((a, b) => a.order - b.order);
    for (const e of visible) map = __rt.assoc(map, e.key, e.fn);
    t.map = map; t.version = version; t.frontier = frontier;
  };
  const resolve = (t, x) => {
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
  const missing = (name, x) => new Error('No protocol method ' + name + ' defined for type ' + (x === null || x === undefined ? 'null' : __rt.formKind(x) ?? typeof x));
  const method = (key, name, arities) => {
    const ts = arities.map(n => methodTable(key, n));
    if (arities.length === 1) {
      const t = ts[0];
      switch (arities[0]) {
        case 1: return x => { const m = resolve(t, x); if (m == null) throw missing(name, x); return m.call(x, x); };
        case 2: return (x, a) => { const m = resolve(t, x); if (m == null) throw missing(name, x); return m.call(x, x, a); };
        case 3: return (x, a, b) => { const m = resolve(t, x); if (m == null) throw missing(name, x); return m.call(x, x, a, b); };
      }
    }
    return function (x, ...args) {
      const i = arities.indexOf(args.length + 1);
      if (i < 0) throw new Error('Invalid arity: ' + (args.length + 1));
      const m = resolve(ts[i], x);
      if (m == null) throw missing(name, x);
      return m.call(x, x, ...args);
    };
  };
  const satisfies = (key, x) => resolve(methodTable(key, 0), x) != null;
  // A type: a native struct type, or with a mutable field the constructor
  // of task-owned JavaScript instances. `view` constructs the JavaScript
  // object a struct's interop reads.
  const type = (fields, specs, view, mutable, ty) => {
    views.set(ty, {view, fields, record: __rt.formKind(__rt.structNew(ty)) === 'record'});
    extend(ty, specs);
    if (!mutable) return ty;
    view.__lightNativeType = ty;
    view.prototype.__lightNativeType = ty;
    return view;
  };
  const viewOf = s => {
    const info = views.get(__rt.structTypeOf(s));
    if (info === undefined) throw new Error('unsupported operation: JavaScript interop with a native struct of another task');
    const args = info.fields.map(f => __rt.structField(s, f));
    if (info.record) args.push(__rt.cljMeta(s), null, null);
    return new info.view(...args);
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
  return {extend, method, satisfies, type, viewOf, register, enter, leave, coreKeys, isRecord};
})();
const native = (() => {
  let gensymFloor=0;
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
  const arr = x => {
    if (Array.isArray(x)) return x;
    let value = val(x);
    const kind = __rt.formKind(value);
    if (kind !== 'list' && kind !== 'vector') {
      const items = __rt.cljSeq(x);
      if (items === null) return [];
      value = val(items);
    }
    const out = [];
    for (let i = 0, n = __rt.formCount(value); i < n; i++) out.push(__rt.formNth(value, i));
    return out;
  };
  const list = a => { let v = __rt.formList(); for(let i=a.length-1;i>=0;i--) v=__rt.formConj(v,a[i]); return v; };
  const vector = a => { let v=__rt.formVector();for(const x of a)v=__rt.formConj(v,x);return v; };
  const map = a => { let v=__rt.formMap();for(let i=0;i<a.length;i+=2)v=__rt.formAssoc(v,a[i],a[i+1]);return v; };
  const set = a => { let v=__rt.formSet();for(const x of a)v=__rt.formConj(v,x);return v; };
  // A set literal with members that are not constants rejects duplicates.
  // Equal members or keys of a reader-distinct literal (`#{\a "a"}`) are not
  // representable natively and fail explicitly instead of collapsing.
  const counted = (v, n, message) => { if (__rt.formCount(v) !== n) throw new Error(message); return v; };
  const literalSet = a => counted(set(a), a.length, 'Duplicate key');
  const constantSet = a => counted(set(a), a.length, 'unsupported operation: native literal set with reader-distinct equal members');
  const constantMap = a => counted(map(a), a.length / 2, 'unsupported operation: native literal map with reader-distinct equal keys');
  const arity = n => { throw new Error('Invalid arity: ' + n); };
  const seq = x => __rt.cljSeq(x);
  const get = (m,k,n=null) => __rt.cljGet(m,k,n);
  const invoke = (f,...args) => typeof f === 'function' ? f(...args) : __rt.formKind(val(f))==='keyword' ? get(args[0],f,args.length>1?args[1]:null) : get(f,args[0],args.length>1?args[1]:null);
  const unsupported = name => function(){throw new Error('unsupported operation: native macro runtime '+name);};
  const each = (op, x, xs) => { for (const y of xs) x = op(x, y); return x; };
  const core = {
    assertions:true,
    invoke, truth, get, identity:x=>x, macroReaderFunction:f=>f,
    sequence:x=>{const items=__rt.cljSeq(x);return items===null?__rt.formList():items;},
    list:(...a)=>list(a), vector:(...a)=>vector(a), hashMap:(...a)=>map(a), hashSet:(...a)=>set(a), set:x=>set(arr(x)), vec:x=>vector(arr(x)),
    arrayMap:(...a)=>{ if (a.length % 2) throw new Error('No value supplied for key'); return map(a); },
    keyword:(...a)=>a.length===1?__rt.cljKeyword(a[0]):a.length===2?__rt.cljKeywordNs(a[0],a[1]):arity(a.length),
    symbol:(...a)=>a.length===1?__rt.cljSymbol(a[0]):a.length===2?__rt.cljSymbolNs(a[0],a[1]):arity(a.length),
    first:x=>__rt.cljFirst(x), second:x=>__rt.cljFirst(__rt.cljNext(x)),
    nth:(x,n,...d)=>d.length?__rt.cljNthOr(x,n,d[0]):__rt.cljNth(x,n),
    count:x=>__rt.cljCount(x),
    seq, isEmpty:x=>__rt.cljSeq(x)===null,
    rest:x=>__rt.cljRest(x), next:x=>__rt.cljNext(x),
    cons:(x,xs)=>__rt.cljCons(x,xs), concat:(...xs)=>list(xs.flatMap(arr)),
    listStar:(...xs)=>list([...xs.slice(0,-1),...arr(xs[xs.length-1])]),
    apply:(f,...xs)=>invoke(f,...xs.slice(0,-1),...arr(xs[xs.length-1])),
    assoc:(m,...xs)=>{for(let i=0;i<xs.length;i+=2)m=__rt.cljAssoc(m,xs[i],xs[i+1]);return m;},
    dissoc:(m,...ks)=>each((m,k)=>__rt.cljDissoc(m,k),m,ks),
    conj:(m,...xs)=>each((m,x)=>__rt.cljConj(m,x),m,xs),
    disj:(s,...xs)=>each((s,x)=>__rt.cljDisj(s,x),s,xs),
    contains:(c,k)=>__rt.cljContains(c,k),
    keys:m=>{if(__rt.formKind(val(m))==='record'){const s=__rt.cljSeq(m);return s===null?null:list(arr(s).map(e=>__rt.formNth(e,0)));}
      const k=__rt.formValue(m)==null?null:__rt.formKeys(__rt.formValue(m));return k===null||__rt.formCount(k)===0?null:list(arr(k));},
    vals:m=>{const k=core.keys(m);return k===null?null:list(arr(k).map(key=>__rt.cljGet(m,key,null)));},
    meta:x=>__rt.cljMeta(x), withMeta:(x,m)=>__rt.cljWithMeta(x,m),
    isKeyword:x=>__rt.formKind(val(x))==='keyword', isSymbol:x=>__rt.formKind(val(x))==='symbol',
    isVector:x=>__rt.formKind(val(x))==='vector', isMap:x=>{const k=__rt.formKind(val(x));return k==='map'||k==='record';}, isSeq:x=>__rt.formKind(val(x))==='list',
    isSet:x=>__rt.formKind(val(x))==='set',
    isNil:x=>val(x)==null, not:x=>!truth(x), boolean:truth,
    equal:(...xs)=>xs.every((x,i)=>i===0||__rt.formEquals(xs[i-1],x)), hash:x=>__rt.formHash(x),
    name:x=>__rt.formName(val(x)), namespace:x=>__rt.formNamespace(val(x)),
    inc:x=>val(x)+1, dec:x=>val(x)-1, multiply:(...xs)=>xs.reduce((a,b)=>a*val(b),1),
    less:(a,b)=>val(a)<val(b), greater:(a,b)=>val(a)>val(b), lessEqual:(a,b)=>val(a)<=val(b), greaterEqual:(a,b)=>val(a)>=val(b),
    str:(...xs)=>xs.map(x=>{x=val(x);if(x==null)return '';if(__rt.formKind(x)!==null)throw new Error('unsupported operation: native macro runtime str of a native form');return String(x);}).join(''),
    identical:(a,b)=>a===b, isRecord:x=>__rt.formKind(val(x))==='record',
    add:(...xs)=>xs.reduce((a,b)=>a+val(b),0), subtract:(a,...xs)=>xs.length?xs.reduce((a,b)=>a-val(b),val(a)):-val(a),
    exInfo:(m,d)=>new Error(val(m)),
    registerReaderNamespace:()=>null, addReaderDefinitions:()=>null, setReaderNamespace:()=>null,
    PersistentVector:{EMPTY:__rt.formVector()}, PersistentArrayMap:{EMPTY:__rt.formMap()}, List:{EMPTY:__rt.formList()},
    PersistentHashSet:{EMPTY:__rt.formSet()},
    Keyword:{}, Symbol:{}, PersistentHashMap:{EMPTY:__rt.formMap()}, EmptyList:{},
  };
  // `extend-type` of a core type extends the native form kind it is; array
  // and hash maps are one native kind (light-cljs-478).
  for (const [name, key] of [['Keyword','keyword'],['Symbol','symbol'],['PersistentVector','vector'],
    ['PersistentArrayMap','map'],['PersistentHashMap','map'],['PersistentHashSet','set'],['List','list'],['EmptyList','list']])
    decl.coreKeys.set(core[name], key);
  for (const name of [/* CORE_KEYS */]) if (!(name in core)) Object.defineProperty(core,name,{get:unsupported('core.'+name)});
  const rt={core,
    nativeInterop:interop,declarationReceiver:x=>x,
    protocol:name=>Object.freeze({name}),
    protocolMethod:(key,name,arities)=>decl.method(key,name,arities),
    protocolPredicate:(protocol,key,x)=>decl.satisfies(key,x),
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
    compilerForms:{gensymScope:()=>null,takeEncodePeak:()=>0,
      takeGensyms:()=>__rt.formVector(gensymFloor),raiseGensyms:n=>{gensymFloor=Math.max(gensymFloor,n);}},
  };
  return rt;
})();
globalThis.__lightNativeRuntime=native;
const roots=new Map();
let initializerSerial=0;
globalThis.__lightInitializerUnit=null;
globalThis.__lightInitializerResult=null;
globalThis.__lightInitializerFailure=null;
globalThis.__lightRegisterUnit=(name,unit)=>{
  if(!roots.has(name)&&(name in globalThis.__lightCompilerRoots))return null;
  if(roots.has(name)&&roots.get(name).unit===unit)return null;
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
      const producer=__rt.need(serial,beginKind);
      if(producer){
        let failed=false,failure;
        try{slot.value=factory(native);}catch(e){failed=true;failure=e;}
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
      else {const args=[];for(let i=1;i<__rt.count(input);i++)args.push(values.get(__rt.nth(input,i)));result=values.get(__rt.nth(input,0))(...args);}
      held=true;heldId=id;heldValue=result;
    }
    globalThis.__lightResult=result;
  }catch(e){globalThis.__lightFailure=String(e);globalThis.__lightResult=null;}
}
