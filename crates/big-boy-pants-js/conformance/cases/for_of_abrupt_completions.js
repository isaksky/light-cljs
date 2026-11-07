// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 3810-3898
// (`for_of_abrupt_completions`, bbp-237): its rows with a user iterable in
// place of the outer `[1, 2]` (array iterators are light-cljs-844), each run
// in its own function and its result appended to one log. Labelled
// `continue outer` and `break outer` through nested for-of and `finally`
// close the inner iterator once each ("FCFC"); `continue same` never closes;
// a `next` that throws, a `done` or `value` getter that throws and a
// non-object `next` result never close; a `return` getter that throws
// replaces a `break` or `return` but not the body's exception; a `next`
// getter is read once, when the loop starts; a const target closes the
// iterator before its TypeError; a `next` that is not callable is a
// TypeError at its call, without closing; `return` is called on the iterator.
// Completes with
// "FCFC,3:0,4:0,7:99:99:TypeError:3,3:1:0,TypeError:1,ok:1".
(function () {
  var log = "";
  function row(f) {
    var v;
    try { v = f(); } catch (e) { v = e instanceof Error ? e.name : "thrown:" + e; }
    log += (log === "" ? "" : ",") + v;
  }
  function pair() {
    var n = 0;
    var it = { next() { n++; return { value: n, done: n > 2 }; } };
    it[Symbol.iterator] = function () { return this; };
    return it;
  }
  row(function () {
    var log = "", n = 0;
    var it = { next() { return { value: ++n, done: false }; }, return() { log += "C"; return {}; } };
    it[Symbol.iterator] = function () { return this; };
    outer: for (let x of pair()) {
      for (let y of it) { try { if (x === 1) continue outer; break outer; } finally { log += "F"; } }
    }
    return log;
  });
  row(function () {
    var n = 0, c = 0;
    var it = { next() { n++; return { value: n, done: n > 2 }; }, return() { c++; return {}; } };
    it[Symbol.iterator] = function () { return this; };
    same: for (let x of it) { try { continue same; } finally {} }
    return n + ":" + c;
  });
  row(function () {
    var closes = 0, caught = 0;
    var it = { next() { throw 7; }, return() { closes++; return {}; } };
    it[Symbol.iterator] = function () { return this; };
    try { for (const x of it) {} } catch (e) { if (e === 7) caught++; }
    it.next = function () { return { get done() { throw 7; } }; };
    try { for (const x of it) {} } catch (e) { if (e === 7) caught++; }
    it.next = function () { return { done: false, get value() { throw 7; } }; };
    try { for (const x of it) {} } catch (e) { if (e === 7) caught++; }
    it.next = function () { return 1; };
    try { for (const x of it) {} } catch (e) { if (e instanceof TypeError) caught++; }
    return caught + ":" + closes;
  });
  row(function () {
    var count = 0, out = [];
    var it = { next() { return { value: 1, done: false }; }, get return() { count++; throw 99; } };
    it[Symbol.iterator] = function () { return this; };
    try { for (let x of it) throw 7; } catch (e) { out.push(e); }
    try { for (let x of it) break; } catch (e) { out.push(e); }
    function f() { for (let x of it) return 1; }
    try { f(); } catch (e) { out.push(e); }
    // A `next` that is not callable is a TypeError at the call.
    it.next = 5;
    try { for (let x of it) {} } catch (e) { out.push(e.name); }
    // The first three, then the TypeError, then how often `return` was read.
    return out.join(":") + ":" + count;
  });
  row(function () {
    var gets = 0, n = 0, closed = 0;
    var it = { get next() { gets++; return function () { n++; return { value: n, done: n > 2 }; }; }, return() { closed++; return {}; } };
    it[Symbol.iterator] = function () { return this; };
    var total = 0; for (let x of it) total += x;
    return total + ":" + gets + ":" + closed;
  });
  row(function () {
    var closed = 0, caught;
    var it = { next() { return { value: 1, done: false }; }, return() { closed++; return {}; } };
    it[Symbol.iterator] = function () { return this; };
    const k = 0;
    try { for (k of it) {} } catch (e) { caught = e.name; }
    return caught + ":" + closed;
  });
  // The iterator's `return` sees the iterator as this, with no argument.
  row(function () {
    var closed = 0, seen = "";
    var it = { next() { return { value: 1, done: false }; }, return() { closed++; seen = this === it ? "ok" : "bad"; return {}; } };
    it[Symbol.iterator] = function () { return this; };
    for (const x of it) break;
    return seen + ":" + closed;
  });
  return log;
})();
