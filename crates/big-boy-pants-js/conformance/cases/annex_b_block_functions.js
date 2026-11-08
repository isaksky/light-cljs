// bbp-authored conformance case (sloppy script).
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 1843-1941,
// sloppy_block_functions_also_bind_var,
// sloppy_eval_block_functions_also_bind_var and
// sloppy_annex_b_globals_skip_ineligible_bindings (light-cljs-859), their rows on
// which QuickJS agrees. ECMAScript Annex B.3.3: a sloppy block or case-clause
// function declaration also assigns a `var` binding of the script, function
// or eval body when it is evaluated, unless a `var` there would conflict with
// an enclosing lexical binding, the name is a parameter, or it is `arguments`
// in a function; strict code keeps block functions lexical. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
// `run` evaluates a row as eval code of a direct eval in a fresh function,
// so its declarations stay its own; `script` through an indirect eval, as
// global code.
function run(source) { return eval(source); }
function script(source) { return (0, eval)(source); }
// Script code: the binding is a global property.
var beforeBlock859 = typeof blockTop859;
{ function blockTop859() { return 1; } }
row(beforeBlock859 === "undefined" && blockTop859() === 1);
row(Object.getOwnPropertyDescriptor(globalThis, "blockTop859") !== undefined);
switch (1) { case 1: function caseTop859() { return 4; } }
row(caseTop859() === 4);
// Function code.
row(run("function g() { var before = typeof h; { function h() {} } return before + ',' + typeof h; } g()") === "undefined,function");
row(run("function g() { { function h() { return 1; } } { function h() { return 2; } } return h(); } g()") === 2);
row(run("function g() { switch (2) { case 1: function s() { return 4; } case 2: } return typeof s; } g()") === "undefined");
row(run("function g() { for (var i = 0; i < 2; i++) { function l() { return i; } } return l(); } g()") === 2);
row(run("function g() { var r = []; for (var i = 0; i < 3; i++) { function h() { return i; } r.push(h()); } return r.join(); } g()") === "0,1,2");
row(run("function g() { { let inner = 1; { function q() { return inner; } } } return q(); } g()") === 1);
row(run("function g() { let h = 1; { function h() {} } return h; } g()") === 1);
row(run("function g(h) { { function h() {} } return typeof h; } g(5)") === "number");
row(run("function g(a = 1) { { function a() {} } return typeof a; } g()") === "number");
row(run("function g(a = 1) { { function h() { return a; } } return h(); } g()") === 1);
row(run("function g() { { let h = 1; { function h() {} } } return typeof h; } g()") === "undefined");
row(run("function g() { for (let h = 0; h < 1; h++) { function h() {} } return typeof h; } g()") === "undefined");
row(run("function g() { for (const h of [1]) { function h() {} } return typeof h; } g()") === "undefined");
row(run("function g() { try { throw 1; } catch ({h}) { { function h() {} } } return typeof h; } g()") === "undefined");
row(run("function g() { try { throw 1; } catch (h) { { function h() { return 2; } } } return h(); } g()") === 2);
row(run("function g() { var r = []; { r.push(typeof h); function h() {} } r.push(typeof h); return r.join(); } g()") === "function,function");
row(run("function g() { function h() { return 1; } { function h() { return 2; } } return h(); } g()") === 2);
row(run("function g() { { function h() { return 1; } { function h() { return 2; } } } return h(); } g()") === 1);
row(run("function g() { var f = 1; { function f() {} } return typeof f; } g()") === "function");
row(run("function g() { var c = function () { return h; }; { function h() { return 1; } } return c()(); } g()") === 1);
row(run("function g() { { function arguments() {} } return typeof arguments; } g()") === "object");
row(run("function g() { { function* h() {} } { async function k() {} } return typeof h + typeof k; } g()") === "undefinedundefined");
row(run("function g() { var o = {h: 1}; with (o) { { function h() { return 7; } } } return typeof o.h + typeof h; } g()") === "numberfunction");
row(run("function* g() { { function h() { return 3; } } yield h(); } g().next().value") === 3);
row(run("(() => { { function h() { return 4; } } return h(); })()") === 4);
row(run("function g() { 'use strict'; { function h() {} } return typeof h; } g()") === "undefined");
// Eval code (Annex B.3.3.3): the eval's variable environment.
row(run("eval('{ function e1() { return 3; } }'); e1()") === 3);
row(script("{ function indirect859() { return 5; } } indirect859()") === 5 && indirect859() === 5);
row(run("function g() { eval('{ function e3() { return 7; } }'); return e3(); } g()") === 7);
row(run("function g() { var h = 1; eval('{ function h() { return 5; } }'); return h(); } g()") === 5);
row(run("function g() { eval('var z = 1'); eval('{ function h() { return z; } }'); return h(); } g()") === 1);
row(run("function g() { return eval('{ function h() { return 6; } } h()'); } g()") === 6);
row(run("function g() { eval('\"use strict\"; { function h() {} }'); return typeof h; } g()") === "undefined");
row(run("function g() { eval('let x; { function x() {} }'); return typeof x; } g()") === "undefined");
// A global binding is created before the ordinary global declarations.
row(script("{ function annexE859() {} } var ordinaryE859; var seen = ''; for (var k in globalThis) { if (k === 'annexE859' || k === 'ordinaryE859') seen += k + ','; } seen") === "annexE859,ordinaryE859,");
failed === 0 ? rows : -failed;
