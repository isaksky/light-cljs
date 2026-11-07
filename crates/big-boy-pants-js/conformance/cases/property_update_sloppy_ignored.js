// bbp-authored conformance case, run as a sloppy script.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 13950-13964,
// compound_assignment_on_properties (bbp-260), its sloppy rows: a write to a
// getter-only property does nothing in sloppy code, while the expression's
// value is still the operator's result, for `++`, `op=` and `&&=` on `o.p`
// and `o[k]`, and to a non-writable data property (a function's length).
// Completes with "6,0,2,9,1,4,1".
var o = { get k() { return 1; } };
var k = "k";
var r = (o.k += 5);
var s = (o[k] -= 1);
var t = o[k]++;
var u = (o[k] &&= 9);
function g(a) {}
var w = (g["len" + "gth"] += 3);
r + "," + s + "," + (t + 1) + "," + u + "," + o.k + "," + w + "," + g.length;
