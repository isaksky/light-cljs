// bbp-authored conformance case (light-cljs-880).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 7148-7218:
// accessor_properties (line 7148), accessor_descriptors_and_integrity (7166)
// and accessors_in_native_operations (7201), their rows with literal
// accessors: own and inherited getters and setters, a getter-only property
// refusing a strict write, accessor and data halves replacing each other,
// accessor function names and descriptors, and getters run by
// Function.prototype.apply, Reflect.apply, Array.prototype.push and ToPrimitive.
// `Object.create` is a `__proto__` literal and `Object.keys` a for-in loop
// here. Left out: the rows using `Object.defineProperty` (light-cljs-881),
// freeze or preventExtensions (light-cljs-882), and `Object.getPrototypeOf`
// after an own `__proto__` accessor (light-cljs-883). Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// accessor_properties (line 7148): the literal-accessor rows.
{
  let a = {get z() { return 4; }, set z(v) { this.z_val = v; }}; let read = a.z; a.z = 5;
  row(read === 4 && a.z_val === 5);
}
{
  let a = {get: 2, set: 3, async: 4, get a() { return this.get; }};
  row(a.a === 2);
}
{
  let p = {get z() { return this.x; }, set z(v) { this.x = v; }}; let a = {__proto__: p}; a.x = 7; let read = a.z; a.z = 8;
  row(read === 7 && a.x === 8 && Object.getOwnPropertyDescriptor(a, 'z') === undefined);
}
{
  let a = {get z() { return 4; }}; let caught = false; try { a.z = 5; } catch(e) { caught = true; }
  row(caught && a.z === 4);
}
{
  let a = {get z() { throw 19; }}; let result = 0; try { a.z; } catch(e) { result = e; }
  row(result === 19);
}
{
  let a = {get z() { return 4; }, set z(v) { this.x = v; }}; let d = Object.getOwnPropertyDescriptor(a, 'z');
  row(d.get.name === 'get z' && d.set.name === 'set z' && d.enumerable && d.configurable && !('value' in d) && !('writable' in d) && !('prototype' in d.get));
}
{
  let a = {get z() { return 1; }, z: 9};
  row(a.z === 9 && Object.getOwnPropertyDescriptor(a, 'z').value === 9);
}
{
  let a = {z: 1, set z(v) { this.x = v; }};
  row(a.z === undefined && (a.z = 9) === 9 && a.x === 9);
}
// accessor_descriptors_and_integrity (line 7166): its literal-accessor rows.
{
  let o = {get \u0069f() { return this.x; }, set \u0069f(v) { this.x = v; }}; o.if = 7;
  row(o.if === 7 && Object.getOwnPropertyDescriptor(o, 'if').get.name === 'get if');
}
{
  let a = {get 'x'() { return 2; }, set 1(v) { this.x1 = v; }, get 1() { return this.x1; }}; a[1] = 3;
  row(a.x === 2 && a[1] === 3);
}
{
  let a = {get async() { return 3; }, get if() { return 4; }, get get() { return 5; }, get set() { return 6; }};
  row(a.async + a.if + a.get + a.set === 18);
}
{
  let a = {get z() { return 1; }, set z(v) { this.x = v; }, get z() { return 2; }}; a.z = 3;
  row(a.z === 2 && a.x === 3);
}
{
  let a = {set z(v) { this.x = v; }, z: 1, get z() { return 2; }};
  row(Object.getOwnPropertyDescriptor(a, 'z').set === undefined && a.z === 2);
}
{
  let n = 0; let a = {get z() { n++; return 1; }}; let listed = ''; for (let k in a) { listed += k; } let has = 'z' in a; let d = Object.getOwnPropertyDescriptor(a, 'z'); let removed = delete a.z;
  row(n === 0 && has && removed && listed === 'z' && d.get.name === 'get z' && !('z' in a));
}
{
  let o = {get z() { return 7; }}; let d = Object.getOwnPropertyDescriptor(o, 'z'); let caught = false; try { new d.get(); } catch(e) { caught = true; }
  row(caught && d.get.length === 0);
}
// accessors_in_native_operations (line 7201): the rows with literal accessors.
{
  let order = ''; let args = {get length() { order += 'l'; return 2; }, get 0() { order += 'a'; return 3; }, get 1() { order += 'b'; return 4; }}; let f = function(a, b) { return this.x + a + b; };
  row(f.apply({x: 2}, args) === 9 && order === 'lab');
}
{
  let args = {get length() { return {valueOf: () => 2}; }, get 0() { return 3; }, get 1() { return 4; }};
  row(Reflect.apply((a, b) => a + b, null, args) === 7);
}
{
  let p = {set 0(v) { this.x = v; }}; let a = {__proto__: p}; a.length = 0;
  row(Array.prototype.push.call(a, 7) === 1 && a.x === 7 && a.length === 1 && Object.getOwnPropertyDescriptor(a, 0) === undefined);
}
{
  let order = ''; let a = {get length() { order += 'g'; return 0; }, set length(v) { order += 's'; this.len = v; }, set 0(v) { order += 'a'; this.x = v; }};
  row(Array.prototype.push.call(a, 7) === 1 && a.x === 7 && a.len === 1 && order === 'gas');
}
{
  let o = {get valueOf() { return () => 7; }};
  row(+o === 7);
}
{
  let o = {get valueOf() { return 0; }, get toString() { return () => '8'; }};
  row(+o === 8);
}
failed === 0 ? rows : -failed;
