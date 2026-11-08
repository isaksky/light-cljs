// bbp-authored conformance case (light-cljs-870).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 20035-20074
// (`error_constructor`, bbp-228) and 20196-20251 (`native_error_constructors`,
// bbp-229): the rows whose message converts an object with ToString or
// whose `options.cause` is a getter, which error_constructors leaves out.
// Each row is one of those expressions. Plus rows written for this slice:
// `new` with a converted message, a message object whose `toString` is not
// callable (its `valueOf` converts), an inherited `cause` getter, a cause
// getter whose receiver is the options object, a getter returning
// undefined (an own `cause` of undefined), the constructors reached through
// `call`, `Reflect.apply`, `Reflect.construct` with a new target, a bound
// constructor and `Array.prototype.map`, and a throwing `valueOf` after a
// non-callable `toString`, a Symbol message throwing before a proxy options
// object is probed, and Error as a proxy `ownKeys` trap. Groups: E the Error constructor, one per native
// error type, X the rows written for this slice. Each group completes as one
// letter per row, "y" for true, "n" for false and "x" for a throw.
// Completes with
// "E:yyyyyyyy|TypeError:yyyy|RangeError:yyyy|ReferenceError:yyyy|SyntaxError:yyyy|EvalError:yyyy|URIError:yyyy|InternalError:yyyy|X:yyyyyyyyyy".
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
  var out = [];
  // Error (error_constructor).
  out.push("E:" + rows([
    function () {
      var log = "";
      var options = { get cause() { log += "c"; return this.n; }, n: 9 };
      var e = Error({ toString: function () { log += "m"; return "text"; } }, options);
      return log === "mc" && e.message === "text" && e.cause === 9;
    },
    function () { var caught = false; try { Error({ toString: function () { throw 42; } }, { get cause() { throw 43; } }); } catch (e) { caught = e === 42; } return caught; },
    function () { var caught = false; try { Error("m", { get cause() { throw 43; } }); } catch (e) { caught = e === 43; } return caught; },
    function () { return new Error({ toString: function () { return "obj"; } }).message === "obj"; },
    function () { return new Error("m", { get cause() { return 9; } }).cause === 9; },
    function () {
      var order = [];
      new Error({ toString: function () { order.push("msg"); return ""; } }, { get cause() { order.push("cause"); } });
      return order.join(",") === "msg,cause";
    },
    function () {
      var e = new Error({ toString: function () { return "t"; } }, { get cause() { return undefined; } });
      var c = Object.getOwnPropertyDescriptor(e, "cause");
      var m = Object.getOwnPropertyDescriptor(e, "message");
      return c.value === undefined && c.writable && c.configurable && !c.enumerable && m.value === "t" && m.writable && m.configurable && !m.enumerable;
    },
    function () { var caught = false; try { new Error({ toString: function () { throw 7; } }); } catch (e) { caught = e === 7; } return caught; }
  ]));
  // The native error constructors (native_error_constructors).
  var names = ["TypeError", "RangeError", "ReferenceError", "SyntaxError", "EvalError", "URIError", "InternalError"];
  var ctors = [TypeError, RangeError, ReferenceError, SyntaxError, EvalError, URIError, InternalError];
  for (var i = 0; i < names.length; i++) {
    (function (NAME, N) {
      out.push(NAME + ":" + rows([
        function () {
          var log = "";
          var e = N({ toString: function () { log += "m"; return "text"; } }, { get cause() { log += "c"; return 9; } });
          return log === "mc" && e.message === "text" && e.cause === 9 && e instanceof N && e.name === NAME;
        },
        function () { var caught = false; try { N({ toString: function () { throw 42; } }, { get cause() { throw 43; } }); } catch (e) { caught = e === 42; } return caught; },
        function () { var caught = false; try { new N("m", { get cause() { throw 43; } }); } catch (e) { caught = e === 43; } return caught; },
        function () { var e = new N({ toString: function () { return "n"; } }); return e.message === "n" && Object.getPrototypeOf(e) === N.prototype && String(e) === NAME + ": n"; }
      ]));
    })(names[i], ctors[i]);
  }
  // Rows written for this slice.
  out.push("X:" + rows([
    function () { return new Error({ toString: null, valueOf: function () { return 7; } }).message === "7"; },
    function () {
      function P() {}
      P.prototype = { get cause() { return this.v; } };
      var o = new P(); o.v = "own";
      return new RangeError("m", o).cause === "own";
    },
    function () {
      var log = "";
      var e = Error.call(null, { toString: function () { log += "m"; return "c"; } }, { get cause() { log += "g"; return 1; } });
      return log === "mg" && e.message === "c" && e.cause === 1;
    },
    function () { var e = Reflect.apply(TypeError, null, [{ toString: function () { return "r"; } }]); return e.message === "r" && e instanceof TypeError; },
    function () {
      function F() {}
      F.prototype = { tag: "F" };
      var e = Reflect.construct(SyntaxError, [{ toString: function () { return "s"; } }, { get cause() { return 2; } }], F);
      return Object.getPrototypeOf(e) === F.prototype && e.message === "s" && e.cause === 2 && e.tag === "F";
    },
    function () { var B = URIError.bind(null, { toString: function () { return "b"; } }); var e = new B(); return e.message === "b" && e instanceof URIError; },
    function () { var a = [{ toString: function () { return "a"; } }].map(EvalError); return a[0].message === "a" && a[0] instanceof EvalError; },
    function () { var caught = false; try { new Error({ toString: 1, valueOf: function () { throw 9; } }); } catch (e) { caught = e === 9; } return caught; },
    function () { var caught = false; try { new Error(Symbol.iterator, new Proxy({}, {})); } catch (e) { caught = e instanceof TypeError; } return caught; },
    function () { var caught = false; try { for (var k in new Proxy({ toString: function () { throw 48; } }, { ownKeys: Error })) {} } catch (e) { caught = e === 48; } return caught; }
  ]));
  return out.join("|");
})();
