// bbp-authored conformance case (light-cljs-890).
// Derives from: crates/big-boy-pants-js/rust/tests/fixtures/js/proxy_get.js lines 5-71
// (run by rust/tests/proxy_get.rs get_traps_follow_ecmascript): the rows
// of its blocks at lines 5-26, 27-41, 42-60 and 61-71 that the Pants engine
// and QuickJS agree on (ECMAScript 10.5.8 [[Get]]: the `get` trap with its
// target, key and receiver, absent traps forwarding with the receiver, the
// trap read through a getter or a proxy handler, its invariants against
// the target's descriptor, built-ins reading through a proxy), with
// rust/tests/js_source.rs line 29421 (a forwarded write running the
// target's setter with the proxy as receiver), compound assignments through
// a proxy on the chain, and light-cljs-766's planner row `p.foo` of a `get`
// trap returning its key. Light-cljs-895 adds object rest from a proxy
// (js_source.rs line 29349) and Object.getOwnPropertyDescriptor through a
// proxy (lines 29346 and 29347, a forwarded and a getter-backed
// descriptor; its TypeError rows at lines 29361-29368 for invalid
// getOwnPropertyDescriptor trap results; and line 29423, the fields read
// in ToPropertyDescriptor's order). Rows left out, each with its reason:
//   'method call' (line 22): `instanceof` walks the proxy's
//     [[GetPrototypeOf]], which fails by name outside for-in until
//     light-cljs-891; the call itself is checked below with `this` the proxy.
//   'callable Proxy trap' (line 37): calling a proxy is its [[Call]],
//     light-cljs-891's.
//   'JSON reads' (line 67): JSON is the core built-ins area's
//     (light-cljs-767).
//   'promise rejection reason' (lines 69-70): a promise resolved with an
//     object reads its `then` only as a data property on Goose
//     ("resolving with a thenable object" fails by name).
//   The block at lines 72-101 (the `apply` trap): [[Call]] is
//     light-cljs-891's, and QuickJS throws at 'classification never reads
//     apply' where the Pants engine passes (a disagreement).
function assert(v, label) {if (!v) throw new Error(label);}
function typeError(f, label) {let e; try {f();} catch (x) {e=x;} assert(e instanceof TypeError, label);}
{
    let p=new Proxy({},{get(t,k){return k}});
    assert(p.foo==='foo', 'planner row');
}
{
    let target={x:1}, receiver={}, handler={get(t,k,r) {
        assert(this===handler && t===target && arguments.length===3, 'trap arguments');
        assert(r===receiver, 'explicit receiver'); return k;
    }}, p=new Proxy(target,handler);
    assert(Reflect.get(p,'x',receiver)==='x', 'Reflect.get');
    let s=Symbol('key'); assert(Reflect.get(p,s,receiver)===s, 'symbol key');
    assert(Reflect.get(p,12,receiver)==='12', 'index key');
    assert(Reflect.get(p,'\ud800',receiver)==='\ud800', 'UTF-16 key');
    let child=Object.create(p); handler.get=(t,k,r)=>r;
    assert(p.x===p && child.x===child, 'inherited receiver');
    assert(Reflect.get(p,'x',null)===null && Reflect.get(p,'x',undefined)===undefined, 'primitive receivers');
    assert(Reflect.get(new Proxy(p,{}),'x',receiver)===receiver, 'nested forwarding receiver');
    let seen, base=new Proxy({get x(){seen=this; return 7;}},{}), obj=Object.create(base);
    assert(obj.x===7 && seen===obj, 'forwarded target getter');
    assert(new Proxy({x:3},{get:null}).x===3 && new Proxy({x:4},{get:undefined}).x===4, 'absent traps');
    let fn=function(){return this;};
    let q=new Proxy({m:fn},{get(t,k){return t[k];}});
    assert(q.m()===q, 'method call this');
    let superBase=new Proxy({get x(){return this.n;}},{});
    let home={__proto__:superBase,m(){return super.x;}};
    assert(home.m.call({n:9})===9, 'super receiver');
}
{
    let log='', target={x:1}, handler={get get(){log+='g';return function(t,k,r){log+='t';return 4;};}};
    let p=new Proxy(target,handler), key={[Symbol.toPrimitive](hint){log+=hint;return 'x';}};
    assert(p[key]===4 && log==='stringgt', 'conversion then trap getter');
    log=''; assert(Reflect.get(p,key)===4 && log==='stringgt', 'Reflect key conversion');
    log=''; typeError(()=>Reflect.get(1,key),'invalid target'); assert(log==='', 'validate target first');
    let inherited=Object.create({get(t,k){return 5;}}); assert(new Proxy({},inherited).x===5,'inherited trap');
    let h=new Proxy({}, {get(t,k){assert(k==='get','handler trap key'); return ()=>6;}});
    assert(new Proxy({},h).x===6,'Proxy handler');
    for (let v of [1,false,'get',{}]) typeError(()=>new Proxy({},{get:v}).x,'noncallable trap');
    for (let h of [{get get(){throw 42;}},{get(){throw 42;}}]) {
        let e; try {new Proxy({},h).x;} catch(x){e=x;} assert(e===42,'trap exception');
    }
}
{
    for (let value of [0,-0,NaN,null,undefined,{},Symbol('fixed')]) {
        let t={};Object.defineProperty(t,'x',{value});
        assert(Object.is(new Proxy(t,{get(){return value;}}).x,value),'SameValue permitted');
        typeError(()=>new Proxy(t,{get(){return value===0 ? (Object.is(value,-0)?0:-0) : 17;}}).x,'frozen data invariant');
    }
    let t={};Object.defineProperty(t,'x',{set(v){}});
    assert(new Proxy(t,{get(){return undefined;}}).x===undefined,'getterless target');
    typeError(()=>new Proxy(t,{get(){return 1;}}).x,'getterless invariant');
    let calls=0, accessor={};Object.defineProperty(accessor,'x',{get(){calls++;throw 99;}});
    assert(new Proxy(accessor,{get(){return 3;}}).x===3 && calls===0,'target getter never invoked');
    assert(new Proxy(Object.preventExtensions({}),{get(){return 4;}}).x===4,'nonextensible target allows virtual read');
    let mutable={x:1};
    typeError(()=>new Proxy(mutable,{get(t){Object.defineProperty(t,'x',{value:2,writable:false,configurable:false});return 1;}}).x,'invariants after mutation');
    let log='', inner=new Proxy({x:1},{getOwnPropertyDescriptor(t,k){log+='d';return Object.getOwnPropertyDescriptor(t,k);},isExtensible(){throw 99;}});
    assert(new Proxy(inner,{get(){log+='g';return 9;}}).x===9 && log==='gd','descriptor only invariant');
    let e;try{new Proxy(new Proxy({},{getOwnPropertyDescriptor(){throw 42;}}),{get(){return 1;}}).x;}catch(x){e=x;}
    assert(e===42,'target descriptor exception');
}
{
    let log='', p=new Proxy([1,2],{get(t,k,r){log+=String(k)+',';return Reflect.get(t,k,r);}});
    assert(Array.prototype.map.call(p,x=>x*2).join(',')==='2,4' && log==='length,constructor,0,1,','array internal reads');
    let d=new Proxy({value:7,configurable:true},{get(t,k,r){return Reflect.get(t,k,r);}});
    assert(Object.defineProperty({},'x',d).x===7,'descriptor internal reads');
    assert(new Error('x',new Proxy({cause:42},{get(t,k){return t[k];}})).cause===42,'Error cause read');
    let a=[...new Proxy([1,2],{get(t,k,r){return Reflect.get(t,k,r);}})];assert(a.join(',')==='1,2','iterator reads');
}
{
    let p,seen; p=new Proxy({set x(v){seen=this===p && v===3;}},{getOwnPropertyDescriptor(){throw 99;},defineProperty(){throw 98;}}); p.x=3;
    assert(seen, 'forwarded setter receiver');
    let p2=new Proxy({x:1},{}), compound=new Proxy({get n(){return 2;},set n(v){this.m=v;}},{}), o={__proto__:compound};
    o.n+=1; o.n++;
    assert(p2.x===1 && o.m===3 && !('m' in compound), 'compound assignment through an inherited proxy');
}
{
    // A built-in trap that converts its first argument, the target.
    assert(new Proxy({valueOf(){return 7;}},{get:Number}).x===7, 'Number as the get trap');
    assert(new Proxy({toString(){return 's';}},{get:String}).x==='s', 'String as the get trap');
    let e; try {new Proxy({valueOf(){throw 42;}},{get:Number}).x;} catch(x){e=x;}
    assert(e===42, 'a throwing conversion in a built-in trap');
}
{
    // js_source.rs proxy_invariants_reflection_and_forwarding and
    // proxy_descriptor_observation_order_and_receivers (light-cljs-895).
    let {a,...rest}=new Proxy({a:1,b:2},{});
    assert(a===1 && rest.b===2 && Object.keys(rest).join(',')==='b', 'object rest from a proxy');
    let t={a:1}, p=new Proxy(t,{}); Object.defineProperty(p,'b',{value:2,enumerable:true});
    assert(Object.getOwnPropertyDescriptor(p,'b').value===2 && Object.getPrototypeOf(p)===Object.prototype && Object.preventExtensions(p)===p && !Object.isExtensible(t), 'forwarded descriptor');
    let h={get ownKeys(){return function(t){return {length:1,get 0(){return 'a';}};}}, getOwnPropertyDescriptor(t,k){return {get enumerable(){return true;},get configurable(){return true;},get value(){return 3;}};}};
    let q=new Proxy({},h);
    assert(Object.keys(q).join(',')==='a' && Object.getOwnPropertyDescriptor(q,'x').value===3, 'getter-backed descriptor fields');
    let log='', r=new Proxy({a:1},{getOwnPropertyDescriptor(){return {get enumerable(){log+='e';return true;},get configurable(){log+='c';return true;},get value(){log+='v';return 2;},get writable(){log+='w';return true;}};}});
    let d=Object.getOwnPropertyDescriptor(r,'a');
    assert(log==='ecvw' && d.value===2 && d.writable && d.enumerable && d.configurable, 'descriptor field order');
    typeError(()=>Object.getOwnPropertyDescriptor(new Proxy({}, {getOwnPropertyDescriptor(){return 1;}}),'a'), 'primitive trap result');
    typeError(()=>Object.getOwnPropertyDescriptor(new Proxy({}, {getOwnPropertyDescriptor(){return {};}}),'a'), 'non-configurable result for a missing property');
    typeError(()=>{let t={a:1}; Object.preventExtensions(t); Object.getOwnPropertyDescriptor(new Proxy(t,{getOwnPropertyDescriptor(){return undefined;}}),'a');}, 'missing property of a non-extensible target');
    typeError(()=>{let t={}; Object.preventExtensions(t); Object.getOwnPropertyDescriptor(new Proxy(t,{getOwnPropertyDescriptor(){return {configurable:true};}}),'a');}, 'new property of a non-extensible target');
    typeError(()=>{let t={}; Object.defineProperty(t,'a',{value:1}); Object.getOwnPropertyDescriptor(new Proxy(t,{getOwnPropertyDescriptor(){return {value:2};}}),'a');}, 'incompatible value');
    typeError(()=>{let t={}; Object.defineProperty(t,'a',{value:1,writable:true}); Object.getOwnPropertyDescriptor(new Proxy(t,{getOwnPropertyDescriptor(){return {value:1,writable:false};}}),'a');}, 'non-writable over a writable target property');
    typeError(()=>Object.getOwnPropertyDescriptor(new Proxy({}, {getOwnPropertyDescriptor(){return {get:1,configurable:true};}}),'a'), 'non-callable getter');
    typeError(()=>Object.getOwnPropertyDescriptor(new Proxy({}, {getOwnPropertyDescriptor(){return {get(){},value:1,configurable:true};}}),'a'), 'accessor and data fields');
}
true;
