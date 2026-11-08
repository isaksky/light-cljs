// bbp-authored conformance case (light-cljs-869).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 20035-20074
// (`error_constructor`, bbp-228), 20196-20251 (`native_error_constructors`,
// bbp-229) and 20255-20292 (`native_error_bound_constructor_prototypes`): each
// row is one of those expressions, `Object.keys(e).length` counted with
// for-in and `Object.create(p)` made with a constructor's `prototype`. Plus
// rows written for this slice (light-cljs-765's probe): an ES5 subclass
// (`MyErr.prototype = new Error()`, `Error.call(this, m)`),
// `Reflect.construct(TypeError, ['m'], F)` taking `F.prototype`, and `new
// Error` without parentheses.
// Not ported: the rows that convert an object with ToString or read `name`,
// `message` or `cause` through getters (in the Error constructor, the native
// error constructors and Error.prototype.toString; light-cljs-870 owns them);
// `Error.prototype.message = 'changed'`, which the oracles disagree on (the
// Pants engine refuses writes to built-in prototypes, QuickJS and Goose allow
// them); `Object.setPrototypeOf` (the objects area, light-cljs-766); the
// `__rt.reduce(Error, ...)` row (a Pants host binding); the uncaught
// `throw new N('boom')` message (the harness compares error names only);
// SuppressedError (the using area, light-cljs-772); the bound derived class
// rows of native_error_bound_constructor_prototypes (a class with a static
// method stops on Goose at get_field with UnsupportedOperand: the objects
// area, light-cljs-766). Groups: E the Error constructor, one per native error
// type, B the bound native error constructors, X the rows written for this
// slice, K a native error constructor still working after `Error = 0` and its
// own global name are reassigned (the last native_error_constructors row),
// then restored. Each group completes as one letter per row, "y" for true,
// "n" for false and "x" for a throw. Completes with
// "E:yyyyyyyyyyyyyyyyyyyyyyy|TypeError:yyyyyyyyyyyyyyyyyyyyy|RangeError:yyyyyyyyyyyyyyyyyyyyy|ReferenceError:yyyyyyyyyyyyyyyyyyyyy|SyntaxError:yyyyyyyyyyyyyyyyyyyyy|EvalError:yyyyyyyyyyyyyyyyyyyyy|URIError:yyyyyyyyyyyyyyyyyyyyy|InternalError:yyyyyyyyyyyyyyyyyyyyy|B:yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy|X:yyyyyyy|K:yyyyyyyyyyy".
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
  function keyCount(o) { var n = 0; for (var k in o) n++; return n; }
  var out = [];
  // Error (error_constructor).
  out.push("E:" + rows([
    function () { return Error("hello").message === "hello"; },
    function () { return new Error("a").message === "a"; },
    function () { return Error().message === "" && Object.getOwnPropertyDescriptor(Error(undefined), "message") === undefined; },
    function () { return Error(12).message === "12"; },
    function () { return new Error("m", { cause: 0 }).cause === 0; },
    function () { return new Error("m", {}).cause === undefined && new Error("m", 5).cause === undefined; },
    function () { return Error.prototype.name === "Error" && Error.prototype.message === "" && Error.prototype.constructor === Error; },
    function () { return Error.length === 1 && Error.name === "Error" && Error.prototype.toString.length === 0 && Error.prototype.toString.name === "toString"; },
    function () { return Object.getPrototypeOf(Error("x")) === Error.prototype && Error("x") instanceof Error; },
    function () { return Error("x").toString() === "Error: x" && Error().toString() === "Error" && "" + Error("x") === "Error: x"; },
    function () { return Error.prototype.toString.call({ name: "", message: "m" }) === "m"; },
    function () { return Error.prototype.toString.call({}) === "Error" && Error.prototype.toString.call({ name: "", message: "" }) === ""; },
    function () { return Error.prototype.toString.call({ name: "N", message: "" }) === "N"; },
    function () { var caught = false; try { Error.prototype.toString.call(1); } catch (e) { caught = true; } return caught; },
    function () { var c; try { throw new Error("getter"); } catch (e) { c = e; } return c.message === "getter" && c instanceof Error; },
    function () {
      var e = Error("m", { cause: undefined });
      var m = Object.getOwnPropertyDescriptor(e, "message");
      var c = Object.getOwnPropertyDescriptor(e, "cause");
      return m.value === "m" && m.writable && m.configurable && !m.enumerable && c.value === undefined && c.writable && c.configurable && !c.enumerable && keyCount(e) === 0;
    },
    function () { function P() {} P.prototype = { cause: 42 }; return Error("m", new P()).cause === 42; },
    function () { var E = Error.bind(null, "bound"); return new E().message === "bound" && new E() instanceof Error; },
    function () { var e = Error("m", { cause: 1 }); e.message = "n"; e.cause = 2; delete e.message; return e.message === "" && e.cause === 2; },
    function () { return globalThis.Error === Error && Object.getPrototypeOf(Error.prototype) === Object.prototype; },
    function () {
      function F() {} F.prototype = Error.prototype;
      return Object.prototype.toString.call(Error()) === "[object Error]" && Object.prototype.toString.call(new F()) === "[object Object]" && Object.prototype.toString.call(Error.prototype) === "[object Object]";
    },
    function () { return Error.call(null, "x").message === "x" && Error.apply(null, ["y"]).message === "y" && Reflect.apply(Error, null, ["z"]).message === "z"; },
    function () { var caught = false; try { Error.prototype.toString.call(undefined); } catch (e) { caught = e instanceof TypeError; } return caught; }
  ]));
  // The native error constructors (native_error_constructors).
  var names = ["TypeError", "RangeError", "ReferenceError", "SyntaxError", "EvalError", "URIError", "InternalError"];
  var ctors = [TypeError, RangeError, ReferenceError, SyntaxError, EvalError, URIError, InternalError];
  for (var i = 0; i < names.length; i++) {
    (function (NAME, N) {
      out.push(NAME + ":" + rows([
        function () { return new N("m").message === "m"; },
        function () { return N("m").name === NAME && N("m").toString() === NAME + ": m"; },
        function () { return N.prototype.name === NAME && N.prototype.message === "" && N.prototype.constructor === N; },
        function () { return Object.getPrototypeOf(N) === Error && Object.getPrototypeOf(N.prototype) === Error.prototype; },
        function () { return N.prototype.toString === Error.prototype.toString && Object.getOwnPropertyDescriptor(N.prototype, "toString") === undefined; },
        function () { return N.name === NAME && N.length === 1; },
        function () {
          var p = Object.getOwnPropertyDescriptor(N.prototype, "name");
          var m = Object.getOwnPropertyDescriptor(N.prototype, "message");
          var c = Object.getOwnPropertyDescriptor(N.prototype, "constructor");
          return p.value === NAME && p.writable && p.configurable && !p.enumerable && m.value === "" && m.writable && m.configurable && !m.enumerable && c.value === N && c.writable && c.configurable && !c.enumerable;
        },
        function () {
          var n = Object.getOwnPropertyDescriptor(N, "name");
          var l = Object.getOwnPropertyDescriptor(N, "length");
          return n.value === NAME && !n.writable && !n.enumerable && n.configurable && l.value === 1 && !l.writable && !l.enumerable && l.configurable;
        },
        function () { return new N("m", { cause: 1 }).cause === 1; },
        function () { return new N() instanceof N && new N() instanceof Error && Object.getPrototypeOf(N()) === N.prototype; },
        function () { return N() !== N() && N().toString() === NAME && N(12).message === "12"; },
        function () { return Object.getOwnPropertyDescriptor(N(undefined), "message") === undefined && N(undefined).message === ""; },
        function () {
          var e = N("m", { cause: undefined });
          var m = Object.getOwnPropertyDescriptor(e, "message");
          var c = Object.getOwnPropertyDescriptor(e, "cause");
          return m.value === "m" && m.writable && m.configurable && !m.enumerable && c.value === undefined && c.writable && c.configurable && !c.enumerable && keyCount(e) === 0;
        },
        function () { function P() {} P.prototype = { cause: 42 }; return N("m", new P()).cause === 42 && N("m", 1).cause === undefined; },
        function () { var B = N.bind(null, "bound"); return new B().message === "bound" && new B() instanceof N && B().name === NAME; },
        function () { return N.call(null, "x").message === "x" && N.apply(null, ["y"]).message === "y" && Reflect.apply(N, null, ["z"]).message === "z"; },
        function () { var e = N("m", { cause: 1 }); e.message = "n"; e.cause = 2; delete e.message; return e.message === "" && e.cause === 2; },
        function () { return Object.prototype.toString.call(N()) === "[object Error]" && Object.prototype.toString.call(N.prototype) === "[object Object]"; },
        function () { var e; try { throw new N("caught"); } catch (caught) { e = caught; } return e instanceof N && e.message === "caught"; },
        function () { return globalThis[NAME] === N && Object.getOwnPropertyDescriptor(globalThis, NAME).writable; },
        function () { return !(new N() instanceof (N === RangeError ? TypeError : RangeError)); }
      ]));
    })(names[i], ctors[i]);
  }
  // Bound native error constructors (native_error_bound_constructor_prototypes).
  var bound = [];
  for (var j = 0; j < names.length; j++) {
    (function (N) {
      bound.push(
        function () { var B = N.bind(null); return Object.getPrototypeOf(B) === Error && B.prototype === Error.prototype; },
        function () { var B = N.bind(null); return Object.getOwnPropertyDescriptor(B, "prototype") === undefined && "prototype" in B; },
        function () { var B = N.bind(null); var C = B.bind(null); return Object.getPrototypeOf(C) === Error && C.prototype === Error.prototype && Object.getOwnPropertyDescriptor(C, "prototype") === undefined; },
        function () { var B = N.bind(null); B.extra = 1; return Object.getPrototypeOf(B) === Error && B.prototype === Error.prototype && B.extra === 1 && Object.getOwnPropertyDescriptor(B, "prototype") === undefined; },
        function () { var B = N.bind(null); B.extra = 1; var C = B.bind(null); C.extra = 2; return Object.getPrototypeOf(C) === Error && C.prototype === Error.prototype && C.extra === 2 && Object.getOwnPropertyDescriptor(C, "prototype") === undefined; }
      );
    })(ctors[j]);
  }
  out.push("B:" + rows(bound));
  // ES5 subclasses, Reflect.construct with a new target, `new Error`.
  out.push("X:" + rows([
    function () {
      function MyErr(m) { this.message = m; }
      MyErr.prototype = new Error();
      MyErr.prototype.name = "MyErr";
      MyErr.prototype.constructor = MyErr;
      var e = new MyErr("x");
      return String(e) === "MyErr: x" && e instanceof Error && e instanceof MyErr && Object.prototype.toString.call(e) === "[object Object]";
    },
    function () {
      function MyErr(m) { var e = Error.call(this, m); this.message = e.message; }
      MyErr.prototype = new Error();
      var e = new MyErr("x");
      return e.message === "x" && e instanceof Error && String(e) === "Error: x";
    },
    function () {
      function F() {}
      F.prototype = { tag: "F" };
      var e = Reflect.construct(TypeError, ["m"], F);
      return Object.getPrototypeOf(e) === F.prototype && e.message === "m" && e.tag === "F" && Object.prototype.toString.call(e) === "[object Error]";
    },
    function () { var e = new Error; return e instanceof Error && e.message === "" && Object.getOwnPropertyDescriptor(e, "message") === undefined; },
    function () { var e = new TypeError; return e instanceof TypeError && e.toString() === "TypeError"; },
    function () { var c = {}; return new Error("m", { cause: c }).cause === c; },
    function () { var e = new Error("m"); e.toString = function () { return "custom"; }; return "" + e === "custom"; }
  ]));
  // A global native error constructor replaced: an existing reference still
  // constructs its own type.
  var saved = Error;
  Error = 0;
  var kept = "";
  for (var k = 0; k < names.length; k++) {
    var N = ctors[k];
    globalThis[names[k]] = 0;
    var e = new N("kept");
    kept += (e instanceof N && e.name === names[k] && e.toString() === names[k] + ": kept") ? "y" : "n";
  }
  var se = new saved("kept");
  kept += se.toString() === "Error: kept" ? "y" : "n";
  globalThis.Error = 0;
  kept += typeof Error === "number" ? "y" : "n";
  Error = saved;
  for (var m = 0; m < names.length; m++) globalThis[names[m]] = ctors[m];
  kept += Error === saved && globalThis.TypeError === TypeError ? "y" : "n";
  kept += new TypeError("back").toString() === "TypeError: back" ? "y" : "n";
  out.push("K:" + kept);
  return out.join("|");
})();
