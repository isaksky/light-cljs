// bbp-authored conformance case (light-cljs-880).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 14321-14416, 15304-15369, 15587-15709:
// object_literals_and_named_properties (line 14321, bbp-114),
// property_access_on_values_that_are_not_ordinary_objects (15304) and
// computed_member_access (15587, bbp-115): literal keys of every kind
// (reserved words, string and numeric keys, escapes, shorthand, duplicates),
// named and computed reads and writes, canonical numeric key names, the
// TypeError of a null or undefined base (after the right-hand side runs), and
// evaluation order. Errors are compared by name (the Pants tests also check
// positions and messages). Left out: the `__rt` vector, map, builder,
// occurrence and atom rows (Pants runtime values, no script counterpart) and
// the opcode checks, and the canonical-key rows of computed_member_access
// that primitive_property_keys already pins (the accumulator, the
// reverse reads, '01' and two grammar lines). Each row is a `row(...)` check; the script completes with
// the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// object_literals_and_named_properties (line 14321, bbp-114).
{
  let o = {x: 1, if: 2, async: 3};
  row((o.x + o.if + o.async) === 6);
}
{
  let o = {x: 1, if: 2, async: 3};
  row((o.missing) === undefined);
}
{
  let o = {x: 1, if: 2, async: 3};
  row((o.y = 4) === 4);
}
{
  let o = {x: 1, if: 2, async: 3}; o.y = 4;
  row((o.y) === 4);
}
{
  let o = {x: 1, if: 2, async: 3}; o.x = 5;
  row((o.x + o.if) === 7);
}
row((({x: 1, x: 2}).x) === 2);
row((({a: {b: 2}}).a.b) === 2);
{
  let get = 1;
  row((({get}).get) === 1);
}
{
  let a = 1, b = 2; let p = {a, b, c: a + b};
  row((p.a + p.b * p.c) === 7);
}
row(({}) !== ({}));
{
  let o = {x: 1, if: 2, async: 3};
  row(o === o);
}
row((typeof {}) === "object");
row((({"b": 2, 3: 4, 1.50: 5, 0x10: 6}).b) === 2);
row((({"b": 2, 3: 4, 1.50: 5, 0x10: 6})["1.5"] + ({"b": 2, 3: 4, 1.50: 5, 0x10: 6})[16]) === 11);
{
  let k = {"if": 7};
  row((k.if) === 7);
}
row((({a: 1, \u0069f: 2}).if + ({n\u{65}w: 3}).new) === 5);
{
  let r = {get: 2, set: 3, async: 4, static: 5, new: 6, typeof: 7,};
  row((r.get + r.set + r.async + r.static + r.new + r.typeof) === 27);
}
{
  let p = {}, q = {}; p.v = q.v = 3;
  row((p.v + q.v) === 6);
}
{
  let m = {f: function (x) { return x * 2; }}; let f = m.f;
  row((f(21)) === 42);
}
{
  let c = {}; c.self = c;
  row((c.self.self.self === c ? 1 : 0) === 1);
}
{
  let u; let r = 'none'; try { u.x; } catch (e) { r = 'caught'; }
  row((r) === "caught");
}
{
  let r = 'none'; try { null.x = 1; } catch (e) { r = 'caught'; }
  row((r) === "caught");
}
{
  let u;
  row((name(() => u.x)) === "TypeError");
}
{
  let n = null;
  row((name(() => { n.x = 1; })) === "TypeError");
}
{
  let hit = 0; try { null.x = (hit = 1); } catch (e) {}
  row((hit) === 1);
}
{
  let log = 0; function f(n) { log = log * 10 + n; return n; } ({a: f(1), b: f(2)});
  row((log) === 12);
}
{
  let log = 0; function f(n) { log = log * 10 + n; return n; } let t = {}; function g() { f(1); return t; } function h() { f(2); return 9; } g().x = h();
  row((log * 100 + t.x) === 1209);
}
{
  let log = 0; function f(n) { log = log * 10 + n; return n; } let d = {x: f(1), x: f(2)};
  row((log * 10 + d.x) === 122);
}
// computed_member_access (line 15587, bbp-115).
{
  let o = {}; o['a' + 'b'] = 13;
  row((o.ab) === 13);
}
{
  let o = {x: 14};
  row((o[(0, 'x')]) === 14);
}
{
  let o = {};
  row(((o['z'] = 15) + o.z) === 30);
}
row(({})['nope'] === undefined);
row(({})[7] === undefined);
row(({})['toString'] === Object.prototype.toString);
{
  let o = {}; o['toString'] = 1;
  row(o.toString === 1 && ({}).toString !== 1);
}
{
  let o = {m: function () { return this; }}; let k = 'm';
  row(o[k]() === o);
}
{
  let o = {p: {q: 3}};
  row(o['p']['q'] === 3);
}
{
  let o = {a: {b: {c: 4}}}; let k = 'b';
  row(o.a[k].c === 4);
}
{
  let o = {f: function (n) { return n + 1; }};
  row(o['f'](1) === 2);
}
{
  let o = {}; o['p'] = {}; o['p']['q'] = 5;
  row(o.p.q === 5);
}
{
  let q = {}; let k = "y"; q[k] = 7;
  row(q.y === 7 && ({})["toString"] === Object.prototype.toString);
}
{
  let r = 'none'; try { null[0]; } catch (e) { r = 'caught'; }
  row((r) === "caught");
}
row((name(() => null[0])) === "TypeError");
{
  let u;
  row((name(() => { u['k'] = 1; })) === "TypeError");
}
{
  function run() { let log = ''; let o = {}; let f = () => { log = log + 'f'; return o; }; let g = () => { log = log + 'g'; return 'k'; }; let h = () => { log = log + 'h'; return 1; }; f()[g()] = h(); return log + o.k; }
  row((run()) === "fgh1");
}
{
  function run() { let log = ''; let o = {k: 2}; let f = () => { log = log + 'f'; return o; }; let g = () => { log = log + 'g'; return 'k'; }; return log + f()[g()] + log; }
  row((run()) === "2fg");
}
{
  let o = {};
  row((o[{}]) === undefined);
}
{
  let o = {}; o[o] = 1;
  row((o['[object Object]']) === 1);
}
{
  let o = {}; o[o]++;
  { const v = (o['[object Object]']); row(v !== v); }
}
{
  let o = {}; o[o] += 1;
  { const v = (o['[object Object]']); row(v !== v); }
}
// property_access_on_values_that_are_not_ordinary_objects (line 15304).
{
  let f = function () {};
  row(typeof f.bind === 'function');
}
{
  let b = true;
  row((b.x) === undefined);
}
{
  let r; try { true.x; } catch (e) { r = 1; }
  row((r) === undefined);
}
{
  let e; try { null.x; } catch (c) { e = c; }
  row((e.name) === "TypeError");
}
failed === 0 ? rows : -failed;
