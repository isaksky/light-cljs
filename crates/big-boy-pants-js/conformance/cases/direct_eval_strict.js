// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 2184-2215,
// strict_direct_eval (bbp-107), all but the row QuickJS answers otherwise
// (a `const` in the temporal dead zone assigned through eval), and
// js_functions.rs lines 334-360, new_target_in_functions_and_arrows' eval
// rows. Strict direct eval (light-cljs-858): the caller's bindings, `this`,
// `arguments` and new.target, declarations local to the eval code, const
// and function-name assignment TypeErrors, the temporal dead zone, extra
// arguments evaluated and ignored, spread arguments. Each row is a `row(...)`
// check; the script completes with the number of rows, or minus the first
// failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
// Each row runs as one script: `run` as eval code of a direct eval in a
// fresh function, so its declarations stay its own; `script` through an
// indirect eval, as global code.
function run(source) { return eval(source); }
function script(source) { return (0, eval)(source); }
row(run("eval('1+1;') === 2 && eval('if (1) 2; else 3;') === 2 && eval('if (0) 2; else 3;') === 3") === true);
row(run("eval('var my_var=2; my_var;') === 2 && typeof my_var === 'undefined'") === true);
row(run("let a = 4; function f(b) { var x = 1; return eval(b); } f.call(1, 'this') === 1 && f('arguments.length', 1) === 2 && f('arguments[1]', 1) === 1 && f('x') === 1 && f('a') === 4 && f('a=3') === 3 && a === 3") === true);
row(run("let a = 2; eval('a') === 2 && eval('a=3') === 3 && a === 3") === true);
row(run("function f() { return (() => eval('this'))(); } f.call(5) === 5") === true);
row(run("function f() { return (() => eval('arguments[1]'))(); } f(2, 7) === 7") === true);
row(run("let fs = []; for (let i = 0; i < 3; i++) fs.push(eval('() => i')); fs[0]() === 0 && fs[1]() === 1 && fs[2]() === 2") === true);
row(run("let fs = []; for (let i = 0; i < 3; i++) { function f() { return eval('() => i'); } fs.push(f()); } fs[0]() === 0 && fs[1]() === 1 && fs[2]() === 2") === true);
row(run("const a = 1; function f() { eval('a=1'); } let caught = false; try { f(); } catch(e) { caught = e instanceof TypeError; } caught") === true);
row(run("const g = function m() { eval('m=1'); }; let caught = false; try { g(); } catch(e) { caught = e instanceof TypeError; } caught") === true);
row(run("function f() { let caught = false; try { eval('a'); } catch(e) { caught = e instanceof ReferenceError; } let a = 1; return caught; } f()") === true);
row(run("function f() { let x = 1; const g = eval('() => ++x'); return g() === 2 && x === 2; } f()") === true);
row(run("function f() { let x = 1; return eval('eval(\"x += 2\")') === 3 && x === 3; } f()") === true);
row(run("let x = 1; { let x = 2; eval('x=3'); } x === 1") === true);
row(run("let n = 0; eval(7, n++) === 7 && n === 1 && eval() === undefined") === true);
row(run("function f() { let x = 4; return eval(...['x + 1']) === 5; } f()") === true);
row(run("function f() { return eval('var x=2; x') === 2 && typeof x === 'undefined'; } f()") === true);
row(run("function f() { let x=1; let g=()=>x; eval('x=4'); return g() === 4; } f()") === true);
row(run("function f() { return eval('function g() { return 7; } g()') === 7 && typeof g === 'undefined'; } f()") === true);
row(run("function f() { let x=2; return eval('let x=3; x') === 3 && x === 2; } f()") === true);
row(run("function f() { let x=2; return eval('var x=3; x') === 3 && x === 2; } f()") === true);
row(run("function f() { eval('f=7'); } f(); f === 7") === true);
row(run("function f() { try { eval('010'); } catch(e) { return e instanceof SyntaxError; } } f()") === true);
row(run("function f() { let x=7; return eval.call(null, 'typeof x') === 'undefined' && eval.bind(null)('typeof x') === 'undefined' && (0,eval)('typeof x') === 'undefined'; } f()") === true);
row(run("function f() { let x=7; return eval?.('typeof x') === 'undefined' && (eval)?.(...['typeof x']) === 'undefined' && eval('x') === 7; } f()") === true);
row(run("let n=0; try { eval('1', (()=>{throw 7;})()); } catch(e) { n=e; } n === 7") === true);
row(run("function f() { let x=1; return eval('x', x=4) === 4; } f()") === true);
row(run("try { throw 8; } catch(e) { eval('e') === 8; }") === true);
row(run("function F() { return eval('new.target'); } new F() === F && F() === undefined") === true);
row(run("function F() { return (() => eval('new.target'))(); } new F() === F && F() === undefined") === true);
row(run("function F() { return eval('() => eval(\"new.target\")'); } new F()() === F && F()() === undefined") === true);
row(run("function F() { return (0, eval)('new.target'); } let caught = false; try { new F(); } catch (e) { caught = e instanceof SyntaxError; } caught") === true);
row(script("let ok = false; try { (() => eval('new.target'))(); } catch (e) { ok = e instanceof SyntaxError; } ok") === true);
// Direct sites of this file's own code.
let n = 0;
row(eval(7, n++) === 7 && n === 1 && eval() === undefined);
row((function (b) { var x = 1; return eval(b); })("x + arguments.length") === 2);
row(eval("var local = 3; local") === 3 && typeof local === "undefined");
failed === 0 ? rows : -failed;
