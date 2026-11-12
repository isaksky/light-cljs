// bbp-authored conformance case (light-cljs-945).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 26594-26635
// (weak_map_operations, without its WeakMap.length row, which
// QuickJS answers otherwise: weak_maps_and_sets_quickjs) and 26676-26688
// (weak_map_property_descriptors_and_delete); rust/tests/map_iterables.rs
// lines 25-79 (map_and_weak_map_accept_entry_iterables and
// entry_failures_close_once_and_preserve_original_throw: their WeakMap
// rows), 81-110 (iterator_protocol_failures_do_not_close: its WeakMap
// rows) and 130-150 (map_iterable_frames_survive_collection_and_migration:
// its WeakMap row); rust/tests/sets.rs lines 25-35, 53-65, 70-88, 92 and
// 170-188 (their WeakSet rows, without sets_preserve_ordinary_object_protocols's
// JSON.stringify, light-cljs-767's); rust/tests/builtin_subclasses.rs lines
// 67, 73, 531 and 534 (WeakMap and WeakSet subclasses); with
// light-cljs-769's planner rows: WeakMap and WeakSet as identity-keyed,
// non-iterable collections whose keys CanBeHeldWeakly accepts (an object,
// or a symbol Symbol.for did not register). Left out: getOrInsert and
// getOrInsertComputed (light-cljs-953), entries dropped once their key is
// unreachable (light-cljs-951) and WeakRef (light-cljs-952). Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-769).
{ let k = {}, s = Symbol('s'), w = new WeakMap([[k, 1]]); w.set(s, 2); let c = ''; for (const bad of [1, 'k', Symbol.for('r'), null]) { try { w.set(bad, 1) } catch (e) { c += e.name[0] } } row([w.get(k), w.get(s), w.has({}), w.delete(1), w.get(1), w.delete(k), w.has(k), c, Object.prototype.toString.call(w), typeof w.size, typeof WeakMap.prototype.forEach].join() === "1,2,false,false,,true,false,TTTT,[object WeakMap],undefined,undefined"); }
{ let k = {}, ws = new WeakSet([k]); let c = ''; try { ws.add(1) } catch (e) { c = e.name } row([ws.has(k), ws.add(k) === ws, ws.delete(k), ws.has(k), typeof ws.size, c, Object.prototype.toString.call(ws)].join() === "true,true,true,false,undefined,TypeError,[object WeakSet]"); }
// weak_map_operations (js_source.rs line 26594), without WeakMap.length.
{ let m = new WeakMap(); let k = {}; let s = Symbol('s'); let v = {}; row(m.set(k, v) === m && m.set(s, 7) === m && m.get(k) === v && m.has(s) && m.get(s) === 7 && m.delete(k) && !m.delete(k) && !m.has(k) && m.get(k) === undefined && m.get(1) === undefined && !m.has(1) && m.delete(1) === false); }
{ let m = new WeakMap(); let n = 0; for (let k of [1, 'k', Symbol.for('r'), null, undefined, true]) { try { m.set(k, 1); } catch (e) { if (e instanceof TypeError) n++; } } row(n === 6); }
{ let m = new WeakMap(); let k = {}; m.set(k, 1); m.set(k, 2); row(m.get(k) === 2 && !m.has({}) && m.set(Symbol.iterator, 3).get(Symbol.iterator) === 3); }
{ let m = new WeakMap(); row(m instanceof WeakMap && m.constructor === WeakMap && WeakMap.name === 'WeakMap' && Object.getPrototypeOf(m) === WeakMap.prototype && m[Symbol.toStringTag] === 'WeakMap' && m.size === undefined && m.clear === undefined && m[Symbol.iterator] === undefined); }
{ let m = new WeakMap(); let k = {}; m.own = 7; Object.freeze(m); m.set(k, 3); row(m.get(k) === 3 && m.own === 7 && Object.isFrozen(m) && Object.keys(m).length === 1); }
row(new WeakMap(undefined) instanceof WeakMap && new WeakMap(null) instanceof WeakMap);
row(name(() => WeakMap()) === "TypeError" && name(() => new WeakMap(1)) === "TypeError");
for (let [method, length] of [['set', 2], ['get', 1], ['has', 1], ['delete', 1]]) {
  let d = Object.getOwnPropertyDescriptor(WeakMap.prototype, method);
  row(d.writable && !d.enumerable && d.configurable && d.value.name === method && d.value.length === length);
  let receivers = [undefined, null, 1, {}, new Map(), WeakMap.prototype, Object.create(WeakMap.prototype), new WeakSet()];
  let errors = 0; for (let r of receivers) { if (name(() => WeakMap.prototype[method].call(r, 1, 2)) === "TypeError") errors++; }
  row(errors === receivers.length);
  row(name(() => new WeakMap.prototype[method]()) === "TypeError");
}
// weak_map_property_descriptors_and_delete (js_source.rs line 26676): a
// WeakMap's own properties are ordinary, apart from its entries.
{ let m = new WeakMap(); Object.defineProperty(m, 'x', {value: 7, configurable: true}); let d = Object.getOwnPropertyDescriptor(m, 'x'); row(d.value === 7 && !d.writable && !d.enumerable && d.configurable && m.x === 7 && delete m.x && !('x' in m) && delete m.x); }
{ let m = new WeakMap(); let s = Symbol('property'); Object.defineProperty(m, s, {value: 9, writable: true, enumerable: true, configurable: true}); m[s] = 10; let d = Object.getOwnPropertyDescriptor(m, s); row(d.value === 10 && d.writable && d.enumerable && d.configurable && delete m[s] && !m.has(s) && m[s] === undefined); }
{ let m = new WeakMap(); let s = Symbol('accessor'); let get = function () { return this.saved; }; let set = function (v) { this.saved = v; }; Object.defineProperty(m, s, {get: get, set: set, configurable: true}); m[s] = 12; let d = Object.getOwnPropertyDescriptor(m, s); row(d.get === get && d.set === set && m[s] === 12 && delete m[s] && m[s] === undefined && delete m.saved); }
{ let m = new WeakMap(); m.x = 1; let s = Symbol(); m[s] = 2; row(delete m.x && delete m[s] && Object.keys(m).length === 0 && Object.getOwnPropertyDescriptor(m, s) === undefined && !(s in m)); }
{ let m = new WeakMap(); let s = Symbol(); Object.defineProperty(m, s, {value: 1}); let ok = delete m[s] === false; let caught = false; try { (function () { 'use strict'; delete m[s]; })(); } catch (e) { caught = e instanceof TypeError; } row(ok && caught && m[s] === 1); }
{ let m = new WeakMap(); Object.defineProperty(m, 'fixed', {value: 3}); let ok = delete m.fixed === false; let caught = false; try { (function () { 'use strict'; delete m.fixed; })(); } catch (e) { caught = e instanceof TypeError; } row(ok && caught && m.fixed === 3); }
{ let m = new WeakMap(); let k = {}; m.set(k, 8); Object.defineProperty(m, 'get', {value: 1, configurable: true}); row(m.get === 1 && delete m.get && m.get(k) === 8); }
// map_and_weak_map_accept_entry_iterables (map_iterables.rs line 25): the
// WeakMap rows.
{ const k = {}; row(new WeakMap([[k, 2]]).get(k) === 2); }
row(!new WeakMap(null).has({}));
{ let k = {}, s = Symbol(); let m = new WeakMap([[k, 1], [s, 2], [k, 3]]); row(m.get(k) === 3 && m.get(s) === 2); }
row(new WeakMap([]) instanceof WeakMap);
{ function* entries() { const a = {}, b = {}; yield {0: a, 1: 2}; yield [b]; return; } let m = new WeakMap(entries()); row(m instanceof WeakMap); }
// entry_failures_close_once_and_preserve_original_throw (line 44): an
// entry that is no object, whose key cannot be held weakly, or whose "0"
// or "1" throws, closes the iterator once and keeps the original throw.
for (let [entry, throws] of [[() => null], [() => undefined], [() => 1], [() => 'ab'], [() => true], [() => Symbol()], [() => [1, 2]], [() => [Symbol.for('w'), 2]], [() => ({get 0() { throw 17; }, get 1() { throw 99; }}), true], [() => ({0: {}, get 1() { throw 17; }}), true]]) {
  for (let close of [() => ({}), () => { throw 99; }, () => 1]) {
    let n = 0, closed = 0, caught;
    let it = {[Symbol.iterator]() { return this; }, next() { n++; return {value: entry()}; }, return() { if (this !== it || arguments.length !== 0) throw 98; closed++; return close(); }};
    try { new WeakMap(it); } catch (e) { caught = e; }
    row((throws ? caught === 17 : caught instanceof TypeError) && n === 1 && closed === 1);
  }
}
{ let closed = 0, ok = false; function* g() { try { yield [1, 2]; } finally { closed++; } } try { new WeakMap(g()); } catch (e) { ok = e instanceof TypeError; } row(ok && closed === 1); }
// iterator_protocol_failures_do_not_close (line 81): the WeakMap rows.
for (let make of [
  () => ({next() { throw 17; }}),
  () => ({next() { return 1; }}),
  () => ({next() { return {get done() { throw 17; }}; }}),
  () => ({next() { return {get value() { throw 17; }}; }}),
  () => ({next: 0}),
  () => ({get next() { throw 17; }}),
]) {
  let closed = 0, caught;
  let it = make(); it[Symbol.iterator] = function () { return this; }; it.return = function () { closed++; return {}; };
  try { new WeakMap(it); } catch (e) { caught = e; }
  row((caught === 17 || caught instanceof TypeError) && closed === 0);
}
{ let n = 0; for (let v of [true, 1, {}, 'ab']) { try { new WeakMap(v); } catch (e) { if (e instanceof TypeError) n++; } } row(n === 4); }
// map_iterable_frames_survive_collection_and_migration (line 130): the
// WeakMap row, its entries' "1" getters.
{ let C = WeakMap, calls = 0; let keys = [{}, {}]; function* g() { for (let i = 0; i < keys.length; i++) yield {0: keys[i], get 1() { calls++; return {n: i + 7}; }}; } let m = new C(g()); row(m.get(keys[0]).n === 7 && m.get(keys[1]).n === 8 && calls === 2); }
// constructors_and_membership (sets.rs line 25): the WeakSet rows, without
// WeakSet.length (weak_maps_and_sets_quickjs).
row(typeof WeakSet === 'function');
{ let k = {}, y = Symbol(), s = new WeakSet([k, y, k]); row(s.has(k) && s.has(y) && s.add(k) === s && s.delete(k) && !s.has(k) && !s.delete(k) && !s.has(1) && !s.delete(Symbol.for('x'))); }
row(new WeakSet(null) instanceof WeakSet && new WeakSet(undefined) instanceof WeakSet);
row(WeakSet.prototype.add.length === 1 && WeakSet.prototype.has.length === 1 && WeakSet.prototype.delete.length === 1);
{ let s = new WeakSet(); row(s.size === undefined && s.clear === undefined && s.forEach === undefined && s[Symbol.iterator] === undefined); }
// sets.rs lines 53-65: integrity, brands and invalid inputs.
{ let s = new WeakSet(), k = {}; Object.preventExtensions(s); s.add(k); row(s.has(k) && s.delete(k)); }
{ let n = 0; for (let C of [Set, WeakSet]) { try { C(); } catch (e) { if (e instanceof TypeError) n++; } } row(n === 2); }
{ let n = 0; for (let v of [1, null, undefined, 'a', Symbol.for('x')]) { try { new WeakSet().add(v); } catch (e) { if (e instanceof TypeError) n++; } } row(n === 5); }
{ let n = 0; for (let f of [WeakSet.prototype.add, WeakSet.prototype.has, WeakSet.prototype.delete]) { try { f.call(new Map(), () => 0); } catch (e) { if (e instanceof TypeError) n++; } } row(n === 3); }
{ let n = 0; try { WeakMap.prototype.has.call(new WeakSet(), {}); } catch (e) { if (e instanceof TypeError) n++; } try { WeakSet.prototype.has.call(new WeakMap(), {}); } catch (e) { if (e instanceof TypeError) n++; } row(n === 2); }
// iterable_errors_and_close (sets.rs line 70): the WeakSet rows.
for (let make of [() => ({next() { throw 17; }}), () => ({next() { return {get done() { throw 17; }}; }}), () => ({next() { return {get value() { throw 17; }}; }})]) {
  let closed = 0, ok = false; let it = make(); it[Symbol.iterator] = function () { return this; }; it.return = function () { closed++; return {}; };
  try { new WeakSet(it); } catch (e) { ok = e === 17; }
  row(ok && closed === 0);
}
for (let close of [() => ({}), () => { throw 99; }, () => 1]) {
  let closed = 0, ok = false; let it = {[Symbol.iterator]() { return this; }, next() { return {value: 1}; }, return() { closed++; return close(); }};
  try { new WeakSet(it); } catch (e) { ok = e instanceof TypeError; }
  row(ok && closed === 1);
}
// set_frames_survive_collection_and_migration (sets.rs line 92): a WeakSet
// of an object and a symbol, read from a Set's forEach.
{ let a = {}, b = Symbol(), w = new WeakSet([a, b]); function* g() { yield a; yield b; yield a; } let s = new Set(g()), n = 0; s.forEach((v, k, o) => { if (v === k && o === s && w.has(v)) n++; }); let it = s.entries(); row(it.next().value[0] === a && it.next().value[1] === b && it.next().done && n === 2); }
{ let k = {}; row(new Set([k, k]).size === 1 && new WeakSet([k]).has(k)); }
// sets_preserve_ordinary_object_protocols (sets.rs line 170): the WeakSet
// row without JSON.stringify.
{
  let s = new WeakSet([{hidden: 17}]);
  s.a = 1; s.b = {n: 2}; s[Symbol()] = 99;
  Object.defineProperty(s, 'secret', {value: 99});
  let ok = Object.keys(s).join() === 'a,b';
  let p = {tag: 7};
  Object.setPrototypeOf(s, p);
  ok = ok && Object.getPrototypeOf(s) === p && s.tag === 7;
  let k = {}; WeakSet.prototype.add.call(s, k);
  class Base { constructor(o) { return o; } }
  class Stamp extends Base { #data = {n: 23}; static read(o) { return o.#data.n; } }
  let stamped = new Stamp(s);
  row(ok && stamped === s && Stamp.read(s) === 23 && WeakSet.prototype.has.call(s, k) && WeakSet.prototype.delete.call(s, k));
}
// builtin_subclasses.rs lines 67, 73, 531 and 534: a subclass's adder runs
// for each entry, and the realm's methods read an instance whose prototype
// its adder took away.
{ let calls = 0; class W extends WeakMap { set(k, v) { calls++; return super.set(k, v + 1); } } let k = {}; let w = new W([[k, 2]]); row(w instanceof W && w.get(k) === 3 && calls === 1); }
{ let calls = 0; class W extends WeakSet { add(k) { calls++; return super.add(k); } } let k = {}; let w = new W([k]); row(w instanceof W && w.has(k) && calls === 1); }
{ class W extends WeakMap { set(k, v) { super.set(k, v); Object.setPrototypeOf(this, null); Object.preventExtensions(this); return this; } } let k = {}; let w = new W([[k, 2]]); row(Object.getPrototypeOf(w) === null && WeakMap.prototype.get.call(w, k) === 2); }
{ class W extends WeakSet { add(k) { super.add(k); Object.setPrototypeOf(this, null); Object.preventExtensions(this); return this; } } let k = {}; let w = new W([k]); row(Object.getPrototypeOf(w) === null && WeakSet.prototype.has.call(w, k)); }
// Identity keys: many objects and symbols, a deleted key and the same key
// again, keys that are functions, arrays and other collections; a
// WeakMap's prototype and tags.
{ let keys = [], m = new WeakMap(); for (let i = 0; i < 500; i++) { let k = i % 3 ? {i} : Symbol('k' + i); keys.push(k); m.set(k, i); } for (let i = 0; i < 500; i += 4) m.delete(keys[i]); let ok = true; for (let i = 0; i < 500; i++) { if (m.get(keys[i]) !== (i % 4 === 0 ? undefined : i) || m.has(keys[i]) !== (i % 4 !== 0)) ok = false; } row(ok && !m.has({i: 1}) && !m.has(Symbol('k0'))); }
{ let m = new WeakMap(); let f = function () {}, a = [], c = new Map(), w = new WeakSet(); m.set(f, 1).set(a, 2).set(c, 3).set(w, 4).set(m, 5); row(m.get(f) === 1 && m.get(a) === 2 && m.get(c) === 3 && m.get(w) === 4 && m.get(m) === 5 && !m.has([])); }
{ let s = new WeakSet(); for (let i = 0; i < 1000; i++) { let k = {}; s.add(k); s.delete(k); } let k = {}; s.add(k); row(s.has(k) && !s.has({})); }
{ let d = Object.getOwnPropertyDescriptor(WeakMap, 'prototype'); row(d.value === WeakMap.prototype && !d.writable && !d.enumerable && !d.configurable && WeakMap.prototype.constructor === WeakMap && Object.getPrototypeOf(WeakMap.prototype) === Object.prototype); }
for (let [p, tag] of [[WeakMap.prototype, 'WeakMap'], [WeakSet.prototype, 'WeakSet']]) { let d = Object.getOwnPropertyDescriptor(p, Symbol.toStringTag); row(d.value === tag && !d.writable && !d.enumerable && d.configurable && Object.prototype.toString.call(p) === '[object ' + tag + ']'); }
row(WeakSet.name === 'WeakSet' && WeakSet.prototype.constructor === WeakSet && name(() => new WeakSet(1)) === 'TypeError' && name(() => new WeakSet.prototype.add()) === 'TypeError');
failed === 0 ? rows : -failed;
