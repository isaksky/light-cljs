// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 1571-1590,
// closures_check_the_temporal_dead_zone_at_run_time (light-cljs-824): a
// function that reads or assigns a script `let` before its declaration runs
// throws ReferenceError, and so does the script's own `typeof` of one;
// afterwards the same functions work. Completes with
// "ReferenceError ReferenceError ReferenceError 3".
function early() { return x; }
function assign() { x = 2; }
var read, written, typed;
try { early(); read = "read"; } catch (e) { read = e.name; }
try { assign(); written = "written"; } catch (e) { written = e.name; }
try { typeof y; typed = "typeof"; } catch (e) { typed = e.name; }
let x = 1;
let y = 2;
assign();
read + " " + written + " " + typed + " " + (early() + 1 === 3 ? "3" : "?");
