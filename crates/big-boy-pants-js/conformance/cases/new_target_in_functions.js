// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/src/vm/dispatch.rs lines 1060-1078
// and 1509 (construct_target sets the frame's new_target; GetNewTarget,
// bbp-99) and rust/src/frontend/lower.rs resolve_new_target (an arrow
// captures its function's new.target): ECMAScript GetNewTarget. new.target is
// the constructor in a construction and undefined in a call, a method call
// or a call through Function.prototype.call; an arrow sees its enclosing
// function's, also after that function returned; each activation has its
// own. Each row is a `row(...)` check; the script completes with the number
// of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function T() { return new.target; }
row(T() === undefined);
row(new T() === T && typeof new T() === "function");
function C() { this.target = new.target; }
row(new C().target === C);
{ const o = {m: C}; o.m(); row(o.target === undefined); }
{ const o = {}; C.call(o); row(o.target === undefined); }
{ const D = C; row(new D().target === C); }
// The constructor decides by new.target.
function Either(x) { if (new.target === undefined) return "called"; this.x = x; }
row(Either(1) === "called" && new Either(2).x === 2);
function Guard() { if (!new.target) throw new TypeError("use new"); this.ok = true; }
row(new Guard().ok === true);
{ let r = "none"; try { Guard(); } catch (e) { r = e.name + ":" + e.message; } row(r === "TypeError:use new"); }
// Arrows capture it.
function Arrow() { this.f = () => new.target; this.g = () => (() => new.target)(); }
{ const a = new Arrow(); row(a.f() === Arrow && a.g() === Arrow); }
{ const o = {}; Arrow.call(o); row(o.f() === undefined && o.g() === undefined); }
// A nested function has its own.
function Outer() { function inner() { return new.target; } this.inner = inner(); this.made = new inner(); this.own = new.target; }
{ const o = new Outer(); row(o.inner === undefined && o.own === Outer && typeof o.made === "function" && o.made.name === "inner"); }
// Each construction of a recursive constructor sees its own.
function R(n) { this.t = new.target; this.child = n > 0 ? new R(n - 1) : (function () { return new.target; })(); }
{ const r = new R(2); row(r.t === R && r.child.t === R && r.child.child.t === R && r.child.child.child === undefined); }
// A called constructor's `this` and new.target in strict code.
function Both() { return [this, new.target]; }
{ const p = Both(); row(p[0] === undefined && p[1] === undefined); }
failed === 0 ? rows : -failed;
