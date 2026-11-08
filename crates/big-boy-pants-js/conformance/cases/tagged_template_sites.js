// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 5968-5995,
// tagged_templates (the site identity, receiver, evaluation order, chained
// and `new` rows; its String.raw and Object.isFrozen rows are left out,
// both pending on Goose), and light-cljs-862. GetTemplateObject (ECMAScript
// 13.2.8.4) gives a site the same template object every time it runs, in
// every closure of its function; two sites with the same text differ; each
// eval or Function compilation makes new sites. The tag reference is read
// first, then the template object is made, then the substitutions run,
// unconverted. Each row is a `row(...)` check; the script completes with the
// number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function tag(s) { return s; }
// One site, run again: from one function, a loop, closures of one function.
row((function () { function site() { return tag`x${1}y`; } var a = site(), b = site(); return a === b && a.length === 2; })());
row((function () { var seen = []; for (var i = 0; i < 4; i++) seen.push(tag`l${i}`); return seen[0] === seen[1] && seen[1] === seen[2] && seen[2] === seen[3]; })());
row((function () { function make() { return function () { return tag`c`; }; } return make()() === make()(); })());
row((function () { var log = []; function site(k) { log.push(tag`s${k}`); } site(1); site("two"); site({}); return log[0] === log[1] && log[1] === log[2]; })());
// A recursive function's site is one site at every depth.
row((function () { function down(n, first) { var t = tag`d`; return n === 0 ? t === first : down(n - 1, first || t); } return down(5); })());
// Distinct sites, even with the same text.
row((function () { var a = tag`x`, b = tag`x`; return a !== b && a[0] === b[0] && a.raw !== b.raw; })());
row((function () { function site() { return tag`same`; } return site() !== tag`same`; })());
// Every eval or Function compilation has its own sites; within one, a
// function's site keeps its object.
row((function () { var x = eval("tag`q`"); return x !== eval("tag`q`") && x[0] === "q"; })());
row(eval("var h = function () { return tag`e`; }; h() === h()"));
row((function () { var k = []; for (var i = 0; i < 2; i++) k.push(new Function("t", "return t`z`")(tag)); return k[0] !== k[1]; })());
row((function () { var f = new Function("t", "return () => t`z`")(tag); return f() === f(); })());
// The tag call: arguments, receiver, evaluation order.
row((function () { function t(s, a, b) { return s.length + ":" + a + "," + b + ":" + arguments.length; } return t`a${1}b${"x"}c` === "3:1,x:3"; })());
row((function () { function t(s) { return arguments.length; } return t`` === 1 && t`${1}${2}` === 3; })());
row((function () { var o = { t(s, x) { return this === o && x === 3; } }; return o.t`x${3}` && o["t"]`y${3}`; })());
row((function () { "use strict"; function t() { return this; } return t`x` === undefined; })());
row((function () { var log = ""; var o = { get t() { log += "g"; return function (s, x) { log += "t"; return this === o; }; } }; return o.t`${log += "s"}` && log === "gst"; })());
row((function () { var n = 0; function t(s, x) { return x; } var x = { toString() { n++; return "x"; } }; return t`${x}` === x && n === 0; })());
row((function () { var log = ""; function t(s, x) { log += s[0]; return function (u, y) { log += u[0]; return x + y; }; } return t`a${log += "1"}``b${log += "2"}` === "11a2" && log === "1a2b"; })());
row((function () { function t(s) { return function C(x) { this.x = x; }; } return new t`x`(42).x === 42; })());
// A tag that is not callable throws TypeError after the substitutions ran.
row((function () { var log = ""; var t = 5; try { t`${log += "s"}`; return false; } catch (e) { return e instanceof TypeError && log === "s"; } })());
failed === 0 ? rows : -failed;
