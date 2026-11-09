// bbp-authored conformance case (light-cljs-891).
// Derives from: none (light-cljs-766's planner row for Proxy.revocable,
// ECMAScript 28.2.2.1 Proxy.revocable and 28.2.2.1.1 Proxy Revocation
// Functions, and QuickJS's js_proxy_revocable and js_proxy_revoke). Scored
// against QuickJS alone: the Pants engine has no Proxy.revocable. The
// result object's `proxy` and `revoke` data properties, the revoke
// function's `length` and `name`, a working proxy until it is revoked,
// then every internal method a TypeError (a read, `in`, keys, a write, a
// call, a prototype read, Array.isArray, Object.prototype.toString), with
// `typeof` unchanged and a second revoke doing nothing. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function typeError(f) { try { f(); } catch (e) { return e instanceof TypeError; } return false; }
// The planner's row (light-cljs-766).
{ let r = Proxy.revocable({}, {}); r.revoke(); row(typeof r.proxy === 'object'); }
// The result and the revoke function.
{
    let r = Proxy.revocable({}, {});
    let d = Object.getOwnPropertyDescriptor(r, 'proxy');
    row(Object.getOwnPropertyNames(r).join() === 'proxy,revoke' && d.writable && d.enumerable && d.configurable && Object.getPrototypeOf(r) === Object.prototype);
    row(Proxy.revocable.length === 2 && Proxy.revocable.name === 'revocable' && typeof r.revoke === 'function' && r.revoke.length === 0 && r.revoke.name === '');
    row(typeError(() => Proxy.revocable(1, {})) && typeError(() => Proxy.revocable({}, null)));
}
// Before and after revocation.
{
    let r = Proxy.revocable({ a: 1 }, { get(t, k) { return k === 'b' ? 2 : t[k]; } });
    row(r.proxy.a === 1 && r.proxy.b === 2 && 'a' in r.proxy);
    row(r.revoke() === undefined && r.revoke() === undefined);
    let p = r.proxy;
    row(typeError(() => p.a) && typeError(() => 'a' in p) && typeError(() => Object.keys(p)) && typeError(() => { p.c = 1; }) && typeError(() => delete p.a));
    row(typeError(() => Object.getPrototypeOf(p)) && typeError(() => Array.isArray(p)) && typeError(() => Object.prototype.toString.call(p)) && typeError(() => Object.isExtensible(p)));
    row(typeError(() => { for (let k in p) {} }) && typeError(() => Object.defineProperty(p, 'x', { value: 1 })) && typeError(() => ({}) instanceof p));
    row(typeof p === 'object');
}
// A revoked function proxy keeps `typeof` "function"; calls and `new` fail.
{
    let r = Proxy.revocable(function () { return 5; }, {});
    let a = r.proxy();
    r.revoke();
    row(a === 5 && typeError(() => r.proxy()) && typeError(() => new r.proxy()) && typeof r.proxy === 'function');
    let arr = Proxy.revocable([], {});
    row(Array.isArray(arr.proxy));
    arr.revoke();
    row(typeError(() => Array.isArray(arr.proxy)));
}
// The revoke function called through call, Reflect.apply and a callback.
{
    let a = Proxy.revocable({}, {}), b = Proxy.revocable({}, {}), c = Proxy.revocable({}, {});
    a.revoke.call(null); Reflect.apply(b.revoke, undefined, []); [1].forEach(c.revoke);
    row(typeError(() => a.proxy.x) && typeError(() => b.proxy.x) && typeError(() => c.proxy.x));
}
// Revoked during an operation: a trap getter's revocation keeps the
// handler the operation already read; toString's tag is decided before
// the @@toStringTag read that revokes; a revoke function as a
// @@toPrimitive method.
{
    let r; const h = { get apply() { r.revoke(); return function () { 'use strict'; return this === h; }; } };
    r = Proxy.revocable(function () {}, h);
    row(r.proxy() === true && typeError(() => r.proxy()));
    let s = Proxy.revocable({}, { get() { s.revoke(); return undefined; } });
    row(Object.prototype.toString.call(s.proxy) === '[object Object]' && typeError(() => Object.prototype.toString.call(s.proxy)));
    let t = Proxy.revocable({}, {});
    let n = Number({ [Symbol.toPrimitive]: t.revoke });
    row(n !== n && typeError(() => t.proxy.x));
}
// A proxy whose target or handler is a revoked proxy is made; using it
// reaches the revoked one.
{
    let r = Proxy.revocable({}, {}); r.revoke();
    let p = new Proxy(r.proxy, {});
    row(typeof p === 'object' && typeError(() => p.x));
    let q = new Proxy({}, r.proxy);
    row(typeError(() => q.x));
}
failed === 0 ? rows : -failed;
