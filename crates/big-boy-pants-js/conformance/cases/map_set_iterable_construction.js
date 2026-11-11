// bbp-authored conformance case (light-cljs-943).
// Derives from: crates/big-boy-pants-js/rust/tests/map_iterables.rs lines 26-41
// (map_and_weak_map_accept_entry_iterables: its Map rows without
// BigInt keys), 44-78 (entry_failures_close_once_and_preserve_original_throw:
// its Map rows without a BigInt entry) and 81-111
// (iterator_protocol_failures_do_not_close: its Map rows); rust/tests/sets.rs
// lines 71-95 (iterable_errors_and_close: its Set rows); and the Map and Set
// rows of rust/tests/builtin_subclasses.rs lines 64 and 70; with
// light-cljs-769's planner rows: new Map(iterable) and new Set(iterable),
// AddEntriesFromIterable's order (the adder read once, before GetIterator,
// then per entry `next`, `done`, `value`, "0", "1" and the adder's call),
// IteratorClose on an abrupt completion after GetIterator but not on a
// failing `next`, `done` or `value`, a subclass's own `set` or `add`, and
// the built-in iteration of arrays that nothing observes. Left out: a Map
// or Set as the iterable (its iterators are light-cljs-944's), WeakMap and
// WeakSet (light-cljs-945) and BigInt keys (light-cljs-771). Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-769).
{ let log = ''; let it = { [Symbol.iterator]() { return { next() { return { done: false, value: 1 }; }, return() { log += 'closed'; return {}; } }; } }; try { new Map(it); } catch (e) { log += e.name; } row(log === "closedTypeError"); }
{ let calls = 0; class M extends Map { set(k, v) { calls++; return super.set(k, v + 1); } } let m = new M([[1, 2], [3, 4]]); row([m instanceof M, m instanceof Map, m.get(1), calls, m.size].join() === "true,true,3,2,2"); }
// map_and_weak_map_accept_entry_iterables (line 26), its Map rows.
row(new Map([[1, 2]]).get(1) === 2);
row(new Map().size === 0 && new Map(null).size === 0 && new Map(undefined).size === 0);
{ let m = new Map([[1, 2], [1, 3], [-0, 4], [NaN, 5]]); row(m.size === 3 && m.get(1) === 3 && m.get(0) === 4 && m.get(NaN) === 5); }
{ function* entries() { yield {0: 1, 1: 2}; yield [3]; yield {}; } let m = new Map(entries()); row(m.get(1) === 2 && m.has(3) && m.get(3) === undefined && m.has(undefined)); }
{ let B = Map.bind(null, [[1, 2]]); row(new B().get(1) === 2); }
row(new Map([]).size === 0 && new Map('').size === 0);
row(new Map([Object.create({0: 1, 1: 2})]).get(1) === 2);
{ let it = {[Symbol.iterator]() { return this; }, next() { return {done: true, get value() { throw 98; }}; }, get return() { throw 99; }}; row(new Map(it).size === 0); }
// entry_failures_close_once_and_preserve_original_throw (line 44), its Map rows.
for (let entry of [() => null, () => undefined, () => 1, () => 'ab', () => true, () => Symbol(), () => ({get 0() { throw 17; }, get 1() { throw 99; }}), () => ({0: {}, get 1() { throw 17; }})]) {
  let throws = entry() !== null && typeof entry() === 'object';
  for (let close of [() => ({}), () => { throw 99; }, () => 1]) {
    let n = 0, closed = 0, ok = false;
    let it = {[Symbol.iterator]() { return this; }, next() { n++; return {value: entry()}; }, return() { if (this !== it || arguments.length !== 0) throw 98; closed++; return close(); }};
    try { new Map(it); } catch (e) { ok = throws ? e === 17 : e instanceof TypeError; }
    row(ok && n === 1 && closed === 1);
  }
}
{ let closed = 0, ok = false; function* g() { try { yield 1; } finally { closed++; } } try { new Map(g()); } catch (e) { ok = e instanceof TypeError; } row(ok && closed === 1); }
// iterator_protocol_failures_do_not_close (line 81), its Map rows.
for (let make of [
  (log) => ({ next() { throw 17; }, return() { log.closed++; return {}; } }),
  (log) => ({ next() { return 1; }, return() { log.closed++; return {}; } }),
  (log) => ({ next() { return {get done() { throw 17; }}; }, return() { log.closed++; return {}; } }),
  (log) => ({ next() { return {get value() { throw 17; }}; }, return() { log.closed++; return {}; } }),
  (log) => ({ next: 0, return() { log.closed++; return {}; } }),
  (log) => ({ get next() { throw 17; }, return() { log.closed++; return {}; } }),
]) {
  let log = { closed: 0 }; let ok = false;
  let inner = make(log); inner[Symbol.iterator] = function () { return this; };
  try { new Map(inner); } catch (e) { ok = e === 17 || e instanceof TypeError; }
  row(ok && log.closed === 0);
}
// iterable_errors_and_close (sets.rs line 71), its Set rows.
for (let make of [
  (log) => ({ next() { throw 17; }, return() { log.closed++; return {}; } }),
  (log) => ({ next() { return {get done() { throw 17; }}; }, return() { log.closed++; return {}; } }),
  (log) => ({ next() { return {get value() { throw 17; }}; }, return() { log.closed++; return {}; } }),
]) {
  let log = { closed: 0 }; let ok = false;
  let inner = make(log); inner[Symbol.iterator] = function () { return this; };
  try { new Set(inner); } catch (e) { ok = e === 17; }
  row(ok && log.closed === 0);
}
// A Set's adder that throws closes the iterator, which keeps the throw.
for (let close of [() => ({}), () => { throw 99; }, () => 1]) {
  let closed = 0, caught;
  class S extends Set { add(v) { throw 17; } }
  let it = {[Symbol.iterator]() { return this; }, next() { return {value: 1}; }, return() { closed++; return close(); }};
  try { new S(it); } catch (e) { caught = e; }
  row(caught === 17 && closed === 1);
}
// IteratorClose reads `return` through a getter once: a throwing getter or
// a returned method that throws leaves the original exception, and a
// returned method is called with the iterator.
for (let C of [Map, Set]) {
  class Bad extends C {} if (C === Set) Bad.prototype.add = function () { throw new TypeError("add"); };
  for (let make of [() => { throw 99; }, () => function () { throw 98; }, () => function () { closed++; return {}; }]) {
    var closed = 0; let reads = 0, caught, self;
    let it = {[Symbol.iterator]() { return this; }, next() { return {value: 1}; }, get return() { reads++; self = this; return make(); }};
    try { new Bad(it); } catch (e) { caught = e; }
    row(caught instanceof TypeError && reads === 1 && self === it);
  }
  row(closed === 1);
}
// The adder: read once from the new object, before GetIterator; a
// non-callable one is a TypeError that nothing closes.
{
  let log = [];
  class M extends Map { get set() { log.push('set'); return function (k, v) { log.push('call ' + k); return Map.prototype.set.call(this, k, v); }; } }
  let it = {[Symbol.iterator]() { log.push('iterator'); let i = 0; return {get next() { log.push('next'); return function () { i++; return i > 2 ? {done: true} : {value: [i, i * 10]}; }; }}; }};
  let m = new M(it);
  row(log.join() === "set,iterator,next,call 1,call 2" && m.get(2) === 20 && m.size === 2);
}
{ let opened = false; class M extends Map {} M.prototype.set = 1; let it = {[Symbol.iterator]() { opened = true; return {next() { return {done: true}; }}; }}; row(name(() => new M(it)) === "TypeError" && !opened && name(() => new M()) === "none"); }
{ class S extends Set {} S.prototype.add = undefined; row(name(() => new S([1])) === "TypeError" && new S().size === 0 && name(() => new S(null)) === "none"); }
// An entry's "0" and "1" are read in order, getters and all.
{ let log = ''; let entry = { get 0() { log += 'k'; return 'a'; }, get 1() { log += 'v'; return 'b'; } }; let m = new Map([entry, entry]); row(log === 'kvkv' && m.get('a') === 'b' && m.size === 1); }
{ let p = new Proxy([5, 6], {}); row(new Map([p]).get(5) === 6); }
// The protocol's `done` and `value` are read once each, in order.
{
  let log = ''; let i = 0;
  let it = {[Symbol.iterator]() { return this; }, next() { i++; return { get done() { log += 'd' + i; return i > 2; }, get value() { log += 'v' + i; return [i, i]; } }; }};
  let m = new Map(it);
  row(log === 'd1v1d2v2d3' && m.size === 2);
}
// Arrays iterate with the realm's iterator: holes read through the
// prototype chain and an array's own @@iterator is used (a changed
// %Array.prototype%[@@iterator] or %ArrayIteratorPrototype%.next is
// maps_and_sets_quickjs's: the Pants engine refuses those writes).
{ let s = new Set([1, , 3]); row(s.size === 3 && s.has(undefined)); }
{ let a = [[1, 2], , [3, 4]]; row(name(() => new Map(a)) === "TypeError"); }
{ let a = [1, 2, 3]; a[Symbol.iterator] = function* () { yield 9; }; row(new Set(a).size === 1 && new Set(a).has(9)); }
{ let arr = [[1, 'a']]; let m = new Map(arr); arr.push([2, 'b']); row(m.size === 1 && new Map(arr).size === 2); }
{ let big = []; for (let i = 0; i < 2000; i++) big.push([i, 'v' + i]); let m = new Map(big); let ok = m.size === 2000; for (let i = 0; i < 2000; i += 97) { if (m.get(i) !== 'v' + i) ok = false; } row(ok); }
{ let big = []; for (let i = 0; i < 2000; i++) big.push(i % 500); let s = new Set(big); row(s.size === 500 && s.has(499) && !s.has(500)); }
// Subclasses (builtin_subclasses.rs lines 64 and 70).
{ class M extends Map {} let m = new M([[1, 2]]); row(m instanceof M && m instanceof Map && Object.getPrototypeOf(m) === M.prototype && m.get(1) === 2 && Map.prototype.get.call(m, 1) === 2); }
{ class S extends Set {} let s = new S([1, 1, 2]); row(s instanceof S && s instanceof Set && s.size === 2 && Set.prototype.has.call(s, 2)); }
{ class S extends Set { constructor(xs) { super(xs); this.tag = 's'; } } let s = new S([3]); row(s.tag === 's' && s.has(3)); }
{ let log = []; function F() {} let bound = F.bind(); Object.defineProperty(bound, 'prototype', { get() { log.push('proto'); return Set.prototype; } }); let s = Reflect.construct(Set, [[1]], bound); row(log.join() === 'proto' && s instanceof Set && s.has(1)); }
failed === 0 ? rows : -failed;
