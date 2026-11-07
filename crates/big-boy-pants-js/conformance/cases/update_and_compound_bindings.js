// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 13567-13666,
// update_expressions_on_bindings (bbp-64), and lines 14015-14155,
// compound_assignment_on_bindings (bbp-84): `++`/`--` and `+= -= *= /= %=`
// on `var`, `let`, parameter and captured bindings, and the TypeError of an
// update to a `const`. Rows that convert a string to a number are left out.
// Each row is a `row(...)` check; the script completes with the number of
// rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
function negativeZero(n) { return n === 0 && 1 / n === -Infinity; }
// `test_inc_dec`, identifier groups, on `let` and `var`.
{ let a, r; a = 1; r = a++; row(r === 1 && a === 2); }
{ let a, r; a = 1; r = ++a; row(r === 2 && a === 2); }
{ let a, r; a = 1; r = a--; row(r === 1 && a === 0); }
{ let a, r; a = 1; r = --a; row(r === 0 && a === 0); }
var v = 5, w;
w = v++; row(w === 5 && v === 6);
w = --v; row(w === 5 && v === 5);
// A postfix update yields the old value converted to a number.
{ let t = true; let r = t++; row(r === 1 && t === 2); }
{ let f = false; let r = f--; row(r === 0 && f === -1); }
{ let n = null; let r = n++; row(r === 0 && n === 1); }
{ let n = null; row(++n === 1); }
{ let u; row(u++ !== u); }
{ let u; let r = u++; row(r !== r && u !== u); }
{ let u; ++u; row(u !== u); }
row(name(() => { let x = undefined; x++; }) === "none");
// Signs, fractions and 2^53.
{ let z = -0; z++; row(z === 1); }
{ let z = 0; row(--z === -1); }
{ let z = -0; row(negativeZero(z--) && z === -1); }
{ let x = 1.5; x--; row(x === 0.5); }
{ let big = 9007199254740992; big++; row(big === 9007199254740992); }
// Parenthesized targets, parameters, and a line break before `++`.
{ let p = 1; (p)++; --((p)); ++(p); row(p === 2); }
{ function g(x) { x--; return x; } row(g(3) === 2); }
{ let a = 1, b = 1
  a
  ++b
  row(b === 2 && a === 1); }
// Postfix binds tighter than binary operators.
{ let a = 1, b = 1; let c = a+++b; row(c === 2 && a === 2 && b === 1); }
// A loop counter.
{ let i = 0, s = 0; while (i < 5) { s = s + i++; } row(s * 10 + i === 105); }
// Closure-captured counters.
{ function mk() { let c = 0; return function () { return ++c; }; } const f = mk(); f(); row(f() === 2); }
{ let c = 10; const dec = () => c--; dec(); row(dec() * 100 + c === 908); }
{ var vc = 0; const inc = () => vc++; inc(); inc(); row(vc === 2); }
// `const`: the read and conversion happen, then the TypeError.
row(name(() => { const c = 1; c++; }) === "TypeError");
row(name(() => { const c = 1; --c; }) === "TypeError");
{ const c = 1; let r = 0; try { c++; } catch (e) { r = 1; } row(r * 10 + c === 11); }
{ const k = 1; const g = () => k++; row(name(g) === "TypeError" && k === 1); }
{ let r = false; const c = 1; try { c--; } catch (e) { r = e.name === "TypeError"; } row(r); }
// Temporal dead zone and unresolvable names.
row(name(() => { x++; let x = 1; }) === "ReferenceError");
row(name(() => { function g() { return ++q; } g(); let q = 1; }) === "ReferenceError");
row(name(() => nope++) === "ReferenceError");
row(name(() => --nope) === "ReferenceError");
// Compound assignment.
{ let a = 7; a += 3; a -= 1; a *= 2; a /= 3; a %= 4; row(a === 2); }
{ let b = 1; let r = (b += 2); row(r === 3 && b === 3); }
{ let a = 10, b = 2; a -= b *= 3; row(a === 4 && b === 6); }
{ let a = 1, b = 2; a += b += 3; row(a === 6 && b === 5); }
{ let p = 2; (p) *= 5; ((p)) -= 1; row(p === 9); }
{ function g(x) { x %= 3; return x; } row(g(7) === 1); }
{ let t = true; t += 1; row(t === 2); }
{ let n = null; n -= 1; row(n === -1); }
{ let f = false; f *= 5; row(f === 0); }
{ let u; u += 1; row(u !== u); }
{ let u; u -= 1; row(u !== u); }
{ let a = 6; a /= 4; row(a === 1.5); }
{ let a = -7; a %= 2; row(a === -1); }
{ let a = 1; a /= 0; row(a === Infinity); }
{ let a = 0; a /= 0; row(a !== a); }
v = 4; v *= v; row(v === 16);
// The old value is read before the right operand runs.
{ let a = 1; function f() { a = 10; return 5; } a += f(); row(a === 6); }
{ let a = 2; a *= (a = 5) + 1; row(a === 12); }
// `+=` on strings concatenates.
{ let s = ""; s += "a"; s += "b"; row(s === "ab"); }
{ let s = ""; row((s += "a") + "c" === "ac"); }
// Closure-captured bindings.
{ function mk() { let c = 0; return (d) => c += d; } const f = mk(); f(2); row(f(3) * 10 + f(0) === 55); }
{ let total = 1; const twice = () => { total *= 2; }; twice(); twice(); row(total === 4); }
// A compound assignment to a `const` is a TypeError, after the right side.
{ let ran = 0; const c = 1; let r = name(() => { c += (ran = 1); }); row(r === "TypeError" && ran === 1 && c === 1); }
row(name(() => { const c = 4; c %= 2; }) === "TypeError");
row(name(() => nope += 1) === "ReferenceError");
failed === 0 ? rows : -failed;
