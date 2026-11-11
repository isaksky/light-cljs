// bbp-authored conformance case (light-cljs-943).
// Derives from: crates/big-boy-pants-js/rust/tests/sets.rs line 33
// (constructors_and_membership's Set.length row) and light-cljs-769's
// QuickJS-only planner row:
// Map.length and Set.length are 0 (ECMAScript 24.1.2, 24.2.2) and
// Map[@@species] and Set[@@species] are getters returning `this`
// (24.1.2.2, 24.2.2.2); and new Map(array) and new Set(array) call a
// changed %Array.prototype%[@@iterator] or %ArrayIteratorPrototype%.next
// (collections.goose: the fast path then gives way). The Pants engine
// gives Map.length 1, has no @@species getters and refuses writes to its
// built-in objects, so areas.edn scores this case against QuickJS alone
// (:pants-gap). Each row is a `row(...)` check; the script completes with
// the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
row([Map.length, Set.length, Map[Symbol.species] === Map, Set[Symbol.species] === Set].join() === "0,0,true,true");
{ let d = Object.getOwnPropertyDescriptor(Map, Symbol.species); row(typeof d.get === 'function' && d.set === undefined && !d.enumerable && d.configurable && d.get.name === 'get [Symbol.species]' && d.get.length === 0); }
{ let d = Object.getOwnPropertyDescriptor(Set, Symbol.species); row(typeof d.get === 'function' && d.set === undefined && !d.enumerable && d.configurable && d.get.name === 'get [Symbol.species]'); }
{ let o = {}; row(Object.getOwnPropertyDescriptor(Map, Symbol.species).get.call(o) === o && Object.getOwnPropertyDescriptor(Set, Symbol.species).get.call(1) === 1); }
{ class M extends Map {} class S extends Set {} row(M[Symbol.species] === M && S[Symbol.species] === S); }
{ let d = Object.getOwnPropertyDescriptor(Map, 'length'); row(d.value === 0 && !d.writable && !d.enumerable && d.configurable); }
// The realm's array iteration changed after the fast path's start checks:
// a changed method runs.
{
  let saved = Array.prototype[Symbol.iterator]; let calls = 0;
  Array.prototype[Symbol.iterator] = function () { calls++; return saved.call(this); };
  let s = new Set([1, 2]);
  Array.prototype[Symbol.iterator] = saved;
  row(calls === 1 && s.size === 2);
}
{
  let proto = Object.getPrototypeOf([][Symbol.iterator]()); let saved = proto.next; let calls = 0;
  proto.next = function () { calls++; return saved.call(this); };
  let m = new Map([[1, 1], [2, 2]]);
  proto.next = saved;
  row(calls === 3 && m.size === 2);
}
failed === 0 ? rows : -failed;
