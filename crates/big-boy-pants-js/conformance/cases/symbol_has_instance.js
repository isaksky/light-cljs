// bbp-authored conformance case.
// Derives from: none (light-cljs-834; ECMAScript 13.10.2 InstanceofOperator,
// 7.3.22 OrdinaryHasInstance and 20.2.3.6 Function.prototype[@@hasInstance],
// as QuickJS implements them in quickjs.c JS_IsInstanceOf, line 8242,
// JS_OrdinaryIsInstanceOf, 8168, and js_function_hasInstance, 42235): a user
// Symbol.hasInstance method, its receiver, argument and ToBoolean'd result;
// exceptions from it; a getter-supplied `prototype` on a non-callable target,
// which is never read; Function.prototype[Symbol.hasInstance]'s attributes,
// name and length, and its calls on any receiver. The Pants engine does not
// implement Symbol.hasInstance, so QuickJS alone scores this case
// (areas.edn :pants-gap). Methods are assigned, not computed-name literals
// (set_computed_name, the functions area's). Getters of Symbol.hasInstance
// and of a callable's `prototype` need computed accessor names,
// Object.defineProperty or Object.setPrototypeOf, which Goose does not run
// yet: serial_tests.goose installs them before corpus/serial/instanceof.js.
// Each row is a `row(...)` check; the script completes with the number of
// rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
function target(m) { const o = {}; o[Symbol.hasInstance] = m; return o; }
function F() {}
const f = new F();
// A user method: called with the target as `this` and the left operand as
// its only argument; its result goes through ToBoolean.
{
  let seen = [], n = 0;
  const even = target(function (v) { seen[n++] = this; seen[n++] = v; seen[n++] = arguments.length; return v % 2 === 0; });
  row(4 instanceof even && !(3 instanceof even) && seen[0] === even && seen[1] === 4 && seen[2] === 1 && seen[4] === 3);
}
row(1 instanceof target(() => "yes"));
row(!(1 instanceof target(() => "")));
row(1 instanceof target(() => ({})) && 1 instanceof target(() => []));
row(!(1 instanceof target(() => 0)) && !(1 instanceof target(() => {})));
row(!(1 instanceof target(() => NaN)) && !(1 instanceof target(() => null)));
row((1 instanceof target(() => 2)) === true && (1 instanceof target(() => -0)) === false);
// An inherited method; a method overrides OrdinaryHasInstance.
{ const C = { __proto__: target(() => true) }; row(1 instanceof C && "s" instanceof C && undefined instanceof C); }
{
  function A() {}
  const hidden = new A();
  const B = target((v) => v === hidden);
  row(hidden instanceof B && !(new A() instanceof B));
}
// The method runs from inside a user frame, and may itself use instanceof.
{
  const numbers = target((v) => typeof v === "number");
  function inner(x) { return x instanceof numbers; }
  row(inner(1) && !inner("1"));
  const objects = target((v) => v instanceof Object);
  row({} instanceof objects && !(1 instanceof objects));
}
// undefined and null are no method: OrdinaryHasInstance then needs a
// callable target.
row(name(() => f instanceof target(undefined)) === "TypeError");
row(name(() => f instanceof target(null)) === "TypeError");
// A method that is not callable is a TypeError.
row(name(() => f instanceof target(1)) === "TypeError");
row(name(() => f instanceof target({})) === "TypeError");
row(name(() => f instanceof target("F")) === "TypeError");
// An exception from the method leaves the operator.
row(name(() => 1 instanceof target(() => { throw new RangeError("r"); })) === "RangeError");
{ let v; try { 1 instanceof target(() => { throw 7; }); } catch (e) { v = e; } row(v === 7); }
// Built-in methods: their result goes through ToBoolean; Number and String
// convert an object argument (its valueOf or toString runs) first.
{
  let log = "";
  const v = { valueOf() { log += "v"; return 1; }, toString() { log += "s"; return ""; } };
  row(v instanceof target(Number) && log === "v");
  row(!(v instanceof target(String)) && log === "vs");
  row(!(0 instanceof target(Number)) && "a" instanceof target(String) && 1 instanceof target(Boolean) && !(0 instanceof target(Boolean)));
  row(f instanceof target(Object) && f instanceof target(Object.getPrototypeOf) && !({ __proto__: null } instanceof target(Object.getPrototypeOf)));
  row(name(() => ({ valueOf() { throw new URIError("u"); } }) instanceof target(Number)) === "URIError");
}
// A built-in that runs as a native frame: [].map's receiver is the target.
{ const a = [1, 2]; a[Symbol.hasInstance] = a.map; let seen = 0; row((function (x) { seen++; return x; }) instanceof a && seen === 2); }
// The realm's method on a non-callable target: OrdinaryHasInstance is false.
row(!(f instanceof target(Function.prototype[Symbol.hasInstance])));
// A non-callable target whose `prototype` is a getter: OrdinaryHasInstance
// is false without reading it.
{
  let reads = 0;
  const p = {};
  const C = { get prototype() { reads++; return p; } };
  C[Symbol.hasInstance] = Function.prototype[Symbol.hasInstance];
  row(!({ __proto__: p } instanceof C) && reads === 0);
}
// Function.prototype[Symbol.hasInstance]: non-writable, non-enumerable,
// non-configurable; named "[Symbol.hasInstance]", length 1.
{
  const d = Object.getOwnPropertyDescriptor(Function.prototype, Symbol.hasInstance);
  row(typeof d.value === "function" && d.writable === false && d.enumerable === false && d.configurable === false);
  row(d.value.name === "[Symbol.hasInstance]" && d.value.length === 1);
  row(F[Symbol.hasInstance] === d.value && Object.hasOwn(Function.prototype, Symbol.hasInstance) && !Object.hasOwn(F, Symbol.hasInstance));
  row(Symbol.hasInstance in F && Symbol.hasInstance in Object && !(Symbol.hasInstance in {}));
}
// Assigning it on a function is refused: the inherited property is not
// writable (a TypeError in strict code), so OrdinaryHasInstance still runs.
{ function G() {} row(name(() => { G[Symbol.hasInstance] = () => true; }) === "TypeError" && !Object.hasOwn(G, Symbol.hasInstance) && !({} instanceof G)); }
// Called directly it is OrdinaryHasInstance with its receiver as C.
row(F[Symbol.hasInstance](f) === true && F[Symbol.hasInstance]({}) === false && Object[Symbol.hasInstance](f) === true);
row(Function.prototype[Symbol.hasInstance].call(F, f) === true && Function.prototype[Symbol.hasInstance].call(Array, []) === true);
// A receiver that is not callable is false, not a TypeError, and so is a
// primitive or missing argument.
row(Function.prototype[Symbol.hasInstance].call({}, f) === false && Function.prototype[Symbol.hasInstance].call(1, f) === false);
row(Function.prototype[Symbol.hasInstance].call(undefined, f) === false && F[Symbol.hasInstance](1) === false && F[Symbol.hasInstance]() === false);
{ function P() {} P.prototype = 1; row(name(() => P[Symbol.hasInstance]({})) === "TypeError" && P[Symbol.hasInstance](1) === false); }
// It is not a constructor.
row(name(() => new F[Symbol.hasInstance](f)) === "TypeError");
failed === 0 ? rows : -failed;
