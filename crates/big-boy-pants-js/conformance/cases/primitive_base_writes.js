// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 18213-18256,
// methods_on_primitive_bases (bbp-73), its strict write rows without
// `__proto__` (pending on Goose, the objects area's): PutValue on a primitive
// base (light-cljs-833). A primitive takes no own property and a string's
// `length` and indices are read-only, so strict code throws a TypeError and
// the primitive is unchanged; `delete` of a string's own `length` or index
// is a TypeError, other deletes are true; `in` with a primitive right
// operand is a TypeError. A setter a realm prototype holds running with the
// primitive as receiver needs Object.defineProperty (the objects area's,
// light-cljs-766); serial_tests.goose installs one for Goose. The sloppy
// rows are primitive_base_writes_sloppy. Each row is a `row(...)` check; the
// script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function throws(f) { try { f(); } catch (e) { return e.name === 'TypeError'; } return false; }
{ let n = 1; row(throws(() => { n.x = 2; }) && n.x === undefined && throws(() => { n['y'] = 2; })); }
{ let s = 'abc'; row(throws(() => { s[5] = 2; }) && throws(() => { s[0] = 'x'; }) && s === 'abc' && s[0] === 'a'); }
{ let s = 'abc'; row(throws(() => { s.length = 1; }) && s.length === 3 && throws(() => { s.toString = 1; }) && s.toString() === 'abc'); }
{ let b = true; row(throws(() => { b.x = 1; }) && b.x === undefined); }
{ let n = 1; row(throws(() => { n.x += 1; }) && throws(() => { n.x++; }) && throws(() => { n[0] = 1; })); }
{ let u; row(throws(() => { u.x = 1; }) && throws(() => { u['x'] = 1; }) && throws(() => { null.x = 1; })); }
{ let n = 4; row(throws(() => { n.toString = 1; }) && n.toString === Number.prototype.toString); }
{ let k = { toString() { return 'z'; } }; row(throws(() => { (1)[k] = 1; }) && throws(() => { 'a'[k] = 1; })); }
// delete and in.
row(throws(() => delete 'abc'.length) && throws(() => delete 'abc'[0]) && throws(() => delete 'abc'['2']));
row(delete 'abc'[5] === true && delete (1).x === true && delete true.x === true && delete 'abc'.x === true);
row(throws(() => 'x' in 1) && throws(() => 'length' in 'abc') && throws(() => 'x' in true));
failed === 0 ? rows : -failed;
