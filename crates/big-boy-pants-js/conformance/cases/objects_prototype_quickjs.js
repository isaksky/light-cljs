// bbp-authored conformance case (light-cljs-883).
// Derives from: none (ECMAScript 20.1.2.2 Object.create with its properties
// argument, 20.1.3.3 Object.prototype.isPrototypeOf, and 10.1.2.1
// OrdinarySetPrototypeOf on functions, Error objects and primitive
// wrappers; QuickJS's js_object_create, js_object_isPrototypeOf and
// JS_SetPrototypeInternal (~/src/quickjs/quickjs.c); and the planner's rows
// (light-cljs-766)), and a proxy's getPrototypeOf trap (light-cljs-845's
// frame, ECMAScript 10.5.1). The Pants engine lacks isPrototypeOf and the
// getPrototypeOf trap, refuses Object.create's properties argument and a
// function's new prototype, so the case is scored against QuickJS alone
// (areas.edn :pants-gap). A proxy on isPrototypeOf's walk fails by name
// until light-cljs-891, so no row walks one. Errors
// are compared by name. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-766).
{ let p = {}; let o = {__proto__: p}; row(p.isPrototypeOf(o)); }
row(Object.create({}, {a: {value: 1, enumerable: true}}).a === 1);
// isPrototypeOf: the whole chain, never the object itself; a primitive is
// on no chain, even with an undefined this.
{ let a = {}, b = Object.create(a), c = Object.create(b); row(a.isPrototypeOf(c) && b.isPrototypeOf(c) && !c.isPrototypeOf(a) && !c.isPrototypeOf(c) && Object.prototype.isPrototypeOf(c)); }
row(Array.prototype.isPrototypeOf([]) && Object.prototype.isPrototypeOf([]) && Function.prototype.isPrototypeOf(Object) && !Object.prototype.isPrototypeOf(Object.create(null)));
row(Object.prototype.isPrototypeOf.call(undefined, 1) === false && Object.prototype.isPrototypeOf.call(null, 'a') === false && !Object.prototype.isPrototypeOf(1));
row(name(() => Object.prototype.isPrototypeOf.call(undefined, {})) === "TypeError" && name(() => Object.prototype.isPrototypeOf.call(null, {})) === "TypeError");
row(Object.prototype.isPrototypeOf.call(1, {}) === false && Number.prototype.isPrototypeOf(1) === false);
row(Object.prototype.isPrototypeOf.length === 1 && Object.prototype.isPrototypeOf.name === 'isPrototypeOf');
{ let p = {}; let o = {}; Object.setPrototypeOf(o, p); row(p.isPrototypeOf(o)); Object.setPrototypeOf(o, null); row(!p.isPrototypeOf(o)); }
// Object.create's properties: Object.defineProperties on the new object,
// every descriptor read before the first definition.
{ let o = Object.create(null, {a: {value: 1, writable: true}, b: {get() { return 2; }, enumerable: true}}); let d = Object.getOwnPropertyDescriptor(o, 'a'); row(Object.getPrototypeOf(o) === null && o.a === 1 && o.b === 2 && d.writable && !d.enumerable && !d.configurable); }
{ let log = ''; let props = {get x() { log += 'x'; return {value: 1}; }, get y() { log += 'y'; return {get value() { log += 'v'; return 2; }}; }}; let o = Object.create({}, props); row(o.x === 1 && o.y === 2 && log === 'xyv'); }
row(name(() => Object.create({}, null)) === "TypeError" && name(() => Object.create({}, {a: 1})) === "TypeError" && name(() => Object.create(1, {})) === "TypeError");
{ let p = {k: 1}; let o = Object.create(p, {}); row(Object.getPrototypeOf(o) === p && o.k === 1); }
// Functions, Error objects and wrappers take a new prototype and keep their kind.
{ let f = () => 7; let p = {m: 1}; row(Object.setPrototypeOf(f, p) === f && f.m === 1 && f() === 7 && typeof f === 'function' && !(f instanceof Function)); }
{ function F() { this.v = 1; } Object.setPrototypeOf(F, null); row(new F().v === 1 && Object.getPrototypeOf(F) === null && F.call === undefined); }
{ let f = function () {}; f.__proto__ = {}; row(Object.getPrototypeOf(f) !== Function.prototype && f.bind === undefined); }
{ let g = Object.prototype.toString; let b = function () {}.bind(null); Object.setPrototypeOf(b, null); row(Object.getPrototypeOf(b) === null && g.call(b) === '[object Function]'); }
{ let e; try { null.x; } catch (x) { e = x; } row(Object.getPrototypeOf(e) === TypeError.prototype && Object.setPrototypeOf(e, null) === e && Object.getPrototypeOf(e) === null && !(e instanceof Error)); }
{ let n = new Number(3); Object.setPrototypeOf(n, String.prototype); row(n instanceof String && Number.prototype.valueOf.call(n) === 3); }
row(Object.getPrototypeOf(1) === Number.prototype && Object.getPrototypeOf('s') === String.prototype && Object.getPrototypeOf(true) === Boolean.prototype);
// A proxy's [[GetPrototypeOf]] runs its trap (light-cljs-845's frame) for
// Object.getPrototypeOf, Reflect.getPrototypeOf and the `__proto__` getter:
// a trapless proxy forwards, a trap read from a getter runs, a trap's throw
// propagates, and a result that is neither an object nor null, or differs
// from a non-extensible target's prototype, is a TypeError.
{ let log = []; let p = new Proxy({}, {getPrototypeOf(t) { log.push('trap'); return null; }}); row(Reflect.getPrototypeOf(p) === null && Object.getPrototypeOf(p) === null && Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get.call(p) === null && log.length === 3); }
{ let q = {}; let p = new Proxy(Object.create(q), {}); row(Object.getPrototypeOf(p) === q && Reflect.getPrototypeOf(new Proxy(p, {})) === q); }
{ let h = {get getPrototypeOf() { return () => Array.prototype; }}; row(Object.getPrototypeOf(new Proxy({}, h)) === Array.prototype); }
{ let p = new Proxy({}, {getPrototypeOf() { throw new RangeError('x'); }}); row(name(() => Object.getPrototypeOf(p)) === "RangeError"); }
{ let p = new Proxy({}, {getPrototypeOf() { return 1; }}); row(name(() => Reflect.getPrototypeOf(p)) === "TypeError"); }
{ let p = new Proxy(Object.preventExtensions({}), {getPrototypeOf() { return Array.prototype; }}); row(name(() => Object.getPrototypeOf(p)) === "TypeError"); }
{ let d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); let p = new Proxy({}, {getPrototypeOf: Reflect.getPrototypeOf}); row(d.get.call(p) === Object.prototype); }
// Reflect.setPrototypeOf answers false for %Object.prototype%'s new
// prototype, where the Pants engine throws.
row(!Reflect.setPrototypeOf(Object.prototype, {}) && Object.getPrototypeOf(Object.prototype) === null);
failed === 0 ? rows : -failed;
