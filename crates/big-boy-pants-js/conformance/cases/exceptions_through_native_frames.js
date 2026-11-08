// bbp-authored conformance case (light-cljs-869).
// Derives from: crates/big-boy-pants-js/rust/tests/native_unwind.rs lines 166-200
// (`a_callback_exception_unwinds_through_the_fold_into_the_callers_catch`),
// 392-425 (`an_exception_unwinds_through_nested_natives`) and 667-690
// (`builtin_fold_errors_unwind`) (bbp-12, bbp-28), with `map` and `forEach`
// for `__rt.reduce` and without its producer waits: a callback's throw leaves
// after the callback's own finally blocks and skips the remaining elements;
// nested `map` callbacks; `forEach` stopping at the throw; an engine
// ReferenceError inside a callback; a catch inside the callback that lets
// `map` go on; a finally block that throws over a callback's exception. Plus
// rows written for this slice (light-cljs-765's probe): getters, setters and
// destructuring getters that throw (bbp-120); `valueOf`, `toString` and
// `Symbol.toPrimitive` that throw under `+`, `*`, `<`, concatenation, a
// template, `String()` and a computed key (bbp-70); `call`, `apply`,
// `Reflect.apply`, `bind`, `new`, `Reflect.construct` and a bound constructor
// forwarding a throw; a throwing default parameter, plain and destructuring;
// a throwing tag function; a throw in an argument skips the call and runs
// the caller's finally block; a TypeError from `null.x` through two frames'
// finally and catch blocks; `throw` inside `switch` inside try/catch/finally.
// Eval code that throws is in exceptions_eval_sloppy. Not ported: Proxy traps that throw (light-cljs-861); `Array.from` closing
// its iterator on a throwing map function (the core built-ins area,
// light-cljs-767); a throwing `Symbol.hasInstance`, which the oracles
// disagree on because the Pants engine does not consult it (Goose and
// QuickJS do; scored against QuickJS alone in the symbol_has_instance slice).
// Completes with
// "m2:1,f1,f2|3|fe:1,2|ReferenceError|2,4|outer|33:40,43,33|caught:50,60,70|g|s1|dg|kg|v|v|v|t|tplt|St|keyt|tp|f,v|a1|a2|a3|a4|c|Rc|Bc|d|1|g|tt|fin,a|i,TypeError,o|cone,two,def,123".
(function () {
  var r = [];
  // A callback's throw unwinds through `map` after its own finally blocks.
  var log = [];
  var m;
  try {
    [1, 2, 3].map(function (x) {
      try { if (x === 2) throw "m" + x; log.push(x); } finally { log.push("f" + x); }
      return x;
    });
  } catch (e) { m = e; }
  r.push(m + ":" + log.join(","));
  // Nested `map` callbacks.
  var nested;
  try { [1].map(function () { return [2].map(function () { throw 3; }); }); } catch (e) { nested = e; }
  r.push(nested);
  // `forEach` stops at the throw.
  var seen = [];
  var fe;
  try { [1, 2, 3].forEach(function (x) { seen.push(x); if (x === 2) throw "fe"; }); } catch (e) { fe = e; }
  r.push(fe + ":" + seen.join(","));
  // An engine ReferenceError inside a callback.
  var re;
  try { [1].map(function () { notDefinedInCallback869; }); } catch (e) { re = e.name; }
  r.push(re);
  // A catch inside the callback lets `map` go on.
  r.push([1, 2].map(function (x) { try { throw x; } catch (e) { return e * 2; } }).join(","));
  // A finally block that throws over a callback's exception.
  var over;
  try { try { [1].map(function () { throw "inner"; }); } finally { throw "outer"; } } catch (e) { over = e; }
  r.push(over);
  // Nested natives: each row's finally block runs as the throw passes.
  var nlog = [];
  function cell(a, x) { if (x === 3) { throw 30 + a; } return a + x; }
  function row(acc, items) {
    try {
      var sum = 0;
      items.forEach(function (x) { sum = cell(sum, x); });
      return acc + sum;
    } finally { nlog.push(40 + acc); }
  }
  function rows(all) {
    var acc = 0;
    try { all.forEach(function (items) { acc = row(acc, items); }); return acc; }
    catch (e) { nlog.push(e); return e; }
  }
  r.push(rows([[1, 2], [1, 2, 3, 4]]) + ":" + nlog.join(","));
  // A built-in that throws inside the fold: Object.prototype.valueOf on
  // undefined, called as a callback.
  var blog = [];
  var br = "none";
  try { [1, 2].map(Object.prototype.valueOf); blog.push(99); }
  catch (e) { blog.push(50); br = "caught"; }
  finally { blog.push(60); }
  blog.push(70);
  r.push(br + ":" + blog.join(","));
  // Getters, setters and destructuring getters that throw.
  var o = { get x() { throw "g"; }, set y(v) { throw "s" + v; } };
  try { o.x; } catch (e) { r.push(e); }
  try { o.y = 1; } catch (e) { r.push(e); }
  try { var { x } = o; } catch (e) { r.push("d" + e); }
  try { o["x"]; } catch (e) { r.push("k" + e); }
  // valueOf, toString and Symbol.toPrimitive that throw.
  var v = { valueOf: function () { throw "v"; } };
  var t = { toString: function () { throw "t"; } };
  try { v + 1; } catch (e) { r.push(e); }
  try { v * 2; } catch (e) { r.push(e); }
  try { v < 1; } catch (e) { r.push(e); }
  try { "" + t; } catch (e) { r.push(e); }
  try { `${t}`; } catch (e) { r.push("tpl" + e); }
  try { String(t); } catch (e) { r.push("S" + e); }
  try { var k = {}; k[t] = 1; } catch (e) { r.push("key" + e); }
  try { ({ [Symbol.toPrimitive]: function () { throw "tp"; } }) + 1; } catch (e) { r.push(e); }
  var flog = [];
  try { try { v + 1; } finally { flog.push("f"); } } catch (e) { flog.push(e); }
  r.push(flog.join(","));
  // call, apply, Reflect.apply, bind, new, Reflect.construct and a bound
  // constructor forward a throw.
  function thrower(n) { throw "a" + n; }
  try { thrower.call(null, 1); } catch (e) { r.push(e); }
  try { thrower.apply(null, [2]); } catch (e) { r.push(e); }
  try { Reflect.apply(thrower, null, [3]); } catch (e) { r.push(e); }
  try { thrower.bind(null, 4)(); } catch (e) { r.push(e); }
  function C() { throw "c"; }
  try { new C(); } catch (e) { r.push(e); }
  try { Reflect.construct(C, []); } catch (e) { r.push("R" + e); }
  try { new (C.bind(null))(); } catch (e) { r.push("B" + e); }
  // A throwing default parameter, plain and destructuring.
  function dp(a = (function () { throw "d"; })()) { return a; }
  try { dp(); } catch (e) { r.push(e); }
  r.push(dp(1));
  function dd({ a } = (function () { throw "g"; })()) { return a; }
  try { dd(); } catch (e) { r.push(e); }
  // A throwing tag function.
  function tag() { throw "tt"; }
  try { tag`a${1}b`; } catch (e) { r.push(e); }
  // A throw in an argument skips the call and runs the caller's finally.
  var alog = [];
  function called() { alog.push("called"); }
  try { try { called(1, (function () { throw "a"; })()); } finally { alog.push("fin"); } } catch (e) { alog.push(e); }
  r.push(alog.join(","));
  // A TypeError from null.x through two frames' finally and catch blocks.
  var elog = [];
  function innerNull() { try { null.x; } finally { elog.push("i"); } }
  function outerNull() { try { innerNull(); } catch (e) { elog.push(e.name); } finally { elog.push("o"); } }
  outerNull();
  r.push(elog.join(","));
  // `throw` inside `switch` inside try/catch/finally.
  var slog = [];
  function sw(n) {
    try {
      switch (n) { case 1: throw "one"; case 2: return "two"; default: break; }
      return "def";
    } catch (e) { return "c" + e; } finally { slog.push(n); }
  }
  r.push([sw(1), sw(2), sw(3), slog.join("")].join(","));
  return r.join("|");
})();
