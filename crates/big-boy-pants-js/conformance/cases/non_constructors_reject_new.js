// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 1475-1520,
// non_constructors_reject_new (bbp-71), and rust/src/vm/dispatch.rs
// construct_target and not_a_constructor (ECMAScript EvaluateNew: IsConstructor
// is checked after the callee and its arguments are evaluated): arrows,
// methods, getters, setters, generator and async functions, built-in
// functions such as Object.is, Symbol, primitives and ordinary objects throw
// a catchable TypeError; the callee is read once and the arguments are
// evaluated but nothing is called. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
row(name(() => { function* G() {} new G(); }) === "TypeError");
row(name(() => { let G = function* () {}; new G(); }) === "TypeError");
row(name(() => { async function A() {} new A(); }) === "TypeError");
row(name(() => { let A = async function () {}; new A(); }) === "TypeError");
row(name(() => new (() => 1)) === "TypeError");
row(name(() => { let a = () => {}; new a(); }) === "TypeError");
row(name(() => { let a = async () => 1; new a(); }) === "TypeError");
row(name(() => new ({m() {}}).m()) === "TypeError");
row(name(() => { let o = {m() {}}; new o.m(); }) === "TypeError");
row(name(() => { let o = {*g() {}}; new o.g(); }) === "TypeError");
row(name(() => { let o = {async m() {}}; new o.m(); }) === "TypeError");
row(name(() => { let o = {get x() { return 1; }, set x(v) {}}; let d = Object.getOwnPropertyDescriptor(o, "x"); new d.get(); }) === "TypeError");
row(name(() => { let o = {set x(v) {}}; let d = Object.getOwnPropertyDescriptor(o, "x"); new d.set(); }) === "TypeError");
row(name(() => new Object.is()) === "TypeError");
row(name(() => new Object.getPrototypeOf({})) === "TypeError");
row(name(() => new Object.prototype.hasOwnProperty("x")) === "TypeError");
row(name(() => new Symbol()) === "TypeError");
row(name(() => new 1) === "TypeError");
row(name(() => new "s") === "TypeError");
row(name(() => new true) === "TypeError");
row(name(() => new null) === "TypeError");
row(name(() => new undefined) === "TypeError");
row(name(() => new Symbol.iterator) === "TypeError");
row(name(() => new ({})) === "TypeError");
row(name(() => new ([])) === "TypeError");
// The error is a TypeError object with a message.
{ let e; try { new Object.is(); } catch (x) { e = x; } row(typeof e === "object" && Object.getPrototypeOf(e) === TypeError.prototype && typeof e.message === "string"); }
// Ordinary functions, function expressions and `function` properties construct.
row((() => { function F() { this.x = 7; } return new F().x === 7; })());
row((() => { let F = function () { this.x = 7; }; return new F().x === 7; })());
row((() => { let o = {m: function () { this.x = 7; }}; return new o.m().x === 7; })());
// The arguments are evaluated, the callee is not called.
{ let called = false, a = () => { called = true; }; let n = 0; try { new a(++n); } catch (e) {} row(!called && n === 1); }
{ let reads = 0, args = 0; let o = {get m() { reads++; return () => 1; }}; try { new o.m(++args); } catch (e) {} row(reads === 1 && args === 1); }
{ let log = ""; const o = {}; function arg() { log += "a"; return {valueOf() { log += "v"; return 1; }}; } try { new o(arg()); } catch (e) { log += e.name; } row(log === "aTypeError"); }
failed === 0 ? rows : -failed;
