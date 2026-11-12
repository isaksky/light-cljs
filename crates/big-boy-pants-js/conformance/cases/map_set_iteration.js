// bbp-authored conformance case (light-cljs-944).
// Derives from: crates/big-boy-pants-js/rust/tests/map_iterables.rs lines 12-260;
// map_entries_are_iterable, map_iteration_observes_overrides_and_abrupt_completion;
// rust/tests/sets.rs set_cursors_and_callbacks_observe_mutations,
// set_iterator_next_enforces_brands; rust/tests/object_to_string.rs iterator rows;
// and light-cljs-769's planner rows. BigInt keys are light-cljs-771's.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && !failed) failed = rows; }
function name(f) { try { f(); return 'none'; } catch (e) { return e.name; } }
row(Map.prototype[Symbol.iterator] === Map.prototype.entries);
row(Set.prototype.keys === Set.prototype.values && Set.prototype[Symbol.iterator] === Set.prototype.values);
{
  let m = new Map([[1, 'a'], [2, 'b'], [3, 'c']]);
  row([...m.keys()].join() === '1,2,3' && [...m.values()].join() === 'a,b,c' && [...m].join('|') === '1,a|2,b|3,c');
  row([...new Map(m)].join('|') === [...m].join('|'));
  let [a, b, ...rest] = m;
  row(a.join() === '1,a' && b.join() === '2,b' && rest[0].join() === '3,c');
}
{
  let s = new Set([1, 2]); let it = s.values(); s.delete(1); s.add(3);
  row([...it].join() === '2,3' && [...new Set([5]).entries()].join('|') === '5,5');
}
for (let C of [Map, Set]) {
  let c = C === Map ? new Map([[1, 10], [2, 20], [3, 30], [4, 40]]) : new Set([1, 2, 3, 4]);
  let it = c.keys(); row(it[Symbol.iterator]() === it && it.next().value === 1);
  c.delete(1); c.delete(2); c.delete(3);
  for (let i = 5; i < 30; i++) { if (C === Map) c.set(i, i * 10); else c.add(i); }
  row(it.next().value === 4 && it.next().value === 5);
  c.clear(); if (C === Map) c.set(50, 500); else c.add(50);
  row(it.next().value === 50 && it.next().done);
  if (C === Map) c.set(60, 600); else c.add(60);
  row(it.next().done && it.next().value === undefined);
  let fresh = c.entries(); let a = fresh.next().value; let b = fresh.next().value;
  row(a !== b && Array.isArray(a) && a.length === 2 && a[0] === 50 && b[0] === 60);
  let p = Object.getPrototypeOf(fresh);
  row(Object.prototype.toString.call(fresh) === (C === Map ? '[object Map Iterator]' : '[object Set Iterator]'));
  row(Object.getPrototypeOf(p) === Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())));
  let tag = Object.getOwnPropertyDescriptor(p, Symbol.toStringTag);
  row(tag.configurable && !tag.writable && !tag.enumerable);
  row(p.next.length === 0 && p.next.name === 'next');
  for (let bad of [undefined, null, {}, [], c, [][Symbol.iterator](), ''[Symbol.iterator](), C === Map ? new Set().values() : new Map().entries()]) {
    row(name(() => p.next.call(bad)) === 'TypeError');
  }
  row(name(() => p.next.call(new Proxy(fresh, {}))) === 'TypeError');
  for (let method of ['keys', 'values', 'entries']) {
    let d = Object.getOwnPropertyDescriptor(C.prototype, method);
    row(d.writable && !d.enumerable && d.configurable && d.value.length === 0);
    for (let bad of [undefined, null, {}, [], C.prototype, Object.create(C.prototype), C === Map ? new Set() : new Map()]) {
      row(name(() => C.prototype[method].call(bad)) === 'TypeError');
    }
    row(name(() => new C.prototype[method]()) === 'TypeError');
  }
  row(name(() => Object.getPrototypeOf([][Symbol.iterator]()).next.call(fresh)) === 'TypeError');
}
{
  let m = new Map([[1, 1]]); let it = m.entries(); m.clear(); m.set(2, 2);
  let r = it.next(), r2 = it.next(); row([r.done, r.value, r2.done, r2.value].join() === 'false,2,2,true,');
}
{
  let m = new Map([[1, 'a'], [2, 'b']]); let it = m.values(); m.set(1, 'A');
  row(it.next().value === 'A'); m.delete(1); m.set(1, 'B'); row([...it].join() === 'b,B');
}
// An override is reached by the ordinary protocol; abrupt next propagates.
{ let m = new Map([[1, 2]]); m[Symbol.iterator] = function* () { yield [3, 4]; }; row(new Map(m).get(3) === 4); }
{ let m = new Map(); m[Symbol.iterator] = () => ({next() { throw 17; }}); let caught; try { [...m]; } catch (e) { caught = e; } row(caught === 17); }
failed ? -failed : rows;
