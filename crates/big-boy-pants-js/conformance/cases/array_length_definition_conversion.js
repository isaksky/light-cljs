// bbp-authored conformance case (light-cljs-898).
// Derives from: crates/big-boy-pants-js/rust/tests/reflect.rs lines 231-238
// (proxy_definition_keeps_resolved_target_during_coercion, its two array
// rows, verbatim below) and the specification's ArraySetLength (ECMAScript
// 10.4.2.4): an array's [[DefineOwnProperty]] of `length` with an object
// value converts it twice, ToUint32 and then ToNumber, each ToPrimitive
// with hint number able to run JavaScript, before it reads the length; the
// two must agree, else a RangeError. Through Object.defineProperty,
// Reflect.defineProperty and Object.defineProperties, and through a proxy
// forwarding [[DefineOwnProperty]] without a trap (Reflect.defineProperty,
// Reflect.set and an assignment with the proxy as receiver), where the
// conversion runs against the target the proxy already resolved. Each row
// is a `row(...)` check; the script completes with the number of rows, or
// minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
function counted(result) { let o = { n: 0, valueOf() { o.n++; return typeof result === "function" ? result(o.n) : result; } }; return o; }
// Object.defineProperty and Reflect.defineProperty, valueOf called twice.
row(Object.defineProperty([1, 2], 'length', { value: { valueOf() { return 1; } } }).length === 1);
{ let a = [1, 2], v = counted(1); let r = Object.defineProperty(a, 'length', { value: v }); row(r === a && a.length === 1 && a[1] === undefined && v.n === 2); }
{ let a = [1, 2], v = counted(1); row(Reflect.defineProperty(a, 'length', { value: v }) === true && a.length === 1 && v.n === 2); }
{ let a = [1, 2], v = counted(5); row(Reflect.defineProperty(a, 'length', { value: v, writable: true }) && a.length === 5 && !(4 in a) && v.n === 2); }
{ let a = [1, 2], v = counted(1); row(Reflect.defineProperty(a, 'length', { value: v, writable: false }) && a.length === 1 && v.n === 2 && name(() => { 'use strict'; a.length = 0; }) === "TypeError"); }
// toString when valueOf is missing, and @@toPrimitive with hint "number".
{ let a = [1, 2, 3]; row(Reflect.defineProperty(a, 'length', { value: { valueOf: null, toString() { return '2'; } } }) && a.length === 2); }
{ let a = [1, 2, 3], hints = []; let v = { [Symbol.toPrimitive](h) { hints.push(h); return 1; } }; row(Reflect.defineProperty(a, 'length', { value: v }) && a.length === 1 && hints.join() === "number,number"); }
{ let a = [1, 2, 3]; row(Reflect.defineProperty(a, 'length', { value: new Number(2) }) && a.length === 2); }
{ let a = [1, 2, 3]; row(Reflect.defineProperty(a, 'length', { value: new String('1') }) && a.length === 1); }
{ let a = [1, 2, 3]; row(Reflect.defineProperty(a, 'length', { value: [2] }) && a.length === 2); }
// Invalid lengths and conversions that disagree: a RangeError (not false),
// the array unchanged.
for (let x of [1.5, -1, '1.5', 'x', NaN, 4294967296]) {
  let a = [1, 2, 3], v = counted(x);
  row(name(() => Reflect.defineProperty(a, 'length', { value: v })) === "RangeError" && a.length === 3 && v.n === 2);
}
{ let a = [1, 2, 3], v = counted(n => n); row(name(() => Object.defineProperty(a, 'length', { value: v })) === "RangeError" && a.length === 3 && v.n === 2); }
{ let a = [1, 2, 3], v = counted(n => n === 1 ? 2 : '2'); row(Reflect.defineProperty(a, 'length', { value: v }) && a.length === 2 && v.n === 2); }
// A conversion's exception propagates; a symbol or an object result is a TypeError.
{ let a = [1, 2, 3], e = {}; let v = { valueOf() { throw e; } }; let caught; try { Reflect.defineProperty(a, 'length', { value: v }); } catch (x) { caught = x; } row(caught === e && a.length === 3); }
{ let a = [1, 2, 3]; row(name(() => Reflect.defineProperty(a, 'length', { value: { valueOf() { return Symbol.iterator; } } })) === "TypeError" && a.length === 3); }
{ let a = [1, 2, 3]; row(name(() => Reflect.defineProperty(a, 'length', { value: { valueOf() { return {}; }, toString() { return {}; } } })) === "TypeError" && a.length === 3); }
// The conversions run before the length is read: a read-only length takes
// its own value, refuses another, and a conversion that changes the array
// is seen by the definition.
{ let a = Object.freeze([1, 2]), v = counted(2); row(Reflect.defineProperty(a, 'length', { value: v }) && v.n === 2 && a.length === 2); }
{ let a = Object.freeze([1, 2]), v = counted(1); row(Reflect.defineProperty(a, 'length', { value: v }) === false && v.n === 2 && a.length === 2); }
{ let a = Object.freeze([1, 2]), v = counted(1); row(name(() => Object.defineProperty(a, 'length', { value: v })) === "TypeError" && v.n === 2 && a.length === 2); }
{ let a = [1, 2]; let v = { valueOf() { Object.defineProperty(a, 'length', { writable: false }); return 1; } }; row(Reflect.defineProperty(a, 'length', { value: v }) === false && a.length === 2); }
{ let a = [1, 2]; let v = { valueOf() { a.push(3); return 1; } }; row(Reflect.defineProperty(a, 'length', { value: v }) && a.length === 1 && a[0] === 1); }
{ let a = [1, 2, 3]; Object.defineProperty(a, 1, { value: 2, configurable: false }); row(Reflect.defineProperty(a, 'length', { value: counted(0) }) === false && a.length === 2); }
// The key converts first, then the length.
{ let a = [1, 2, 3], log = ''; let k = { toString() { log += 'k'; return 'length'; } }; let v = { valueOf() { log += 'v'; return 1; } }; row(Reflect.defineProperty(a, k, { value: v }) && a.length === 1 && log === "kvv"); }
// A descriptor field's getter runs before the conversions.
{ let a = [1, 2, 3], log = ''; let v = { valueOf() { log += 'v'; return 1; } }; let d = { get value() { log += 'g'; return v; }, get enumerable() { log += 'e'; return false; } }; row(Reflect.defineProperty(a, 'length', d) && a.length === 1 && log === "egvv"); }
// Object.defineProperties reads every descriptor, then defines in key order.
{ let a = [1, 2, 3], log = ''; let v = { valueOf() { log += 'v'; return 1; } }; let r = Object.defineProperties(a, { x: { value: 5, enumerable: true }, length: { value: v }, y: { value: 6 } }); row(r === a && a.length === 1 && a.x === 5 && a.y === 6 && log === "vv"); }
{ let a = [1, 2, 3]; row(name(() => Object.defineProperties(a, { x: { value: 5 }, length: { value: counted(-1) }, y: { value: 6 } })) === "RangeError" && a.x === 5 && !('y' in a) && a.length === 3); }
{ let a = [1, 2, 3], o = {}; let v = { valueOf() { o.seen = a.length; return 2; } }; Object.defineProperties(a, { 0: { value: 'z' }, length: { value: v } }); row(a.length === 2 && a[0] === 'z' && o.seen === 3); }
// Through a proxy without a defineProperty trap: the Pants engine's rows.
{ let a=[1,2],h={},p=new Proxy(a,h),n=0;let v={valueOf(){n++;h.defineProperty=()=>true;return 1;}};row(Reflect.defineProperty(p,'length',{value:v}) && a.length===1 && n===2); }
{ let a=[1,2],h={},p=new Proxy(a,h),n=0;let v={valueOf(){n++;h.defineProperty=()=>true;return 1;}};row(Reflect.set({},'length',v,p) && a.length===1 && n===2); }
// The same with Object.defineProperty, a proxy of a proxy, and failures.
{ let a = [1, 2, 3], p = new Proxy(a, {}), v = counted(2); row(Object.defineProperty(p, 'length', { value: v }) === p && a.length === 2 && v.n === 2); }
{ let a = [1, 2, 3], p = new Proxy(new Proxy(a, {}), {}), v = counted(1); row(Reflect.defineProperty(p, 'length', { value: v }) && a.length === 1 && v.n === 2); }
{ let a = [1, 2, 3], r = Proxy.revocable(a, {}), n = 0; let v = { valueOf() { n++; r.revoke(); return 1; } }; row(Reflect.defineProperty(r.proxy, 'length', { value: v }) && a.length === 1 && n === 2 && name(() => Reflect.defineProperty(r.proxy, 'length', { value: 0 })) === "TypeError"); }
{ let a = Object.freeze([1, 2]), p = new Proxy(a, {}), v = counted(1); row(Reflect.defineProperty(p, 'length', { value: v }) === false && v.n === 2 && a.length === 2); }
{ let a = Object.freeze([1, 2]), p = new Proxy(a, {}), v = counted(1); row(name(() => Object.defineProperty(p, 'length', { value: v })) === "TypeError" && v.n === 2); }
{ let a = [1, 2, 3], p = new Proxy(a, {}), v = counted(1.5); row(name(() => Reflect.defineProperty(p, 'length', { value: v })) === "RangeError" && a.length === 3 && v.n === 2); }
{ let a = [1, 2, 3], p = new Proxy(a, {}); let r = Object.defineProperties(p, { length: { value: counted(1) }, x: { value: 4 } }); row(r === p && a.length === 1 && a.x === 4); }
// A proxy receiver of a trapless [[Set]]: its [[GetOwnProperty]], then its
// [[DefineOwnProperty]] of {value: v}, which converts v on the target.
{ let a = [1, 2, 3], p = new Proxy(a, {}), v = counted(1); p.length = v; row(a.length === 1 && v.n === 2); }
{ let a = [1, 2, 3], p = new Proxy(a, {}), v = counted(1); row(Reflect.set(p, 'length', v) && a.length === 1 && v.n === 2); }
{ let a = [1, 2, 3], p = new Proxy(a, {}), v = counted(1); row(Reflect.set(a, 'length', v, p) && a.length === 1 && v.n === 2); }
{ let a = [1, 2, 3], p = new Proxy(a, {}), v = counted(-1); row(name(() => Reflect.set({}, 'length', v, p)) === "RangeError" && a.length === 3 && v.n === 2); }
// A defineProperty trap receives the unconverted value; the target's own
// definition converts it.
{ let a = [1, 2, 3], seen; let v = counted(1); let p = new Proxy(a, { defineProperty(t, k, d) { seen = d.value; return Reflect.defineProperty(t, k, d); } }); row(Reflect.defineProperty(p, 'length', { value: v }) && seen === v && a.length === 1 && v.n === 2); }
failed === 0 ? rows : -failed;
