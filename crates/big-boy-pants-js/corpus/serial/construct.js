// [[Construct]] (light-cljs-831): `new` on ordinary functions allocates an
// object inheriting F.prototype, runs F with it as `this` and F as
// new.target, and gives it unless F returns an object, also through a
// finally block and across a generator that yields inside the constructor;
// the realm's Object, Array and Error construct; a non-constructor is a
// catchable TypeError after its arguments; an exception leaves the
// constructor. Every budget cut matches the uncut run (serial_tests.goose).
function construct() {
  "use strict";
  function F(x) { this.x = x; this.t = new.target; }
  function G() { this.y = 1; return { y: 2 }; }
  function H() { this.z = 1; return 5; }
  function T() { try { this.a = 1; return 2; } finally { this.b = 3; } }
  function N() {} N.prototype = 5;
  function L(n) { this.n = n; this.next = n > 0 ? new L(n - 1) : null; }
  function* steps() { yield 1; yield 2; }
  function Y() { const it = steps(); this.first = it.next().value; this.second = it.next().value; this.done = it.next().done; }
  function E() { this.e = 1; throw "boom"; }
  function called() { return [this, new.target]; }
  const f = new F(2); const t = new T(); const l = new L(2); const y = new Y();
  let thrown = ""; try { new E(); } catch (e) { thrown = e; }
  let order = ""; const o = {};
  try { new o((order += "a", 1)); } catch (e) { order += e.name; }
  const c = called(); const err = new TypeError("t");
  return f.x === 2 && f.t === F && Object.getPrototypeOf(f) === F.prototype && f.constructor === F &&
    new G().y === 2 && new H().z === 1 && t.a * 10 + t.b === 13 &&
    Object.getPrototypeOf(new N()) === Object.prototype && l.n + l.next.n + l.next.next.n === 3 && l.next.next.next === null &&
    y.first === 1 && y.second === 2 && y.done === true && thrown === "boom" && order === "aTypeError" &&
    c[0] === undefined && c[1] === undefined && Object.getPrototypeOf(new Object()) === Object.prototype &&
    new Array(3).length === 3 && err.message === "t" && Object.getPrototypeOf(err) === TypeError.prototype;
}
