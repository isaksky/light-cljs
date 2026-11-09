// bbp-authored conformance case (light-cljs-882), run as a sloppy script.
// Derives from: none (ECMAScript 10.4.4.2, a mapped arguments object's
// [[DefineOwnProperty]], as SetIntegrityLevel reaches it; light-cljs-860's
// mapped arguments objects; QuickJS is the oracle, since the Pants engine's
// `arguments` is always an unmapped copy). Freezing a mapped arguments
// object makes each index non-writable, which unmaps it: the parameter and
// the index go their own ways. Sealing keeps the mapping, and so does
// preventExtensions. Each row is a `row(...)` check; the script completes
// with the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
{ function f(a, b) { Object.freeze(arguments); a = 5; arguments[1] = 7; return arguments[0] + ':' + a + ':' + arguments[1] + ':' + b + ':' + Object.isFrozen(arguments); } row(f(1, 2) === "1:5:2:2:true"); }
{ function f(a, b) { Object.seal(arguments); a = 5; arguments[1] = 7; return arguments[0] + ':' + b + ':' + Object.isSealed(arguments) + ':' + (delete arguments[0]); } row(f(1, 2) === "5:7:true:false"); }
{ function f(a) { Object.preventExtensions(arguments); a = 3; arguments[2] = 1; return arguments[0] + ':' + arguments.length + ':' + (2 in arguments); } row(f(1) === "3:1:false"); }
// A mapped index already redefined (a shape key) is unmapped by freezing
// too, and stays mapped through sealing.
{ function f(a) { Object.defineProperty(arguments, 0, {enumerable: false}); Object.freeze(arguments); a = 9; return arguments[0] + ':' + a; } row(f(4) === "4:9"); }
{ function f(a) { Object.defineProperty(arguments, 0, {enumerable: false}); Object.seal(arguments); a = 9; return arguments[0] + ':' + a; } row(f(4) === "9:9"); }
// A frozen arguments object's closures see the parameter, not the index.
{ function f(a) { let get = () => a; Object.freeze(arguments); a = 2; return get() + ':' + arguments[0]; } row(f(1) === "2:1"); }
// Only the arguments passed are mapped; a missing one is an ordinary parameter.
{ function f(a, b) { Object.seal(arguments); b = 3; a = 4; return arguments.length + ':' + arguments[0] + ':' + (1 in arguments) + ':' + b; } row(f(1) === "1:4:false:3"); }
failed === 0 ? rows : -failed;
