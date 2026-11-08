// bbp-authored conformance case (light-cljs-869).
// Derives from: crates/big-boy-pants-js/rust/tests/exceptions.rs lines 532-596
// (`engine_errors_are_error_instances`) and 633-661
// (`javascript_run_time_errors_are_catchable_error_objects`) (bbp-230): for
// an undefined name, a call of null, a property read of undefined, a TDZ read
// (direct and through a hoisted function) and a constant assignment, the
// caught error is an instance of its constructor and of Error, its prototype
// is its constructor's, its `message` is an own, writable, configurable,
// non-enumerable string, it has no enumerable own key (for-in for
// `Object.keys`) and no own `cause`, Object.prototype.toString gives
// "[object Error]" and toString() is name + ": " + message; each failure makes
// a fresh error object; a caught error mutated and rethrown is the same
// object. Stack overflow is an InternalError "stack overflow" (both oracles
// agree on this message), not a RangeError, also when each call passes 150
// arguments (the value-stack limit) and through `map` callbacks (native
// frames), and a 500-frame recursion runs afterwards, as do 1,000-frame
// recursions with a try/catch or a try/finally in every frame. `Array(-1)`
// is a RangeError "invalid array length". The property read's message is
// "cannot read property 'x' of undefined" on both oracles (light-cljs-871).
// Not checked: the other message texts, which differ between the oracles
// (run_time_error_messages and run_time_error_messages_quickjs hold the
// run-time messages; light-cljs-839 the callee names); `a.length = -1`
// (the core built-ins area, light-cljs-767); `JSON.parse` errors (the core built-ins
// area); the uncaught rethrow's reported position (the harness compares no
// positions). Completes with
// "ReferenceError:yyyyyyy0nyy|TypeError:yyyyyyy0nyy|TypeError:yyyyyyy0nyy|ReferenceError:yyyyyyy0nyy|ReferenceError:yyyyyyy0nyy|TypeError:yyyyyyy0nyy|n|y:changed|InternalError:stack overflow:y:n|500|InternalError|InternalError|1000|1000:1001|RangeError:invalid array length|cannot read property 'x' of undefined".
(function () {
  var r = [];
  function yn(b) { return b ? "y" : "n"; }
  function describe(body) {
    try { body(); return "none"; } catch (e) {
      var C = e.constructor;
      var d = Object.getOwnPropertyDescriptor(e, "message");
      var keys = 0;
      for (var k in e) keys++;
      return e.name + ":" + [
        yn(e instanceof C), yn(e instanceof Error), yn(Object.getPrototypeOf(e) === C.prototype),
        yn(d !== undefined && typeof d.value === "string"), yn(d.writable), yn(!d.enumerable), yn(d.configurable),
        keys, yn(Object.getOwnPropertyDescriptor(e, "cause") !== undefined),
        yn(Object.prototype.toString.call(e) === "[object Error]"),
        yn(e.toString() === e.name + ": " + e.message)
      ].join("");
    }
  }
  r.push(describe(function () { nope; }));
  r.push(describe(function () { null(); }));
  r.push(describe(function () { var u; u.x; }));
  r.push(describe(function () { x; let x; }));
  r.push(describe(function () { f(); let x = 1; function f() { return x; } }));
  r.push(describe(function () { const c = 1; c = 2; }));
  // Each failure makes a fresh error object.
  var a, b;
  try { undefinedTwice869; } catch (e) { a = e; }
  try { undefinedTwice869; } catch (e) { b = e; }
  r.push(yn(a === b));
  // A caught error mutated and rethrown is the same object.
  var first;
  try { try { nope; } catch (e) { first = e; e.message = "changed"; throw e; } }
  catch (f) { r.push(yn(f === first) + ":" + f.message); }
  // Stack overflow.
  function deep() { return deep(); }
  try { deep(); } catch (e) {
    r.push([e.name, e.message, yn(e instanceof InternalError), yn(e instanceof RangeError)].join(":"));
  }
  function count(k) { return k === 0 ? 0 : 1 + count(k - 1); }
  r.push(count(500));
  function wide(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, b0, b1, b2, b3, b4, b5, b6, b7, b8, b9,
                c0, c1, c2, c3, c4, c5, c6, c7, c8, c9, d0, d1, d2, d3, d4, d5, d6, d7, d8, d9,
                e0, e1, e2, e3, e4, e5, e6, e7, e8, e9, f0, f1, f2, f3, f4, f5, f6, f7, f8, f9,
                g0, g1, g2, g3, g4, g5, g6, g7, g8, g9, h0, h1, h2, h3, h4, h5, h6, h7, h8, h9,
                i0, i1, i2, i3, i4, i5, i6, i7, i8, i9, j0, j1, j2, j3, j4, j5, j6, j7, j8, j9,
                k0, k1, k2, k3, k4, k5, k6, k7, k8, k9, l0, l1, l2, l3, l4, l5, l6, l7, l8, l9,
                m0, m1, m2, m3, m4, m5, m6, m7, m8, m9, n0, n1, n2, n3, n4, n5, n6, n7, n8, n9,
                o0, o1, o2, o3, o4, o5, o6, o7, o8, o9) {
    return wide(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, b0, b1, b2, b3, b4, b5, b6, b7, b8, b9,
                c0, c1, c2, c3, c4, c5, c6, c7, c8, c9, d0, d1, d2, d3, d4, d5, d6, d7, d8, d9,
                e0, e1, e2, e3, e4, e5, e6, e7, e8, e9, f0, f1, f2, f3, f4, f5, f6, f7, f8, f9,
                g0, g1, g2, g3, g4, g5, g6, g7, g8, g9, h0, h1, h2, h3, h4, h5, h6, h7, h8, h9,
                i0, i1, i2, i3, i4, i5, i6, i7, i8, i9, j0, j1, j2, j3, j4, j5, j6, j7, j8, j9,
                k0, k1, k2, k3, k4, k5, k6, k7, k8, k9, l0, l1, l2, l3, l4, l5, l6, l7, l8, l9,
                m0, m1, m2, m3, m4, m5, m6, m7, m8, m9, n0, n1, n2, n3, n4, n5, n6, n7, n8, n9,
                o0, o1, o2, o3, o4, o5, o6, o7, o8, o9) + 1;
  }
  try { wide(); } catch (e) { r.push(e.name); }
  function viaMap(n) { return [n].map(function (x) { return viaMap(x + 1); }); }
  try { viaMap(0); } catch (e) { r.push(e.name); }
  // Recursions with a handler in every frame still run afterwards.
  function caught(n) { try { return n === 0 ? 0 : caught(n - 1) + 1; } catch (e) { throw e; } }
  r.push(caught(1000));
  var unwound = 0;
  function finals(n) { try { return n === 0 ? 0 : finals(n - 1) + 1; } finally { unwound++; } }
  r.push(finals(1000) + ":" + unwound);
  // Array(-1).
  try { Array(-1); } catch (e) { r.push(e.name + ":" + e.message); }
  // The property read's message.
  try { var u; u.x; } catch (e) { r.push(e.message); }
  return r.join("|");
})();
