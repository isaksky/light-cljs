// bbp-authored conformance case (light-cljs-871).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 14374-14391
// (`object_literals_and_named_properties`: `u.x` and `n.x = 1`), 18253-18272
// (`methods_on_primitive_bases`: undefined and null bases at the member
// operator, `u['x'] = 1`) and 6562 (`number_to_string_with_radix`), with
// light-cljs-765's probe rows: the messages of the TypeErrors and RangeErrors
// the engine raises where QuickJS and the Pants engine word them the same.
// A named read or a write on an undefined or null base names its key (a
// write's computed key too, as its ToPropertyKey spells it: an index, a
// fraction, -0, a number past the index range, true, null, a non-ASCII
// string) and the base; a read through a method call, an update, a compound
// assignment, a tagged template and an assignment's right-hand side are
// reads; a strict write; Number.prototype.toString's radix RangeError.
// Not checked here: the rows where the Pants engine words a message
// otherwise (run_time_error_messages_quickjs); and, since all three engines
// differ, a strict write to a getter-only accessor (QuickJS "no setter for
// property"), a strict `'abc'.length = 1` (QuickJS "'length' is read-only")
// and `({valueOf: null, toString: null}) + 1` (QuickJS "toPrimitive"); the
// callee names of not-a-function and not-a-constructor errors
// (light-cljs-839). Completes with the messages joined by "|":
// "TypeError: cannot read property 'f' of undefined|TypeError: cannot read property 'x' of null|TypeError: cannot read property 'z' of undefined|TypeError: cannot read property 'x' of undefined|TypeError: cannot read property 'length' of undefined|TypeError: cannot read property 'y' of undefined|TypeError: cannot read property 'x' of undefined|TypeError: cannot read property 'x' of undefined|TypeError: cannot read property 'x' of undefined|TypeError: cannot read property 'f' of undefined|TypeError: cannot read property 'y' of undefined|TypeError: cannot read property 'x' of undefined|TypeError: cannot set property 'x' of null|TypeError: cannot set property 'k' of undefined|TypeError: cannot set property 'f' of undefined|TypeError: cannot set property '3' of undefined|TypeError: cannot set property '1.5' of undefined|TypeError: cannot set property '0' of undefined|TypeError: cannot set property '4294967295' of undefined|TypeError: cannot set property '9007199254740992' of undefined|TypeError: cannot set property '1e+21' of undefined|TypeError: cannot set property 'true' of undefined|TypeError: cannot set property 'null' of undefined|TypeError: cannot set property 'é' of undefined|TypeError: cannot set property 'f' of undefined|TypeError: cannot set property 'f' of undefined|RangeError: radix must be between 2 and 36|RangeError: radix must be between 2 and 36".
(function () {
  var u, n = null, o = {};
  var rows = [
    // Named reads.
    function () { u.f(); },
    function () { n.x++; },
    function () { o.y.z; },
    function () { u.x; },
    function () { u.length; },
    function () { "abc".x.y; },
    function () { u.x += 1; },
    function () { u.x--; },
    function () { u.x.y = 2; },
    function () { u.f``; },
    function () { u.x = u.y; },
    function () { var r = u.x; return r; },
    // Writes, named and computed.
    function () { n.x = 1; },
    function () { u["k"] = 1; },
    function () { u.f = 1; },
    function () { u[3] = 1; },
    function () { u[1.5] = 1; },
    function () { u[-0] = 1; },
    function () { u[4294967295] = 1; },
    function () { u[2 ** 53] = 1; },
    function () { u[1e21] = 1; },
    function () { u[true] = 1; },
    function () { u[null] = 1; },
    function () { u["é"] = 1; },
    function () { "use strict"; u.f = 1; },
    function () { "use strict"; u["f"] = 1; },
    // Number.prototype.toString's radix.
    function () { (1).toString(1); },
    function () { (1).toString(37); }
  ];
  var out = [];
  for (var i = 0; i < rows.length; i++) {
    try { rows[i](); out.push("none"); } catch (e) { out.push(e.name + ": " + e.message); }
  }
  return out.join("|");
})();
