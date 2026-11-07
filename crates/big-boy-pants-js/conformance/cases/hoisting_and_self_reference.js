// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 1306-1345, 1829-1889:
// hoisted_declarations_and_self_references (line 1306, bbp-94) and
// top_level_function_declarations_are_var_scoped (1829, bbp-94); js_source.rs
// var_declarations_hoist_to_the_function (11901, bbp-94) and
// entry_function_name_refers_to_the_running_function (13083), whose entry
// functions are called from the script here. Script-level rows use names no
// other row declares. Left out: opcode and capture checks (Pants compiler
// internals); early errors (redeclarations in a block, `var` beside a block
// function, assigning an entry function's name: exporter SyntaxErrors); the
// named-function-expression rows (function_expression_names); `1; var w =
// 2;`, whose completion is the script's own (var_statement_completion).
// Errors are compared by name. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// Script-level `var` is hoisted and undefined before its statement.
row(hoistedLater === undefined);
var hoistedLater = 1;
assignedFirst = 3; var assignedFirst;
row(assignedFirst === 3);
// Declarations are created when their block is entered.
{ const r = g(); function g() { return 7; } row(r === 7); }
// A script declaration reads its global binding: replacing it is observed
// inside the original function too.
function countDown(n) { return n === 0 ? 0 : 1 + countDown(n - 1); }
const origCountDown = countDown;
countDown = function (n) { return 100; };
row(origCountDown(3) === 101);
function selfCall(n) { return n > 0 ? selfCall(n - 1) : 42; }
row(selfCall(3) === 42);
// Function declarations at a function's or script's top level are
// var-scoped: they share one binding with a parameter, a `var` or another
// declaration of the same name; the last one wins.
{ function f(a) { function a() { return 7; } return a(); } row(f(1) === 7); }
function twiceDeclared() { return 1; }
function twiceDeclared() { return 2; }
row(twiceDeclared() === 2);
var varThenFunction; function varThenFunction() { return 3; }
row(varThenFunction() === 3);
function functionThenVar() { return 3; } var functionThenVar;
row(functionThenVar() === 3);
var initializedVar = 4; function initializedVar() { return 3; }
row(initializedVar === 4);
{ function f() { const r = h(); var h = 5; function h() { return 6; } return r * 10 + h; } row(f() === 65); }
{
  function f(a) {
    function g() { return a() + b(); }
    function a() { return 10; }
    function b() { return 1; }
    function b() { return 2; }
    return g();
  }
  row(f(1) === 12);
}
function redeclaredRecursion(n) { return -1; }
function redeclaredRecursion(n) { return n === 0 ? 10 : redeclaredRecursion(n - 1) + 1; }
row(redeclaredRecursion(3) === 13);
function returnsSelf() { return returnsSelf; } function returnsSelf() { return 8; }
row(returnsSelf() === 8);
function singleDeclaration(n) { return n === 0 ? 0 : singleDeclaration(n - 1); } var singleDeclaration;
row(singleDeclaration(2) === 0);
// In a block, a function declaration is lexical.
{ function f(a) { { function a() { return 9; } } return a; } row(f(1) === 1); }
// `var` in a function: hoisted to its entry, undefined there.
{ function f() { const r = x; var x = 2; return r; } row(f() === undefined); }
{ function f() { { var x = 1; } return x; } row(f() === 1); }
{
  function f(b) { if (b) var y = 1; else { var z = 2; } return y === undefined ? z : y; }
  row(f(false) * 10 + f(true) === 21);
}
{
  function f() {
    try { var a = 1; throw 0; } catch (e) { var b = 2; } finally { var c = 3; }
    switch (a) { case 1: var d = 4; }
    L: var g = 5;
    return a + b + c + d + g;
  }
  row(f() === 15);
}
{ function f() { for (; false;) var x = 1; return x; } row(f() === undefined); }
{ function f() { for (let i = 0; i < 3; i++) { var last = i; } return last; } row(f() === 2); }
{ function f() { const before = d; do var d = 7; while (false); return before === undefined ? d : -1; } row(f() === 7); }
// A `var` named like a parameter is that parameter's binding.
{ function f(a) { var a; return a; } row(f(5) === 5); }
{ function f(a) { var a = 2; return a; } row(f(5) === 2); }
// `var x;` does not reset an assigned value; repeating `var` is fine.
var notReset = 1; var notReset;
row(notReset === 1);
var repeated = 1; { var repeated = repeated + 1; } var repeated = repeated * 10;
row(repeated === 20);
// A closure created before the `var` statement sees its later value.
{ function f() { var g = function () { return x; }; var x = 3; return g(); } row(f() === 3); }
var incCount = 0; function incByOne() { incCount = incCount + 1; } incByOne(); incByOne();
row(incCount === 2);
// Closures made in successive iterations share the body's one `var`.
{
  function run(bumping) {
    let i = 0;
    let first, last;
    while (i < 2) {
      var j = i;
      const get = bumping ? function () { j = j + 1; return j; } : function () { return j; };
      if (i === 0) { first = get; }
      last = get;
      i = i + 1;
    }
    return first() * 100 + last();
  }
  row(run(false) === 101 && run(true) === 203);
}
// A never-assigned `var` captured by a closure holds undefined.
{ function f() { function g() { return u; } var u; return g(); } row(f() === undefined); }
// A function's `var` is not visible outside it.
{ function f() { var inner = 1; } f(); row(name(function () { return inner; }) === "ReferenceError"); }
// A catch block's `var` named like its parameter: the initializer assigns
// the parameter (ECMAScript Annex B).
{
  function f() { try { throw 1; } catch (e) { var e = 2; var r = e; } return e === undefined ? r : -1; }
  row(f() === 2);
}
// A `var` shadows an enclosing binding of the same name.
{ let s = 1; function f() { s = 2; var s; return s; } row(f() * 10 + s === 21); }
// A function's own name is the running function object; a parameter or a
// body binding of that name is an ordinary local.
{ function f() { return f === f; } row(f()); }
{ function fact(n) { return n <= 1 ? 1 : n * fact(n - 1); } row(fact(4) === 24); }
{ function f(n) { return n ? f(n - 1) : 0; } row(f(4) === 0); }
{ function f(f) { return f * 2; } row(f(4) === 8); }
{ function f() { let f = 3; return f; } row(f() === 3); }
{ function f() { { const f = 5; return f; } } row(f() === 5); }
failed === 0 ? rows : -failed;
