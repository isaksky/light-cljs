// bbp-authored conformance case (light-cljs-891).
// Derives from: none (light-cljs-766's planner rows for the `construct`,
// `defineProperty`, `isExtensible`, `preventExtensions`, `setPrototypeOf`
// and `getPrototypeOf` traps, ECMAScript 10.5.1 to 10.5.6 and 10.5.13, and
// the Pants engine's js_source.rs proxy_invariants_reflection_and_forwarding
// and reflect.rs proxy_internal_paths rows QuickJS answers). Scored against
// QuickJS alone: the Pants engine refuses these traps ("unsupported Proxy
// trap: construct", rust/src/vm/proxy.rs) and runs getPrototypeOf only for
// for-in. The traps' arguments, ToBoolean of their results and the
// invariants each checks; a trapless proxy forwarding each method; the
// built-ins and operations that reach them: `new`, super(),
// Object.defineProperty and defineProperties, Reflect.defineProperty, an
// assignment that defines on a proxy receiver, Object.isExtensible,
// preventExtensions and setPrototypeOf and their Reflect counterparts, the
// `__proto__` setter, instanceof and Object.prototype.isPrototypeOf. Each
// row is a `row(...)` check; the script completes with the number of rows,
// or minus the first failing row's number. Object.seal, freeze, isSealed
// and isFrozen of a proxy, and getOwnPropertyDescriptor through one, are
// not here (light-cljs-895 and its follow-up).
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function typeError(f) { try { f(); } catch (e) { return e instanceof TypeError; } return false; }
// The planner's rows (light-cljs-766).
{ let p = new Proxy(function(){}, { construct(t, a){ return {v: 3} } }); row(new p().v === 3); }
{ let log=[]; let p = new Proxy({}, { defineProperty(t,k,d){ log.push(k); return Reflect.defineProperty(t,k,d) } }); p.x = 1; row(log.join() === 'x'); }
{ let p = new Proxy({}, { isExtensible(t) { return true } }); row(Object.isExtensible(p) === true); }
{ let p = new Proxy({}, { preventExtensions(t) { Object.preventExtensions(t); return true } }); row(Object.preventExtensions(p) === p); }
{ let p = new Proxy({}, { setPrototypeOf(t, v) { return Reflect.setPrototypeOf(t, v) } }); Object.setPrototypeOf(p, null); row(Object.getPrototypeOf(p) === null); }
{ let p = new Proxy({}, { getPrototypeOf(t) { return Array.prototype } }); row(p instanceof Array); }
// Every trap that throws: the exception propagates.
{
    let n = 0;
    for (let [trap, op] of [['construct', p => new p()], ['defineProperty', p => Object.defineProperty(p, 'x', { value: 1 })], ['getPrototypeOf', p => Object.getPrototypeOf(p)],
                            ['setPrototypeOf', p => Object.setPrototypeOf(p, null)], ['isExtensible', p => Object.isExtensible(p)], ['preventExtensions', p => Object.preventExtensions(p)],
                            ['defineProperty', p => { p.y = 1; }], ['getPrototypeOf', p => p instanceof Array]]) {
        let h = {}; h[trap] = function () { throw 42; };
        try { op(new Proxy(function () {}, h)); } catch (e) { if (e === 42) n++; }
    }
    row(n === 8);
}
// [[Construct]]: (target, arguments array, newTarget), an object result.
{
    let target = function () {}, seen;
    let handler = { construct(t, a, nt) { seen = [this === handler, t === target, Array.isArray(a), a.join(), nt === P, arguments.length]; return { k: 1 }; } };
    let P = new Proxy(target, handler);
    row(new P(1, 2).k === 1 && seen.join() === 'true,true,true,1,2,true,3');
    row(typeError(() => new (new Proxy(function () {}, { construct() { return 1; } }))()));
    let log = '';
    let Q = new Proxy(function () {}, { construct(t, a, nt) { log += a.length + String(nt === D); return { k: 2 }; } });
    class D extends Q { constructor() { super(1, 2); } }
    row(new D().k === 2 && log === '2true');
    row(Reflect.construct(P, [3], Array).k === 1 && seen[4] === false);
}
// [[DefineOwnProperty]]: (target, key, a descriptor object of the fields
// given, in FromPropertyDescriptor's order), ToBoolean, invariants.
{
    let seen;
    let p = new Proxy({}, { defineProperty(t, k, d) { seen = Object.keys(d).join(); return Reflect.defineProperty(t, k, d); } });
    Object.defineProperty(p, 'x', { get() { return 1; }, enumerable: true }); let a = seen;
    p.y = 2;
    row(a === 'get,enumerable' && seen === 'value,writable,enumerable,configurable' && p.x + p.y === 3);
    let log = '';
    let q = new Proxy({ a: 1 }, { getOwnPropertyDescriptor(t, k) { log += 'g' + k; return Reflect.getOwnPropertyDescriptor(t, k); }, defineProperty(t, k, d) { log += 'd' + k + Object.keys(d).join(''); return Reflect.defineProperty(t, k, d); } });
    q.a = 2; q.b = 3;
    row(log === 'gadavaluegbdbvaluewritableenumerableconfigurable' && q.a + q.b === 5);
    let r = new Proxy({}, { defineProperty(t, k, d) { log += k; return Reflect.defineProperty(t, k, d); } });
    log = ''; Object.defineProperties(r, { a: { value: 1 }, b: { value: 2 } });
    row(log === 'ab' && r.a + r.b === 3);
    // (A strict assignment that this trap refuses is a TypeError on Goose
    // and V8, as [[Set]]'s false answer is; QuickJS ignores it, so no row.)
    let f = new Proxy({}, { defineProperty() { return false; } });
    row(typeError(() => Object.defineProperty(f, 'x', { value: 1 })) && Reflect.defineProperty(f, 'y', { value: 1 }) === false);
    let liar = new Proxy({}, { defineProperty() { return true; } });
    row(typeError(() => Object.defineProperty(liar, 'x', { value: 1, configurable: false })) && Reflect.defineProperty(liar, 'y', { value: 1 }) === true);
    let fixed = {}; Object.preventExtensions(fixed);
    row(typeError(() => Reflect.defineProperty(new Proxy(fixed, { defineProperty() { return true; } }), 'x', { value: 1 })));
    let t = {}; Object.defineProperty(t, 'x', { value: 1, writable: true });
    row(typeError(() => Reflect.defineProperty(new Proxy(t, { defineProperty() { return true; } }), 'x', { value: 1, writable: false, configurable: false })));
    let c = { x: 1 };
    row(typeError(() => Reflect.defineProperty(new Proxy(c, { defineProperty() { return true; } }), 'x', { value: 1, configurable: false })));
    let o = { x: 1 }, fwd = new Proxy(o, {});
    row(Reflect.defineProperty(fwd, 'y', { value: 3, configurable: true }) && o.y === 3 && Object.defineProperty(fwd, 'z', { value: 4 }) === fwd && o.z === 4);
}
// [[IsExtensible]] and [[PreventExtensions]].
{
    let t = { a: 1 }, p = new Proxy(t, {});
    row(Object.isExtensible(p) && Reflect.isExtensible(p) && Object.preventExtensions(p) === p && !Object.isExtensible(t) && !Reflect.isExtensible(p));
    let fixed = {}; Object.preventExtensions(fixed);
    row(typeError(() => Object.isExtensible(new Proxy(fixed, { isExtensible() { return true; } }))));
    row(Object.isExtensible(new Proxy({}, { isExtensible() { return 1; } })) === true);
    row(typeError(() => Object.preventExtensions(new Proxy({}, { preventExtensions() { return true; } }))) && Reflect.preventExtensions(new Proxy({}, { preventExtensions() { return false; } })) === false);
    row(typeError(() => Object.preventExtensions(new Proxy({}, { preventExtensions() { return false; } }))));
    let args; let q = new Proxy(fixed, { isExtensible(target) { args = [target === fixed, arguments.length]; return false; } });
    row(Object.isExtensible(q) === false && args.join() === 'true,1');
}
// [[SetPrototypeOf]] and [[GetPrototypeOf]].
{
    let t = {}, p = new Proxy(t, {});
    Object.setPrototypeOf(p, Array.prototype);
    row(Object.getPrototypeOf(t) === Array.prototype && p.__proto__ === Array.prototype && Reflect.getPrototypeOf(p) === Array.prototype);
    let u = {}; new Proxy(u, {}).__proto__ = Array.prototype;
    row(Object.getPrototypeOf(u) === Array.prototype);
    let fixed = {}; Object.preventExtensions(fixed);
    row(typeError(() => Object.setPrototypeOf(new Proxy(fixed, { setPrototypeOf() { return true; } }), Array.prototype)) && Reflect.setPrototypeOf(new Proxy({}, { setPrototypeOf() { return false; } }), null) === false);
    row(typeError(() => Object.setPrototypeOf(new Proxy({}, { setPrototypeOf() { return false; } }), null)) && typeError(() => { new Proxy({}, { setPrototypeOf() { return false; } }).__proto__ = null; }));
    let args; let q = new Proxy(t, { setPrototypeOf(target, v) { args = [target === t, v === null, arguments.length]; return true; } });
    row(Reflect.setPrototypeOf(q, null) === true && args.join() === 'true,true,2');
    let g = new Proxy({}, { getPrototypeOf(t) { return Array.prototype; } });
    row(Array.prototype.isPrototypeOf(g) && Object.prototype.isPrototypeOf(g) && !Function.prototype.isPrototypeOf(g));
    let o = Object.create(g);
    row(o instanceof Array && Array.prototype.isPrototypeOf(o) && !(o instanceof Function));
    row(typeError(() => Object.getPrototypeOf(new Proxy(fixed, { getPrototypeOf() { return Array.prototype; } }))));
    row(typeError(() => Object.getPrototypeOf(new Proxy({}, { getPrototypeOf() { return 1; } }))));
}
// A proxy target's [[IsExtensible]] and [[GetPrototypeOf]] for a trap's
// invariants run its traps.
{
    let log = '', target = { x: 1 };
    let inner = new Proxy(target, { isExtensible(t) { log += 'e'; return true; } });
    // (V8 and Goose call the inner isExtensible here, as the specification
    // does; QuickJS skips it for a configurable property, so the log is not
    // compared.)
    row(!('x' in new Proxy(inner, { has() { log += 'h'; return false; } })) && log[0] === 'h');
    let e; try { delete new Proxy(new Proxy(target, { isExtensible() { throw 42; } }), { deleteProperty() { return true; } }).x; } catch (x) { e = x; } row(e === 42);
    row(Object.isExtensible(new Proxy(new Proxy({}, {}), { isExtensible() { return true; } })) && Reflect.preventExtensions(new Proxy(new Proxy({}, {}), { preventExtensions(t) { return Reflect.preventExtensions(t); } })));
    let fixed = Object.create(Array.prototype); Object.preventExtensions(fixed);
    let wrapped = new Proxy(fixed, { getPrototypeOf(t) { log += 'p'; return Reflect.getPrototypeOf(t); } });
    log = '';
    row(Object.getPrototypeOf(new Proxy(wrapped, { getPrototypeOf() { return Array.prototype; } })) === Array.prototype && log === 'p');
    row(typeError(() => Object.getPrototypeOf(new Proxy(wrapped, { getPrototypeOf() { return null; } }))) && Reflect.setPrototypeOf(new Proxy(wrapped, { setPrototypeOf() { return true; } }), Array.prototype));
    // (A new property reported on a non-extensible proxy target is a
    // TypeError on V8 and Goose; QuickJS misses that target's state, so no
    // row.)
    row(Reflect.defineProperty(new Proxy(new Proxy({}, {}), { defineProperty() { return true; } }), 'x', { value: 1 }));
}
// Reflect through a trapless proxy (reflect.rs proxy_internal_paths), and
// the trapless rows of js_source.rs.
{
    let o = { x: 1 }, p = new Proxy(o, {});
    row(Reflect.get(p, 'x') === 1 && Reflect.set(p, 'x', 2) && o.x === 2 && Reflect.has(p, 'x') && Reflect.defineProperty(p, 'y', { value: 3, configurable: true }) && Reflect.deleteProperty(p, 'y') && Reflect.getPrototypeOf(p) === Object.prototype && Reflect.isExtensible(p) && Reflect.preventExtensions(p) && !Reflect.isExtensible(p));
    let t = { a: 1 }, q = new Proxy(t, {});
    Object.defineProperty(q, 'b', { value: 2, enumerable: true });
    row(t.b === 2 && Object.getPrototypeOf(q) === Object.prototype && Object.preventExtensions(q) === q && !Object.isExtensible(t));
    let n = 0;
    for (let name of ['set', 'defineProperty', 'deleteProperty', 'getPrototypeOf', 'setPrototypeOf', 'isExtensible', 'preventExtensions']) {
        let h = {}; h[name] = () => true; let r = new Proxy({}, h);
        try { Reflect[name](r, 'x', {}); } catch (e) { if (e instanceof TypeError) n++; }
    }
    row(n === 3);
}
failed === 0 ? rows : -failed;
