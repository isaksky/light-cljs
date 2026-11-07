// Script declarations (light-cljs-824): a strict script's var, function, let
// and const bindings. var and function names are global properties, read
// and written through the object on every access; let and const are not;
// a function reads all of them, and a later redefinition through
// globalThis is seen. Completes with true.
var count = 1;
let step = 2;
const scale = 10;
function bump() { count = count + step; return count * scale; }
var first = bump();
globalThis.count = 100;
step = 5;
var second = bump();
var hidden = globalThis.step === undefined && !("scale" in globalThis);
var kept;
try { kept = typeof missing === "undefined" && missing; } catch (e) { kept = e.name === "ReferenceError"; }
first === 30 && second === 1050 && count === 105 && globalThis.bump === bump && hidden && kept;
