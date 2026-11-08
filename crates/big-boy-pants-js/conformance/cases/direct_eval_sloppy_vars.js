// bbp-authored conformance case (sloppy script).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 1993-2045,
// sloppy_eval_declares_vars_in_the_caller (bbp-108), its rows on which QuickJS
// agrees and that need no class heritage, and lines 2218-2235
// (strict_direct_eval's sloppy rows). Sloppy eval code declares its `var` and
// function bindings in the caller's variable environment, seen by closures
// made before and after it, deletable, shadowed by nearer bindings; a clash
// with an intervening `let` is a SyntaxError; at script level they are
// configurable globals. Each row is a `row(...)` check; the script completes
// with the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
// Each row runs as one script: `run` as eval code of a direct eval in a
// fresh function, so its declarations stay its own; `script` through an
// indirect eval, as global code.
function run(source) { return eval(source); }
function script(source) { return (0, eval)(source); }
row(run("function f() { eval(\"var z = 1\"); return z; } f() === 1") === true);
row(run("function f() { eval(\"var z = 1\"); return (function () { return z; })(); } f() === 1") === true);
row(run("function f() { const g = () => z; eval(\"var z = 2\"); return g(); } f() === 2") === true);
row(run("var c = 'global'; let f = function f(a, b = c, probe = () => c) { eval('var c = 1'); return c === 1 && b === 'global' && probe() === 'global'; }; f() && c === 'global'") === true);
row(run("function f() { let z; { eval('var z = 1'); } } let ok=false; try { f(); } catch(e) { ok=e instanceof SyntaxError; } ok") === true);
row(run("function f() { 'use strict'; eval('var z = 1'); return z; } let ok=false; try { f(); } catch(e) { ok=e instanceof ReferenceError; } ok") === true);
row(script("eval('var g2 = 1'); delete g2") === true);
row(run("let f = function myfunc() { eval('myfunc = 1'); return myfunc; }; f() === f") === true);
row(run("function f() { let g=()=>z; eval('function z() { return 7; }'); return g()() === 7; } f()") === true);
row(run("function f(x) { var y=2; eval('var x=3; var y=4'); return x+y; } f(1) === 7") === true);
row(run("let z=8; function f() { let g=()=>z; eval('var z=2'); return g()===2 && delete z && g()===8; } f() && z===8") === true);
row(run("function f() { eval('var z=1'); eval('z+=2'); return eval('()=>z'); } f()() === 3") === true);
row(run("function f() { let x=0; { let z; try { eval('x=1; var z=2; var y=3'); } catch(e) { return e instanceof SyntaxError && x===0 && typeof y==='undefined'; } } } f()") === true);
row(run("function f() { return eval('var z=1; delete z') && typeof z === 'undefined'; } f()") === true);
row(run("let z=8; function f() { return eval('var z=1; delete z; z') === 8; } f()") === true);
row(run("function f() { var x=1; eval('var x;'); return x === 1 && !delete x; } f()") === true);
row(run("let z=9; function f() { let read=()=>()=>z; eval('var z=2'); return read()() === 2; } f()") === true);
row(run("let z=9; function f() { let read=()=>{ 'use strict'; return z; }; eval('var z=2'); return read() === 2; } f()") === true);
row(run("function f() { eval('eval(\\\"var z=3\\\")'); return z === 3; } f()") === true);
row(run("function f() { eval('var z=1'); return eval('eval(\\\"z=3\\\")') === 3 && z === 3; } f()") === true);
row(run("let z=8; function f() { eval('var z=1'); function g() { let read=()=>z; eval('var z=2'); return read() === 2; } return g() && z === 1; } f() && z === 8") === true);
row(run("function f() { eval('var z=2'); return (function() { let z=3; return eval('z'); })() === 3; } f()") === true);
row(run("let z=8; function f() { { let z=2; let caught=false; try { eval('var z'); } catch(e) { caught=e instanceof SyntaxError; } return caught && z === 2; } } f() && z===8") === true);
row(run("let z=8; function f() { eval('let z=2; (()=>{ try { eval(\\\"var z=3\\\"); return z === 3; } catch(e) { return false; } })()'); return typeof z === 'number'; } f()") === true);
row(run("let caught=false; try { { let z; eval('var z=1'); } } catch(e) { caught=e instanceof SyntaxError; } caught") === true);
row(run("function f() { return eval('function z() { return z; } var saved=z; z=3; saved')() === 3; } f()") === true);
row(run("function f() { var x=1; { const x=2; return eval('x') === 2; } } f()") === true);
row(run("function f() { eval('var z=1; eval(\\\"var z=3\\\")'); return z === 3; } f()") === true);
row(run("function f() { eval('var z=1'); z=eval('delete z; var other=1; 2'); return z===2; } f()") === true);
row(run("function f() { return eval('var z=(delete z,2); z') === 2; } f()") === true);
row(run("function f() { return eval('var [z=(delete z,2)]=[undefined]; z') === 2; } f()") === true);
row(run("function f() { const read=()=>z; eval('var z=1'); z=eval('delete z; eval(\\\"var z=3\\\"); 2'); return z===2 && read()===2; } f()") === true);
row(run("function F() { const read=()=>z; eval('var z=new.target'); return read; } new F()() === F") === true);
row(run("function F() { eval('var z=1'); this.ok=eval('eval(\\\"new.target\\\")') === F && z === 1; } new F().ok") === true);
row(run("let x=1; eval('x+=2'); x") === 3);
row(script("let x=0; eval('x=1; var injected107=2'); x === 1 && injected107 === 2 && delete injected107") === true);
row(run("function f() { let n=0; eval('while(n < 25) n++'); return n; } f()") === 25);
// Direct sites of this file's own code.
row((function () { var before = () => late; eval("var late = 4"); return before() === 4 && late === 4; })());
eval("var scripted858 = 1");
row(scripted858 === 1 && Object.getOwnPropertyDescriptor(globalThis, "scripted858").configurable && delete globalThis.scripted858);
failed === 0 ? rows : -failed;
