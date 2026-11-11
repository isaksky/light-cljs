// bbp-authored conformance case (light-cljs-943).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 21698-21716
// (map_number_and_object_keys), 21769-21826 (map_receivers_properties_and_explicit_gaps,
// without its Map.length row, which QuickJS answers otherwise), 21830-21880
// (map_ordinary_property_integrity), 21990-22013 (map_symbol_keys_and_tag) and
// 3940-3962 (rope_strings_are_javascript_map_keys, its strings built by `+`
// since String.fromCharCode is light-cljs-903's); rust/tests/sets.rs lines
// 25-45 (constructors_and_membership: its Set rows without BigInt, WeakSet
// and Set.length), 59-68 (brands_and_invalid_inputs: its Map and Set rows,
// without the iterator methods and forEach) and 170-185
// (sets_preserve_ordinary_object_protocols); with light-cljs-769's planner
// rows: Map and Set objects, SameValueZero keys with a hash index, the keyed
// methods and `size`, their receivers, their ordinary properties and tags.
// Left out: iteration and forEach (light-cljs-944), WeakMap and WeakSet
// (light-cljs-945), BigInt keys (light-cljs-771), Map.groupBy, getOrInsert
// and the Set methods (light-cljs-953), and Map.length, Set.length and
// @@species, which the Pants engine answers otherwise
// (maps_and_sets_quickjs). Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-769).
{ let m = new Map(); m.set(-0, 'z'); m.set(NaN, 1); let o = {}; m.set(o, 2); row([m.get(0), m.get(NaN), m.get(o), m.get({}), m.size, m.has(-0), m.delete(o), m.delete(o), m.set(1, 2) === m, m.size, m.set('1', 3).size, m.get(1)].join() === "z,1,2,,3,true,true,false,true,3,4,2"); }
{ let s = new Set([1, 2, 2, '2', NaN, NaN]); row([s.size, s.has('2'), s.has(NaN), s.add(3) === s, s.delete(2), s.size, s.has(2)].join() === "4,true,true,true,true,4,false"); }
row(name(() => Map()) === "TypeError" && name(() => new Map([1])) === "TypeError" && name(() => Map.prototype.get.call({}, 1)) === "TypeError" && name(() => Set.prototype.add.call(new Map(), 1)) === "TypeError" && name(() => new Set(1)) === "TypeError");
{ let m = new Map(); m.x = 1; Object.freeze(m); m.set(1, 1); let c = name(() => { "use strict"; m.x = 2; }); row([Object.isFrozen(m), m.x, c, m.size, Reflect.ownKeys(m).join()].join() === "true,1,TypeError,1,x"); }
row([Object.prototype.toString.call(new Map()), Object.prototype.toString.call(new Set()), typeof Object.getOwnPropertyDescriptor(Map.prototype, 'size').get, Map.prototype[Symbol.toStringTag], Set.prototype.add.name, Map.prototype.set.length].join() === "[object Map],[object Set],function,Map,add,2");
// map_number_and_object_keys (line 21698).
row(name(() => Map()) === "TypeError");
{ let m = new Map(); for (let i = 0; i < 1000; i++) m.set(i, i); let ok = m.size === 1000; for (let i = 0; i < 1000; i++) { if (m.get(i) !== i) ok = false; } m.set(-2147483648, 1); row(ok && m.get(-2147483648) === 1 && m.get(-2147483647 - 1) === 1 && m.get(-2147483647.5 - 0.5) === 1); }
{ let m = new Map(); m.set(-0, 'z'); m.set(NaN, 1); row(m.get(0) === 'z' && m.get(NaN) === 1 && m.size === 2 && m.has(-0) && m.has(0 / 0)); }
{ let m = new Map(); let keys = []; let values = []; for (let i = 0; i < 1000; i++) { let k = {id: i}; let v = {}; keys.push(k); values.push(v); m.set(k, v); } let ok = m.size === 1000; for (let i = 0; i < 1000; i++) { if (m.get(keys[i]) !== values[i]) ok = false; } row(ok && m.get({id: 0}) === undefined); }
{ let m = new Map(); row(m.set(1, 2) === m && m.delete(1) === true && m.delete(1) === false && !m.has(1) && m.get(1) === undefined && m.size === 0); }
{ let m = new Map(); m.set(1, 2); m.set(1, 3); let ok = m.size === 1 && m.get(1) === 3; m.set(2, 4); row(ok && m.clear() === undefined && m.size === 0 && !m.has(1) && !m.has(2) && m.set(1, 5) === m && m.size === 1 && m.get(1) === 5); }
{ let m = new Map(); m.set(undefined, 1); m.set(null, 2); m.set(true, 3); m.set(false, 4); m.set('a' + 'b', 5); row(m.size === 5 && m.get() === 1 && m.get(null) === 2 && m.get(true) === 3 && m.get(false) === 4 && m.get('ab') === 5 && !m.has('missing')); }
row(new Map(undefined).size === 0 && new Map(null).size === 0 && new Map().size === 0);
{ let caught = false; try { Map(); } catch (e) { caught = true; } row(caught); }
// map_receivers_properties_and_explicit_gaps (line 21769), without Map.length.
{ let m = new Map(); row(m instanceof Map && Object.getPrototypeOf(m) === Map.prototype && m.constructor === Map && Map.name === 'Map' && typeof m === 'object'); }
{ let d = Object.getOwnPropertyDescriptor(Map, 'prototype'); row(d.value === Map.prototype && !d.writable && !d.enumerable && !d.configurable); }
{ let d = Object.getOwnPropertyDescriptor(Map.prototype, 'size'); row(typeof d.get === 'function' && d.set === undefined && !d.enumerable && d.configurable && d.get.name === 'get size' && d.get.length === 0); }
{ let m = new Map(); m.foo = 7; m[1] = 8; row(m.size === 0 && m.foo === 7 && m[1] === 8 && 'size' in m && Object.keys(m).length === 2 && delete m.foo && !('foo' in m)); }
{ let m = new Map(); Object.defineProperty(m, 'foo', {value: 9}); row(m.foo === 9 && Object.getOwnPropertyDescriptor(m, 'foo').value === 9 && Object.getOwnPropertyDescriptor(m, 'size') === undefined); }
{ let m = new Map(); Object.freeze(m); m.set(1, 2); row(Object.isFrozen(m) && !Object.isExtensible(m) && m.get(1) === 2 && m.size === 1); }
{ let m = new Map(); let p = {x: 4}; Object.setPrototypeOf(m, p); row(m.x === 4 && Object.getPrototypeOf(m) === p && Map.prototype.set.call(m, 1, 2) === m && Map.prototype.get.call(m, 1) === 2); }
{ let m = new Map(); let o = {get valueOf() { throw 1; }, get toString() { throw 2; }}; m.set(o, 9); row(m.get(o) === 9 && m.has(o) && m.delete(o)); }
{ let m = new Map(); m.set(); row(m.has(undefined) && m.size === 1 && m.get() === undefined && m.delete() && !m.has()); }
{ let m = new Map(); m.set(0, 'a'); m.set(-0, 'b'); m.set(NaN, 1); m.set(0/0, 2); row(m.size === 2 && m.get(0) === 'b' && m.get(NaN) === 2 && m.delete(-0) && !m.has(0)); }
for (let [method, length] of [['set', 2], ['get', 1], ['has', 1], ['delete', 1], ['clear', 0]]) {
  let d = Object.getOwnPropertyDescriptor(Map.prototype, method);
  row(d.writable && !d.enumerable && d.configurable && d.value.name === method && d.value.length === length);
  let receivers = [undefined, null, 1, 'x', {}, [], Map.prototype, Object.create(Map.prototype), new Set()];
  let errors = 0; for (let r of receivers) { if (name(() => Map.prototype[method].call(r, 1, 2)) === "TypeError") errors++; }
  row(errors === receivers.length);
  row(name(() => new Map.prototype[method]()) === "TypeError");
}
row(name(() => new Map(1)) === "TypeError" && name(() => new Map({})) === "TypeError" && name(() => Map.prototype.size) === "TypeError" && name(() => Object.getOwnPropertyDescriptor(Map.prototype, 'size').get.call({})) === "TypeError" && name(() => { "use strict"; let m = new Map(); m.size = 2; }) === "TypeError");
// map_ordinary_property_integrity (line 21830).
for (let operation of ['preventExtensions', 'freeze']) {
  { let m = new Map(); m.x = 1; Object[operation](m); row(Object.setPrototypeOf(m, Map.prototype) === m && Object.getPrototypeOf(m) === Map.prototype); }
  for (let action of [(m) => { m.y = 2; }, (m) => { Object.defineProperty(m, 'y', {value: 2}); }, (m) => { Object.setPrototypeOf(m, {}); }]) {
    let m = new Map(); m.x = 1; Object[operation](m);
    row(name(() => { "use strict"; action(m); }) === "TypeError" && m.x === 1 && !('y' in m) && Object.getPrototypeOf(m) === Map.prototype);
  }
  { let m = new Map(); m.x = 1; m.set('entry', 1); Object[operation](m); m.set('entry', 2); let updated = m.get('entry') === 2 && m.size === 1; m.set('new', 3); let added = m.get('new') === 3 && m.size === 2; let removed = m.delete('entry') && !m.has('entry') && m.size === 1; m.clear(); row(updated && added && removed && m.size === 0 && !m.has('new') && m.x === 1); }
}
{ let m = new Map(); m.x = 1; row(Object.isExtensible(m) && Object.preventExtensions(m) === m && !Object.isExtensible(m) && !Object.isFrozen(m)); }
{ let m = new Map(); m.x = 1; Object.preventExtensions(m); m.x = 2; let d = Object.getOwnPropertyDescriptor(m, 'x'); row(m.x === 2 && d.value === 2 && d.writable && d.enumerable && d.configurable); }
{ let m = new Map(); m.x = 1; Object.preventExtensions(m); Object.defineProperty(m, 'x', {value: 3, enumerable: false}); let d = Object.getOwnPropertyDescriptor(m, 'x'); row(m.x === 3 && d.writable && !d.enumerable && d.configurable); }
{ let m = new Map(); m.x = 1; Object.preventExtensions(m); row(delete m.x && !('x' in m) && Object.getOwnPropertyDescriptor(m, 'x') === undefined); }
{ let m = new Map(); m.x = 1; let d = Object.getOwnPropertyDescriptor(m, 'x'); row(d.writable && d.configurable && Object.freeze(m) === m && !Object.isExtensible(m) && Object.isFrozen(m)); }
{ let m = new Map(); m.x = 1; Object.freeze(m); let d = Object.getOwnPropertyDescriptor(m, 'x'); row(m.x === 1 && d.value === 1 && !d.writable && d.enumerable && !d.configurable); }
{ let m = new Map(); m.x = 1; Object.freeze(m); row(Object.defineProperty(m, 'x', {value: 1}) === m && m.x === 1); }
// map_symbol_keys_and_tag (line 21990).
for (let r of [new Map(), Map.prototype, Object.create(Map.prototype), Object.create(Object.create(Map.prototype))]) row(Object.prototype.toString.call(r) === "[object Map]");
{ let m = new Map(); Object.setPrototypeOf(m, null); row(Object.prototype.toString.call(m) === '[object Object]'); }
{ let m = new Map(); let a = Symbol('key'); let b = Symbol('key'); m.set(a, 1); m.set(b, 2); m.set('Symbol(key)', 3); row(m.size === 3 && m.get(a) === 1 && m.get(b) === 2 && m.get('Symbol(key)') === 3 && !m.has(Symbol('key'))); }
{ let m = new Map(); let s = Symbol(); m.set(s, 1); m.set(s, 2); row(m.size === 1 && m.get(s) === 2 && m.delete(s) && !m.delete(s) && !m.has(s)); }
{ let m = new Map(); m.set(Symbol.iterator, 1); m.set(Symbol.toStringTag, 2); row(m.get(Symbol.iterator) === 1 && m.get(Symbol.toStringTag) === 2 && m.size === 2); }
row(typeof Symbol.toStringTag === 'symbol' && Symbol.toStringTag !== Symbol('Symbol.toStringTag') && Map.prototype[Symbol.toStringTag] === 'Map' && new Map()[Symbol.toStringTag] === 'Map');
{ let d = Object.getOwnPropertyDescriptor(Map.prototype, Symbol.toStringTag); row(d.value === 'Map' && !d.writable && !d.enumerable && d.configurable); }
{ let m = new Map(); let r = Symbol.for('registered'); m.set(r, 1); row(m.get(Symbol.for('registered')) === 1 && m.has(r)); }
// rope_strings_are_javascript_map_keys (line 3940): one text built from
// either end and as one literal.
{
  let appended = ''; let prepended = ''; const letters = 'abcdefghijklmnopqrstuvwxyz0123456789';
  for (let i = 0; i < letters.length; i++) { appended += letters[i]; prepended = letters[letters.length - 1 - i] + prepended; }
  let flat = 'abcdefghijklmnopqrstuvwxyz0123456789';
  let m = new Map(); m.set(appended, 1);
  let found = m.get(prepended) === 1 && m.has(flat);
  m.set(flat, 2);
  row(found && m.size === 1 && m.get(prepended) === 2 && m.delete(prepended) && !m.has(appended) && m.size === 0);
}
{ let m = new Map(); m.set('é', 1); m.set('Ā', 2); m.set('😀', 3); row(m.get('é') === 1 && m.get('Ā') === 2 && m.get('😀') === 3 && !m.has('e') && m.size === 3); }
// Insertion, deletion and growth: a map that sets and deletes keys keeps
// answering, and a set's entries are SameValueZero.
{ let m = new Map(); for (let i = 0; i < 5000; i++) { m.set('k' + i, i); if (i % 3 === 0) m.delete('k' + (i - 1)); } let ok = true; for (let i = 0; i < 5000; i++) { let want = i % 3 === 2 ? undefined : i; if (m.get('k' + i) !== want) ok = false; } row(ok && m.size === 5000 - 1666); }
{ let m = new Map(); for (let i = 0; i < 10000; i++) { m.set(7, i); m.delete(7); } m.set(7, 'x'); row(m.size === 1 && m.get(7) === 'x'); }
{ let m = new Map(); for (let i = 0; i < 100; i++) { m.set(i, i); if (i % 10 === 9) m.clear(); } row(m.size === 0 && !m.has(5) && m.set(5, 5).get(5) === 5); }
// A populated collection's entries are no properties: its index keys are
// ordinary ones, and reading, defining or deleting them leaves the entries.
{ let m = new Map([["k", "v"], ["j", "w"]]); let s = new Set(["a"]); row(m[0] === undefined && m[1] === undefined && s[0] === undefined && !(0 in m) && !(0 in s) && Object.getOwnPropertyDescriptor(m, 0) === undefined && Object.keys(m).length === 0 && Reflect.ownKeys(s).length === 0); }
{ let m = new Map([["k", "v"]]); m[0] = "x"; let d = Object.getOwnPropertyDescriptor(m, "0"); row(m[0] === "x" && d.value === "x" && d.writable && d.enumerable && m.get("k") === "v" && m.size === 1 && delete m[0] && m[0] === undefined && m.has("k") && m.size === 1 && Object.keys(m).length === 0); }
{ let s = new Set(["a", "b"]); let ok = delete s[0] && delete s[1] && s.has("a") && s.has("b") && s.size === 2; Object.defineProperty(s, 1, {value: 1}); row(ok && Reflect.ownKeys(s).join() === "1" && s[1] === 1 && s.has("b") && s.size === 2); }
// constructors_and_membership (sets.rs line 25), without BigInt and WeakSet.
row(typeof Set === 'function' && new Set([1, 2, 2]).size === 2);
{ let s = new Set([NaN, NaN, -0, 0, 1]); row(s.size === 3 && s.has(NaN) && s.has(0) && s.add(7) === s && s.delete(1) && !s.delete(1)); }
{ let s = new Set('a😀a'); row(s.size === 2 && s.has('😀') && new Set(null).size === 0 && new Set(undefined).size === 0); }
{ function* g() { yield 2; yield 3; yield 2; } let s = new Set(g()); row(s.size === 2 && s.has(3)); }
{ let C = Set.bind(null, [1, 2]); row(new C().size === 2); }
{ let s = new Set([1]); row(s.clear() === undefined && s.size === 0 && !s.has(1)); }
row(Set.prototype.add.length === 1 && Set.prototype.has.length === 1 && Set.prototype.delete.length === 1 && Set.prototype.clear.length === 0);
{ let s = new Set(); s.x = 4; row(Object.keys(s).join() === 'x' && Object.getOwnPropertyDescriptor(s, 'x').value === 4 && delete s.x && Object.getPrototypeOf(s) === Set.prototype && s instanceof Set); }
// brands_and_invalid_inputs (sets.rs line 59): the Map and Set rows.
{ let n = 0; try { Set(); } catch (e) { if (e instanceof TypeError) n++; } row(n === 1); }
{ let n = 0; for (let v of [1, true, {}]) { try { new Set(v); } catch (e) { if (e instanceof TypeError) n++; } } row(n === 3); }
{ let n = 0; for (let f of [Set.prototype.add, Set.prototype.has, Set.prototype.delete, Set.prototype.clear]) { try { f.call(new Map(), () => 0); } catch (e) { if (e instanceof TypeError) n++; } } row(n === 4); }
{ let n = 0; try { Map.prototype.has.call(new Set(), 1); } catch (e) { if (e instanceof TypeError) n++; } row(n === 1); }
// sets_preserve_ordinary_object_protocols (sets.rs line 170).
{ let s = new Set([1]); Object.freeze(s); s.add(2); s.delete(1); row(Object.isFrozen(s) && !Object.isExtensible(s) && s.size === 1 && s.has(2)); }
row(Object.prototype.toString.call(new Set()) === '[object Set]' && Set.prototype[Symbol.toStringTag] === 'Set' && Object.prototype.toString.call(Set.prototype) === '[object Set]');
{ let d = Object.getOwnPropertyDescriptor(Set.prototype, 'size'); row(typeof d.get === 'function' && d.set === undefined && d.get.name === 'get size' && name(() => Set.prototype.size) === "TypeError" && name(() => d.get.call(new Map())) === "TypeError"); }
{ let d = Object.getOwnPropertyDescriptor(Set, 'prototype'); row(d.value === Set.prototype && !d.writable && Set.prototype.constructor === Set && Set.name === 'Set'); }
failed === 0 ? rows : -failed;
