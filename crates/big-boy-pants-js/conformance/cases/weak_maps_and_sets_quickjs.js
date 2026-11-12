// bbp-authored conformance case (light-cljs-945).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs line 26599
// (its WeakMap.length) and light-cljs-769's QuickJS-only planner row:
// WeakMap.length and WeakSet.length are 0 (ECMAScript 24.3.2, 24.4.2); and
// new WeakMap(array) and new WeakSet(array) read a `return` the script put
// on %IteratorPrototype% when a key they cannot hold closes the array's
// iterator (collections.goose: the fast path gives way to the protocol at
// that element), and call a changed %ArrayIteratorPrototype%.next. The
// Pants engine gives WeakMap.length 1 and refuses writes to its built-in
// objects, so areas.edn scores this case against QuickJS alone
// (:pants-gap). Each row is a `row(...)` check; the script completes with
// the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
row([WeakMap.length, WeakSet.length].join() === "0,0");
{ let d = Object.getOwnPropertyDescriptor(WeakMap, 'length'); row(d.value === 0 && !d.writable && !d.enumerable && d.configurable); }
// A key a weak collection cannot hold, from an array: the TypeError closes
// the array's iterator, whose `return` comes from %IteratorPrototype%.
{
  let proto = Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())); let closed = 0, caught;
  proto.return = function () { closed++; return {}; };
  let k = {};
  try { new WeakMap([[k, 1], [2, 2], [k, 3]]); } catch (e) { caught = e; }
  let first = caught instanceof TypeError && closed === 1; caught = undefined;
  try { new WeakSet([k, Symbol.for('r'), k]); } catch (e) { caught = e; }
  delete proto.return;
  row(first && caught instanceof TypeError && closed === 2);
}
{
  let proto = Object.getPrototypeOf([][Symbol.iterator]()); let saved = proto.next; let calls = 0;
  proto.next = function () { calls++; return saved.call(this); };
  let a = {}, b = Symbol('b');
  let m = new WeakMap([[a, 1], [b, 2]]); let s = new WeakSet([a, b]);
  proto.next = saved;
  row(calls === 6 && m.get(a) === 1 && m.get(b) === 2 && s.has(a) && s.has(b));
}
failed === 0 ? rows : -failed;
