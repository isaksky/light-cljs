// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 13824-13960,
// compound_assignment_on_properties (bbp-260): `op=` on `o.p` and `o[k]`, the
// base and the key once, the key converted once, then the read, the right
// operand, the operator and the write (observed through a log), getters and
// setters, and the TypeErrors of a null or undefined base and of a
// getter-only target in strict code. Each row is a `row(...)` check; the
// script completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
{ let o = {x: 1}; o.x += 2; row(o.x === 3); }
{ let o = {x: 3}; o["x"] *= 2; row(o.x === 6); }
// The key is evaluated once.
{ let a = [1, 1]; let i = 0; a[i = i + 1] += 4; row(i === 1 && a[1] === 5 && a[0] === 1); }
{ let a = [0, 0], i = 0; a[i++] -= 1; row(i === 1 && a[0] === -1 && a[1] === 0); }
// The base is evaluated once.
{ let n = 0; const o = {x: 1}; function get() { n = n + 1; return o; } get().x += 1; row(n === 1 && o.x === 2); }
{ let n = 0; const o = {x: 1}; const get = () => { n++; return o; }; get()["x"] **= 3; get()["x"] += get().x; row(n === 3 && o.x === 2); }
// The expression's value is the written value; right-associative.
{ let o = {x: 1}; let r = (o.x += 2); row(r === 3 && o.x === 3); }
{ let o = {a: 10, b: 2}; o.a -= o.b *= 3; row(o.a === 4 && o.b === 6); }
{ let o = {a: 1}, b = 2; o.a += b += 3; row(o.a === 6 && b === 5); }
{ let a = [1]; let b = 2; b *= a[0] += 4; row(b === 10 && a[0] === 5); }
// Parenthesized targets, chains and nested references.
{ let o = {p: 2}; (o.p) *= 5; ((o)["p"]) -= 1; row(o.p === 9); }
{ let o = {a: {b: [1, 2]}}; o.a.b[1] += 10; o["a"].b[0] -= 1; row(o.a.b[0] === 0 && o.a.b[1] === 12); }
{ let a = [[5]]; a[0][a[0][0] -= 5] += 1; row(a[0][0] === 1); }
// Every operator, on a computed key.
{ let o = {v: 7}, k = "v"; o[k] -= 1; o[k] /= 4; o[k] %= 1; row(o.v === 0.5); }
{ let o = {v: 2}, k = "v"; o[k] **= 10; row(o.v === 1024); }
{ let o = {v: 12}, k = "v"; o[k] &= 10; o[k] |= 1; o[k] ^= 3; row(o.v === 10); }
{ let o = {v: 1}, k = "v"; o[k] <<= 4; o[k] >>= 1; row(o.v === 8); }
{ let o = {v: -16}, k = "v"; o[k] >>>= 28; row(o.v === 15); }
// `+=` concatenates when either value is a string; the others apply ToNumber.
{ let o = {s: "a"}; o.s += 1; o["s"] += true; row(o.s === "a1true"); }
{ let o = {n: 1}; o["n"] += "2"; row(o.n === "12"); }
{ let o = {s: "6"}; o["s"] *= "2"; row(o.s === 12); }
{ let o = {t: true, u: null}; o["t"] += 1; o["u"] -= 1; row(o.t === 2 && o.u === -1); }
{ let a = [1]; a["0"] += 1; row(a[0] === 2); }
{ let o = {}; o[1.5] = 1; o[1.5] += 1; row(o["1.5"] === 2); }
{ let o = {n: 0}, a = [0]; for (let i = 1; i <= 4; i++) { o.n += i; a[0] -= i; } row(o.n === 10 && a[0] === -10); }
// An absent property reads as undefined.
{ let o = {}; o["x"] += 1; row(o.x !== o.x); }
{ let o = {}; o["k"] += "v"; row(o.k === "undefinedv"); }
{ let a = []; a[2] |= 4; row(a.length * 10 + a[2] === 34); }
// The order: base, key, read, right operand, write.
{ let log = ""; const o = {x: 1};
  function base() { log += "b"; return o; } function key() { log += "k"; return "x"; }
  function rhs() { log += "r" + o.x; o.x = 100; return 2; }
  let r = base()[key()] += rhs(); log += "," + r + "," + o.x; row(log === "bkr1,3,3"); }
{ let log = ""; const o = {x: 1}; const base = () => { log += "b"; return o; };
  base().x *= (log += "r" + o.x, o.x = 7, 5); log += "," + o.x; row(log === "br1,5"); }
// A key object converts once, after the key expression and before the read.
{ let log = ""; const o = {p: 1}; const k = {toString() { log += "c"; return "p"; }};
  o[(log += "k", k)] += (log += "r", 2); row(log === "kcr" && o.p === 3); }
{ let log = ""; const o = {v: 10, get x() { log += "g"; return this.v; }, set x(n) { log += "s" + n; this.v = n; }};
  let r = (o["x"] -= (log += "r", 3)); row(log === "grs7" && r === 7 && o.v === 7); }
// A null or undefined base is a catchable TypeError, after the key but
// before the right operand runs.
row(name(() => { let o = null; o.x += 1; }) === "TypeError");
{ let log = "", u; try { u[(log += "k", "p")] += (log += "r", 1); } catch (e) { log += "c"; } row(log === "kc"); }
{ let log = ""; try { null.x *= (log += "r", 1); } catch (e) { log += "c"; } row(log === "c"); }
// A getter-only target is a TypeError in strict code, after the right operand.
{ let log = ""; const o = {get k() { log += "g"; return 1; }};
  try { o["k"] += (log += "r", 1); } catch (e) { log += e.name; } row(log === "grTypeError" && o.k === 1); }
// So is a non-writable data property (a function's length), after the right operand.
{ let log = ""; function f(a, b) {} const k = "length";
  try { f[k] *= (log += "r", 3); } catch (e) { log += e.name; } row(log === "rTypeError" && f.length === 2); }
// And one Object.defineProperty made non-writable (light-cljs-881).
{ let log = ""; const o = {}; Object.defineProperty(o, "k", {value: 2});
  try { o["k"] *= (log += "r", 3); } catch (e) { log += e.name; } row(log === "rTypeError" && o.k === 2); }
failed === 0 ? rows : -failed;
