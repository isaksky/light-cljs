// bbp-authored conformance case (light-cljs-881), run as a sloppy script.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 7166-18600:
// define_property_sloppy_writes (line 16901, bbp-119),
// define_property_array_data_attributes (line 16780, bbp-120), its sloppy
// rows, array_length_assignment (line 18392), its sloppy non-configurable
// and read-only rows, delete_property_references (line 17876, bbp-124), its
// sloppy non-configurable rows; a mapped arguments index (light-cljs-860,
// ECMAScript 10.4.4.2) under an accessor definition; and the planner's row
// (light-cljs-766). A write [[Set]] refuses and a delete of a
// non-configurable property do nothing in sloppy code, while strict code
// in the same script throws. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
// The planner's row (light-cljs-766).
{ var o = {}; Object.defineProperty(o, 'k', {value: 1}); o.k = 2; row(o.k === 1); }
// define_property_sloppy_writes (line 16901).
{
  let o = {}; Object.defineProperty(o, 'k', {value: 1});
  function f() { 'use strict'; o.k = 3; }
  let r = 'none'; try { f(); } catch (e) { r = 'caught'; }
  row(r === 'caught' && o.k === 1);
}
// define_property_array_data_attributes (line 16780): sloppy rows.
{ let o = {}; Object.defineProperty(o, '0', {value: 1}); Object.defineProperty(o, 'k', {value: 1}); let v = (o[0] = 2); let w = (o['k'] = 2); row('' + v + w + o[0] + o.k === "2211"); }
{ let a = [1]; Object.defineProperty(a, 0, {writable: false}); a[0] = 2; row(a[0] === 1); }
{ let a = [1]; Object.defineProperty(a, 'length', {writable: false}); a[1] = 2; row(a.length === 1 && a[1] === undefined); }
// array_length_assignment (line 18392): sloppy rows.
{ let a = [1, 2, 3, 4, 5]; Object.defineProperty(a, 3, {configurable: false}); let result = (a.length = 1); row(result === 1 && a.length === 4 && a[4] === undefined && a[3] === 4); }
for (let write of [a => { a.length = 5; }, a => { a.length = 0; }, a => { a.length = 2; }, a => { a[a.length] = 1; }]) {
  let a = [1, 2]; Object.defineProperty(a, 'length', {writable: false}); write(a); row(a.length === 2 && a[0] === 1 && a[2] === undefined);
}
// delete_property_references (line 17876): sloppy non-configurable rows.
{ let o = {}; Object.defineProperty(o, 'x', {value: 1}); row(delete o.x === false && o.x === 1); }
{ let a = [1, 2]; Object.defineProperty(a, 0, {configurable: false}); row(delete a[0] === false && a[0] === 1); }
// Update, compound and logical assignment to a non-writable property
// (light-cljs-830's rows) do nothing; the expression is the operator's result.
{ let o = {}; Object.defineProperty(o, 'k', {value: 1}); let k = 'k'; let r = (o.k += 5); let s = (o[k] -= 1); let t = o[k]++; let u = (o[k] &&= 9); row('' + r + s + t + u + o.k === "60191"); }
// Mapped arguments: an accessor definition unmaps the index. (A data
// definition writes its value through to the parameter, which the Pants
// engine does not do, so those rows are light-cljs-868's.)
{ function f(a) { Object.defineProperty(arguments, 0, {get: () => 9}); a = 3; return '' + arguments[0] + a; } row(f(1) === "93"); }
failed === 0 ? rows : -failed;
