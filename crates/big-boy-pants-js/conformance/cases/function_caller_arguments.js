// bbp-authored conformance case (sloppy script).
// Derives from: ~/src/quickjs/quickjs.c lines 16340-16352
// (js_throw_type_error, revision 7d1c13c) and 59972-59985
// (JS_AddIntrinsicBaseObjects), with the outputs of its qjs for each row
// (light-cljs-853): Function.prototype's `caller`
// and `arguments` are accessors, configurable and not enumerable, whose
// getter and setter are one %ThrowTypeError%. It throws TypeError "invalid
// property access", except when it gets no argument and its receiver is a
// sloppy ordinary function (QuickJS's has_prototype), where it returns
// undefined: reading `f.caller` or `f.arguments` of a sloppy `function`
// gives undefined, of any other function throws, and every write throws.
// %ThrowTypeError%'s `length` (0) and `name` ("") are not configurable. The
// Pants engine keeps both names unsupported (rust/src/vm/function.rs
// FUNCTION_PROTOTYPE_NAMES), so this case is scored against QuickJS alone.
// Left out: Object.isExtensible (pending), a class constructor's (Goose fails a class's own-property
// lookups by name until its eager `prototype` exists, light-cljs-766),
// Object.isFrozen and Object.isExtensible of %ThrowTypeError%
// (pending built-ins; QuickJS seals it) and a strict arguments object's
// `callee` (the arguments-object child of light-cljs-764). Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function failure(thunk) { try { thunk(); return "none"; } catch (e) { return e.name + ": " + e.message; } }
var thrown = "TypeError: invalid property access";
function kind(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
function sloppy(a) { return a; }
// The accessors' shape: one function, getter and setter of both.
var dc = Object.getOwnPropertyDescriptor(Function.prototype, "caller");
var da = Object.getOwnPropertyDescriptor(Function.prototype, "arguments");
row(typeof dc.get === "function" && dc.get === dc.set && da.get === dc.get && da.set === dc.get);
row(dc.enumerable === false && dc.configurable === true && da.enumerable === false && da.configurable === true);
row(!("value" in dc) && !("writable" in dc));
row(Object.prototype.hasOwnProperty.call(Function.prototype, "caller") && Object.prototype.hasOwnProperty.call(Function.prototype, "arguments"));
row(!Object.prototype.hasOwnProperty.call(sloppy, "caller") && !Object.prototype.hasOwnProperty.call(sloppy, "arguments"));
row("caller" in sloppy && "arguments" in sloppy);
// %ThrowTypeError% itself.
var thrower = dc.get;
row(Object.getPrototypeOf(thrower) === Function.prototype && thrower.length === 0 && thrower.name === "");
{ const l = Object.getOwnPropertyDescriptor(thrower, "length"); const n = Object.getOwnPropertyDescriptor(thrower, "name"); row(l.writable === false && l.enumerable === false && l.configurable === false && n.writable === false && n.configurable === false); }
row(!Object.prototype.hasOwnProperty.call(thrower, "prototype"));
// It is not extensible (QuickJS seals it): a new property is refused,
// silently in sloppy code and with a TypeError in strict code.
thrower.extra = 1;
row(!Object.prototype.hasOwnProperty.call(thrower, "extra") && thrower.extra === undefined);
row(kind(() => { "use strict"; thrower.extra = 1; }) === "TypeError");
row(kind(() => { "use strict"; thrower.length = 1; }) === "TypeError" && thrower.length === 0);
// Sloppy ordinary functions read undefined.
row(sloppy.caller === undefined && sloppy.arguments === undefined);
row((function () {}).caller === undefined && (function named() {})["arguments"] === undefined);
row(thrower.call(sloppy) === undefined && Reflect.apply(thrower, sloppy, []) === undefined);
// Any other function, or an argument, throws.
row(failure(() => (function () { "use strict"; }).caller) === thrown);
row(failure(() => (function () { "use strict"; }).arguments) === thrown);
row(failure(() => (() => 1).caller) === thrown);
row(failure(() => ({ m() {} }).m.caller) === thrown);
row(failure(() => (function* () {}).caller) === thrown && failure(() => (async function () {}).arguments) === thrown);
row(failure(() => Array.prototype.push.caller) === thrown && failure(() => Function.prototype.caller) === thrown);
row(failure(() => sloppy.bind().caller) === thrown);
row(failure(() => thrower()) === thrown && failure(() => thrower.call(sloppy, 1)) === thrown);
row(failure(() => thrower.call({})) === thrown);
// Writes throw, sloppy target or not.
row(failure(() => { sloppy.caller = 1; }) === thrown && failure(() => { sloppy.arguments = 1; }) === thrown);
row(failure(() => { (function () { "use strict"; }).caller = 1; }) === thrown);
row(sloppy.caller === undefined);
// Inside a running sloppy function, as before.
row((function inner() { return inner.caller === undefined && inner.arguments === undefined; })());
failed === 0 ? rows : -failed;
