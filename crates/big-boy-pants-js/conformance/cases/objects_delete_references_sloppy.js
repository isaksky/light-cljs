// bbp-authored conformance case (light-cljs-880), run as a sloppy script.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 17876-18140:
// delete_property_references (line 17876, bbp-124), its sloppy rows without
// defineProperty or freeze: deleting an array's `length`, a string's index
// or `length`, or a function's `prototype` gives false; `delete x` of an
// unresolvable name gives true. The frozen and non-configurable sloppy rows
// wait for light-cljs-881 and light-cljs-882. Each row is a `row(...)` check;
// the script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// delete_property_references (line 17876, bbp-124): its sloppy rows.
{
  let arr = [1, 2];
  row(delete arr.length === false && arr.length === 2);
}
row(delete 'abc'[2] === false && delete 'abc'['length'] === false);
{
  let f = function () {};
  row(delete f.prototype === false && typeof f.prototype === 'object');
}
row(delete x);
{
  let a = {x: 1, y: 1};
  row(delete a.x && a.x === undefined && delete a.missing);
}
failed === 0 ? rows : -failed;
