// bbp-authored conformance case (light-cljs-889), a sloppy script.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 8675-8678
// (private_class_edge_cases's sloppy `with` row, bbp-291): a `with` object
// with a '#x' property does not shadow a private name, which no environment
// lookup resolves, with a direct eval inside the class body as well. Each row
// is a `row(...)` check; the script completes with the number of rows, or
// minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
{ let result; with ({'#x': 9}) { class C { #x=1; m() {return this.#x;} } result=new C().m(); } row(result === 1); }
{ let result; with ({'#x': 9, x: 3}) { class C { #x=1; m() {return eval('this.#x') + x;} } result=new C().m(); } row(result === 4); }
failed === 0 ? rows : -failed;
