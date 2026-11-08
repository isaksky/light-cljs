// bbp-authored conformance case (light-cljs-869), a sloppy script.
// Derives from: none (light-cljs-765's probe rows; the sloppy-mode
// counterparts of exceptions_catch_bindings, after
// crates/big-boy-pants-js/rust/tests/exceptions.rs lines 481-512,
// `catch_parameters_are_block_scoped_bindings`, bbp-11): ECMAScript Annex
// B.3.4, a `var e` in a catch block whose parameter is `e` assigns the
// parameter and the function's own `e` is hoisted but stays undefined; a
// function declaration inside a catch block is callable there and, by Annex
// B.3.3, visible as a function-scoped var after the block. Completes with
// "undefined|undefined|undefined|2|f|function".
(function () {
  var r = [];
  function annex() {
    var before = typeof e;
    var inner;
    try { throw 1; } catch (e) { var e = 2; inner = e; }
    return [before, typeof e, String(e), inner].join("|");
  }
  r.push(annex());
  function declared() {
    var got;
    try { throw 1; } catch (e) { function e2() { return "f"; } got = e2(); }
    return [got, typeof e2].join("|");
  }
  r.push(declared());
  return r.join("|");
})();
