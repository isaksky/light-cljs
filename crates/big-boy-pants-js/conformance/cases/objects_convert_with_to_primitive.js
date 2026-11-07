// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 18621-18718,
// objects_convert_with_to_primitive (bbp-70's acceptance rows; ECMAScript
// ToPrimitive and OrdinaryToPrimitive): an object operand of `+`, `==`, `!=`,
// `<`, `*`, `-`, unary `+` and `++` converts by its valueOf, or its toString
// when valueOf is not callable or gives an object; one that neither method
// converts is a TypeError, and a throwing valueOf propagates. The rows that
// need Function.prototype.bind, String(...), Proxy or Object.create stay with
// their areas. Each row is a `row(...)` check; the script completes with the
// number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return typeof e === "object" && e !== null ? e.name : typeof e; } }
row(({}) != "abc");
row(({}) == "[object Object]");
row("[object Object]" == ({}));
row({ valueOf: function () { return 1; } } == 1);
row(1 < { valueOf: function () { return 2; } });
{ let o = {}; row(o == o); }
{ let o = { valueOf: function () { throw 1; } }; row(o != null && o != undefined && o != {}); }
row("" + {} === "[object Object]");
row({} + "" === "[object Object]");
row({ valueOf: function () { return 2; } } * 3 === 6);
row({ valueOf: function () { return 2; } } + "x" === "2x");
row({ toString: function () { return "s"; }, valueOf: function () { return {}; } } + "" === "s");
row({ valueOf: 1, toString: function () { return "s"; } } + "" === "s");
{
  let n = 0;
  let a = { valueOf: function () { n = n * 10 + 1; return 8; } };
  let b = { valueOf: function () { n = n * 10 + 2; return 3; } };
  let r = a - b;
  row(n * 10 + r === 125);
}
row(+{ x: 7, valueOf: function () { return this.x; } } === 7);
{ let caught = 0; try { ({ valueOf: function () { return {}; }, toString: function () { return {}; } }) + 1; } catch (e) { caught = 1; } row(caught === 1); }
row(name(function () { return ({ valueOf: function () { return {}; }, toString: function () { return {}; } }) + 1; }) === "TypeError");
row(name(function () { return ({ __proto__: null }) + 1; }) === "TypeError");
{ let r; try { ({ valueOf: function () { throw "boom"; } }) + 1; } catch (e) { r = e; } row(r === "boom"); }
{ function f() {} f.valueOf = function () { return 3; }; row(f * 2 === 6); }
{ let a = []; a.valueOf = function () { return 4; }; row(a - 1 === 3 && a + "" === "4"); }
row({ valueOf: Object.prototype.toString } + "" === "[object Object]");
{
  // A throwing left operand stops the right one's conversion.
  let n = 0;
  let b = { valueOf: function () { n++; return 2; } };
  try { ({ valueOf: function () { throw 1; } }) - b; } catch (e) {}
  row(n === 0);
}
{ let proto = { valueOf: function (arg) { return arg === undefined ? this.n : 99; } }; let o = { __proto__: proto, n: 4 }; row(o * 2 === 8); }
{ let o = { valueOf: function () { return 41; } }; o++; row(o === 42); }
{ let o = { valueOf: function () { return "5"; } }; row(-o === -5 && o % 3 === 2 && o / 5 === 1); }
{ let o = { valueOf: function () { return true; } }; row(o == 1 && o + 1 === 2); }
row(Error("m") + "" === "Error: m" && `${TypeError("t")}` === "TypeError: t");
failed === 0 ? rows : -failed;
