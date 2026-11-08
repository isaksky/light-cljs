// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 2541-2618,
// indirect_eval (bbp-106), its compile-error rows without e.stack
// (light-cljs-856). Source an indirect eval cannot compile throws a
// catchable SyntaxError, an instance of the realm's SyntaxError with the
// frontend's message, before any of it runs; an exception eval code throws
// while it runs propagates to the caller unchanged. Each row is a `row(...)`
// check; the script completes with the number of rows, or minus the first
// failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function caught(source) { try { (0, eval)(source); return null; } catch (e) { return e; } }
row((function () { let r = 0; try { (0, eval)("1 +"); } catch (e) { r = 1; } return r; })() === 1);
row(caught("1 +") instanceof SyntaxError);
row((function () { const e = caught("1 +"); return e.name === "SyntaxError" && typeof e.message === "string" && e.message.length > 0; })());
// Nothing before the error runs: compilation fails first.
var ran = 0;
row(caught("ran = 1; )") instanceof SyntaxError && ran === 0);
row(caught("let z; let z;") instanceof SyntaxError);
row(caught("return 1") instanceof SyntaxError);
row(caught("'use strict'; with ({}) {}") instanceof SyntaxError);
// Run-time exceptions of eval code are the caller's to catch.
row(caught("throw 7") === 7);
row(caught("\nmissing") instanceof ReferenceError);
row(caught("null.x") instanceof TypeError);
failed === 0 ? rows : -failed;
