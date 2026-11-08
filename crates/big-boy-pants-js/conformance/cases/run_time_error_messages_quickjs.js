// bbp-authored conformance case (light-cljs-871).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 15655-15675
// and 16015-16035 (computed reads of null and undefined bases) and
// light-cljs-765's probe rows: the run-time error messages Goose words as
// QuickJS does where the Pants engine words them otherwise (areas.edn
// :pants-gap). A computed read on an undefined or null base names no key
// (QuickJS's OP_get_array_el; the Pants engine names it), also when its key
// is an object whose conversion would throw (QuickJS raises the TypeError
// before converting; the Pants engine says "cannot read a property of"),
// through a computed update or compound assignment (ToPropertyKey); GetIterator
// of undefined or null is a read of its @@iterator, spelled 'Symbol.iterator'
// (the Pants engine spells it 'Symbol(Symbol.iterator)'), from spread, array
// destructuring and for-of, while `for (var x of 1)` stays "value is not
// iterable" (the Pants engine: "undefined is not a function"); a write to a
// well-known symbol key on undefined and null; a key with an embedded NUL
// ends at it (QuickJS's %s); `delete` on an undefined or null base and object destructuring
// of one are "cannot convert to object" (the Pants engine: "cannot delete
// property 'x' of null", "cannot destructure null or undefined"); a strict
// delete of a non-configurable property is "could not delete property"; the
// built-ins' ToObject of undefined or null (hasOwnProperty, Object.hasOwn,
// Object.getOwnPropertyDescriptor, Array.prototype.map, forEach and join)
// is "cannot convert to object", and Object.getPrototypeOf(null) is "not an
// object" (the Pants engine names the value and the operation).
// Not checked here: QuickJS's C buffer bounds (a non-ASCII key cut near 64
// bytes, a message near 256); a computed write whose key is an object on an undefined
// base (QuickJS converts the key and names it; Goose does not convert it
// yet); the built-ins still pending on Goose (Object.keys, Object.entries,
// Object.assign, Array.from and others, the objects and core built-ins
// areas). Completes with the messages joined by "|":
// "TypeError: cannot read property of null|TypeError: cannot read property of null|TypeError: cannot read property of undefined|TypeError: cannot read property of undefined|TypeError: cannot read property of undefined|TypeError: cannot read property of null|TypeError: cannot read property of undefined|TypeError: cannot read property of undefined|TypeError: cannot read property of undefined|TypeError: cannot read property 'Symbol.iterator' of undefined|TypeError: cannot read property 'Symbol.iterator' of null|TypeError: cannot read property 'Symbol.iterator' of undefined|TypeError: cannot read property 'Symbol.iterator' of null|TypeError: cannot read property 'Symbol.iterator' of null|TypeError: value is not iterable|TypeError: cannot set property 'Symbol.iterator' of undefined|TypeError: cannot set property 'Symbol.iterator' of null|TypeError: cannot set property 'a' of null|TypeError: cannot convert to object|TypeError: cannot convert to object|TypeError: cannot convert to object|TypeError: cannot convert to object|TypeError: cannot convert to object|TypeError: cannot convert to object|TypeError: cannot convert to object|TypeError: could not delete property|TypeError: cannot convert to object|TypeError: cannot convert to object|TypeError: cannot convert to object|TypeError: cannot convert to object|TypeError: cannot convert to object|TypeError: cannot convert to object|TypeError: not an object".
(function () {
  var u, n = null, s = Symbol.iterator;
  var rows = [
    // Computed reads.
    function () { null["foo"]; },
    function () { null[0]; },
    function () { u[s]; },
    function () { u["f"](); },
    function () { var k = "q"; return u[k]; },
    function () { null[{ toString: function () { throw 1; } }]; },
    function () { u[{ toString: function () { throw 1; } }]; },
    function () { u["x"] += 1; },
    function () { u["k"]++; },
    // GetIterator of undefined and null.
    function () { [...undefined]; },
    function () { [...null]; },
    function () { var [a] = u; },
    function () { var [a] = null; },
    function () { for (var x of null) {} },
    // A primitive that is not iterable.
    function () { for (var x of 1) {} },
    // Writes to a symbol key.
    function () { u[s] = 1; },
    function () { n[s] = 1; },
    // A key with an embedded NUL ends there (QuickJS formats it with %s).
    function () { n["a\u0000b"] = 1; },
    // delete and object destructuring.
    function () { delete n.x; },
    function () { delete u[1]; },
    function () { var { a } = u; },
    function () { var { a } = null; },
    function () { var { a: { b } } = {}; },
    function () { var { [s]: q } = u; },
    function () { "use strict"; delete n.x; },
    function () { "use strict"; delete "abc".length; },
    // The built-ins' ToObject.
    function () { ({}).hasOwnProperty.call(null, "x"); },
    function () { Object.hasOwn(null, "a"); },
    function () { Object.getOwnPropertyDescriptor(null, "x"); },
    function () { [].map.call(null, function (x) { return x; }); },
    function () { [].forEach.call(undefined, function () {}); },
    function () { Array.prototype.join.call(null); },
    function () { Object.getPrototypeOf(null); }
  ];
  var out = [];
  for (var i = 0; i < rows.length; i++) {
    try { rows[i](); out.push("none"); } catch (e) { out.push(e.name + ": " + e.message); }
  }
  return out.join("|");
})();
