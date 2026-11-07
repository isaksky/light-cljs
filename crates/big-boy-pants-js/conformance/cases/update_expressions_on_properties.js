// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 13694-13780,
// update_expressions_on_properties (bbp-76): `++`/`--` on `o.p` and `o[k]`,
// the seven test_inc_dec member groups, the base and the key evaluated once,
// an absent property giving NaN, a key object converted once, a getter and
// setter observed through a log, and the TypeErrors of a null or undefined
// base and of a getter-only target in strict code. Each row is a `row(...)`
// check; the script completes with the number of rows, or minus the first
// failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// test_inc_dec, member groups.
{ let a = {x: true}; a.x++; row(a.x === 2); }
{ let a = {x: true}; a.x--; row(a.x === 0); }
{ let a = [true]; a[0]++; row(a[0] === 2); }
{ let a, r; a = {x: true}; r = a.x++; row(r === 1 && a.x === 2); }
{ let a, r; a = {x: true}; r = a.x--; row(r === 1 && a.x === 0); }
{ let a, r; a = [true]; r = a[0]++; row(r === 1 && a[0] === 2); }
{ let a, r; a = [true]; r = a[0]--; row(r === 1 && a[0] === 0); }
// The key and the base are evaluated once.
{ let i = 0; const a = [1, 1]; a[i = i + 1]++; row(i === 1 && a[1] === 2); }
{ let a = [0, 0], i = 0; a[i++]++; row(i === 1 && a[0] === 1 && a[1] === 0); }
{ let n = 0; const o = {x: 1}; function get() { n = n + 1; return o; } get().x++; row(n === 1 && o.x === 2); }
{ let n = 0; const o = {x: 1}; const get = () => { n++; return o; }; let r = --get()["x"]; row(n === 1 && r === 0 && o.x === 0); }
// Prefix updates give the new number.
{ let o = {p: true}; let r = ++o.p; row(r === 2 && o.p === 2); }
{ let o = {p: 1}; let r = --o["p"]; row(r === 0 && o.p === 0); }
// Parenthesized targets, chains and nested references.
{ let o = {p: 1}; (o.p)++; ++(o["p"]); --((o).p); row(o.p === 2); }
{ let o = {a: {b: [1, 2]}}; o.a.b[1]++; ++o["a"].b[0]; row(o.a.b[0] === 2 && o.a.b[1] === 3); }
{ let a = [[5]]; a[0][a[0][0]++ - 5]--; row(a[0][0] === 5); }
// A string key that is an array index, and a numeric name.
{ let a = [0]; a["0"]++; row(a[0] === 1); }
{ let o = {}; o[1.5] = 1; o[1.5]++; row(o["1.5"] === 2); }
{ let a = []; a[2]++; row(a.length === 3 && a[2] !== a[2]); }
{ let o = {n: 0}; for (let i = 0; i < 5; i++) o.n++; row(o.n === 5); }
{ let o = {a: 1, b: 1}; let c = o.a+++o.b; row(c === 2 && o.a === 2 && o.b === 1); }
// An absent property reads as undefined: NaN is yielded and written.
{ let o = {}; let r = o.x++; row(r !== r && o.x !== o.x); }
{ let o = {}; let r = ++o["y"]; row(r !== r); }
{ let o = {z: -0}; let r = o.z--; row(1 / r === -Infinity && o.z === -1); }
{ let o = {n: null}; o.n++; row(o.n === 1); }
{ let o = {p: {}}; o.p++; row(o.p !== o.p); }
// A key object converts once per reference, before the read.
{ let log = ""; const o = {p: 1}; const k = {toString() { log += "k"; return "p"; }}; o[k]++; let r = o[k]--; row(log === "kk" && r === 2 && o.p === 1); }
// The read goes through a getter and the write through a setter, once each.
{ let log = ""; const o = {v: 3, get x() { log += "g"; return this.v; }, set x(n) { log += "s" + n; this.v = n; }};
  let r = o.x++; let s = ++o["x"]; row(log === "gs4gs5" && r === 3 && s === 5 && o.v === 5); }
// A null or undefined base is a catchable TypeError, before the key converts.
row(name(() => { let o = null; o.x++; }) === "TypeError");
row(name(() => { let u; --u[0]; }) === "TypeError");
{ let log = ""; let u; const k = {toString() { log += "k"; return "p"; }}; row(name(() => u[k]++) === "TypeError" && log === ""); }
// A getter-only target is a TypeError in strict code, after the read.
{ let log = ""; const o = {get k() { log += "g"; return 1; }}; row(name(() => o.k++) === "TypeError" && name(() => --o["k"]) === "TypeError" && log === "gg" && o.k === 1); }
// So is a non-writable data property (a function's length); it keeps its value.
{ function f(a) {} const k = "length"; row(name(() => f[k]++) === "TypeError" && name(() => --f.length) === "TypeError" && f.length === 1); }
failed === 0 ? rows : -failed;
