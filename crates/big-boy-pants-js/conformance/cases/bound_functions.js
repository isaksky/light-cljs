// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 860-985:
// bound_functions (line 860, bbp-103) and
// bound_function_properties_and_native_callbacks (966, bbp-103), their call
// and construction rows; call_forwarding_reuses_operands_without_changing_call_semantics
// (55), its `bind` row; bound_argument_and_hop_limits (1033), its ten-hop
// row; QuickJS tests/test_builtin.js test_function (line 83), its two `bind`
// groups; js_source.rs construct rows 1499 and 1511 (bbp-118) and the
// Array target of 5834; js_source.rs 3113's `bind` call with a spread
// argument is in spread_calls; OrdinaryHasInstance step 2 (light-cljs-852).
// Bound `this` and arguments, nested binds, `new` with new.target replaced
// by the target, `instanceof` through the target, `call`, `apply`,
// Reflect.apply and Reflect.construct of a bound function, built-in targets
// and the native frames that call one (map, forEach, ToPrimitive,
// Array.prototype.toString's join and @@iterator).
// Rewritten: `__rt.vector` and `__rt.nth` rows use arrays, the
// `__rt.reduce` callback row a map callback. Left out: publication,
// collection, migration and suspension rows (Pants runtime tests), custom
// limits of the Pants test runtime, a bound function as a getter
// (Object.defineProperty, the objects area, light-cljs-766), and a bound
// @@hasInstance of a plain object (the Pants engine refuses a non-callable
// right operand before reading @@hasInstance; QuickJS and Goose call it). Each row is a `row(...)` check;
// the script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// Bound `this`, then the bound arguments before the call's.
{ function f(a, b) { return [this, a, b]; } const g = f.bind(1, 2); const v = g(3); row(v[0] * 100 + v[1] * 10 + v[2] === 123); }
{ function f(a, b) { var tab = [this]; for (var i = 0; i < arguments.length; i++) tab.push(arguments[i]); return tab; } const g = f.bind(1, 2); row(g(3).toString() === "1,2,3"); }
{ function f(a, b) { return this * 100 + a * 10 + b; } row(f.bind(1).bind(2, 3)(4) === 134); }
{ function f() { return this; } row(f.bind()() === undefined); }
{ function f(a, b) { return this.x + a + b; } const g = f.bind({x: 10}, 2); row(g.call({x: 99}, 3) === 15); }
{ function f(a, b) { return this + a + b; } const g = f.bind(1, 2); row(g.call(7, 3) === 6 && g.apply(8, [3]) === 6 && Reflect.apply(g, 9, [3]) === 6); }
{ function f() { return 7; } let g = f; for (let i = 0; i < 10; i++) { g = g.bind(); } row(g() === 7); }
row(name(function () { Function.prototype.bind.call(1); }) === "TypeError");
row(name(function () { Function.prototype.bind.call({}); }) === "TypeError");
// Construction: the target constructs, new.target replaced by the target.
{ function C(a) { this.x = a; } const B = C.bind(null, 1); row(new B().x === 1); }
{ function constructor1(a) { this.x = a; } const g = constructor1.bind(null, 1); const r = new g(); row(r.x === 1); }
{
  function C(a) { this.x = a; }
  const B = C.bind(null, 7).bind(null);
  const o = new B();
  row(o.x === 7 && o instanceof B && o instanceof C && B.prototype === undefined && typeof B === "function");
}
{ function C(x) { this.x = x; } const receiver = {x: 0}; const B = C.bind(receiver, 7); const o = new B(); row(receiver.x === 0 && o.x === 7); }
{
  function C() {}
  const B = C.bind();
  B.prototype = {};
  const o = new B();
  row(o instanceof B && Object.getPrototypeOf(o) === C.prototype && Object.getPrototypeOf(o) !== B.prototype);
}
{ function C() { return {x: 9}; } row((new (C.bind(null))()).x === 9); }
{ let r; function F() { r = new.target; } const B = F.bind(null); new B(); row(r === F); }
{ function F() { this.t = new.target; } const B = F.bind(null); row(Reflect.construct(B, []).t === F && Reflect.construct(F, [], B).t === B); }
row(name(function () { const B = ((a) => a).bind(null, 1); new B(); }) === "TypeError");
row(name(function () { const B = Object.is.bind(null); new B(); }) === "TypeError");
row(name(function () { const B = ((a) => a).bind(null); Reflect.construct(B, []); }) === "TypeError");
// instanceof uses the target (OrdinaryHasInstance step 2).
{ function C() {} const o = new C(); const B = C.bind(null).bind(null); row(o instanceof B && !({} instanceof B) && !(1 instanceof B)); }
{ const B = Array.bind(null); row([] instanceof B && !({} instanceof B)); }
// Built-in targets.
row(Object.is.bind(null, 1)(1) && Object.prototype.toString.call(Object.is.bind()) === "[object Function]");
row(new (Array.bind(null, 1, 2))()[0] === 1 && Array.bind(null, 3)().length === 3);
row(Number.bind(null, {valueOf() { return 4; }})() === 4 && String.bind(null)("s") === "s");
// From native frames: callbacks, a ToPrimitive method, join, @@hasInstance
// and @@iterator.
{ function f(k, x) { return k + x; } row([1, 2].map(f.bind(null, 10)).toString() === "11,12"); }
{ let s = 0; [1, 2].forEach(function (x) { s += this.k * x; }.bind({k: 10})); row(s === 30); }
row([1, 2].map(Object.is.bind(null, 2)).toString() === "false,true");
row(({valueOf: function () { return this.v; }.bind({v: 5})}) + 1 === 6);
{ const a = [1]; a.join = function () { return "j" + this.length; }.bind([1, 2]); row(a.toString() === "j2"); }
{ const it = {[Symbol.iterator]: function () { return this[Symbol.iterator](); }.bind([4, 5])}; let s = 0; for (const x of it) s += x; row(s === 9); }
{ function f(x) { return this.n + x; } const g = f.bind({n: 1}); row(g.call.bind(g)(null, 2) === 3 && Reflect.apply.bind(null, g, null)([3]) === 4); }
failed === 0 ? rows : -failed;
