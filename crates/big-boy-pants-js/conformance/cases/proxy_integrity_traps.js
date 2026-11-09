// bbp-authored conformance case (light-cljs-897).
// Derives from: none (ECMAScript 7.3.15 SetIntegrityLevel, 7.3.16
// TestIntegrityLevel, 20.1.2.6 Object.freeze and 20.1.2.22 Object.seal
// through a proxy's defineProperty, preventExtensions and isExtensible
// traps, and QuickJS's js_object_seal and js_object_isSealed). Scored
// against QuickJS alone: the Pants engine refuses these traps
// ("unsupported Proxy trap: defineProperty", rust/src/vm/proxy.rs) and has
// no Proxy.revocable. The descriptors seal and freeze define with, false
// from preventExtensions or defineProperty a TypeError, isExtensible's
// answer ending the tests, a revoked proxy, and a proxy target's
// isExtensible trap in the ownKeys invariants. Left out where QuickJS
// departs from the specification and V8: the tests read IsExtensible
// before the keys (QuickJS reads it last), freeze skips a key whose
// descriptor is gone (QuickJS defines it anyway and throws), and freeze
// defines {writable: false} on a data property that is already read-only
// (QuickJS leaves `writable` out); those rows hold on Goose as the
// specification says (corpus/serial/proxy-integrity-wait.js). Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function typeError(f) { try { f(); } catch (e) { return e instanceof TypeError; } return false; }
// The descriptors: freeze clears `writable` on a writable data property and
// only `configurable` on an accessor; seal only `configurable`.
{ let log=[], p=new Proxy({a:1,get b(){return 2}},{defineProperty(t,k,d){log.push(k+':'+Object.keys(d).join('/')+':'+d.configurable+d.writable); return Reflect.defineProperty(t,k,d);}}); row(Object.freeze(p)===p && log.join()==='a:writable/configurable:falsefalse,b:configurable:falseundefined'); }
{ let log=[], s=Symbol('s'), p=new Proxy({a:1,b:2,[s]:3},{defineProperty(t,k,d){log.push(String(k)+':'+Object.keys(d).join('/')); return Reflect.defineProperty(t,k,d);}}); row(Object.seal(p)===p && log.join()==='a:configurable,b:configurable,Symbol(s):configurable'); }
// A false answer is a TypeError; the trap's exception propagates.
{ let p=new Proxy({},{preventExtensions(){return false;}}); row(typeError(()=>Object.seal(p)) && typeError(()=>Object.freeze(p))); }
{ let t={a:1}, p=new Proxy(t,{defineProperty(){return false;}}); row(typeError(()=>Object.seal(p)) && !Object.isExtensible(t) && !Object.isSealed(t)); }
{ let ok=false, p=new Proxy({a:1},{defineProperty(){throw 9;}}); try{Object.freeze(p);}catch(e){ok=e===9;} row(ok); }
{ let ok=false, p=new Proxy({a:1},{preventExtensions(){throw 9;}}); try{Object.seal(p);}catch(e){ok=e===9;} row(ok); }
// The trap's invariants: a true preventExtensions with an extensible
// target, a defineProperty that reports a key non-configurable while the
// target's stays configurable.
{ let p=new Proxy({},{preventExtensions(){return true;}}); row(typeError(()=>Object.freeze(p))); }
{ let t={a:1}, p=new Proxy(t,{defineProperty(){return true;}}); row(typeError(()=>Object.seal(p)) && Object.getOwnPropertyDescriptor(t,'a').configurable); }
// isExtensible answers the tests: true is false at once.
{ let log='', p=new Proxy({}, {isExtensible(t){log+='e'; return true;}}); row(Object.isSealed(p)===false && Object.isFrozen(p)===false && log==='ee'); }
{ let ok=false, p=new Proxy({}, {isExtensible(){throw 9;}}); try{Object.isFrozen(p);}catch(e){ok=e===9;} row(ok); }
// A revoked proxy, also as a target.
{ let r=Proxy.revocable({},{}); r.revoke(); row(typeError(()=>Object.seal(r.proxy)) && typeError(()=>Object.freeze(r.proxy)) && typeError(()=>Object.isSealed(r.proxy)) && typeError(()=>Object.isFrozen(r.proxy))); }
{ let r=Proxy.revocable({a:1},{}), p=new Proxy(r.proxy,{ownKeys(){r.revoke();return ['a'];}}); row(typeError(()=>Reflect.ownKeys(p))); }
// A proxy target's isExtensible trap in the ownKeys invariants: its answer
// must be its target's, and decides whether the trap's keys may differ.
{ let log='', inner=new Proxy(Object.preventExtensions({a:1}),{isExtensible(t){log+='e';return false;}}); row(Reflect.ownKeys(new Proxy(inner,{ownKeys(){return ['a'];}})).join()==='a' && typeError(()=>Reflect.ownKeys(new Proxy(inner,{ownKeys(){return ['a','b'];}}))) && log==='ee'); }
{ let inner=new Proxy({a:1},{isExtensible(){return false;}}); row(typeError(()=>Reflect.ownKeys(new Proxy(inner,{ownKeys(){return ['a'];}})))); }
// Seal and freeze through a proxy whose traps forward to another proxy.
{ let log='', t={a:1}, inner=new Proxy(t,{defineProperty(t,k,d){log+='i'+k;return Reflect.defineProperty(t,k,d);}}), p=new Proxy(inner,{defineProperty(t,k,d){log+='o'+k;return Reflect.defineProperty(t,k,d);}}); row(Object.freeze(p)===p && Object.isFrozen(t) && log==='oaia'); }
failed === 0 ? rows : -failed;
