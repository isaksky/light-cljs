// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 3085-3122:
// array_spread_and_spread_arguments (3085), rows 3, 4 and 7 and the
// `(function() {})(...null)` half of row 6, and spread_protocol_errors_and_order
// (3102): the `f(...[], ...[1], ...[2, 3])` row, the evaluation-order row and
// the 65,536-element RangeError row; bbp-130's spread-arguments half
// (light-cljs-850). Calls, method calls (named and computed), `new`, empty and
// string spreads, spread `arguments`, built-in callees, Function.prototype.call
// forwarding and conversion of a spread first argument. Left out: the
// `bind` half of the `bind` and `Reflect.apply` row (light-cljs-852) and
// the 128-value stack-limit row (a custom limit of the Pants test
// runtime). Each row is a
// `row(...)` check; the script completes with the number of rows, or minus the
// first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
{ function f(a, b, c) { return a + b + c; } row(f(...[1, 2], 3) === 6); }
{ function f(a, b, c) { return a + b * 10 + c * 100; } row(f(...[1, 2], 3) === 321); }
{ var o = { m(a) { return this === o && a === 1; } }; row(o.m(...[1]) && o['m'](...[1])); }
{ function C(a, b) { this.s = a + b; } row(new C(...[1, 2]).s === 3); }
{ function F(a, b) { this.n = a + b; } row(new F(...[2, 3]).n === 5); }
{ function F() { return new.target; } row(new F(...[]) === F); }
{ function f() { return arguments.length; } row(f(...'abc', ...[1]) === 4); }
{ function f() { return arguments.length; } row(f(...[]) === 0); }
{ function f(a, b, c) { return a + b * 10 + c * 100; } row(f(...[], ...[1], ...[2, 3]) === 321); }
{ function f(a, b) { return b === undefined && arguments.length === 2; } row(f(...[1, ,])); }
{ function f(a, b) { return arguments.length === 2 && b === '\u{1f600}'; } row(f(...'A\u{1f600}')); }
{ function g(x, y, z) { return x + y + z; } function f() { return g(...arguments); } row(f(1, 2, 3) === 6); }
{ function* gen() { yield 1; yield 2; } function f(a, b) { return a + b; } row(f(...gen()) === 3); }
{
  let log = '';
  function v(n) { log += n; return n; }
  function f() { return arguments[0] + arguments[1] + arguments[2]; }
  let it = { next() { log += 'n'; return { done: log.length > 2, value: 2 }; } };
  it[Symbol.iterator] = function () { log += 'i'; return this; };
  f(v(1), ...it, v(3));
  row(log === '1in3');
}
{
  let log = '';
  function f() { log += 'f'; return 0; }
  function g() { log += 'g'; return f; }
  g()(...(log += 'a', [1]));
  row(log === 'gaf');
}
row(name(() => (function () {})(...null)) === "TypeError");
row(name(() => (function () {})(...1)) === "TypeError");
row(name(() => { const x = 1; x(...[1]); }) === "TypeError");
row(name(() => new (() => 1)(...[])) === "TypeError");
// Too many arguments: the Pants engine and Goose refuse more than 65,535
// during collection, after exactly 65,536 `next` calls; QuickJS collects the
// whole iterator and then refuses more than 65,534 (its original row, an
// endless iterator, never ends there), so the iterator ends after 70,000.
{
  let n = 0;
  let it = { next() { return { value: ++n, done: n > 70000 }; } };
  it[Symbol.iterator] = function () { return this; };
  let caught = false;
  try { (function () {})(...it); } catch (e) { caught = e instanceof RangeError; }
  row(caught && (n === 65536 || n === 70001));
}
{ function f() { return arguments.length; } row(f(...new Array(65534)) === 65534); }
// Built-in callees and Function.prototype.call forwarding.
row(Array.isArray(...[[]]));
row(Object.is(...[NaN, NaN]));
row(String(...[{ toString() { return 'x'; } }]) === 'x');
row(Number(...[{ valueOf() { return 4; } }, 9]) === 4);
row(new Number(...[{ valueOf() { return 5; } }]).valueOf() === 5);
row(new Array(...[1, 2, 3]).length === 3);
row(new Error(...['m']).message === 'm');
{ function f(a, b) { return this.n + a + b; } row(f.call(...[{ n: 1 }, 2, 3]) === 6); }
{ function f(a, b) { return this.n + a + b; } let o = {n: 10}; row(Reflect.apply(f, o, [...[1, 2]]) === 13 && f.apply(...[o, [1, 2]]) === 13); }
{ var o = { n: 4, m(a) { return this.n + a; } }; row(o.m.call(...[o, 1]) === 5); }
row(String.call(...[undefined, { toString() { return 'y'; } }]) === 'y');
{
  let a = [1];
  a.push(...[2, 3]);
  row(a.length === 3 && a[2] === 3);
}
// A direct-eval site whose callee is no longer %eval% is an ordinary spread
// call (QuickJS's test_eval2 binds `eval` as a sloppy parameter instead).
{
  const original = globalThis.eval;
  globalThis.eval = function (a, b) { return a + b; };
  const sum = eval(...[1, 2]);
  let hook = 0;
  globalThis.eval = String;
  const text = eval(...[{ toString() { hook++; return 'z'; } }]);
  globalThis.eval = original;
  row(sum === 3 && text === 'z' && hook === 1);
}
failed === 0 ? rows : -failed;
