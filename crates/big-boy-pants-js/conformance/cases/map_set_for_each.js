// bbp-authored conformance case (light-cljs-944).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 21719-21766;
// map_for_each_live_iteration and map_bigint_for_each_deletes_all_keys (object
// keys only); rust/tests/sets.rs set_cursors_and_callbacks_observe_mutations;
// light-cljs-769's planner rows; ECMAScript 24.1.3.5 and 24.2.3.6.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && !failed) failed = rows; }
function name(f) { try { f(); return 'none'; } catch (e) { return e.name; } }
{
  let m = new Map([[1, 'a'], [2, 'b'], [3, 'c']]), out = [];
  let result = m.forEach((v, k, mm) => { out.push(k + v); if (k === 1) { mm.delete(2); mm.set(4, 'd'); } });
  row(result === undefined && [out.join(), [...m.keys()].join(), [...m.values()].join(), [...m].join('|'), Object.prototype.toString.call(m[Symbol.iterator]())].join(';') === '1a,3c,4d;1,3,4;a,c,d;1,a|3,c|4,d;[object Map Iterator]');
}
{
  let s = new Set(['a', 'b']), out = '';
  s.forEach(function (v, k, ss) { out += v + k + (ss === s) + this.t; if (v === 'a') ss.add('c'); }, {t: '!'});
  row(out === 'aatrue!bbtrue!cctrue!');
}
for (let C of [Map, Set]) {
  let make = () => C === Map ? new Map([[1, 10], [2, 20], [3, 30], [4, 40]]) : new Set([1, 2, 3, 4]);
  let c = make(), out = [];
  c.forEach((v, k, self) => { out.push(k); self.delete(k); }); row(out.join() === '1,2,3,4' && c.size === 0);
  c = make(); out = [];
  c.forEach((v, k) => { out.push(k); if (k === 1) { c.delete(2); c.delete(3); for (let i = 5; i < 20; i++) { if (C === Map) c.set(i, i); else c.add(i); } } });
  row(out.join() === '1,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19');
  c = make(); out = []; c.forEach((v, k) => { out.push(k); c.clear(); }); row(out.join() === '1');
  c = make(); out = []; c.forEach((v, k) => { out.push(k); if (k === 1) { c.clear(); if (C === Map) c.set(7, 70); else c.add(7); } }); row(out.join() === '1,7');
  c = make(); out = []; c.forEach((v, k) => { out.push(k); if (k === 1) { c.delete(1); if (C === Map) c.set(1, 11); else c.add(1); } if (out.length === 5) c.clear(); }); row(out.join() === '1,2,3,4,1');
  c = make(); let calls = 0, caught; try { c.forEach(() => { calls++; throw 17; }); } catch (e) { caught = e; } row(calls === 1 && caught === 17);
  for (let callback of [undefined, null, 0, {}, 'x']) row(name(() => new C().forEach(callback)) === 'TypeError');
  for (let bad of [undefined, null, {}, [], C.prototype, C === Map ? new Set() : new Map()]) row(name(() => C.prototype.forEach.call(bad, () => {})) === 'TypeError');
  let d = Object.getOwnPropertyDescriptor(C.prototype, 'forEach'); row(d.writable && !d.enumerable && d.configurable && d.value.name === 'forEach' && d.value.length === 1);
  row(name(() => new C.prototype.forEach()) === 'TypeError');
  c = make(); let nested = 0; c.forEach(() => { c.forEach(() => nested++); }); row(nested === 16);
  let that = {}; let ok = true; c.forEach(function (v, k, self) { 'use strict'; if (this !== that || self !== c || arguments.length !== 3 || (C === Set && v !== k)) ok = false; }, that); row(ok);
}
{ let a = {}, b = {}, m = new Map([[a, 1], [b, 2]]); m.forEach((v, k) => m.delete(k)); row(m.size === 0); }
failed ? -failed : rows;
