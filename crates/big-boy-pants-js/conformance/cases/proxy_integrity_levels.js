// bbp-authored conformance case (light-cljs-897).
// Derives from: crates/big-boy-pants-js/rust/tests/object_statics.rs lines 58-60
// (sealed_objects_keep_writable_values_and_observe_proxy_descriptors, its
// proxy rows), rust/tests/js_source.rs line 29424
// (proxy_descriptor_observation_order_and_receivers, its Object.freeze row)
// and rust/tests/fixtures/js/proxy_has.js lines 176-184 and 188-192 (an
// inner proxy's isExtensible, ownKeys and getOwnPropertyDescriptor during
// the outer ownKeys trap's invariants, through Reflect.ownKeys only: the
// getOwnPropertyDescriptor rows are light-cljs-895's), with ECMAScript
// 7.3.15 SetIntegrityLevel, 7.3.16
// TestIntegrityLevel and 10.5.11 [[OwnPropertyKeys]]. The rows both oracles
// agree on: Object.seal, freeze, isSealed and isFrozen of a proxy through
// its ownKeys and getOwnPropertyDescriptor traps or without traps, nested
// proxies, symbol keys, an array target, the invariants those traps meet,
// and the built-ins reached through forEach, call and Reflect.apply; a
// proxy target's isExtensible, ownKeys and getOwnPropertyDescriptor in the
// ownKeys trap's invariants. The defineProperty, preventExtensions and
// isExtensible traps and a revoked proxy are the Pants engine's gap
// (proxy_integrity_traps). Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function typeError(f) { try { f(); } catch (e) { return e instanceof TypeError; } return false; }
// object_statics.rs: seal reads keys (k) only; isSealed reads keys and each
// descriptor (kd); a throwing ownKeys leaves the target non-extensible; a
// throwing getOwnPropertyDescriptor does not stop seal, which never reads one.
{ let log='', t={a:1}, p=new Proxy(t,{ownKeys(){log+='k';return ['a'];},getOwnPropertyDescriptor(t,k){log+='d';return Object.getOwnPropertyDescriptor(t,k);}}); row(Object.seal(p)===p && Object.isSealed(p) && Object.isSealed(t) && log==='kkd'); }
{ let ok=false, o={a:1}, p=new Proxy(o,{ownKeys(){throw 17;}}); try{Object.seal(p);}catch(e){ok=e===17;} row(ok && !Object.isExtensible(o) && !Object.isSealed(o)); }
{ let o={a:1}, p=new Proxy(o,{getOwnPropertyDescriptor(){throw 17;}}); row(Object.seal(p)===p && Object.isSealed(o)); }
// js_source.rs: freeze and isFrozen read keys and descriptors twice each.
{ let n=0,t={x:1}, p=new Proxy(t,{ownKeys(){n++;return ['x'];},getOwnPropertyDescriptor(t,k){n++;return Object.getOwnPropertyDescriptor(t,k);}}); row(Object.freeze(p)===p && Object.isFrozen(p) && n===4 && Object.isFrozen(t)); }
// Trapless proxies forward every step.
{ let t=Object.freeze({a:1,b:[]}), p=new Proxy(t,{}); row(Object.isFrozen(p) && Object.isSealed(p)); }
row(Object.isSealed(new Proxy({},{}))===false && Object.isFrozen(new Proxy(Object.preventExtensions({}),{})) && Object.isSealed(new Proxy(Object.preventExtensions({}),{})));
row(Object.isFrozen(new Proxy(Object.preventExtensions({a:1}),{}))===false && Object.isSealed(new Proxy(Object.preventExtensions({a:1}),{}))===false);
{ let t=Object.seal({a:1}), p=new Proxy(t,{}); row(Object.isSealed(p) && !Object.isFrozen(p)); }
{ let t={a:1, get g(){return 1}}, p=new Proxy(t,{}); row(Object.freeze(p)===p && Object.isFrozen(t) && Object.getOwnPropertyDescriptor(t,'a').writable===false && typeof Object.getOwnPropertyDescriptor(t,'g').get==='function'); }
{ let a=[1,2], p=new Proxy(a,{}); row(Object.freeze(p)===p && Object.isFrozen(a) && a.length===2 && Object.isFrozen(p)); }
{ let s=Symbol(), t={[s]:1}; Object.seal(new Proxy(t,{})); row(!Object.getOwnPropertyDescriptor(t,s).configurable && Object.isSealed(t)); }
{ let t={a:1}, p=new Proxy(new Proxy(t,{}),{}); row(Object.freeze(p)===p && Object.isFrozen(t) && Object.isFrozen(p)); }
// The traps' invariants: keys that a now non-extensible target lacks; a
// getOwnPropertyDescriptor result incompatible with a frozen target.
{ let t={a:1}, p=new Proxy(t,{ownKeys(){return ['a','zz'];}}); row(typeError(()=>Object.freeze(p)) && !Object.isExtensible(t) && !Object.isFrozen(t)); }
{ let t=Object.freeze({a:1}), p=new Proxy(t,{getOwnPropertyDescriptor(){return {value:1,configurable:true,writable:true,enumerable:true};}}); row(typeError(()=>Object.isFrozen(p))); }
// Through forEach, call and Reflect.apply.
{ let t={a:1}; [new Proxy(t,{})].forEach(Object.freeze); row(Object.isFrozen(t)); }
{ let t={a:1}; Object.freeze.call(null,new Proxy(t,{})); row(Reflect.apply(Object.isSealed,null,[new Proxy(t,{})]) && Object.isFrozen(t)); }
// A proxy target in the ownKeys trap's invariants (proxy_has.js): its
// isExtensible (a getter, then the call), its ownKeys, then each of its
// keys' getOwnPropertyDescriptor, before the trap's keys are accepted.
for (let sealed of [false,true]) {
    let target={x:1}; if(sealed)Object.preventExtensions(target);
    let log='', inner=new Proxy(target,{
        get isExtensible(){log+='e';return function(t){log+='E';return !sealed;};},
        ownKeys(t){log+='k';return ['x'];},
        getOwnPropertyDescriptor(t,k){log+='d';return Object.getOwnPropertyDescriptor(t,k);}
    });
    let p=new Proxy(inner,{ownKeys(){log+='K';return ['x'];},getOwnPropertyDescriptor(t,k){log+='D';return Object.getOwnPropertyDescriptor(target,k);}});
    row(Reflect.ownKeys(p).join(',')==='x' && log==='KeEkd');
}
for (let h of [{isExtensible(){return false;}},{get isExtensible(){throw 42;}},{isExtensible(){throw 42;}}]) {
    let inner=new Proxy({x:1},h), p=new Proxy(inner,{ownKeys(){return ['x'];},getOwnPropertyDescriptor(){return {value:1,configurable:true};}});
    let e; try{Reflect.ownKeys(p);}catch(x){e=x;}
    row(e===42 || e instanceof TypeError);
}
// The checks against a proxy target's keys: a non-configurable key
// omitted, a key added to or omitted from a non-extensible target; an
// extensible one accepts extra keys; the inner getOwnPropertyDescriptor
// decides which keys are non-configurable; symbol keys; for-in.
{ let t={}; Object.defineProperty(t,'x',{value:1}); row(typeError(()=>Reflect.ownKeys(new Proxy(new Proxy(t,{}),{ownKeys(){return [];}})))); }
{ let t=Object.preventExtensions({a:1}); row(typeError(()=>Reflect.ownKeys(new Proxy(new Proxy(t,{}),{ownKeys(){return ['a','b'];}}))) && typeError(()=>Reflect.ownKeys(new Proxy(new Proxy(t,{}),{ownKeys(){return [];}})))); }
{ let t={a:1}; Object.defineProperty(t,'b',{value:2}); row(Reflect.ownKeys(new Proxy(new Proxy(t,{}),{ownKeys(){return ['b','a','c'];}})).join()==='b,a,c'); }
{ let log='', t={}; Object.defineProperty(t,'x',{value:1}); let inner=new Proxy(t,{getOwnPropertyDescriptor(t,k){log+='d'+k;return Object.getOwnPropertyDescriptor(t,k);},ownKeys(t){log+='k';return Reflect.ownKeys(t);}}); row(typeError(()=>Reflect.ownKeys(new Proxy(inner,{ownKeys(){log+='K';return [];}}))) && log==='Kkdx'); }
{ let s=Symbol('s'), t={[s]:1,a:2}; Object.defineProperty(t,'b',{value:3}); let k=Reflect.ownKeys(new Proxy(new Proxy(t,{}),{ownKeys(){return [s,'b','a'];}})); row(k.length===3 && k[0]===s && k[1]==='b' && k[2]==='a'); }
{ let t={a:1}, p=new Proxy(new Proxy(t,{}),{ownKeys(){return ['a'];}}), s=''; for(let k in p) s+=k; row(s==='a'); }
// A sealed proxy of a proxy: every step passes through both.
{ let log='', t={a:1}, inner=new Proxy(t,{ownKeys(t){log+='i';return Reflect.ownKeys(t);}}), p=new Proxy(inner,{ownKeys(){log+='o';return ['a'];}}); row(Object.seal(p)===p && Object.isSealed(t) && log==='oi'); }
failed === 0 ? rows : -failed;
