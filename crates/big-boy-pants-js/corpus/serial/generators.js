// Generators (light-cljs-790): a generator yielding through finally blocks,
// next() arguments, return() and throw() on generators suspended at their
// start and at a yield, an exception completing one, re-entry, and their
// prototypes (conformance/cases/generator_finally.js as a function).
function generators() {
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
  function* cleanup() {
    try {
      try { yield "1"; log += "unreachable"; }
      finally { log += "a"; yield "2"; log += "b"; }
    } finally { log += "c"; }
  }
  g = cleanup(); row(g.next());
  row(g.return("7"));
  row(g.next()); row(g.next());
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
  function* fails() { yield "1"; throw "9"; }
  g = fails(); g.next();
  try { g.next(); } catch (e) { log += "t" + e + ";"; }
  row(g.next());
  function* reentrant() { try { g.next(); } catch (e) { log += e.name + ";"; } yield "3"; }
  g = reentrant(); row(g.next());
  var proto = Object.getPrototypeOf(f());
  log += (proto === f.prototype ? "P" : "p") + (Object.getPrototypeOf(proto) === Object.getPrototypeOf(f3.prototype) ? "G" : "g") + ";";
  log += Object.prototype.toString.call(g) + ";" + (g[Symbol.iterator]() === g ? "I" : "i") + ";";
  g.x = "4"; log += g.x + ";";
  return log ==="Y1;Y2;[next_arg]D3;Du;Y1;D23+;Y1;aY2;bcD7;Du;Y40!;D8;D5;Du;6;;Du;t9;Du;TypeError;Y3;PG;[object Generator];I;4;";
}
