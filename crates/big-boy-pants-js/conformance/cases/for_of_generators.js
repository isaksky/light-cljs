// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 3703-3712
// and 3771-3793 (`for_of_loops` and `for_of_builtin_iterators`, bbp-237: the
// sum, the per-iteration closures, `let`/`const` bodies and `for (let x of
// x)`) with generators in place of arrays (array iterators are
// light-cljs-844), and lines 3810-3825 (`for_of_abrupt_completions`'
// labelled jumps), each row run in its own function and its result appended
// to one log. A sum over a generator; `break` out of a loop over a generator
// suspended in `try`/`finally` runs its finally block (the generator's
// `return`), after which the generator is done; an exception from the body
// does the same; a labelled `continue` across two generator loops; closures
// capture each iteration's `let` and `const` binding; a `const` binding is
// not assignable; `for (let x of x)` reads x in its temporal dead zone;
// a generator's own @@iterator is %IteratorPrototype%[@@iterator].
// Completes with
// "6,1F:true,1F:boom:true,a1b1a2b1,1:3:23,2,ReferenceError,true:3".
(function () {
  var log = "";
  function row(f) {
    var v;
    try { v = f(); } catch (e) { v = e instanceof Error ? e.name : "thrown:" + e; }
    log += (log === "" ? "" : ",") + v;
  }
  function* count(n) { for (let i = 1; i <= n; i++) yield i; }
  row(function () { var s = 0; for (const x of count(3)) s += x; return s; });
  row(function () {
    var out = "";
    function* g() { try { yield 1; yield 2; } finally { out += "F"; } }
    var it = g();
    for (const x of it) { out += x; break; }
    return out + ":" + it.next().done;
  });
  row(function () {
    var out = "";
    function* g() { try { yield 1; yield 2; } finally { out += "F"; } }
    var it = g();
    try { for (const x of it) { out += x; throw "boom"; } } catch (e) { out += ":" + e; }
    return out + ":" + it.next().done;
  });
  row(function () {
    var out = "";
    outer: for (const x of count(2)) {
      for (const y of count(3)) { out += "a" + x + "b" + y; continue outer; }
    }
    return out;
  });
  row(function () {
    var fs = [], gs = [];
    for (let x of count(2)) { fs.push(function () { return x; }); x += 10; }
    for (const y of count(3)) gs.push(function () { return y; });
    var n = 0;
    for (let x of count(2)) { let f = function () { return x; }; x += 10; n += f(); }
    return fs[0]() - 10 + ":" + gs[2]() + ":" + n;
  });
  row(function () {
    var n = 0;
    for (const x of count(2)) { try { x = 3; } catch (e) { if (e instanceof TypeError) n++; } }
    return n;
  });
  row(function () { for (let x of x) {} return "no error"; });
  row(function () {
    var it = count(3);
    var same = it[Symbol.iterator]() === it, s = 0;
    for (const x of it) s += x;
    return same + ":" + (s === 6 ? 3 : s);
  });
  return log;
})();
