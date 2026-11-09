// bbp-authored conformance case (light-cljs-882).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 5968-18627:
// object_extensibility (line 17343, bbp-122), its freeze, array and
// function rows; accessor_descriptors_and_integrity (line 7166, bbp-125),
// its integrity rows; tagged_templates (line 5968) and
// tagged_templates_descriptor_operations (line 6169), their Object.isFrozen
// and Object.freeze rows (light-cljs-862's template objects); array_push
// (line 18509), its frozen rows; rust/tests/object_statics.rs
// sealed_objects_keep_writable_values_and_observe_proxy_descriptors
// (line 52), its rows without a Proxy; and the planner's rows
// (light-cljs-766). SetIntegrityLevel makes the object non-extensible and
// every own property non-configurable and, frozen, every data property
// non-writable; TestIntegrityLevel checks the same, over every kind's own
// keys: ordinary objects and their index keys, arrays (dense, sparse,
// with named properties), mapped and unmapped arguments objects (freezing
// unmaps), closures (their lazy `length`, `name` and `prototype`), bound
// functions, errors, String wrappers, generators and iterators. Left out:
// rows using Object.create, Object.keys or Symbol() (light-cljs-766's other
// children), Proxy targets (light-cljs-891), `__rt` runtime values and
// realm objects (see objects_extensibility), and the QuickJS-only row where
// a strict write past a sealed array's end leaves its length changed (the
// Pants engine and Goose keep it). Errors are compared by name. Each row is
// a `row(...)` check; the script completes with the number of rows, or
// minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
function desc(o, k) { let d = Object.getOwnPropertyDescriptor(o, k); return d === undefined ? "absent" : ('get' in d ? 'accessor' : '' + d.value + ':' + d.writable) + ':' + d.enumerable + d.configurable; }
// The planner's rows (light-cljs-766).
{ let o = {x: 1}; Object.freeze(o); row(Object.isFrozen(o) === true); }
row(Object.isSealed(Object.seal({a: 1})) === true);
{ let a = Object.freeze([1]); let r; try { a.push(2); r = 'no'; } catch (e) { r = e instanceof TypeError; } row(r === true); }
{ let o = Object.seal({a: 1}); let r; try { delete o.a; r = 'no'; } catch (e) { r = e instanceof TypeError; } row(r === true); }
{ let o = {a: 1}; Object.freeze(o); let r; try { o.a = 2; r = 'none'; } catch (e) { r = (e instanceof TypeError) + ':' + o.a; } row(r === "true:1"); }
// freeze (object_extensibility).
{ let f = Object.freeze({x: 1}); row(name(() => { f.x = 2; }) === "TypeError" && f.x === 1 && name(() => { f.y = 2; }) === "TypeError" && f.y === undefined); }
{ let f = Object.freeze({x: 1}); let d = Object.getOwnPropertyDescriptor(f, 'x'); row(Object.isFrozen(f) && !Object.isExtensible(f) && d.value === 1 && !d.writable && d.enumerable && !d.configurable); }
{ let f = Object.freeze({x: 1}); row(name(() => Object.defineProperty(f, 'x', {value: 2})) === "TypeError" && Object.defineProperty(f, 'x', {value: 1}) === f && Object.freeze(f) === f && Object.seal(f) === f); }
{ let f = Object.freeze({x: 1}); row(name(() => { f.x++; }) === "TypeError" && name(() => { delete f.x; }) === "TypeError" && f.x === 1); }
row(Object.isFrozen({}) === false && Object.isFrozen(Object.preventExtensions({})) && !Object.isFrozen(Object.preventExtensions({x: 1})) && Object.isSealed(Object.preventExtensions({})));
{ let o = {}; Object.defineProperty(o, 'k', {value: 1}); let before = Object.isFrozen(o); Object.preventExtensions(o); row(!before && Object.isFrozen(o)); }
{ let o = {}; Object.defineProperty(o, 'k', {value: 1, writable: true}); Object.preventExtensions(o); row(!Object.isFrozen(o) && Object.isSealed(o)); }
{ let o = Object.freeze({inner: {n: 1}}); o.inner.n = 2; row(o.inner.n === 2 && !Object.isFrozen(o.inner)); }
row(Object.isFrozen('s') && Object.freeze('s') === 's' && Object.freeze(null) === null && Object.isFrozen(undefined) && Object.isFrozen(1) && Object.isSealed(true));
row(Object.freeze.length === 1 && Object.seal.length === 1 && Object.isFrozen.length === 1 && Object.isSealed.length === 1 && Object.freeze.name === "freeze" && Object.isSealed.name === "isSealed");
// Frozen objects built alike, accessors, index keys and many keys.
{ let a = {x: 1, y: 2}, b = {x: 3, y: 4}; Object.freeze(a); Object.freeze(b); b = {x: 5, y: 6}; row(desc(a, 'y') === "2:false:truefalse" && b.y === 6 && Object.isExtensible(b)); }
{ let o = {get g() { return 1; }, v: 2}; Object.freeze(o); row(desc(o, 'g') === "accessor:truefalse" && Object.isFrozen(o) && o.g === 1); }
{ let x = 0; let a = {get z() { return x; }, set z(v) { x = v; }}; Object.freeze(a); a.z = 9; let d = Object.getOwnPropertyDescriptor(a, 'z'); row(Object.isFrozen(a) && a.z === 9 && !d.configurable && d.set !== undefined); }
{ let o = {0: 'a', 1: 'b', x: 1}; Object.freeze(o); row(name(() => { o[0] = 'z'; }) === "TypeError" && o[0] === 'a' && desc(o, 1) === "b:false:truefalse" && Object.isFrozen(o)); }
{ let o = {}; o[3] = 'x'; o.k = 1; Object.seal(o); o[3] = 'y'; row(desc(o, 3) === "y:true:truefalse" && name(() => { delete o[3]; }) === "TypeError" && name(() => { o[4] = 1; }) === "TypeError"); }
{ let o = {}; for (let i = 0; i < 100; i++) o['k' + i] = i; delete o.k0; Object.freeze(o); let n = 0, s = ''; for (let k in o) { n += o[k]; if (k === 'k99') s = desc(o, k); } row(n === 4950 && s === "99:false:truefalse" && Object.isFrozen(o)); }
{ let o = Object.freeze({b: 1, a: 2, 1: 3}); let s = ''; for (let k in o) s += k; row(s === '1ba'); }
{ let o = Object.freeze({x: 1}); let c = {...o}; c.x = 2; row(c.x === 2 && !Object.isFrozen(c) && Object.isExtensible(c)); }
{ let o = {x: 1}; Object.preventExtensions(o); delete o.x; row(Object.isFrozen(o) && Object.isSealed(o)); }
// seal (object_statics.rs line 52).
{ let o = {a: 1}; row(Object.seal(o) === o && Object.isSealed(o) && !Object.isFrozen(o) && !Object.isExtensible(o) && !Object.getOwnPropertyDescriptor(o, 'a').configurable && (o.a = 2) === 2); }
{ let a = [1, 2]; Object.seal(a); a[0] = 3; row(Object.isSealed(a) && a[0] === 3 && !Object.getOwnPropertyDescriptor(a, 'length').configurable && Object.getOwnPropertyDescriptor(a, 'length').writable); }
{ let o = Object.seal({a: 1}), n = 0; try { o.b = 2; } catch (e) { if (e instanceof TypeError) n++; } try { delete o.a; } catch (e) { if (e instanceof TypeError) n++; } row(n === 2); }
row(Object.seal(7) === 7 && Object.seal(null) === null && Object.isSealed(null) && Object.isSealed(undefined) && !Object.isSealed({}) && Object.isSealed(Object.freeze({a: 1})));
{ let o = Object.seal({get g() { return 1; }}); row(name(() => Object.defineProperty(o, 'g', {value: 1})) === "TypeError" && name(() => Object.defineProperty(o, 'h', {get() { return 1; }})) === "TypeError" && o.g === 1); }
{ let o = Object.seal({a: 1}); Object.defineProperty(o, 'a', {writable: false}); row(Object.isFrozen(o) && desc(o, 'a') === "1:false:truefalse"); }
// Arrays (object_extensibility): a frozen array's elements and `length`.
{ let a = Object.freeze([1, , 3]); let d = Object.getOwnPropertyDescriptor(a, 0); let l = Object.getOwnPropertyDescriptor(a, 'length'); row(Object.isFrozen(a) && !Object.isExtensible(a) && a.length === 3 && a[0] === 1 && a[1] === undefined && !d.writable && d.enumerable && !d.configurable && !l.writable && Object.defineProperty(a, 0, {value: 1}) === a); }
for (let write of [a => { a[0] = 9; }, a => { a[1] = 9; }, a => { a[7] = 9; }, a => { a.length = 0; }, a => { a.name = 1; }, a => { Object.defineProperty(a, 0, {value: 2}); }, a => { a.push(4); }, a => { Array.prototype.push.call(a, 4); }, a => { delete a[0]; }]) {
  let a = Object.freeze([1, , 3]); row(name(() => write(a)) === "TypeError" && a.length === 3 && a[0] === 1 && !(1 in a));
}
{ let a = Object.freeze([1, , 3]); row(delete a[1] && delete a[5] && a.length === 3); }
{ let a = Object.freeze([]); row(name(() => a.push()) === "TypeError" && a.length === 0 && Object.isFrozen(a) && Object.isSealed(a)); }
{ let a = Object.freeze([1, 2]); row(Object.defineProperty(a, 'length', {value: 2}) === a && name(() => Object.defineProperty(a, 'length', {value: 1})) === "TypeError" && a.length === 2); }
{ let a = [1, 2]; Object.defineProperty(a, 1, {enumerable: false}); Object.freeze(a); let d = Object.getOwnPropertyDescriptor(a, 1); row(!d.enumerable && !d.writable && !d.configurable && Object.isFrozen(a)); }
{ let a = [1]; Object.defineProperty(a, 0, {writable: false, configurable: false}); Object.defineProperty(a, 'length', {writable: false}); let before = Object.isFrozen(a); Object.preventExtensions(a); row(!before && Object.isFrozen(a)); }
{ let a = [1]; a.tag = 'x'; Object.freeze(a); let d = Object.getOwnPropertyDescriptor(a, 'tag'); row(name(() => { a.tag = 'y'; }) === "TypeError" && !d.writable && d.enumerable && !d.configurable && a.tag === 'x' && Object.isFrozen(a)); }
{ let a = []; a.tag = 'x'; Object.defineProperty(a, 'length', {writable: false}); Object.preventExtensions(a); row(!Object.isFrozen(a)); }
{ let a = []; Object.defineProperty(a, 'length', {writable: false}); row(Object.isFrozen(Object.preventExtensions(a))); }
{ let x = 0; let a = []; Object.defineProperty(a, 0, {set: function (v) { x = v; }, configurable: true}); Object.freeze(a); a[0] = 4; row(Object.isFrozen(a) && a[0] === undefined && x === 4); }
{ let a = []; a[5000] = 1; Object.freeze(a); row(name(() => { a[5000] = 2; }) === "TypeError" && a[5000] === 1 && Object.isFrozen(a) && a.length === 5001); }
{ let a = []; a[5000] = 1; Object.seal(a); a[5000] = 2; row(name(() => { delete a[5000]; }) === "TypeError" && a[5000] === 2 && Object.isSealed(a) && !Object.isFrozen(a)); }
{ let a = Object.freeze([1, 2, 3]); let s = 0; for (let x of a) s += x; row(s === 6 && a.map(x => x * 2).join() === "2,4,6" && a.join('-') === "1-2-3"); }
// A sealed array: elements stay writable; no deletes, no new elements; a
// shorter `length` stops past the last element.
{ let a = [1, 2, 3]; Object.seal(a); row(name(() => { a.length = 0; }) === "TypeError" && a.length === 3 && a[2] === 3); }
{ let a = [1, , 3, , ]; Object.seal(a); a.length = 3; row(a.length === 3 && Object.isSealed(a)); }
{ let a = [1, 2]; Object.seal(a); a.length = 5; row(a.length === 5 && !(3 in a) && Object.isSealed(a)); }
{ let a = [1, 2]; Object.seal(a); row(name(() => { delete a[0]; }) === "TypeError" && a.length === 2 && a[0] === 1); }
{ let a = [1, 2]; Object.seal(a); Object.defineProperty(a, 0, {writable: false}); let d = Object.getOwnPropertyDescriptor(a, 0); row(!d.writable && !d.configurable && a[1] === 2 && Object.isSealed(a) && !Object.isFrozen(a) && name(() => { a[0] = 5; }) === "TypeError" && (a[1] = 5) === 5); }
{ let a = [1, 2]; Object.seal(a); Object.defineProperty(a, 0, {value: 7}); row(a[0] === 7 && desc(a, 0) === "7:true:truefalse" && name(() => Object.defineProperty(a, 0, {enumerable: false})) === "TypeError"); }
// Functions, bound functions, errors, wrappers, generators.
{ let g = () => 1; g.x = 1; row(Object.freeze(g) === g && Object.isFrozen(g) && !Object.isExtensible(g) && g() === 1 && g.x === 1 && name(() => { g.x = 2; }) === "TypeError" && name(() => { g.y = 2; }) === "TypeError"); }
{ function F() {} Object.freeze(F); let d = Object.getOwnPropertyDescriptor(F, 'prototype'); row(Object.isFrozen(F) && !d.writable && !d.configurable && d.value === F.prototype && name(() => { F.prototype = 1; }) === "TypeError"); }
{ let g = function () {}; Object.seal(g); row(g.name === 'g' && g.length === 0 && Object.isSealed(g) && Object.getOwnPropertyDescriptor(g, 'prototype').writable && desc(g, 'name') === "g:false:falsefalse"); }
{ function F() { this.x = 1; } Object.freeze(F); row(new F().x === 1); }
{ let e; try { null.x; } catch (caught) { e = caught; } row(!Object.isFrozen(e) && Object.isExtensible(e) && Object.freeze(e) === e && Object.isFrozen(e)); }
{
  let targets = [function () {}.bind(null), new Error('e'), (function* () {})(), [1][Symbol.iterator](), new String('ab'), new Boolean(true)];
  let ok = true;
  for (let t of targets) { t.k = 1; ok = ok && !Object.isSealed(t) && Object.seal(t) === t && Object.isSealed(t) && desc(t, 'k') === "1:true:truefalse" && Object.freeze(t) === t && Object.isFrozen(t) && desc(t, 'k') === "1:false:truefalse"; }
  row(ok);
}
{ let s = new String('ab'); Object.freeze(s); row(Object.isFrozen(s) && s[0] === 'a' && s.length === 2 && name(() => { s[0] = 'z'; }) === "TypeError"); }
{ let g = (function* () { yield 1; })(); Object.freeze(g); row(g.next().value === 1 && Object.isFrozen(g)); }
// Arguments objects: freezing a mapped one unmaps its indices.
{ let f = function (a) { 'use strict'; Object.freeze(arguments); return Object.isFrozen(arguments) && arguments[0] === 4 && name(() => { arguments[0] = 5; }) === "TypeError"; }; row(f(4)); }
// Template objects are frozen (tagged_templates, light-cljs-862).
{ function tag(s) { return s; } function site() { return tag`x${1}y`; } let a = site(); row(Object.isFrozen(a) && Object.isFrozen(a.raw) && !Object.isExtensible(a) && Object.isSealed(a)); }
{ function tag(s) { let d = Object.getOwnPropertyDescriptor(s, 'raw'); let e = Object.getOwnPropertyDescriptor(s, '0'); return !d.writable && !d.enumerable && !d.configurable && !e.writable && e.enumerable && !e.configurable && Object.freeze(s) === s && Object.seal(s) === s; } row(tag`x`); }
failed === 0 ? rows : -failed;
