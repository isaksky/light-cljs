// bbp-authored conformance case (light-cljs-790).
// Derives from: ~/src/quickjs/tests/test_builtin.js lines 1007-1064
// (test_generator: f and f3) and rust/tests/fixtures/js/generator_return_throw.js
// (untouched, cleanup and catches, without `using` and yield*): a generator
// yielding through a finally block, next() arguments, and return() and
// throw() on generators suspended at their start and at a yield. Values
// are strings, so only string concatenation runs. Completes with the log of
// every step.
(function () {
  var log = "";
  function row(r) { log += (r.done ? "D" : "Y") + (r.value === undefined ? "u" : r.value) + ";"; }
  function* f() {
    var ret;
    yield "1";
    ret = yield "2";
    log += "[" + ret + "]";
    return "3";
  }
  var g = f();
  row(g.next()); row(g.next()); row(g.next("next_arg")); row(g.next());
  function* f3() {
    var ret;
    try {
      ret = "2" + (yield "1");
    } catch (e) {
    } finally {
      ret += "+";
    }
    return ret;
  }
  g = f3();
  row(g.next()); row(g.next("3"));
  // A finally block runs once on return(); a yield inside it suspends again.
  function* cleanup() {
    try {
      try { yield "1"; log += "unreachable"; }
      finally { log += "a"; yield "2"; log += "b"; }
    } finally { log += "c"; }
  }
  g = cleanup(); row(g.next());
  row(g.return("7"));
  row(g.next()); row(g.next());
  // throw() at a yield runs the catch block; at the start it completes.
  function* catches() { try { yield "1"; } catch (e) { yield e + "!"; } return "8"; }
  g = catches(); g.next();
  row(g.throw("40")); row(g.next());
  var entered = "";
  function* untouched() { entered += "e"; try { yield "1"; } finally { entered += "f"; } }
  g = untouched();
  row(g.return("5")); row(g.next());
  var caught = "none";
  g = untouched();
  try { g.throw("6"); } catch (e) { caught = e; }
  log += caught + ";" + entered + ";"; row(g.next());
  // An exception out of the body completes the generator.
  function* fails() { yield "1"; throw "9"; }
  g = fails(); g.next();
  try { g.next(); } catch (e) { log += "t" + e + ";"; }
  row(g.next());
  // Re-entering a running generator is a TypeError.
  function* reentrant() { try { g.next(); } catch (e) { log += e.name + ";"; } yield "3"; }
  g = reentrant(); row(g.next());
  // Generator objects and their prototypes.
  var proto = Object.getPrototypeOf(f());
  log += (proto === f.prototype ? "P" : "p") + (Object.getPrototypeOf(proto) === Object.getPrototypeOf(f3.prototype) ? "G" : "g") + ";";
  log += Object.prototype.toString.call(g) + ";" + (g[Symbol.iterator]() === g ? "I" : "i") + ";";
  g.x = "4"; log += g.x + ";";
  return log;
})();
