// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 1594-1600,
// closures_check_the_temporal_dead_zone_at_run_time ("assignment to constant
// variable 'c'"; light-cljs-824): assigning a script `const`, from a
// function or at the top level, throws TypeError and leaves the value.
// Completes with "TypeError TypeError 1".
const k = 1;
function set() { k = 2; }
var inner, outer;
try { set(); inner = "assigned"; } catch (e) { inner = e.name; }
try { k = 3; outer = "assigned"; } catch (e) { outer = e.name; }
inner + " " + outer + " " + (k === 1 ? "1" : "?");
