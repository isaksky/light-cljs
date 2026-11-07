// bbp-authored conformance case, run as a sloppy script (areas.edn :mode).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 19695-19708,
// sloppy_implicit_globals_and_delete (light-cljs-824): assigning an
// undeclared name in sloppy code creates a writable, enumerable,
// configurable global property, which `delete` removes; `delete` of a
// script var, function or lexical binding is false and keeps it, and of an
// unknown name true. Completes with "WEC 3 deleted undefined kept kept kept true 7 5".
function make() { implicit = 3; }
function flag(b, yes, no) { return b ? yes : no; }
make();
var d = Object.getOwnPropertyDescriptor(globalThis, "implicit");
var attrs = flag(d.writable, "W", "-") + flag(d.enumerable, "E", "-") + flag(d.configurable, "C", "-");
var seen = flag(implicit === 3, "3", "?");
var deleted = flag(delete implicit, "deleted", "kept");
var gone = typeof implicit;
var v = 1;
function f() { return 2; }
let w = 4;
var kept = flag(delete v, "deleted", "kept") + " " + flag(delete f, "deleted", "kept") + " " +
  flag(delete w, "deleted", "kept") + " " + flag(delete nothing_here, "true", "false");
again = 5;
attrs + " " + seen + " " + deleted + " " + gone + " " + kept + " " + flag(v + f() + w === 7, "7", "?") + " " +
  flag(globalThis.again === 5, "5", "?");
