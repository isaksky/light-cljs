// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 19646, 19734,
// global_object_bindings (failure("missing = 1") is a ReferenceError) and
// sloppy_implicit_globals_and_delete ("function f() { 'use strict'; u2 = 1; }
// f()"; light-cljs-824): in strict code, assigning a name no binding or
// global property holds throws ReferenceError and creates nothing.
// Completes with "ReferenceError ReferenceError ReferenceError undefined absent".
function assign() { missing = 1; }
var called, compound, read;
try { assign(); called = "assigned"; } catch (e) { called = e.name; }
try { missing2 += 1; compound = "assigned"; } catch (e) { compound = e.name; }
try { missing3; read = "read"; } catch (e) { read = e.name; }
called + " " + compound + " " + read + " " + typeof missing + " " + ("missing" in globalThis ? "?" : "absent");
