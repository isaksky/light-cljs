// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 14015-14155,
// compound_assignment_on_bindings (bbp-84): `&= |= ^= <<= >>= >>>= **=` on
// `let`, `var`, parameter and captured bindings, the value of the
// assignment, its order (the old value read before the right operand) and
// the TypeError of one on a `const`. Each row is a `row(...)` check; the
// script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
{ let a = 3; a **= 2; row(a === 9); }
{ let a = 6; a &= 3; row(a === 2); }
{ let a = 6; a |= 1; row(a === 7); }
{ let a = 6; a ^= 3; row(a === 5); }
{ let a = 1; a <<= 4; row(a === 16); }
{ let a = -16; a >>= 2; row(a === -4); }
{ let a = -1; a >>>= 28; row(a === 15); }
{ let s = " 0x10 "; s |= 1; row(s === 17); }
{ let t = true; t <<= 3; row(t === 8); }
{ let u; u |= 0; row(u === 0); }
{ let u; u **= 0; row(u === 1); }
var v = 2; v **= 10; v >>>= 1; v ^= 1; row(v === 513);
{ let b = 5; let r = (b &= 6); row(r === 4 && b === 4); }
{ let a = 1, b = 2; a <<= b |= 1; row(a === 8 && b === 3); }
{ let p = 2; (p) **= 3; ((p)) >>= 1; row(p === 4); }
{ function g(x) { x ^= 255; return x; } row(g(15) === 240); }
// The old value is read before the right operand runs.
{ let a = 1; function f() { a = 10; return 4; } a <<= f(); row(a === 16); }
{ let a = 2; a **= (a = 3); row(a === 8); }
// Captured bindings.
{ function mk() { let c = 1; return (d) => c <<= d; } const f = mk(); f(2); row(f(1) === 8); }
{ let m = 0xFF; const clear = (bit) => { m &= ~(1 << bit); }; clear(0); clear(7); row(m === 126); }
// An object operand converts through valueOf.
{ let a = { valueOf: function () { return 12; } }; a &= 10; row(a === 8); }
// A compound assignment to a `const` is a TypeError, after the right side.
{ let ran = 0; const c = 1; let r = name(() => { c |= (ran = 1); }); row(r === "TypeError" && ran === 1 && c === 1); }
row(name(() => { const k = 2; k **= 2; }) === "TypeError");
row(name(() => nope >>>= 1) === "ReferenceError");
failed === 0 ? rows : -failed;
