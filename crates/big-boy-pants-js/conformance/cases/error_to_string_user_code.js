// bbp-authored conformance case (light-cljs-870).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 20035-20074
// (`error_constructor`, bbp-228): the Error.prototype.toString rows whose
// `name` or `message` is a getter or an object converted with ToString,
// which error_constructors leaves out. Plus light-cljs-765's probe rows: two
// getters called name first, an inherited `name` getter, `name` and
// `message` objects with `toString`, a throwing `message` getter, and an
// error whose `name` is itself (`String(e)` overflows the call stack, an
// InternalError on both oracles). Plus rows written for this slice:
// getters returning undefined (the "Error" and "" defaults), a getter's
// receiver, a nested Error.prototype.toString inside a conversion, and the
// conversions that reach Error.prototype.toString as a ToPrimitive method
// (`+`, `String()`, a template), through `Reflect.apply` and as an array's
// `join`, and a Symbol name throwing before an inherited proxy is read for
// `message`. Each group
// completes as one letter per row, "y" for true, "n" for false and "x" for a
// throw. Completes with "T:yyyyyyyy|P:yyyyyyyyyy".
(function () {
  function rows(fns) {
    var s = "";
    for (var i = 0; i < fns.length; i++) {
      var v;
      try { v = fns[i]() === true ? "y" : "n"; } catch (e) { v = "x"; }
      s += v;
    }
    return s;
  }
  var ts = Error.prototype.toString;
  var out = [];
  // Error.prototype.toString (error_constructor).
  out.push("T:" + rows([
    function () {
      var log = "";
      var o = { get name() { log += "n"; return { toString: function () { log += "N"; return "custom"; } }; }, get message() { log += "m"; return { toString: function () { log += "M"; return "text"; } }; } };
      return ts.call(o) === "custom: text" && log === "nNmM";
    },
    function () { var caught = false; try { ts.call({ get name() { throw 44; }, get message() { throw 45; } }); } catch (e) { caught = e === 44; } return caught; },
    function () { var caught = false; try { ts.call({ name: { toString: function () { throw 42; } }, get message() { throw 43; } }); } catch (e) { caught = e === 42; } return caught; },
    function () { var caught = false; try { ts.call({ name: "N", message: { toString: function () { throw 43; } } }); } catch (e) { caught = e === 43; } return caught; },
    // The probe's rows.
    function () {
      var log = [];
      var o = { get name() { log.push("n"); return "G"; }, get message() { log.push("m"); return "M"; } };
      return ts.call(o) + "|" + log.join("") === "G: M|nm";
    },
    function () { function F() {} F.prototype = { get name() { return "PG"; } }; var o = new F(); o.message = "mm"; return ts.call(o) === "PG: mm"; },
    function () {
      var o = { name: { toString: function () { return "ON"; } }, message: { toString: function () { return "OM"; } } };
      return ts.call(o) === "ON: OM";
    },
    function () {
      var e = new Error("m"); e.name = e;
      var caught = false;
      try { String(e); } catch (x) { caught = x instanceof InternalError; }
      return caught;
    }
  ]));
  // Rows written for this slice.
  out.push("P:" + rows([
    function () { var caught = false; try { ts.call({ name: "N", get message() { throw 46; } }); } catch (e) { caught = e === 46; } return caught; },
    function () { return ts.call({ get name() {}, message: "m" }) === "Error: m" && ts.call({ name: "N", get message() { return undefined; } }) === "N"; },
    function () { return ts.call({ get name() { return this.n; }, n: "T", message: "" }) === "T"; },
    function () {
      var o = { name: { toString: function () { return ts.call({ get name() { return "in"; } }); } }, message: "out" };
      return ts.call(o) === "in: out";
    },
    function () { function F() {} F.prototype = Error.prototype; var o = new F(); o.name = { toString: function () { return "Z"; } }; return "" + o === "Z"; },
    function () { var e = new TypeError("t"); var log = ""; e.name = { toString: function () { log += "s"; return "S"; } }; return String(e) === "S: t" && `${e}` === "S: t" && log === "ss"; },
    function () { return Reflect.apply(ts, { get name() { return "R"; }, message: { valueOf: function () { return 1; }, toString: null } }, []) === "R: 1"; },
    function () { var e = new Error("x"); e.message = { toString: function () { return "y"; } }; return e.toString() === "Error: y"; },
    function () { var caught = false; try { ts.call({ __proto__: new Proxy({}, {}), name: Symbol.iterator }); } catch (e) { caught = e instanceof TypeError; } return caught; },
    function () { var a = [1]; a.join = ts; a.name = { toString: function () { return "AN"; } }; a.message = "am"; return String(a) === "AN: am"; }
  ]));
  return out.join("|");
})();
