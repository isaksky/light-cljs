// bbp-authored conformance case (light-cljs-884).
// Derives from: crates/big-boy-pants-js/rust/tests/object_statics.rs lines 38-140 (bbp-318):
// Object.values and Object.entries (EnumerableOwnProperties: each key's
// descriptor re-read before its value, getters run), Object.assign ([[Get]]
// on each source, [[Set]] on the target, null and undefined sources
// skipped, strings and wrappers), Object.fromEntries (GetIterator, the
// iterator closed on an abrupt completion while an entry is processed),
// Object.getOwnPropertyDescriptors (no value read through a getter), and
// the planner's rows (light-cljs-766). Left out: rows where QuickJS
// disagrees with the specification and the Pants engine (it snapshots
// Object.assign's enumerability, and closes an iterator whose `next` or
// `value` throws); typed arrays (light-cljs-769); a BigInt key
// (light-cljs-771); Symbol() (light-cljs-885), so the symbol rows use
// Symbol.iterator; a proxy's [[Get]] and [[Set]] (light-cljs-890);
// Object.assign onto a realm object. Each row is a `row(...)` check; the
// script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-766).
row(Object.entries({a: 1, b: 2}).join(';') === "a,1;b,2");
row(Object.values({a: 1, b: 2}).join() === "1,2");
{ let o = Object.assign({}, {a: 1}, {b: 2}); row(o.a + o.b === 3); }
row(Object.fromEntries([['a', 1]]).a === 1);
row(Object.getOwnPropertyDescriptors({a: 1}).a.value === 1);
// object_statics_enumeration_and_assignment (line 68).
row(Object.assign.length === 2 && Object.entries.length === 1 && Object.values.length === 1 && Object.fromEntries.length === 1 && Object.getOwnPropertyDescriptors.length === 1);
{ let s = Symbol.iterator, o = {b: 2, 2: 20, 1: 10, [s]: 3}; Object.defineProperty(o, 'hidden', {value: 9}); row(Object.entries(o).map(e => e.join(':')).join() === '1:10,2:20,b:2' && Object.values(o).join() === '10,20,2'); }
{ let log = '', o = {get a() { log += 'a'; delete this.b; this.c = 3; return 1; }, b: 2}; row(Object.values(o).join() === '1' && log === 'a'); }
{ let o = {get a() { Object.defineProperty(this, 'b', {enumerable: false}); return 1; }, b: 2}; row(Object.entries(o).join(';') === 'a,1'); }
row(Object.entries('ab').map(e => e.join(':')).join() === '0:a,1:b' && Object.values(7).length === 0);
{ let a = []; a[3] = 7; let e = Object.entries(a); row(e.length === 1 && e[0][0] === '3' && e[0][1] === 7); }
{
  let s = Symbol.iterator, log = '', p = {set a(v) { log += 's' + v; this.x = v; }}; let t = Object.create(p);
  let o = {get a() { log += 'g'; return 7; }, [s]: 8}; Object.defineProperty(o, 'hide', {value: 4});
  row(Object.assign(t, null, undefined, o, {z: 9}) === t && log === 'gs7' && t.x === 7 && t[s] === 8 && t.z === 9 && !('hide' in t) && !Object.getOwnPropertyDescriptor(t, 'a'));
}
{ let o = {}; Object.defineProperty(o, '__proto__', {value: null, enumerable: true}); let t = Object.assign({}, o); row(Object.getPrototypeOf(t) === null); }
{ let t = {}; Object.defineProperty(t, 'b', {value: 0}); let ok = false; try { Object.assign(t, {a: 1, b: 2, c: 3}); } catch (e) { ok = e instanceof TypeError; } row(ok && t.a === 1 && !('c' in t)); }
{ let t = Object.freeze({}); row(name(() => Object.assign(t, {a: 1})) === "TypeError" && Object.assign(t, {}) === t); }
{ let n = Object.assign(7, {a: 1}); row(typeof n === 'object' && n.valueOf() === 7 && n.a === 1); }
row(Object.assign({}, 'ab', [5])[0] === 5 && Object.assign({}, 'ab')[1] === 'b' && Object.keys(Object.assign({}, 1, true)).length === 0);
{ let t = {}; row(Object.assign(t) === t && name(() => Object.assign(null, {})) === "TypeError" && name(() => Object.assign()) === "TypeError"); }
{ let ok = false, o = {get a() { throw 9; }, b: 1}, t = {}; try { Object.assign(t, o); } catch (e) { ok = e === 9; } row(ok && !('b' in t)); }
{ let ok = false; try { Object.values({get a() { throw 8; }}); } catch (e) { ok = e === 8; } row(ok); }
{ let f = function () {}; f.a = 1; row(Object.entries(f).join() === 'a,1' && Object.values(f).length === 1); }
{ let n = 0; for (let f of [Object.assign, Object.entries, Object.values, Object.getOwnPropertyDescriptors]) { try { f(null); } catch (e) { if (e instanceof TypeError) n++; } } row(n === 4); }
// Proxy sources: [[OwnPropertyKeys]] and [[GetOwnProperty]] run their traps.
{
  let log = '', p = new Proxy({a: 1}, {ownKeys() { log += 'k'; return ['a']; }, getOwnPropertyDescriptor(t, k) { log += 'd'; return {value: 1, writable: true, enumerable: false, configurable: true}; }});
  row(Object.values(p).length === 0 && Object.assign({}, p).a === undefined && log === 'kdkd');
}
// object_statics_from_entries_and_iterator_close (line 102).
{ let s = Symbol.iterator, o = Object.fromEntries([['a', 1], [s, 2], ['a', 3], ['__proto__', 7]]); row(o.a === 3 && o[s] === 2 && o.__proto__ === 7 && Object.getPrototypeOf(o) === Object.prototype && Object.getOwnPropertyDescriptor(o, 'a').writable); }
{
  let log = '', key = {toString() { log += 'k'; return 'a'; }};
  function* entries() { yield {get 0() { log += '0'; return key; }, get 1() { log += '1'; return 7; }}; }
  row(Object.fromEntries(entries()).a === 7 && log === '01k');
}
{
  let closed = 0, error = {}, ok = false;
  let it = {[Symbol.iterator]() { return this; }, next() { return {value: [{toString() { throw error; }}, 2]}; }, return() { closed++; throw 9; }};
  try { Object.fromEntries(it); } catch (e) { ok = e === error; }
  row(ok && closed === 1);
}
{ let closed = 0, ok = false; function* it() { try { yield 1; } finally { closed++; } } try { Object.fromEntries(it()); } catch (e) { ok = e instanceof TypeError; } row(ok && closed === 1); }
{
  let closed = 0, error = {}, ok = false;
  let it = {[Symbol.iterator]() { return this; }, next() { return {value: {get 0() { throw error; }}}; }, return() { closed++; return {}; }};
  try { Object.fromEntries(it); } catch (e) { ok = e === error; }
  row(ok && closed === 1);
}
{
  let closed = 0, ok = false, n = 0;
  let it = {[Symbol.iterator]() { return this; }, next() { n++; return n > 2 ? {done: true} : {value: ['k' + n, n]}; }, get return() { closed++; return undefined; }};
  let o = Object.fromEntries(it);
  row(o.k1 === 1 && o.k2 === 2 && closed === 0);
}
{ let n = 0; for (let x of [null, undefined, 1]) { try { Object.fromEntries(x); } catch (e) { if (e instanceof TypeError) n++; } } row(n === 3); }
row(Object.keys(Object.fromEntries([])).length === 0);
row(Object.fromEntries(Object.entries({x: 1, y: 2})).y === 2 && Object.fromEntries('').constructor === Object);
{ let n = 0; for (let it of [{[Symbol.iterator]() { return this; }, next: 1}, {[Symbol.iterator]() { return this; }}]) { try { Object.fromEntries(it); } catch (e) { if (e instanceof TypeError) n++; } } row(n === 2); }
// The statics as callbacks of Array.prototype.map and forEach.
{ let log = ""; let r = [{a: 1}, {get b() { log += "b"; return 2; }}].map(Object.values); row(r.map(v => v.join()).join(";") === "1;2" && log === "b" && [[["k", 3]]].map(Object.fromEntries)[0].k === 3); }
// own_property_descriptors_preserve_attributes_without_getting_values (line 38).
{
  let s = Symbol.iterator, calls = 0, get = function () { calls++; return 7; }, o = {a: 1, [s]: 2};
  Object.defineProperty(o, 'hidden', {get, enumerable: false, configurable: true});
  let d = Object.getOwnPropertyDescriptors(o);
  row(d.a.value === 1 && d.a.writable && d.a.enumerable && d.a.configurable && d[s].value === 2 && d.hidden.get === get && d.hidden.set === undefined && !d.hidden.enumerable && !('value' in d.hidden) && calls === 0);
}
{ let d = Object.getOwnPropertyDescriptors('ab'); row(d[0].value === 'a' && !d[0].writable && d.length.value === 2 && !d.length.enumerable && Object.keys(Object.getOwnPropertyDescriptors(1)).length === 0); }
{ let o = Object.create(null); o.__proto__ = 7; let d = Object.getOwnPropertyDescriptors(o); row(Object.getPrototypeOf(d) === Object.prototype && d.__proto__.value === 7); }
{
  let log = '', s = Symbol.iterator;
  let p = new Proxy({a: 1, b: 2, [s]: 3}, {ownKeys() { log += 'k'; return ['b', s, 'a']; }, getOwnPropertyDescriptor(t, k) { log += 'd'; if (k === 'b') { delete t.a; return undefined; } return Object.getOwnPropertyDescriptor(t, k); }});
  let d = Object.getOwnPropertyDescriptors(p);
  row(log === 'kddd' && Reflect.ownKeys(d).length === 1 && d[s].value === 3);
}
{ let calls = 0, ok = false, p = new Proxy({a: 1, b: 2}, {getOwnPropertyDescriptor(t, k) { calls++; throw 17; }}); try { Object.getOwnPropertyDescriptors(p); } catch (e) { ok = e === 17; } row(ok && calls === 1); }
{ let d = Object.getOwnPropertyDescriptors(function f(a) {}); row(d.length.value === 1 && d.name.value === 'f' && d.prototype.writable && !d.prototype.enumerable); }
failed === 0 ? rows : -failed;
