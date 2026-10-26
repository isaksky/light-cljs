// First-party bounded native-form runtime. Mutable JS belongs to this one task.
const native = (() => {
  let gensymFloor=0;
  const val = x => __rt.formValue(x);
  const interop = x => {
    const value = val(x), kind = __rt.formKind(value);
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
  const literalSet = a => { const v=set(a); if(__rt.formCount(v)!==a.length)throw new Error('Duplicate key'); return v; };
  const seq = x => __rt.cljSeq(x);
  const get = (m,k,n=null) => __rt.cljGet(m,k,n);
  const invoke = (f,...args) => typeof f === 'function' ? f(...args) : __rt.formKind(val(f))==='keyword' ? get(args[0],f,args.length>1?args[1]:null) : get(f,args[0],args.length>1?args[1]:null);
  const unsupported = name => function(){throw new Error('unsupported operation: native macro runtime '+name);};
  const each = (op, x, xs) => { for (const y of xs) x = op(x, y); return x; };
  const core = {
    assertions:true,
    invoke, truth, get, identity:x=>x, macroReaderFunction:f=>f,
    sequence:x=>{const items=__rt.cljSeq(x);return items===null?__rt.formList():items;},
    list:(...a)=>list(a), vector:(...a)=>vector(a), hashMap:(...a)=>map(a), hashSet:(...a)=>set(a), set:x=>set(arr(x)),
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
    keys:m=>{const k=__rt.formValue(m)==null?null:__rt.formKeys(__rt.formValue(m));return k===null||__rt.formCount(k)===0?null:list(arr(k));},
    vals:m=>{const k=core.keys(m);return k===null?null:list(arr(k).map(key=>__rt.cljGet(m,key,null)));},
    meta:x=>__rt.cljMeta(x), withMeta:(x,m)=>__rt.cljWithMeta(x,m),
    isKeyword:x=>__rt.formKind(val(x))==='keyword', isSymbol:x=>__rt.formKind(val(x))==='symbol',
    isVector:x=>__rt.formKind(val(x))==='vector', isMap:x=>__rt.formKind(val(x))==='map', isSeq:x=>__rt.formKind(val(x))==='list',
    isSet:x=>__rt.formKind(val(x))==='set',
    isNil:x=>val(x)==null, not:x=>!truth(x), boolean:truth,
    equal:(...xs)=>xs.every((x,i)=>i===0||__rt.formEquals(xs[i-1],x)), hash:x=>__rt.formHash(x),
    name:x=>__rt.formName(val(x)), namespace:x=>__rt.formNamespace(val(x)),
    add:(...xs)=>xs.reduce((a,b)=>a+val(b),0), subtract:(a,...xs)=>xs.length?xs.reduce((a,b)=>a-val(b),val(a)):-val(a),
    exInfo:(m,d)=>new Error(val(m)),
    registerReaderNamespace:()=>null, addReaderDefinitions:()=>null, setReaderNamespace:()=>null,
    PersistentVector:{EMPTY:__rt.formVector()}, PersistentArrayMap:{EMPTY:__rt.formMap()}, List:{EMPTY:__rt.formList()},
    PersistentHashSet:{EMPTY:__rt.formSet()},
  };
  for (const name of [/* CORE_KEYS */]) if (!(name in core)) Object.defineProperty(core,name,{get:unsupported('core.'+name)});
  const rt={core,
    nativeInterop:interop,declarationReceiver:x=>x,
    keyword:(ns,n)=>__rt.formKeyword(ns,n),symbol:(ns,n)=>__rt.formSymbol(ns,n),get,
    vector:(...xs)=>vector(xs),vectorFromArray:vector,listFromArray:list,mapFromArray:map,arrayMapFromArray:map,constantMapFromArray:map,
    mapFromArrays:(ks,vs)=>map(ks.flatMap((k,i)=>[k,vs[i]])),
    mapShape:keys=>keys,mapFromShape:(ks,vs)=>map(ks.flatMap((k,i)=>[k,vs[i]])),
    setFromArray:set,arraySetFromArray:set,constantSetFromArray:set,literalSetFromArray:literalSet,
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
globalThis.__lightRegisterBundle=names=>{for(const name of names)if(!(name in globalThis.__lightCompilerRoots))Object.defineProperty(globalThis.__lightCompilerRoots,name,{configurable:true,get:()=>{throw new Error('unsupported operation: helper not preloaded: '+name);}});return null;};
globalThis.__lightEnterCompilerFrontier=()=>null;
globalThis.__lightLeaveCompilerFrontier=()=>null;
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
