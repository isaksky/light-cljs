// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 18722-18786,
// symbol_to_primitive_general_dispatch (bbp-310; ECMAScript ToPrimitive step
// 1: GetMethod(input, @@toPrimitive), called with the hint): an object's own
// or inherited Symbol.toPrimitive method runs before valueOf and toString,
// with `this` the object and one argument, "default" for `+` and `==`,
// "number" for unary `+`, `-` and `<`, "string" for a template substitution
// and a property key. undefined or null is no method; anything else that is
// not callable, and an object result, are TypeErrors; an exception it
// throws propagates. The rows with String(...), Number(...), BigInt, Date,
// JSON, bind and arrow functions stay with their areas, and so do the rows
// with a Symbol.toPrimitive getter, which need a computed accessor name
// (set_computed_name) or Object.defineProperty; methods are assigned, not
// defined with computed names. Each row is a `row(...)` check;
// the script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return typeof e === "object" && e !== null ? e.name : typeof e; } }
function hinted(hint) {
  var o = { valueOf: function () { throw "valueOf"; }, toString: function () { throw "toString"; } };
  o[Symbol.toPrimitive] = function (h) {
    if (h !== hint || this !== o || arguments.length !== 1) throw "hint";
    return 42;
  };
  return o;
}
{ var o = {}; o[Symbol.toPrimitive] = function () { return 42; }; row(+o === 42); }
row(+hinted("number") === 42);
row(hinted("number") - 2 === 40);
row(hinted("number") < 43);
row(hinted("default") + 1 === 43);
row(hinted("default") == 42);
row(42 == hinted("default"));
row(`${hinted("string")}` === "42");
{ var a = {}; a[hinted("string")] = 7; row(a[42] === 7 && hinted("string") in a && delete a[hinted("string")] && !(42 in a)); }
{ var lit = { [hinted("string")]: 7 }; row(lit["42"] === 7); }
// Inherited; undefined and null are no method.
{
  var proto = {}; proto[Symbol.toPrimitive] = function (h) { return h; };
  var heir = { __proto__: proto };
  row(`${heir}` === "string" && heir + "" === "default" && +{ __proto__: proto } !== +{ __proto__: proto });
}
{ var n = { valueOf: function () { return 7; } }; n[Symbol.toPrimitive] = null; row(+n === 7); }
{ var u = { toString: function () { return "s"; } }; u[Symbol.toPrimitive] = undefined; row(`${u}` === "s"); }
// Two operands convert left to right, each with the hint.
{
  var log = "";
  var l = {}; l[Symbol.toPrimitive] = function (h) { log += "a" + h; return 8; };
  var r = {}; r[Symbol.toPrimitive] = function (h) { log += "b" + h; return 2; };
  row(l - r === 6 && log === "anumberbnumber");
}
// Identity comparisons never convert.
{
  var count = 0;
  var c = {}; c[Symbol.toPrimitive] = function () { count++; return 1; };
  row(c === c && c == c && c != null && c != undefined && c != {} && count === 0);
}
// A symbol result is a property key.
{ var sym = Symbol.iterator; var k = {}; k[Symbol.toPrimitive] = function () { return sym; }; var t = {}; t[k] = 7; row(t[sym] === 7); }
// Functions and arrays consult it too.
{ var f = function () { return 0; }; f[Symbol.toPrimitive] = function (h) { return h; }; row(f + "" === "default" && `${f}` === "string"); }
{ var arr = []; arr[Symbol.toPrimitive] = function (h) { return 42; }; row(arr + 1 === 43 && +arr === 42); }
// Errors: a non-callable method, an object result, a symbol result where
// a string or number is needed, and exceptions.
{ var bad = {}; bad[Symbol.toPrimitive] = 1; row(name(function () { return +bad; }) === "TypeError"); }
{ var obj = {}; obj[Symbol.toPrimitive] = {}; row(name(function () { return +obj; }) === "TypeError"); }
{ var res = { valueOf: function () { return 42; } }; res[Symbol.toPrimitive] = function () { return {}; }; row(name(function () { return +res; }) === "TypeError"); }
{ var fn = {}; fn[Symbol.toPrimitive] = function () { return function () {}; }; row(name(function () { return `${fn}`; }) === "TypeError"); }
{ var sy = {}; sy[Symbol.toPrimitive] = function () { return Symbol.iterator; }; row(name(function () { return `${sy}`; }) === "TypeError" && name(function () { return +sy; }) === "TypeError"); }
{
  var got = 0;
  var direct = { valueOf: function () { throw 8; } }; direct[Symbol.toPrimitive] = function () { throw 7; };
  try { +direct; } catch (e) { got = e; }
  row(got === 7);
}
failed === 0 ? rows : -failed;
