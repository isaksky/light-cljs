// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/src/vm/dispatch.rs lines 1050-1052
// and rust/src/vm/native/construct.rs (built-in constructors; Error
// constructs an error, clj-quickjs-228) and ECMAScript NativeError(message,
// options) with new.target the constructor: `new Error(m)` and each native
// error type give a new error whose prototype is its type's prototype, whose
// own `message` is ToString(m) (absent when m is undefined), whose `name` is
// inherited, and whose `cause` is own when options has one; with or without
// `new` the result is the same. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function own(o, k) { return Object.getOwnPropertyDescriptor(o, k) !== undefined; }
row(new Error("m").message === "m");
row(new TypeError("t").name === "TypeError");
row(new Error("m").name === "Error");
row(Object.getPrototypeOf(new Error("m")) === Error.prototype);
row(Object.getPrototypeOf(new TypeError("t")) === TypeError.prototype);
row(Object.getPrototypeOf(TypeError.prototype) === Error.prototype);
row(new RangeError("r").name === "RangeError" && Object.getPrototypeOf(new RangeError()) === RangeError.prototype);
row(new ReferenceError("r").name === "ReferenceError" && Object.getPrototypeOf(new ReferenceError()) === ReferenceError.prototype);
row(new SyntaxError("s").name === "SyntaxError" && Object.getPrototypeOf(new SyntaxError()) === SyntaxError.prototype);
row(new EvalError("e").name === "EvalError" && Object.getPrototypeOf(new EvalError()) === EvalError.prototype);
row(new URIError("u").name === "URIError" && Object.getPrototypeOf(new URIError()) === URIError.prototype);
row(new Error("m").constructor === Error && new TypeError().constructor === TypeError);
row(typeof new Error() === "object");
row(new Error("a") !== new Error("a"));
// message: own, ToString of the argument; absent for undefined.
row(own(new Error("m"), "message") && !own(new Error(), "message") && new Error().message === "");
row(!own(new Error(undefined), "message"));
row(new Error(42).message === "42" && new Error(null).message === "null" && new Error(true).message === "true");
row(!own(new Error("m"), "name"));
{ const d = Object.getOwnPropertyDescriptor(new Error("m"), "message"); row(d.writable === true && d.enumerable === false && d.configurable === true); }
// cause from options.
{ const c = {}; const e = new Error("m", {cause: c}); row(e.cause === c && own(e, "cause")); }
row(!own(new Error("m", {}), "cause") && !own(new Error("m", 1), "cause"));
{ const e = new TypeError("t", {cause: undefined}); row(own(e, "cause") && e.cause === undefined); }
// toString through the prototype.
row("" + new TypeError("t") === "TypeError: t" && new Error().toString() === "Error");
// Thrown and caught.
{ let r; try { throw new RangeError("out"); } catch (e) { r = e; } row(r.name === "RangeError" && r.message === "out" && Object.getPrototypeOf(r) === RangeError.prototype); }
// Called and constructed agree.
row(Object.getPrototypeOf(Error("m")) === Error.prototype && Error("m").message === "m" && TypeError("t").name === "TypeError");
// The prototype's own name and message.
row(Error.prototype.name === "Error" && Error.prototype.message === "" && TypeError.prototype.message === "");
failed === 0 ? rows : -failed;
