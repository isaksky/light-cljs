// bbp-authored conformance case (sloppy script).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 2299-2363,
// eval_in_parameter_expressions (bbp-109), its rows on which QuickJS agrees and
// that need no super property (light-cljs-766), and lines 2366-2377,
// shadowed_eval_in_parameter_defaults. A direct eval in a parameter
// expression declares its variables in the parameter scope, separate from
// the body's; `arguments` and the function's own name stay visible;
// conflicts are SyntaxErrors. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
// Each row runs as one script: `run` as eval code of a direct eval in a
// fresh function, so its declarations stay its own; `script` through an
// indirect eval, as global code.
function run(source) { return eval(source); }
function script(source) { return (0, eval)(source); }
row(run("let f=function(a=eval('1'), b=arguments[0]) { return b; }; f(12) === 12") === true);
row(run("let f=function(a=eval('1'), b=()=>arguments) { return b; }; f(12)()[0] === 12") === true);
row(run("(function() { 'use strict'; let f=function f(a=eval('f')) { return a; }; return f() === f; })()") === true);
row(run("let f=(a=eval('var c=1'), probe=()=>c)=>{ var c=2; return c===2 && probe()===1; }; f()") === true);
row(run("let f=(a=eval('var arguments=1'), probe=()=>arguments)=>{ var arguments=2; return arguments===2 && probe()===1; }; f()") === true);
row(run("var c='global'; let f=function f(a=eval('var c=1'), b=c, probe=()=>c) { return b===1 && c===1 && probe()===1; }; f() && c==='global'") === true);
row(run("let f=function f(a=eval('var c=1'), probe=(d=eval('c'))=>d) { return probe()===1; }; f()") === true);
row(run("function f(a=eval('var c=1'), b=eval('c+=1'), probe=()=>c) { eval('var c=3'); return c===3 && probe()===2; } f()") === true);
row(run("function f(a=eval('var c=1'), probe=()=>c) { var c; return c===undefined && probe()===1; } f()") === true);
row(run("function f(a=eval('var c=1'), probe=()=>c) { function c() { return 2; } return c()===2 && probe()===1; } f()") === true);
row(run("let f=(v, a=eval('var c=v'), probe=()=>c)=>probe; let g=f(1), h=f(2); g()===1 && h()===2") === true);
row(run("function f(a=eval('\"use strict\"; var c=1'), b=typeof c) { return b==='undefined'; } f()") === true);
row(run("function f(a=eval('eval(\"var c=1\")'), b=c) { return b===1; } f()") === true);
row(run("function f(a=eval('var f=1')) { return f===1; } f()") === true);
row(run("function f(a=eval('var c=1'), probe=()=>eval('c')) { eval('var c=2'); return probe()===1 && c===2; } f()") === true);
row(run("function f(a=eval('function c() { return 1; }'), probe=()=>c) { var c=2; return c===2 && probe()()===1; } f()") === true);
row(run("let c=0; function f(a=()=>eval('var c=1; c')) { return a()===1 && c===0; } f()") === true);
row(run("function F(a=eval('new.target')) { this.ok=a===F; } new F().ok") === true);
row(run("function f(a=eval(...['var c=1']), b=c) { return b===1; } f()") === true);
row(run("function f(a=eval('var c=1'), b=eval('delete c')) { return b && typeof c==='undefined'; } f()") === true);
row(run("function f(a=eval('var c=1')) { return ()=>c; } f()() === 1") === true);
row(run("function f(a=eval('var c=1')) { return eval('()=>c'); } f()() === 1") === true);
row(run("function f(eval, x=eval(1,2)) { return x; } f((a,b)=>a+b)") === 3);
row(run("function f(eval, x=eval(...[1,2])) { return x; } f((a,b)=>a+b)") === 3);
row(run("function f(eval, x=eval('1')) { return x; } f(globalThis.eval)") === 1);
// A direct site of this file's own code.
row((function (a = eval("var p = 5"), read = () => p) { var p = 6; return p === 6 && read() === 5; })());
failed === 0 ? rows : -failed;
