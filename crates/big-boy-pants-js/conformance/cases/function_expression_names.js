// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 1326-1343, 1370-1411:
// named_function_expression_name_is_immutable_in_nested_closures (line 1370,
// bbp-93) and the named-function-expression rows of
// hoisted_declarations_and_self_references (1306, bbp-94). Errors are
// compared by name, not by the Pants message and position. Each row is a `row(...)` check; the script completes with
// the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The name is immutable: assigning it is a TypeError in strict code, and
// it does not leak outside.
{ const g = function h() { h = 1; }; row(name(g) === "TypeError"); }
{ const g = function h() {}; row(name(function () { return h; }) === "ReferenceError"); }
// Shadowing the name inside is an ordinary binding.
{ const g = function h(h) { return h * 2; }; row(g(4) === 8); }
// Nested closures see the same immutable binding.
{ const f = function myfunc() { (() => { myfunc = 1; })(); }; row(name(f) === "TypeError"); }
{ const f = function myfunc() { (function () { myfunc = 1; })(); }; row(name(f) === "TypeError"); }
{ const f = function myfunc() { { function g() { myfunc = 1; } g(); } }; row(name(f) === "TypeError"); }
{
  const f = function m() { let r = 0; try { (() => { m = 1; })(); } catch (e) { r = 1; } return r; };
  row(f() === 1);
}
{
  const f = function m() { try { (() => { m = 1; })(); } catch (e) {} return m; };
  row(f() === f);
}
// Every closure form throws a catchable TypeError and keeps the identity.
{
  const f = function m() { let caught = false; try { (() => { m = 1; })(); } catch (e) { caught = e.name === "TypeError"; } return caught && m === f; };
  row(f());
}
{
  const f = function m() { let caught = false; try { (function () { m = 1; })(); } catch (e) { caught = e.name === "TypeError"; } return caught && m === f; };
  row(f());
}
{
  const f = function m() { let caught = false; try { { function g() { m = 1; } g(); } } catch (e) { caught = e.name === "TypeError"; } return caught && m === f; };
  row(f());
}
failed === 0 ? rows : -failed;
