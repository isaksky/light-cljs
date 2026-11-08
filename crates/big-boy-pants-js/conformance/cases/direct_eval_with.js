// bbp-authored conformance case (sloppy script).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 16384-16395
// (with_statement_global_eval), 16399-16410 (with_statement_scope_boundaries)
// and the eval rows of with_statement_resolution (16224-16319), without a tagged
// template or Object.create. Eval code inside `with` resolves names through
// the object first; its `var` assigns the object's property and declares in
// the enclosing variable environment; function boundaries and parameter
// scopes stop the object. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
// Each row runs as one script: `run` as eval code of a direct eval in a
// fresh function, so its declarations stay its own; `script` through an
// indirect eval, as global code.
function run(source) { return eval(source); }
function script(source) { return (0, eval)(source); }
row(script("let o={x:1}; with(o){eval('var x=2');} o.x===2 && globalThis.x===undefined") === true);
row(script("with({x:1}){eval('var x; x===1')}") === true);
row(script("let f, o={x:1}; with(o){f=eval('var x; ()=>x');} o.x=3; f()===3") === true);
row(script("let o={x:1}; with(o){eval('function x(){return 3;}');} o.x===1 && globalThis.x()===3") === true);
row(script("let o={x:1}; with(o){eval('function x(){return 3;} x===1')}") === true);
row(script("let o={x:1}; with(o){eval('function x(){return 3;} var x=2');} o.x===2 && globalThis.x()===3") === true);
row(run("let f; with({x:1}){f=function(x){return x===2 && eval('x')===2;};} f(2)") === true);
row(run("let f; with({x:1}){f=(x)=>x===2 && eval('x')===2;} f(2)") === true);
row(run("let f; with({x:1}){f=function(){var x=2; return x===2 && eval('x')===2;};} f()") === true);
row(run("let f; with({x:1}){f=function(a=x,b=eval('x')){var x=2; return a===1 && b===1 && x===2 && eval('x')===2;};} f()") === true);
row(run("let f; with({x:1}){f=function(a=()=>x,b=eval('()=>x')){var x=2; return a()===1 && b()===1 && x===2 && eval('x')===2;};} f()") === true);
row(run("let f; with({x:1}){f=(a=x,b=eval('x'))=>a===1 && b===1;} f()") === true);
row(run("function test() {\n            var o1 = {x:'o1', y:'o1'}, x = 'local';\n            eval('var z=\"var_obj\"');\n            with (o1) {\n                if (x !== 'o1' || eval('x') !== 'o1') return false;\n                var f = function() {\n                    o2 = {x:'o2'};\n                    with (o2) {\n                        return x === 'o2' && y === 'o1' && z === 'var_obj'\n                            && eval('x') === 'o2' && eval('y') === 'o1'\n                            && eval('z') === 'var_obj' && eval('eval(\"x\")') === 'o2';\n                    }\n                };\n                if (!f()) return false;\n            }\n            return f();\n        } test()") === true);
row(run("let f, o={x:1}; with(o) { f=eval('(function() { return x; })'); } o.x=9; f()===9") === true);
row(run("let o={x:2, eval}; with(o) { eval('x')===2 && eval(...['x'])===2 }") === true);
row(run("function f(){var x=1, o={x:2}; with(o) { eval('var x=3; var y=4'); } return x===1 && o.x===3 && y===4;} f()") === true);
row(run("function f(){var x=1, o={x:2}; with(o) { return eval('let x=3; x')===3 && eval('\"use strict\"; x')===2; }} f()") === true);
// A direct site of this file's own code.
row((function () { var o = { x: 1 }; with (o) { eval("var x = 2"); } return o.x === 2; })());
failed === 0 ? rows : -failed;
