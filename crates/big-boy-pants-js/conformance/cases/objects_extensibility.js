// bbp-authored conformance case (light-cljs-882).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 17343-18627:
// object_extensibility (line 17343, bbp-122), its preventExtensions,
// primitive, array, function and error rows; array_push (line 18509), its
// preventExtensions and freeze rows; rust/tests/reflect.rs (line 40), its
// isExtensible and preventExtensions rows; the Proxy invariants that read a
// non-extensible target (light-cljs-845's for-in, ownKeys); and the
// planner's rows (light-cljs-766). [[Extensible]] cleared refuses a new
// property in [[Set]] and [[DefineOwnProperty]] and a new array element,
// while existing properties stay writable and configurable and a shorter
// or equal `length` still works. Left out: rows using Object.create,
// Object.keys, Object.setPrototypeOf, `__proto__` assignment and Reflect.set
// (light-cljs-883 and the rest of light-cljs-766's children), Proxy traps
// for [[PreventExtensions]], [[IsExtensible]], [[GetOwnProperty]] and
// [[GetPrototypeOf]] reached through Reflect (light-cljs-891), `__rt` runtime
// values (the Pants engine answers them frozen, the V8 build's runtime
// makes them extensible objects, and Goose fails by name), and realm
// objects as targets (the Pants engine and Goose refuse to change them,
// QuickJS allows it). Errors are compared by name. Each row is a `row(...)`
// check; the script completes with the number of rows, or minus the first
// failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-766).
{ let o = {x: 1}; Object.preventExtensions(o); row(Object.isExtensible(o) === false); }
{ let a = [1]; Object.preventExtensions(a); row(name(() => { a[1] = 2; }) === "TypeError"); }
{ let o = {}; Reflect.preventExtensions(o); row(Reflect.isExtensible(o) === false); }
// object_extensibility (line 17343): the upstream row.
{ let a = {x: 1}; let before = Object.isExtensible(a); Object.preventExtensions(a); let err = false; try { a.y = 2; } catch (e) { err = true; } row(before === true && Object.isExtensible(a) === false && typeof a.y === 'undefined' && err); }
// preventExtensions returns its argument; existing properties stay writable and configurable.
{ let a = {x: 1}; row(Object.preventExtensions(a) === a); }
{ let a = {x: 1}; Object.preventExtensions(a); a.x = 3; row(a.x === 3); }
{ let a = {x: 1}; Object.preventExtensions(a); Object.defineProperty(a, 'x', {value: 4, enumerable: false}); let d = Object.getOwnPropertyDescriptor(a, 'x'); row(d.value === 4 && d.writable && !d.enumerable && d.configurable && !Object.isFrozen(a)); }
// A new property: a TypeError in strict code; defineProperty always throws.
{ let a = {x: 1}; Object.preventExtensions(a); row(name(() => { a['k'] = 1; }) === "TypeError" && name(() => { a[0] = 1; }) === "TypeError" && !('k' in a) && !(0 in a)); }
{ let a = {x: 1}; Object.preventExtensions(a); row(name(() => Object.defineProperty(a, 'w', {value: 1})) === "TypeError" && Object.getOwnPropertyDescriptor(a, 'w') === undefined); }
{ let a = {x: 1}; Object.preventExtensions(a); row(name(() => Object.defineProperty(a, 'g', {get: () => 1})) === "TypeError" && !('g' in a)); }
// A non-extensible object still loses configurable properties, and cannot get them back.
{ let a = {x: 1}; Object.preventExtensions(a); row(delete a.x && a.x === undefined && Object.getOwnPropertyDescriptor(a, 'x') === undefined); }
{ let a = {x: 1}; Object.preventExtensions(a); delete a.x; row(name(() => { a.x = 1; }) === "TypeError" && !('x' in a)); }
// An inherited setter still runs; an inherited data property does not let one in.
{ let p = {set z(v) { this.seen = 1; }}; let o = {__proto__: p, seen: 0}; Object.preventExtensions(o); o.z = 3; row(o.seen === 1 && !Object.getOwnPropertyDescriptor(o, 'z')); }
{ let p = {z: 1}; let o = {__proto__: p}; Object.preventExtensions(o); row(name(() => { o.z = 2; }) === "TypeError" && p.z === 1 && !Object.getOwnPropertyDescriptor(o, 'z')); }
// Primitives: not extensible, returned unchanged.
row(Object.isExtensible(1) === false && Object.preventExtensions(1) === 1 && Object.isExtensible() === false && Object.preventExtensions('s') === 's' && Object.preventExtensions(null) === null);
row(name(() => Reflect.isExtensible(1)) === "TypeError" && name(() => Reflect.preventExtensions(1)) === "TypeError" && name(() => Reflect.isExtensible()) === "TypeError");
// Arrays: existing elements stay writable; no new element, by index, push or definition.
{ let a = [1]; Object.preventExtensions(a); a[0] = 5; let r = false; try { a[1] = 2; } catch (e) { r = true; } row(r && a[0] === 5 && a.length === 1 && !Object.isFrozen(a)); }
{ let a = [1, 2]; Object.preventExtensions(a); row(name(() => a.push(3)) === "TypeError" && a.length === 2); }
{ let a = [1, 2]; Object.preventExtensions(a); row(name(() => Array.prototype.push.call(a, 3)) === "TypeError" && a.length === 2 && a[2] === undefined); }
{ let a = []; Object.preventExtensions(a); row(a.push() === 0); }
{ let a = [1, , 3]; Object.preventExtensions(a); row(name(() => { a[1] = 2; }) === "TypeError" && !(1 in a) && name(() => Object.defineProperty(a, 1, {value: 2})) === "TypeError"); }
{ let a = [1, 2, 3]; Object.preventExtensions(a); a.length = 1; row(a.length === 1 && a[1] === undefined && name(() => { a[1] = 2; }) === "TypeError"); }
{ let a = [1]; Object.preventExtensions(a); row(name(() => { a.tag = 1; }) === "TypeError" && a.tag === undefined); }
{ let a = [1, 2]; Object.preventExtensions(a); Object.defineProperty(a, 0, {value: 7}); delete a[1]; row(a[0] === 7 && !(1 in a) && a.length === 2 && name(() => { a[1] = 1; }) === "TypeError"); }
{ let a = []; a[5000] = 1; Object.preventExtensions(a); a[5000] = 2; row(a[5000] === 2 && name(() => { a[0] = 1; }) === "TypeError" && a.length === 5001); }
{ let a = [1]; Object.preventExtensions(a); row(Object.isFrozen(Object.preventExtensions([])) === false && !Object.isExtensible(a) && Object.isExtensible([])); }
// Functions, bound functions, errors, wrappers, generators and iterators.
row((() => { let g = () => 1; return Object.isExtensible(g) && !Object.isFrozen(g); })());
{ let g = () => 1; Object.preventExtensions(g); row(name(() => { g.x = 1; }) === "TypeError" && g() === 1 && g.name === 'g' && g.length === 0); }
{ function F() {} Object.preventExtensions(F); let p = F.prototype; row(typeof p === 'object' && p.constructor === F && Object.getPrototypeOf(new F()) === p); }
{ function F() { this.v = 1; } Object.preventExtensions(F); row(new F().v === 1 && Object.isExtensible(new F())); }
{ let e; try { null.x; } catch (caught) { e = caught; } row(Object.isExtensible(e) && Object.preventExtensions(e) === e && !Object.isExtensible(e) && name(() => { e.extra = 1; }) === "TypeError"); }
{
  let targets = [function () {}.bind(null), new Error('e'), (function* () {})(), [1][Symbol.iterator](), new String('ab'), new Number(1)];
  let ok = true;
  for (let t of targets) { ok = ok && Object.isExtensible(t) && Object.preventExtensions(t) === t && !Object.isExtensible(t) && name(() => { t.k = 1; }) === "TypeError" && !Reflect.defineProperty(t, 'k', {value: 1}) && !('k' in t); }
  row(ok);
}
{ let s = new String('ab'); Object.preventExtensions(s); row(s[0] === 'a' && s.length === 2 && name(() => { s[5] = 'q'; }) === "TypeError"); }
// Reflect: booleans, not exceptions (rust/tests/reflect.rs line 40).
{ let o = {a: 1}; row(Reflect.isExtensible(o) && Reflect.preventExtensions(o) && !Reflect.isExtensible(o) && !Reflect.defineProperty(o, 'b', {value: 1}) && Reflect.defineProperty(o, 'a', {value: 2}) && o.a === 2 && Reflect.preventExtensions(o)); }
row(Reflect.isExtensible.length === 1 && Reflect.preventExtensions.length === 1 && Object.isExtensible.length === 1 && Object.preventExtensions.length === 1 && Reflect.isExtensible.name === "isExtensible" && Object.preventExtensions.name === "preventExtensions");
// Proxy invariants read the target's real extensibility (light-cljs-845's for-in).
{ let t = Object.preventExtensions({a: 1}); let p = new Proxy(t, {ownKeys() { return ['a', 'b']; }}); row(name(() => { for (let k in p) {} }) === "TypeError"); }
{ let t = Object.preventExtensions({a: 1}); let p = new Proxy(t, {ownKeys() { return []; }}); row(name(() => { for (let k in p) {} }) === "TypeError"); }
{ let t = Object.preventExtensions({a: 1}); let p = new Proxy(t, {ownKeys() { return ['a']; }}); let s = ''; for (let k in p) s += k; row(s === 'a'); }
{ let t = {a: 1}; let p = new Proxy(t, {ownKeys() { return ['a', 'b']; }, getOwnPropertyDescriptor(t, k) { return k === 'b' ? undefined : Reflect.getOwnPropertyDescriptor(t, k); }}); let s = ''; for (let k in p) s += k; row(s === 'a'); }
failed === 0 ? rows : -failed;
