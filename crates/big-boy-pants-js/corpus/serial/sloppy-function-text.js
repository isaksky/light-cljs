// Function.prototype.toString and Function.prototype's caller/arguments
// (light-cljs-853), a sloppy script. Source text from the image (non-ASCII
// included), the native form of built-in and bound functions, the TypeError
// of a receiver that is not callable, and %ThrowTypeError% as getter and
// setter: undefined for a sloppy ordinary function, TypeError otherwise.
// QuickJS gives true; the Pants engine implements neither built-in, so
// only Goose runs it (serial_tests.goose, at every cut and limit point).
function failure(thunk) { try { thunk(); return ""; } catch (e) { return e.name + ":" + e.message; } }
function sloppy(a, b) { return a; }
var strict = function () { "use strict"; return 1; };
var texts = [sloppy, strict, (x) => x + 1, { m(é) { return "μ😀"; } }.m, function* g() {}].map(function (f) { return f.toString(); });
var native = "() {\n    [native code]\n}";
var thrown = "TypeError:invalid property access";
texts[0] === "function sloppy(a, b) { return a; }" &&
  texts[1] === "function () { \"use strict\"; return 1; }" &&
  texts[2] === "(x) => x + 1" &&
  texts[3] === "m(é) { return \"μ😀\"; }" && texts[3].length === 22 &&
  texts[4] === "function* g() {}" &&
  Object.toString() === "function Object" + native &&
  sloppy.bind().bind().toString() === "function bound bound sloppy" + native &&
  failure(function () { Function.prototype.toString.call({}); }) === "TypeError:not a function" &&
  sloppy.caller === undefined && sloppy.arguments === undefined &&
  failure(function () { return strict.caller; }) === thrown &&
  failure(function () { return strict.arguments; }) === thrown &&
  failure(function () { return Object.caller; }) === thrown &&
  failure(function () { sloppy.caller = 1; }) === thrown &&
  Object.getOwnPropertyDescriptor(Function.prototype, "caller").get === Object.getOwnPropertyDescriptor(Function.prototype, "arguments").set &&
  // An object name converts by its toString, in a Convert frame above the
  // FunctionText frame.
  (function () { delete Function.prototype.name; var bf = sloppy.bind(); delete bf.name; bf.name = { toString: function () { return "obj"; } }; return bf.toString() === "function obj" + native; })();
