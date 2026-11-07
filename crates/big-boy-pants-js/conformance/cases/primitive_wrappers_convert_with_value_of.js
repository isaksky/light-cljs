// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 19266-19285,
// primitive_wrappers_convert_with_value_of (bbp-72), every row,
// then the realm rows of rust/src/realm.rs's Number and String constructors
// and prototypes (light-cljs-832): wrapper objects compare loosely through
// their prototypes' valueOf, never strictly, and two wrappers compare by
// identity. Methods are called on wrappers or through Function.prototype.call
// (reads on primitive bases are light-cljs-833's). Each row is a `row(...)`
// check; the script completes with the number of rows, or minus the first
// failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
row((new Number(1)) == 1);
row(2 == (new Number(2)));
row((new String('abc')) == 'abc');
row(typeof new Number(1) === 'object');
row((new Number(1) === 1) === false);
row((new Number(1) == new Number(1)) === false);
row(new Number(5) + 1 === 6);
row(new String('a') + 'b' === 'ab');
row(new Number() == 0 && new String() == '');
row(Number('0x10') === 16 && Number() === 0);
row(String(null) === 'null' && String(1e21) === '1e+21');
row((new String('abc') == new String('abc')) === false);
{ let n = new Number(1); n.valueOf = function () { return 7; }; row(n + 1 === 8 && n == 7); }
{ let s = new String('a'); s.valueOf = function () { return {}; }; s.toString = function () { return 'c'; }; row(s + 'b' === 'cb'); }
// The constructors and prototypes.
row(typeof Number === 'function' && globalThis.Number === Number && Number.length === 1 && Number.name === 'Number');
row(typeof String === 'function' && globalThis.String === String && String.length === 1 && String.name === 'String');
row(Number.prototype.constructor === Number && String.prototype.constructor === String);
row(Object.getPrototypeOf(Number.prototype) === Object.prototype && Object.getPrototypeOf(String.prototype) === Object.prototype);
row(Number.prototype.valueOf() === 0 && String.prototype.valueOf() === '' && String.prototype.toString() === '');
{ const d = Object.getOwnPropertyDescriptor(Number, 'prototype'); row(d.value === Number.prototype && !d.writable && !d.enumerable && !d.configurable); }
{ const d = Object.getOwnPropertyDescriptor(Number.prototype, 'toString'); row(d.writable && !d.enumerable && d.configurable && d.value.length === 1 && d.value.name === 'toString'); }
{ const d = Object.getOwnPropertyDescriptor(String.prototype, 'valueOf'); row(d.writable && !d.enumerable && d.configurable && d.value.length === 0 && d.value.name === 'valueOf'); }
row(Object.getPrototypeOf(new Number(1)) === Number.prototype && Object.getPrototypeOf(Object('s')) === String.prototype);
row(new Number(1.5).valueOf() === 1.5 && new Number(-0).toString() === '0' && new String('xy').toString() === 'xy');
row(Number.prototype.toString.call(255) === '255' && Number.prototype.valueOf.call(new Number(3)) === 3 && String.prototype.valueOf.call('q') === 'q');
row(Object.prototype.toString.call(new Number(1)) === '[object Number]' && Object.prototype.toString.call('x') === '[object String]' && Object.prototype.toString.call(String.prototype) === '[object String]');
// The other standard names are there, for the core-builtins area to make.
row('isNaN' in Number && 'MAX_SAFE_INTEGER' in Number && 'toFixed' in Number.prototype && 'fromCharCode' in String && 'charAt' in String.prototype);
// Brand checks: thisNumberValue and thisStringValue.
{
  let n = 0;
  const xs = [undefined, null, true, '1', {}, new String('1')];
  for (let i = 0; i < xs.length; i++) { try { Number.prototype.valueOf.call(xs[i]); } catch (e) { if (e.name === 'TypeError') n++; } }
  const ys = [undefined, null, 1, {}, new Number(1)];
  for (let i = 0; i < ys.length; i++) { try { String.prototype.toString.call(ys[i]); } catch (e) { if (e.name === 'TypeError') n++; } }
  row(n === 11);
}
{ let caught = ''; try { new Number(1).toString(1); } catch (e) { caught = e.name; } row(caught === 'RangeError' && new Number(7).toString(10) === '7' && new Number(7).toString(undefined) === '7'); }
failed === 0 ? rows : -failed;
