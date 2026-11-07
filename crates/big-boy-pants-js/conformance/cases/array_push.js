// bbp-authored conformance case.
// Derives from: rust/tests/js_source.rs lines 18509-18627 (`array_push`; light-cljs-842,
// bbp-155; ECMAScript 23.1.3.23, QuickJS's js_array_push): several items and
// the return value, `[].push()`, holes kept, array-like receivers and
// ToLength of their `length` ('1.9', -2, NaN, Infinity, 2^53 - 2), the
// 2^53 - 1 TypeError before any write, an inherited `length`, a push after a
// sparse write, null and undefined receivers, a String object's refused
// `length`, and a `length` getter, its valueOf and setters on an index and
// on `length` observed in order. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's number.
//
// Rows of `array_push` that wait for the objects area (light-cljs-766):
// those using Object.defineProperty (a non-writable index or `length`),
// Object.create, Object.preventExtensions or Object.freeze, and
// `a.length = 4294967295` (dense runs refuse it). `{__proto__: ...}` stands
// in for Object.create, and holes come from `Array(n)` because array
// literal holes (the append_hole opcode) are not implemented on Goose yet.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function throwsType(f) { try { f(); } catch (e) { return e instanceof TypeError; } return false; }
const push = Array.prototype.push;
{ let a = [1, 2]; let n = a.push(3, 4); row(n === 4 && a[2] === 3 && a[3] === 4 && a.length === 4); }
row([].push() === 0 && push.length === 1 && push.name === 'push' && typeof push === 'function');
{ let a = Array(2); row(a.push(undefined, {x: 7}) === 4 && a.length === 4 && !(0 in a) && !(1 in a) && (2 in a) && a[2] === undefined && a[3].x === 7); }
{ let o = {length: 1}; row(push.call(o, 'x') === 2 && o[1] === 'x' && o.length === 2 && !Array.isArray(o)); }
{ let o = {}; row(push.call(o, 7) === 1 && o[0] === 7 && o.length === 1); }
{ let o = {length: '1.9'}; row(push.call(o, 7) === 2 && o[1] === 7 && o.length === 2); }
{ let o = {length: -2}; row(push.call(o, 7) === 1 && o[0] === 7 && o.length === 1); }
{ let o = {length: 0 / 0}; row(push.call(o) === 0 && o.length === 0); }
{ let o = {length: 1 / 0}; row(push.call(o) === 9007199254740991 && o.length === 9007199254740991); }
{ let o = {length: 9007199254740990}; row(push.call(o, 7) === 9007199254740991 && o['9007199254740990'] === 7 && o.length === 9007199254740991); }
// Over 2^53 - 1: a TypeError before any write.
{ let o = {length: 9007199254740991}; row(throwsType(() => push.call(o, 7)) && o.length === 9007199254740991 && o['9007199254740991'] === undefined); }
{ let o = {length: 1 / 0, 0: 1}; row(throwsType(() => push.call(o, 7)) && o.length === 1 / 0); }
{ let o = {length: 9007199254740990}; row(throwsType(() => push.call(o, 7, 8)) && o['9007199254740990'] === undefined && o.length === 9007199254740990); }
// An inherited `length`: the new one is the receiver's own.
{ let p = {length: 2}; let o = {__proto__: p}; row(push.call(o, 7) === 3 && o[2] === 7 && p.length === 2 && Object.getOwnPropertyDescriptor(o, 'length').value === 3); }
// After a sparse write.
{ let a = []; a[5000] = 1; row(a.push(7) === 5002 && a[5001] === 7 && a[0] === undefined && a.length === 5002); }
{ let a = [1, 2, 3]; a.length = 1; row(a.push(9) === 2 && a[1] === 9 && !(2 in a)); }
// Null and undefined receivers, and a detached call.
row(throwsType(() => push.call(null)) && throwsType(() => push.call(undefined, 1)) && throwsType(() => push()));
// A String object takes the index, then refuses its `length`.
{ let w = new String('ab'); row(throwsType(() => push.call(w, 'c')) && w.length === 2 && w[2] === 'c'); }
{ let w = new String('ab'); row(throwsType(() => push.call(w)) && w.length === 2 && !(2 in w)); }
// A getter on `length`, its valueOf, an index setter and a `length` setter,
// in order.
{
  let log = '';
  let o = { get length() { log += 'l'; return { valueOf() { log += 'v'; return 1; } }; }, set 1(v) { log += 's' + v; }, set 2(v) { log += 't' + v; }, set length(v) { log += 'n' + v; } };
  row(push.call(o, 7, 8) === 3 && log === 'lvs7t8n3');
}
{ let o = {length: { valueOf() { return 1; } }}; row(push.call(o, 7) === 2 && o[1] === 7 && o.length === 2); }
{ let len = {}; len[Symbol.toPrimitive] = function (hint) { return hint === 'number' ? 2 : 9; }; let o = {length: len}; row(push.call(o, 'z') === 3 && o[2] === 'z'); }
// A throwing getter, valueOf and setter end the push there.
{ let n = 0; try { push.call({ get length() { throw 5; } }, 1); } catch (e) { n = e; } row(n === 5); }
{ let n = 0; try { push.call({ length: { valueOf() { throw 6; } } }, 1); } catch (e) { n = e; } row(n === 6); }
{ let o = { length: 0, set 0(v) { throw v + 1; } }, n = 0; try { push.call(o, 7, 8); } catch (e) { n = e; } row(n === 8 && o[1] === undefined && o.length === 0); }
// A getter-only `length` refuses the final write.
{ let o = { get length() { return 0; } }; row(throwsType(() => push.call(o, 7)) && o[0] === 7); }
// A symbol length is a TypeError.
row(throwsType(() => push.call({length: Symbol.iterator}, 1)));
// Many pushes grow one run.
{ let a = []; for (let i = 0; i < 300; i++) { a.push(i, -i); } row(a.length === 600 && a[598] === 299 && a[599] === -299); }
// Not a constructor; the method is the realm's on every array.
row(throwsType(() => new push()) && [].push === push && Object.getOwnPropertyDescriptor(Array.prototype, 'push').writable && !Object.getOwnPropertyDescriptor(Array.prototype, 'push').enumerable);
failed === 0 ? rows : -failed;
