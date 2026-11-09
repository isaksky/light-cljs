// bbp-authored conformance case (light-cljs-880).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 1687-1743:
// computed_property_names (line 1687, bbp-233): computed keys in data
// properties, methods, accessors and generator methods, key conversion order
// and abrupt keys, data and accessor halves replacing each other, a computed
// `__proto__` key is an own property, and the function names SetFunctionName
// gives (light-cljs-855), with a few more such names (string, numeric,
// accessor and well-known-symbol keys). The `__rt.effect` order row is a
// local log here; `Object.keys` order is read with for-in. Left out: the rows
// with `Symbol()` keys (light-cljs-885) and a computed class value
// (light-cljs-886). Each row is a `row(...)` check; the script completes with
// the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// computed_property_names (line 1687, bbp-233).
{
  let k = 'x';
  row(({[k]: 1}).x === 1);
}
row(({[1 + 1]: 'two'})['2'] === 'two');
row(({['m']() { return this.v; }, v: 7}).m() === 7);
{
  let n = 0; function key() { throw 17; } try { ({[key()]: ++n}); } catch(e) { n += e; }
  row(n === 17);
}
{
  let log = ''; let key = {toString() { log += 'k'; return 'x'; }}; function value() { log += 'v'; return 7; } let o = {[key]: value()};
  row(log === 'kv' && o.x === 7);
}
{
  let n = 0; let key = {toString() { throw 23; }}; try { ({[key]: ++n}); } catch(e) { n += e; }
  row(n === 23);
}
{
  let o = {[2]() {}};
  row(o[2].name === '2');
}
{
  let o = {get ['x']() { return 1; }, ['x']: 7}; let d = Object.getOwnPropertyDescriptor(o, 'x');
  row(o.x === 7 && d.writable && d.enumerable && d.configurable && d.get === undefined);
}
{
  let o = {['x']: 7, get ['x']() { return 2; }};
  row(o.x === 2 && Object.getOwnPropertyDescriptor(o, 'x').set === undefined);
}
{
  let p = {set x(v) { throw 1; }}; let o = {__proto__: p, ['x']: 7, ['__proto__']: 9};
  row(o.x === 7 && o.__proto__ === 9 && Object.getPrototypeOf(o) === p);
}
{
  let o = {['f']: function() {}, ['g']: () => 1, ['h']: function named() {}};
  row(o.f.name === 'f' && o.g.name === 'g' && o.h.name === 'named');
}
{
  let f = function original() {}; let o = {['x']: f};
  row(o.x.name === 'original');
}
{
  let o = {['m']() {}}; let d = Object.getOwnPropertyDescriptor(o.m, 'name');
  row(d.value === 'm' && !d.writable && !d.enumerable && d.configurable && !('prototype' in o.m));
}
{
  let k = 'x'; function f() { let o = {[k]() { return k; }}; return o.x(); }
  row(f() === 'x');
}
{
  let o = {*['g']() { yield this.v; }, v: 7};
  row(o.g().next().value === 7 && o.g.name === 'g');
}
{
  function make(k) { return {[k]: () => 1}; } let a = make('a'), b = make('b');
  row(a.a.name === 'a' && b.b.name === 'b');
}
{
  let log = ''; let k = {toString() { log += 's'; return {}; }, valueOf() { log += 'n'; return 'm'; }}; let o = {[k]() { return 7; }};
  row(o.m() === 7 && o.m.name === 'm' && log === 'sn');
}
{
  let o = {['b']: 1, a: 2, ['b']: 3, [2]: 4, [1]: 5}; let s = ''; for (let k in o) s += k + ',';
  row(s === '1,2,b,a,' && o.b === 3);
}
{
  let o = {['f']: function() {}}; o.f.extra = 7; let d = Object.getOwnPropertyDescriptor(o.f, 'name');
  row(d.value === 'f' && !d.writable && !d.enumerable && d.configurable);
}
{
  function* make() { return {[yield 'key']: yield 7}; } let it = make();
  row(it.next().value === 'key' && it.next('x').value === 7 && it.next(9).value.x === 9);
}
{
  let k = {toString() { return {}; }, valueOf() { return {}; }};
  row((name(() => ({ [k]: 1 }))) === "TypeError");
}
{
  let log = []; function e(n) { log.push(n); return n; } ({[e(1)]: e(2), [e(3)]: e(4)});
  row(log.join() === '1,2,3,4');
}
// The function names SetFunctionName gives (light-cljs-855): computed keys of every kind.
{
  let o = {['a' + 'b']: function () {}, [3]: () => 0, [1.5]: function () {}};
  row(o.ab.name === 'ab' && o[3].name === '3' && o[1.5].name === '1.5');
}
{
  let o = {get ['g' + 1]() { return 1; }, set ['s' + 1](v) {}};
  row(Object.getOwnPropertyDescriptor(o, 'g1').get.name === 'get g1' && Object.getOwnPropertyDescriptor(o, 's1').set.name === 'set s1');
}
{
  let o = {[Symbol.iterator]() {}, [Symbol.toPrimitive]: function () {}};
  row(o[Symbol.iterator].name === '[Symbol.iterator]' && o[Symbol.toPrimitive].name === '[Symbol.toPrimitive]');
}
failed === 0 ? rows : -failed;
