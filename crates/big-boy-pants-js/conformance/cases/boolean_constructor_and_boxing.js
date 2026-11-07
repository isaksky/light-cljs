// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/boolean_bigint_wrappers.rs lines 36-56,
// boolean_constructor_and_boxing (bbp-302), its Boolean rows
// without the BigInt values (the BigInt area's, light-cljs-771), with
// Object.getPrototypeOf for instanceof (light-cljs-834), Function.prototype.call
// for reads on primitive bases (light-cljs-833), index loops for for-of and
// for-in for Object.keys and getOwnPropertyNames (the core-builtins area's).
// Each row is a `row(...)` check; the script completes with the number of
// rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
row(typeof Boolean === 'function' && globalThis.Boolean === Boolean && Boolean.length === 1 && Boolean.name === 'Boolean');
row(Boolean() === false && Boolean(undefined) === false && Boolean(null) === false && Boolean(0) === false && Boolean(-0) === false && Boolean(NaN) === false && Boolean('') === false);
row(Boolean(1) && Boolean('false') && Boolean(Symbol.iterator) && Boolean({}) && Boolean(new Boolean(false)));
{ let n = 0; let x = {valueOf() { n++; throw 1; }, toString() { n++; throw 2; }}; row(Boolean(x) && new Boolean(x).valueOf() === true && n === 0); }
{ let n = 0; let b = new Boolean(false, n++); row(n === 1 && typeof b === 'object' && b.valueOf() === false && b.toString() === 'false' && Object.getPrototypeOf(b) === Boolean.prototype); }
{ let a = Object(true), b = new Object(false); let keys = 0; for (const k in a) keys++; row(a.valueOf() === true && b.valueOf() === false && Object(a) === a && Object.getPrototypeOf(a) === Boolean.prototype && keys === 0); }
row(Boolean.prototype.valueOf() === false && Boolean.prototype.toString() === 'false' && Object.getPrototypeOf(Boolean.prototype) === Object.prototype && Boolean.prototype.constructor === Boolean);
row(Boolean.prototype.valueOf.call(true) === true && Boolean.prototype.toString.call(false) === 'false' && Object.getPrototypeOf(true) === Boolean.prototype);
{ let b = new Boolean; b.x = 7; let keys = ''; for (const k in b) keys += k; row(b.x === 7 && Boolean.prototype.x === undefined && keys === 'x'); }
row(Object.prototype.toString.call(true) === '[object Boolean]' && Object.prototype.toString.call(Object(false)) === '[object Boolean]');
row(Boolean.call(null, 1) === true && Boolean.call(null, 0) === false);
{ let d = Object.getOwnPropertyDescriptor(Boolean, 'prototype'); row(d.value === Boolean.prototype && !d.writable && !d.enumerable && !d.configurable); }
{
  let ok = true; const ks = ['toString', 'valueOf'];
  for (let i = 0; i < ks.length; i++) { let k = ks[i]; let d = Object.getOwnPropertyDescriptor(Boolean.prototype, k); ok = ok && d.writable && !d.enumerable && d.configurable && d.value.length === 0 && d.value.name === k; }
  row(ok);
}
{
  let n = 0; const xs = [undefined, null, 0, 'false', {}, new Number(0), new String('')]; const ks = ['valueOf', 'toString'];
  for (let i = 0; i < xs.length; i++) { for (let j = 0; j < ks.length; j++) { try { Boolean.prototype[ks[j]].call(xs[i]); } catch (e) { if (e.name === 'TypeError') n++; } } }
  row(n === 14);
}
row(Object.prototype.valueOf.call(false).valueOf() === false && typeof Object.prototype.valueOf.call(false) === 'object');
// A Boolean object is truthy and compares loosely by its value.
row(!!new Boolean(false) && new Boolean(false) == false && new Boolean(true) == 1 && new Boolean(true) !== true);
failed === 0 ? rows : -failed;
