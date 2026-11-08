// bbp-authored conformance case (sloppy script).
// Derives from: rust/src/frontend/lower.rs lines 1355-1369 (a sloppy
// function holding a direct eval makes its variable environment a binding
// cell over a private null-prototype object) and light-cljs-855. Every name
// in such a function resolves through that environment first (IsCell, then
// the binding object's own entries), so a function whose eval never runs
// must still see its own and outer bindings, never Object.prototype's
// properties. Running eval itself is light-cljs-858's. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
var toString = 42, hasOwnProperty = 1, valueOf = 2;
row((function (go) { if (go) eval(""); return toString; })(false) === 42);
row((function (go) { var x = 3; if (go) eval(""); return hasOwnProperty + valueOf + x; })(false) === 6);
row((function (go) { var x = 3; function g() { return x; } if (go) eval(""); x = 4; return g() + typeof constructor; })(false) === "4function");
row((function (go) { if (go) eval(""); with ({ y: 1 }) { return y + typeof absentName; } })(false) === "1undefined");
row((function (go) { if (go) eval(""); var f = () => toString; return f(); })(false) === 42);
failed === 0 ? rows : -failed;
