// bbp-authored conformance case.
// Derives from: rust/tests/js_source.rs lines 19887-19963 (`array_join_and_to_string`;
// light-cljs-842, bbp-156; ECMAScript 23.1.3.18 and 23.1.3.36, QuickJS's
// js_array_join and js_array_toString): separators, null, undefined and
// holes, nested arrays, ToPrimitive of an array (`'' + [1, 2]`, a template
// substitution, String, Number, `==` and `<`), objects with toString,
// `b.join = 5; b.toString()`, array-like, string and number receivers, a
// `length` with valueOf, a toString receiver with its own `join` (data or
// getter), getters observed in order, an exception from an element, a
// cycle's catchable stack overflow and wide strings. Each row is a `row(...)`
// check; the script completes with the number of rows, or minus the first
// failing row's number.
//
// Rows of `array_join_and_to_string` that wait for the objects area
// (light-cljs-766): those using Object.defineProperty, Object.create or
// Object.setPrototypeOf (an inherited element behind a hole, a `join`
// accessor defined on an array, `[Object.create(null)].join()`). Holes come
// from `Array(n)` because array literal holes (the append_hole opcode) are
// not implemented on Goose yet.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function throwsType(f) { try { f(); } catch (e) { return e instanceof TypeError; } return false; }
const join = Array.prototype.join, toString = Array.prototype.toString;
row([1, 2, 3, 4].join() === '1,2,3,4' && [1, 2].join('-') === '1-2' && [1, 2].join(undefined) === '1,2' && [].join() === '');
row([null, undefined, 1].join() === ',,1' && [[1, 2], [3]].join(';') === '1,2;3' && [1, 2].join(null) === '1null2' && [1, 2].join('') === '12');
{ let h = Array(3); h[0] = 1; h[2] = 3; row(h.join() === '1,,3' && Array(4).join('x') === 'xxx'); }
row([1, 2].toString() === '1,2' && '' + [1, 2] === '1,2' && `${[1, 2]}` === '1,2' && String([1, [2, [3]]]) === '1,2,3');
row(Number([5]) === 5 && [3] == 3 && [1, 2] < [1, 3] && [] + [] === '' && [1] * [2] === 2);
row([{}].join() === '[object Object]' && [{ toString: function () { return 'x'; } }].join() === 'x' && [1.5, true, -0, 'a'].join() === '1.5,true,0,a');
{ let b = [1]; b.join = 5; row(b.toString() === '[object Array]' && '' + b === '[object Array]'); }
row(join.call({ 0: 'a', 2: 'c', length: '3.9' }, '-') === 'a--c');
row(join.call('abc', '-') === 'a-b-c' && join.call(3) === '' && join.call({ length: -2 }) === '' && join.call({ length: NaN }) === '' && join.call(true) === '');
row(join.call({ length: { valueOf() { return 2; } }, 0: 4, 1: 5 }) === '4,5');
row(toString.call({ join() { return this.x; }, x: 'ok' }) === 'ok' && toString.call({ join: null }) === '[object Object]' && toString.call({}) === '[object Object]');
row(toString.call({ get join() { return function () { return this[1] + 'x'; }; }, 1: 2 }) === '2x');
{ let o = { join() { return 42; } }; row(toString.call(o) === 42 && toString.call('ab') === '[object String]'); }
{ let a = [1, 2]; a[0] = { toString() { a.push(3); a[1] = 4; return 'x'; } }; row(a.join() === 'x,4' && a.length === 3); }
row([1, 2].join({ toString() { return '-'; } }) === '1-2');
{
  let log = '';
  let o = { get length() { log += 'l'; return { valueOf() { log += 'v'; return 2; } }; }, get 0() { log += 'a'; return { toString() { log += 't'; return 'x'; } }; }, get 1() { log += 'b'; return null; } };
  let s = { toString() { log += 's'; return '-'; } };
  row(join.call(o, s) === 'x-' && log === 'lvsatb');
}
{ let hit = 0; row([].join({ toString() { hit++; return ''; } }) === '' && hit === 1); }
row(join.name === 'join' && join.length === 1 && toString.length === 0 && toString.name === 'toString');
{ let d = Object.getOwnPropertyDescriptor(Array.prototype, 'join'); row(d.writable && !d.enumerable && d.configurable && Object.getOwnPropertyDescriptor(Array.prototype, 'toString').writable); }
// An exception from an element's toString ends the join.
{ let caught = false; try { [1, { toString() { throw 42; } }].join(); } catch (e) { caught = e === 42; } row(caught && [2, 3].join() === '2,3'); }
{ let caught = false; try { [1, { toString() { return {}; }, valueOf() { return {}; } }].join(); } catch (e) { caught = e instanceof TypeError; } row(caught); }
row(throwsType(() => [1, Symbol.iterator].join()) && throwsType(() => [1].join(Symbol.iterator)));
// A cycle recurses to the catchable stack overflow: an Error, not a RangeError.
{ let a = [1]; a[1] = a; let caught = false; try { a.join(); } catch (e) { caught = e instanceof Error && !(e instanceof RangeError); } row(caught && [3].join() === '3'); }
row(throwsType(() => join.call(null)) && throwsType(() => join.call(undefined)) && throwsType(() => toString.call(null)) && throwsType(() => toString.call(undefined)));
// Wide strings stay exact.
row(['aĀ', 'b'].join('ā') === 'aĀāb' && ['\ud83d', '\ude00'].join('') === '😀' && [1, 2].join('€').length === 3);
// A longer join.
{ let a = []; for (let i = 0; i < 500; i++) { a.push(i % 10); } let s = a.join(); row(s.length === 999 && s[0] === '0' && s[998] === '9'); }
row(throwsType(() => new join()) && throwsType(() => new toString()));
failed === 0 ? rows : -failed;
