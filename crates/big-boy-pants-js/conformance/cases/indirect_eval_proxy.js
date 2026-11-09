// bbp-authored conformance case (sloppy script).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 2147-2170,
// proxy_eval_is_indirect (bbp-282; light-cljs-856). A proxy of %eval%
// called is an indirect eval even at a direct-eval site, so eval code cannot
// see the caller's locals. On Goose the proxy's [[Call]] forwards to
// %eval% as any call does (light-cljs-891, proxy.goose). Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
row((function (eval) { let local = 42; return eval("typeof local"); })(new Proxy(globalThis.eval, {})) === "undefined");
row((function (eval) { let local = 42; return eval(...["typeof local"]); })(new Proxy(globalThis.eval, {})) === "undefined");
row((function () { let local = 42; with ({ eval: new Proxy(globalThis.eval, {}) }) { return eval("typeof local"); } })() === "undefined");
failed === 0 ? rows : -failed;
