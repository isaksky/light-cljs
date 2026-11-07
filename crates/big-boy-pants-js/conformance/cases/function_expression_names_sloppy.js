// bbp-authored conformance case (sloppy script).
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 1441-1494,
// sloppy_assignment_to_function_expression_name_is_ignored (bbp-96): in
// sloppy code an assignment to a named function expression's own name is
// ignored and yields its right-hand side; a "use strict" body makes it a
// TypeError, and a `const` stays immutable. Errors are compared by name.
// Each row is a `row(...)` check; the script completes with the number of
// rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
{ var f = function myfunc() { myfunc = 1; return myfunc; }; row(f() === f); }
{ var f = function myfunc() { myfunc = 1; (() => { myfunc = 1; })(); return myfunc; }; row(f() === f); }
{ var f = function myfunc() { 'use strict'; myfunc = 1; return myfunc; }; row(name(f) === "TypeError"); }
{ var f = function myfunc() { 'use strict'; myfunc = 1; (() => { myfunc = 1; })(); return myfunc; }; row(name(f) === "TypeError"); }
{ const f = function m() { return (m = 5); }; row(f() === 5); }
{
  let calls = 0;
  const f = function m() { const v = (m = (++calls, 5)); return v === 5 && m === f; };
  row(f() && calls === 1);
}
{
  let calls = 0;
  const f = function m() { const v = (() => (m = (++calls, 5)))(); return v === 5 && m === f; };
  row(f() && calls === 1);
}
{ const f = function m() { 'use strict'; return (m = 5); }; row(name(f) === "TypeError"); }
{ const c = 1; row(name(function () { c = 2; }) === "TypeError" && c === 1); }
failed === 0 ? rows : -failed;
