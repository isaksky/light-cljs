// bbp-authored conformance case (light-cljs-869).
// Derives from: crates/big-boy-pants-js/rust/tests/exceptions.rs lines 481-512
// (`catch_parameters_are_block_scoped_bindings`) (bbp-11), its execution rows:
// the parameter shadows an outer binding only inside the catch body, can be
// assigned and captured by a closure, and `catch ({a})` binds a pattern. Plus
// rows written for this slice (light-cljs-765's probe): `catch ([p, q = 2])`,
// `catch ({m: {n}, o = 4})` and `catch ([h, ...rest])`; per-iteration catch
// bindings captured by closures; a throwing default inside a catch pattern;
// `throw null` into `catch ({a})` is a TypeError; a catch parameter shadowing
// a function parameter; a `var e` in a catch block whose parameter is `e`
// (ECMAScript Annex B.3.4: the var initialiser assigns the parameter, and the
// function's own `e` stays undefined). exceptions_catch_bindings_sloppy has
// the sloppy-mode rows.
// Not ported: the early-error rows (`let e` or `function e` redeclaring the
// parameter, `try {}` with no handler, `throw` followed by a newline): they
// are exporter SyntaxErrors, not execution cases. Completes with
// "21|23|21|3|7|2|3|cd|TypeError|1|undefined,2".
(function () {
  var r = [];
  // The parameter shadows an outer binding only inside the catch body.
  let e = 1;
  let x;
  try { throw 2; } catch (e) { x = e; }
  r.push(x * 10 + e);
  // It can be assigned and captured by a closure.
  let k;
  try { throw 1; } catch (e) { k = function () { e = e + 1; return e; }; e = 10; }
  r.push(k() + k());
  // Patterns.
  let a = 1;
  let y;
  try { throw { a: 2 }; } catch ({ a }) { y = a; }
  r.push(y * 10 + a);
  try { throw [1]; } catch ([p, q = 2]) { r.push(p + q); }
  try { throw { m: { n: 3 } }; } catch ({ m: { n }, o = 4 }) { r.push(n + o); }
  try { throw [1, 2, 3]; } catch ([h, ...rest]) { r.push(rest.length); }
  // A fresh catch binding per iteration, captured by closures.
  var fs = [];
  for (var i = 0; i < 3; i++) { try { throw i; } catch (c) { fs.push(function () { return c; }); } }
  r.push(fs[0]() + fs[1]() + fs[2]());
  // A throwing default inside a catch pattern.
  var cd;
  try { try { throw {}; } catch ({ d = (function () { throw "cd"; })() }) { cd = "no"; } } catch (err) { cd = err; }
  r.push(cd);
  // `throw null` into an object pattern is a TypeError.
  var tn = "none";
  try { try { throw null; } catch ({ a }) { tn = "bound"; } } catch (err) { tn = err.name; }
  r.push(tn);
  // A catch parameter shadowing a function parameter.
  function sh(e) { try { throw 2; } catch (e) { e = 3; } return e; }
  r.push(sh(1));
  // Annex B.3.4: `var e` in a catch block whose parameter is `e`.
  function annex() {
    var inner;
    try { throw 1; } catch (e) { var e = 2; inner = e; }
    return [typeof e, inner].join(",");
  }
  r.push(annex());
  return r.join("|");
})();
