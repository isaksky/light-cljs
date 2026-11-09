// bbp-authored conformance case (light-cljs-890).
// Derives from: none (light-cljs-766's planner rows for the `set` and
// `deleteProperty` traps, and ECMAScript 10.5.9 [[Set]] and 10.5.10
// [[Delete]]). Scored against QuickJS alone: the Pants engine refuses both
// traps ("unsupported Proxy trap: set"). The planner's rows: a `set` trap
// seeing the key and value of `p.a = 1`, and a `deleteProperty` trap
// forwarding `delete t[k]`. Then the traps' arguments (target, key, value
// and receiver; target and key), ToBoolean of their results, a false
// result a TypeError in strict code and false from Reflect.set and
// Reflect.deleteProperty, the invariants against the target's descriptor
// (a non-writable, non-configurable data property keeps its value and an
// accessor without a setter refuses; a non-configurable property, or any
// property of a non-extensible target, cannot be reported deleted), a proxy
// on an object's prototype chain receiving the object as receiver,
// trapless forwarding to the target (a delete, and a write through the
// target's chain or setter), compound assignments, and the built-ins that
// write or delete through a proxy (Object.assign, Array.prototype.push,
// Reflect.set and Reflect.deleteProperty). Each row is a `row(...)` check;
// the script completes with the number of rows, or minus the first failing
// row's number. A trapless write that defines on the proxy itself goes
// through its [[DefineOwnProperty]], light-cljs-891's, and is not here.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function typeError(f) { try { f(); } catch (e) { return e instanceof TypeError; } return false; }
// The planner's rows (light-cljs-766).
{ let seen; let p = new Proxy({}, { set(t,k,v){ seen=k+v; return true } }); p.a = 1; row(seen === 'a1'); }
{ let p = new Proxy({x:1}, { deleteProperty(t,k){ return delete t[k] } }); row((delete p.x) + ',' + ('x' in p) === 'true,false'); }
// [[Set]] (10.5.9).
{
    let target = {}, receiver = {}, args, handler = { set(t, k, v, r) { args = [this === handler, t === target, k, v, r, arguments.length]; return true; } };
    let p = new Proxy(target, handler);
    row((p.x = 5) === 5 && args[0] === true && args[1] === true && args[2] === 'x' && args[3] === 5 && args[4] === p && args[5] === 4 && !('x' in target));
    row(Reflect.set(p, 'y', 6, receiver) && args[4] === receiver);
    let s = Symbol('s'); p[s] = 1; row(args[2] === s);
    p[12] = 2; row(args[2] === '12');
    let child = Object.create(p); child.z = 3; row(args[4] === child && args[2] === 'z' && !child.hasOwnProperty('z'));
}
row(typeError(() => { new Proxy({}, { set() { return false; } }).x = 1; }));
row(typeError(() => { new Proxy({}, { set() { return 0; } }).x = 1; }));
row(Reflect.set(new Proxy({}, { set() { return ''; } }), 'x', 1) === false);
row(Reflect.set(new Proxy({}, { set() { return {}; } }), 'x', 1) === true);
{ let n = 0; let p = new Proxy({}, { get set() { n++; return () => true; } }); p.x = 1; p.y = 2; row(n === 2); }
row(typeError(() => { new Proxy({}, { set: 1 }).x = 1; }));
{ let e; try { new Proxy({}, { set() { throw 42; } }).x = 1; } catch (x) { e = x; } row(e === 42); }
{
    let t = {}; Object.defineProperty(t, 'x', { value: 1 });
    row(Reflect.set(new Proxy(t, { set() { return true; } }), 'x', 1) === true);
    row(typeError(() => { new Proxy(t, { set() { return true; } }).x = 2; }));
    let a = {}; Object.defineProperty(a, 'x', { get() { return 1; } });
    row(typeError(() => { new Proxy(a, { set() { return true; } }).x = 2; }));
    let w = {}; Object.defineProperty(w, 'x', { value: 1, writable: true });
    row(Reflect.set(new Proxy(w, { set() { return true; } }), 'x', 2) === true);
}
// A proxy on the chain without a trap: the target's chain decides, the
// receiver taking the property.
{
    let o = { __proto__: new Proxy({}, {}) }; o.a = 4;
    row(o.a === 4 && Object.keys(o).join() === 'a');
    let seen, base = new Proxy({ set x(v) { seen = [this, v]; } }, {}), c = { __proto__: base };
    c.x = 7; row(seen[0] === c && seen[1] === 7 && !c.hasOwnProperty('x'));
    let ro = {}; Object.defineProperty(ro, 'x', { value: 1 });
    let d = { __proto__: new Proxy(ro, {}) };
    row(typeError(() => { d.x = 2; }) && !d.hasOwnProperty('x'));
    let got, inner = { __proto__: new Proxy({}, { set(t, k, v, r) { got = [r, k + v]; return true; } }) };
    let outer = { __proto__: new Proxy(inner, {}) };
    outer.q = 1; row(got[0] === outer && got[1] === 'q1' && !outer.hasOwnProperty('q'));
}
// Compound and update assignments read through `get` and write through `set`.
{
    let store = { n: 1 }, p = new Proxy({}, { get(t, k) { return store[k]; }, set(t, k, v) { store[k] = v; return true; } });
    p.n += 2; p.n++; p.n ??= 9; p.m ??= 5;
    row(store.n === 4 && store.m === 5);
}
// Built-ins that write through a proxy: Set(O, P, V, true).
{
    let log = [], p = new Proxy({}, { set(t, k, v) { log.push(k + '=' + v); t[k] = v * 2; return true; } });
    row(Object.assign(p, { a: 1, b: 2 }).a === 2 && log.join() === 'a=1,b=2');
    row(typeError(() => Object.assign(new Proxy({}, { set() { return false; } }), { a: 1 })));
    let arr = [], q = new Proxy(arr, { set(t, k, v, r) { log.push(String(k)); return Reflect.set(t, k, v); } });
    log = []; row(Array.prototype.push.call(q, 7, 8) === 2 && arr.join() === '7,8' && log.join() === '0,1,length');
}
// [[Delete]] (10.5.10).
{
    let target = { x: 1 }, args, handler = { deleteProperty(t, k) { args = [this === handler, t === target, k, arguments.length]; return true; } };
    let p = new Proxy(target, handler);
    row(delete p.x && args.join() === 'true,true,x,2' && target.x === 1);
    row(Reflect.deleteProperty(p, 'y') === true && args[2] === 'y');
    row(Reflect.deleteProperty(new Proxy({}, { deleteProperty() { return 0; } }), 'x') === false);
    row(typeError(() => { delete new Proxy({}, { deleteProperty() { return false; } }).x; }));
    let fixed = {}; Object.defineProperty(fixed, 'x', { value: 1 });
    row(typeError(() => { delete new Proxy(fixed, { deleteProperty() { return true; } }).x; }));
    row(Reflect.deleteProperty(new Proxy(fixed, { deleteProperty() { return false; } }), 'x') === false);
    let sealed = Object.preventExtensions({ x: 1 });
    row(typeError(() => Reflect.deleteProperty(new Proxy(sealed, { deleteProperty() { return true; } }), 'x')));
    row(Reflect.deleteProperty(new Proxy(sealed, { deleteProperty() { return true; } }), 'absent') === true);
    let t2 = { y: 1 }; row(delete new Proxy(t2, {}).y && !('y' in t2));
    row(Reflect.deleteProperty(new Proxy(new Proxy(fixed, {}), {}), 'x') === false);
    let e; try { delete new Proxy({}, { deleteProperty() { throw 42; } }).x; } catch (x) { e = x; } row(e === 42);
}
// A forwarded [[Set]] of an array receiver's `length` converts the value
// twice (the Pants engine runs no ToPrimitive of an object length).
{
    let a = [1, 2], n = 0;
    row(Reflect.set(new Proxy({}, {}), 'length', { valueOf() { n++; return 1; } }, a) && a.length === 1 && n === 2);
}
// A function's `call` read through a proxy on its prototype chain, with the
// function as receiver (the Pants engine refuses changing a function's
// prototype).
{
    const f = () => 0;
    Object.setPrototypeOf(f, new Proxy({}, { get(t, k, r) { return r === f && k === 'call' ? 7 : 0; } }));
    let k = 'call';
    row(f.call === 7 && f[k] === 7);
}
failed === 0 ? rows : -failed;
