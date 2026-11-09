// bbp-authored conformance case (light-cljs-891).
// Derives from: none (light-cljs-766's planner row for the `apply` trap,
// ECMAScript 10.5.12 [[Call]] and 10.5.13 [[Construct]] without a trap,
// and the Pants engine's js_source.rs
// proxy_invariants_reflection_and_forwarding,
// proxy_forwarded_keys_sloppy_errors_and_callable_tags and
// builtin_subclasses.rs transparent_proxy_new_target_preserves_subclass_identity).
// The rows both oracles agree on: the `apply` trap with its arguments
// (target, this value, an array of the arguments), a trapless proxy calling
// or constructing its target, a proxy reached by method calls, call, apply,
// Reflect.apply and construct, spread calls, `new`, class heritage and as a
// map callback or a trap, a proxy as newTarget, IsCallable and
// IsConstructor of a proxy, and Object.prototype.toString's tags. The
// `construct` trap is the Pants engine's gap (proxy_remaining_traps). Each
// row is a `row(...)` check; the script completes with the number of rows,
// or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function typeError(f) { try { f(); } catch (e) { return e instanceof TypeError; } return false; }
// The planner's row (light-cljs-766).
{ let p = new Proxy(function(){ return 1 }, { apply(t, th, a){ return 2 } }); row(p() === 2); }
// The trap's arguments: the handler as this, the target, the this value
// and a new array of the arguments.
{
    let target = function () { return 0; }, seen;
    let handler = { apply(t, th, a) { seen = [this === handler, t === target, th, Array.isArray(a), a.length, a[0], a[1], arguments.length]; a.push(9); return 5; } };
    let p = new Proxy(target, handler), o = { p };
    row(o.p(1, 2) === 5 && seen.join() === 'true,true,[object Object],true,2,1,2,3');
    row(p(3) === 5 && seen[2] === undefined && seen[4] === 1);
    row(p.call(7, 'a') === 5 && seen[2] === 7 && seen[5] === 'a');
    row(p.apply(o, [4, 5, 6]) === 5 && seen[2] === o && seen[4] === 3);
    row(Reflect.apply(p, 8, []) === 5 && seen[2] === 8 && seen[4] === 0);
    row(p(...[1, 2, 3]) === 5 && seen[4] === 3);
}
// A throwing trap, a trap that forwards, and a trap that is itself a proxy.
{
    let e; try { new Proxy(function () {}, { apply() { throw 42; } })(); } catch (x) { e = x; } row(e === 42);
    let p = new Proxy(function (a, b) { return this.n + a + b; }, { apply(t, th, a) { return Reflect.apply(t, th, a) * 10; } });
    row(p.call({ n: 1 }, 2, 3) === 60);
    let q = new Proxy(function () { return 1; }, { apply: new Proxy(function (t, th, a) { return a[0] * 2; }, {}) });
    row(q(21) === 42);
}
// Without a trap: the target's [[Call]] and [[Construct]].
{
    let f = new Proxy(function (x) { return this.n + x; }, {}), o = { n: 2, f: f };
    row(o.f(3) === 5 && typeof f === 'function' && new (new Proxy(function () { this.a = 7; }, {}))().a === 7);
    let g = new Proxy(function (a, b) { return this.x + a + b; }, {});
    row(g.call({ x: 1 }, 2, 3) + g.apply({ x: 10 }, [20, 30]) + Reflect.apply(g, { x: 100 }, [1, 2]) === 169);
    let h = new Proxy(function (...a) { return a.length; }, {});
    row(h(...[1, 2, 3]) === 3 && new (new Proxy(Array, {}))(...[1, 2]).length === 2);
    let nested = new Proxy(new Proxy(function () { return 'in'; }, {}), {});
    row(nested() === 'in');
    let C = new Proxy(function (a) { this.a = a; }, {}), c = Reflect.construct(C, [4]);
    row(c.a === 4 && Object.getPrototypeOf(c) === C.prototype);
}
// Class heritage and newTarget through a trapless proxy.
{
    class B { constructor() { this.b = 1; } }
    let P = new Proxy(B, {});
    class D extends P { constructor() { super(); this.d = 2; } }
    let d = new D();
    row(d.b + d.d === 3 && d instanceof B && d instanceof D);
    let C = new Proxy(Object, {}), o = new C(7);
    row(Object.getPrototypeOf(o) === Object.prototype && Object.keys(o).length === 0);
    class A extends Array {}
    let E = new Proxy(A, {}), a = new E(1, 2);
    row(a instanceof A && a.length === 2 && Object.getPrototypeOf(a) === A.prototype);
    let ok; try { class X extends Proxy {} } catch (e) { ok = e instanceof TypeError; } row(ok && Proxy.prototype === undefined);
}
// IsCallable and IsConstructor follow the target; a class still needs `new`.
{
    row(typeError(() => new Proxy(class {}, {})()));
    let arrow = new Proxy(() => 1, {});
    row(typeError(() => new arrow()) && arrow() === 1);
    row(typeError(() => new Proxy({}, {})()) && typeof new Proxy({}, {}) === 'object');
    row([1, 2].map(new Proxy(function (x) { return x * 3; }, {})).join() === '3,6');
}
// Object.prototype.toString: a callable proxy is a Function, a proxy of an
// array an Array.
{
    let h = { apply() { throw 42; } }, p = new Proxy(function () {}, h);
    row(Object.prototype.toString.call(p) === '[object Function]' && Object.prototype.toString.call(new Proxy((function () {}).bind(null), h)) === '[object Function]' && Object.prototype.toString.call(new Proxy([], {})) === '[object Array]');
}
failed === 0 ? rows : -failed;
