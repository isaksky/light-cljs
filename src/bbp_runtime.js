// First-party bounded native-form runtime. Mutable JS belongs to this one task.
const native = (() => {
  const val = x => __rt.formValue(x);
  const truth = x => val(x) !== null && val(x) !== undefined && val(x) !== false;
  const arr = x => {
    if (x == null) return [];
    if (Array.isArray(x)) return x;
    const out = [];
    for (let i = 0; i < __rt.formCount(x); i++) out.push(__rt.formNth(x, i));
    return out;
  };
  const list = a => { let v = __rt.formList(); for(let i=a.length-1;i>=0;i--) v=__rt.formConj(v,a[i]); return v; };
  const vector = a => { let v=__rt.formVector();for(const x of a)v=__rt.formConj(v,x);return v; };
  const map = a => { let v=__rt.formMap();for(let i=0;i<a.length;i+=2)v=__rt.formAssoc(v,a[i],a[i+1]);return v; };
  const get = (m,k,n=null) => m == null ? n : __rt.formGet(m,k,n);
  const invoke = (f,...args) => typeof f === 'function' ? f(...args) : __rt.formKind(f)==='keyword' ? get(args[0],f,args.length>1?args[1]:null) : get(f,args[0],args.length>1?args[1]:null);
  const unsupported = name => function(){throw new Error('unsupported operation: native macro runtime '+name);};
  const core = {
    invoke, truth, get, identity:x=>x, macroReaderFunction:f=>f,
    list:(...a)=>list(a), vector:(...a)=>vector(a), hashMap:(...a)=>map(a),
    first:x=>x==null||__rt.formCount(x)===0?null:__rt.formNth(x,0),
    second:x=>x==null||__rt.formCount(x)<2?null:__rt.formNth(x,1),
    nth:(x,n,d)=>val(n)<__rt.formCount(x)?__rt.formNth(x,val(n)):d===undefined?(()=>{throw new Error('index out of bounds')})():d,
    count:x=>x==null?0:__rt.formCount(x),
    seq:x=>x==null||__rt.formCount(x)===0?null:x,
    rest:x=>list(arr(x).slice(1)), next:x=>arr(x).length<2?null:list(arr(x).slice(1)),
    cons:(x,xs)=>list([x,...arr(xs)]), concat:(...xs)=>list(xs.flatMap(arr)),
    listStar:(...xs)=>list([...xs.slice(0,-1),...arr(xs[xs.length-1])]),
    apply:(f,...xs)=>invoke(f,...xs.slice(0,-1),...arr(xs[xs.length-1])),
    assoc:(m,...xs)=>{for(let i=0;i<xs.length;i+=2)m=__rt.formAssoc(m,xs[i],xs[i+1]);return m;},
    conj:(m,...xs)=>{for(const x of xs)m=__rt.formConj(m,x);return m;},
    meta:x=>__rt.formMeta(x), withMeta:(x,m)=>__rt.formWithMeta(x,m),
    isKeyword:x=>__rt.formKind(x)==='keyword', isSymbol:x=>__rt.formKind(x)==='symbol',
    isVector:x=>__rt.formKind(x)==='vector', isMap:x=>__rt.formKind(x)==='map', isSeq:x=>__rt.formKind(x)==='list',
    isNil:x=>val(x)==null, not:x=>!truth(x), boolean:truth,
    equals:(a,b)=>__rt.formEquals(a,b), eq:(a,b)=>__rt.formEquals(a,b),
    name:x=>__rt.formName(x), namespace:x=>__rt.formNamespace(x),
    add:(...xs)=>xs.reduce((a,b)=>a+val(b),0), subtract:(a,...xs)=>xs.length?xs.reduce((a,b)=>a-val(b),val(a)):-val(a),
    exInfo:(m,d)=>new Error(val(m)),
    registerReaderNamespace:()=>null, addReaderDefinitions:()=>null, setReaderNamespace:()=>null,
    PersistentVector:{EMPTY:__rt.formVector()}, PersistentArrayMap:{EMPTY:__rt.formMap()}, List:{EMPTY:__rt.formList()},
  };
  const rt={core,
    keyword:(ns,n)=>__rt.formKeyword(ns,n),symbol:(ns,n)=>__rt.formSymbol(ns,n),get,
    vector:(...xs)=>vector(xs),vectorFromArray:vector,listFromArray:list,mapFromArray:map,arrayMapFromArray:map,constantMapFromArray:map,
    fnRest:(args,n)=>list(Array.prototype.slice.call(args,n)),
    makeIFn:f=>f,
    compilerForms:{gensymScope:()=>null,takeEncodePeak:()=>0},
  };
  return rt;
})();
globalThis.__lightNativeRuntime=native;
globalThis.__lightRegisterUnit=name=>{if(!(name in globalThis.__lightCompilerRoots))Object.defineProperty(globalThis.__lightCompilerRoots,name,{configurable:true,get:()=>{throw new Error("unsupported operation: helper not preloaded: "+name);}});return null;};
globalThis.__lightEnterCompilerFrontier=()=>null;
globalThis.__lightLeaveCompilerFrontier=()=>null;
globalThis.__lightCompilerRoots={};
const values=new Map();
globalThis.__lightResult=null;
globalThis.__lightFailure=null;
for(let serial=1;;serial++){
  const command=__rt.need(serial,1);
  const op=__rt.nth(command,0), id=__rt.nth(command,1), input=__rt.nth(command,2);
  globalThis.__lightFailure=null;
  try {
    let result;
    if(op===0) result=(0,eval)(input);
    else if(op===1) result=input;
    else if(op===2){const args=[];for(let i=1;i<__rt.count(input);i++)args.push(values.get(__rt.nth(input,i)));result=values.get(__rt.nth(input,0))(...args);}
    else {values.delete(id);result=null;}
    if(op!==3)values.set(id,result);
    globalThis.__lightResult=result;
  }catch(e){globalThis.__lightFailure=String(e);globalThis.__lightResult=null;}
}
