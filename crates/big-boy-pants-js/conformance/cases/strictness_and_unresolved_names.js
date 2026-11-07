// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 11697-11717, 11832-11897:
// unknown_names_fail_with_reference_errors_at_their_position (line 11697),
// const_reassignment_fails_with_a_type_error (11832) and
// use_before_initialization_fails_with_reference_errors (11854). Each
// failing script runs inside a function here, its script-level bindings in a
// block; errors are compared by name (the Pants tests also check messages
// and positions). The rows of use_strict_directives (11439, bbp-95) are in
// strict_directives_sloppy and the use_strict_completion_* cases. Each row is
// a `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// Unknown names.
row(name(function () { let a = 1; return a + missing; }) === "ReferenceError");
// Strict assignment to an undeclared name, after evaluating the right-hand
// side.
{
  let evaluated = false;
  row(name(function () { undeclaredTarget = (evaluated = true); }) === "ReferenceError" && evaluated);
  row(typeof undeclaredTarget === "undefined");
}
// Only when the code runs.
row(name(function () { if (false) { nope; nope = 1; } return 1; }) === "none");
// A binding is visible only in its block.
row(name(function () { { let inner = 1; } return inner; }) === "ReferenceError");
// Assignment to a const.
row(name(function () { const c = 1; let d = 2; d = c; c = d + 1; }) === "TypeError");
{ const c = 1; if (c > 1) { c = 2; } row(c === 1); }
{ const c = 1; { let c = 5; c = c + 1; } row(c === 1); }
// The right-hand side is evaluated first.
row(name(function () { const c = 1; c = missing; }) === "ReferenceError");
// A const in its temporal dead zone reports the ReferenceError first. At
// script level: in a function or block QuickJS throws a TypeError here (the
// specification's SetMutableBinding and the Pants engine: ReferenceError),
// so that variant is not ported.
var lateConstError = "none";
try { lateConst = 1; } catch (e) { lateConstError = e.name; }
const lateConst = 2;
row(lateConstError === "ReferenceError" && lateConst === 2);
// Parameters are assignable.
{ function f(p) { p = p + 1; return p; } row(f(1) === 2); }
// Use before initialization.
row(name(function () { x; let x = 1; }) === "ReferenceError");
row(name(function () { let y = 1; let x = y + x; }) === "ReferenceError");
row(name(function () { { { x; } let x = 1; } }) === "ReferenceError");
row(name(function () { let x = 1; { x; let x = 2; } }) === "ReferenceError");
row(name(function () { x = 1; let x; }) === "ReferenceError");
row(name(function () { let a = k; const k = 1; }) === "ReferenceError");
// Each loop iteration's binding starts uninitialized: the second fails.
{
  let i = 0;
  const e = name(function () { while (i < 3) { if (i === 1) { z; } let z = i; i = i + 1; } });
  row(e === "ReferenceError" && i === 1);
}
// Never reached: no error. After the declaration: initialized.
{ if (false) { x; } let x = 4; row(x === 4); }
{ let u; row(u === undefined); }
// `let v;` re-initializes to undefined on every iteration.
{
  let i = 0, seen = 0;
  while (i < 2) { let v; if (v === undefined) seen = seen + 1; v = 1; i = i + 1; }
  row(seen === 2);
}
failed === 0 ? rows : -failed;
