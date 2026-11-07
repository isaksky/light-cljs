// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/fixtures/js/logical_assignment.js lines 24-45
// (its property rows): `&&=`, `||=` and `??=` on `o.p` and `o[k]`, short
// circuiting without a write, the base and the key evaluated once, a key
// object converted once, a getter and setter observed through a log, and a
// getter-only target in strict code. Each row is a `row(...)` check; the
// script completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
{ let o = {a: 1, b: 0, c: null}; o.a &&= 5; o.b &&= 5; o.c ??= 7; row(o.a === 5 && o.b === 0 && o.c === 7); }
{ let o = {a: 1, b: 0}, k = "a", j = "b"; o[k] ||= 5; o[j] ||= 5; row(o.a === 1 && o.b === 5); }
{ let o = {a: 0, b: undefined}; let r = (o["a"] ??= 9); let s = (o["b"] ??= 9); row(r === 0 && s === 9 && o.a === 0 && o.b === 9); }
{ let a = [0, 1]; a[0] ||= "x"; a[1] &&= "y"; a[2] ??= "z"; row(a[0] === "x" && a[1] === "y" && a[2] === "z"); }
// The right side runs only when the assignment happens.
{ let n = 0; const o = {p: 1}; o.p ||= (n++, 2); o["p"] ??= (n++, 3); o["q"] &&= (n++, 4); row(n === 0 && o.p === 1 && !("q" in o)); }
// The base and the key are evaluated once.
{ let log = ""; const o = {x: 0}; function base() { log += "b"; return o; } function key() { log += "k"; return "x"; }
  base()[key()] ||= (log += "r", 5); row(log === "bkr" && o.x === 5); }
{ let n = 0; const o = {}; const base = () => { n++; return o; }; base().y ??= 1; base()["y"] ??= 2; row(n === 2 && o.y === 1); }
{ let i = 0; const a = [0, 0]; a[i++] ||= 4; row(i === 1 && a[0] === 4 && a[1] === 0); }
// A key object converts once per reference, before the read.
{ let log = ""; const o = {p: null}; const k = {toString() { log += "c"; return "p"; }}; o[k] ??= (log += "r", 1); o[k] ??= (log += "r", 2); row(log === "crc" && o.p === 1); }
// The read goes through a getter; the write, only when it happens, through a setter.
{ let log = ""; const o = {v: 0, get x() { log += "g"; return this.v; }, set x(n) { log += "s" + n; this.v = n; }};
  o.x ||= 3; o["x"] ||= 4; o.x &&= 5; o["x"] ??= 6; row(log === "gs3ggs5g" && o.v === 5); }
// A null or undefined base is a TypeError.
row(name(() => { let o = null; o.x ||= 1; }) === "TypeError");
row(name(() => { let u; u["k"] ??= 1; }) === "TypeError");
// A getter-only target is a TypeError in strict code when the write happens.
{ let log = ""; const o = {get k() { log += "g"; return 0; }}; let r = (o["k"] &&= 1);
  row(r === 0 && name(() => { o["k"] ||= 1; }) === "TypeError" && name(() => { o.k ??= 1; }) === "none" && log === "ggg"); }
failed === 0 ? rows : -failed;
