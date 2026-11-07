// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 3713-3767
// (`for_of_loops`, bbp-237, its rows 3 to 5): user iterables with object
// literal `next` and `return` methods, each row run in its own function and
// its result appended to one log. `next` is called n + 1 times; `return`
// exactly once on `break`, on a `return` from the enclosing function and on a
// `throw` from the body, never on exhaustion or `continue`; the body's
// exception survives a `return` that throws, while a `return` that throws
// after `break` replaces the break; a non-object `return` result after
// `break` is a TypeError, as is iterating a number; a method returning a
// primitive is a TypeError and an exception from the method or a `next`
// getter propagates, without running the body or closing; Function.prototype
// .call as the method calls the iterable itself. Arrays and strings as
// iterables are bbp/for_of_builtin_iterators (light-cljs-844).
// Completes with
// "4:0:4,1:1,1:2:42,1:3:17,17:1,99:2,TypeError,TypeError,TypeError,3:1,TypeError;method;getter;0,3".
(function () {
  var log = "";
  function row(f) {
    var v;
    try { v = f(); } catch (e) { v = e instanceof Error ? e.name : "thrown:" + e; }
    log += (log === "" ? "" : ",") + v;
  }
  function counting() {
    var state = { calls: 0, closes: 0 };
    state.iterable = {};
    state.iterable[Symbol.iterator] = function () {
      return {
        next() { state.calls++; return { value: state.calls, done: state.calls > 3 }; },
        return() { state.closes++; return {}; }
      };
    };
    return state;
  }
  // Exhaustion and `continue` never close: next runs n + 1 times.
  row(function () {
    var s = counting(), sum = 0;
    for (const x of s.iterable) { if (x === 2) continue; sum += x; }
    return s.calls + ":" + s.closes + ":" + sum;
  });
  // `break` closes once.
  row(function () {
    var s = counting();
    for (const x of s.iterable) break;
    return s.calls + ":" + s.closes;
  });
  // A `return` from the enclosing function closes, and the value survives.
  row(function () {
    var s = counting(), r;
    function f() { for (const x of s.iterable) { if (x === 1) s.closes++; return 42; } }
    r = f();
    return s.calls + ":" + s.closes + ":" + r;
  });
  // A `throw` from the body closes, and the exception survives.
  row(function () {
    var s = counting(), caught;
    s.closes = 2;
    try { for (const x of s.iterable) throw 17; } catch (e) { caught = e; }
    return s.calls + ":" + s.closes + ":" + caught;
  });
  // The body's exception survives a `return` that throws.
  row(function () {
    var closed = 0, caught;
    var it = { next() { return { value: 1, done: false }; }, return() { closed++; throw 99; } };
    it[Symbol.iterator] = function () { return this; };
    try { for (let x of it) throw 17; } catch (e) { caught = e; }
    return caught + ":" + closed;
  });
  // A `return` that throws after `break` replaces the break.
  row(function () {
    var closed = 1, caught;
    var it = { next() { return { value: 1, done: false }; }, return() { closed++; throw 99; } };
    it[Symbol.iterator] = function () { return this; };
    try { for (let x of it) break; } catch (e) { caught = e; }
    return caught + ":" + closed;
  });
  // A non-object `return` result after `break` is a TypeError.
  row(function () {
    var it = { next() { return { value: 1, done: false }; }, return() { return 1; } };
    it[Symbol.iterator] = function () { return this; };
    for (const x of it) break;
    return "no error";
  });
  // A number is not iterable.
  row(function () { for (const x of 1) {} return "no error"; });
  // Neither is an object without @@iterator.
  row(function () { for (const x of {}) {} return "no error"; });
  // The iterator is the method's result, called with the iterable as this.
  row(function () {
    var receivers = 0, n = 0, closes = 0;
    var iterable = {};
    iterable[Symbol.iterator] = function () {
      if (this === iterable) receivers++;
      return { next() { n++; return { value: n, done: n > 2 }; }, return() { closes++; return {}; } };
    };
    for (const x of iterable) {}
    return n + ":" + receivers;
  });
  // Acquisition failures: a method returning a primitive, a method that
  // throws and a `next` getter that throws; the body never runs and nothing
  // is closed.
  row(function () {
    var out = "", closes = 0;
    function attempt(method) {
      var iterable = {};
      iterable[Symbol.iterator] = method;
      try { for (const x of iterable) out += "body"; } catch (e) { out += (e instanceof Error ? e.name : e) + ";"; }
    }
    attempt(function () { return 1; });
    attempt(function () { throw "method"; });
    attempt(function () { return { get next() { throw "getter"; }, return() { closes++; return {}; } }; });
    return out + closes;
  });
  // Function.prototype.call as the method calls the iterable itself.
  row(function () {
    var n = 0;
    function f() { return { next() { n++; return { value: n, done: n > 2 }; } }; }
    f[Symbol.iterator] = Function.prototype.call;
    var s = 0;
    for (const x of f) s += x;
    return s;
  });
  return log;
})();
