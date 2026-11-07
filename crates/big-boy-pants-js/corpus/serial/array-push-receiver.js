// Array.prototype.push onto a receiver serial_tests.goose supplies
// (light-cljs-842) with `n` items (0, 1 or 2), called through
// Function.prototype.call so that a protected receiver (whose own reads a
// compiler job answers by demand) reaches push itself. Only Goose runs it
// that way: the tests give it arrays and array-likes in rows the job did not
// allocate, which push must refuse (NativeBoundary) before it changes them.
function pushOnto(a, n) { const push = Array.prototype.push; return n === 0 ? push.call(a) : n === 1 ? push.call(a, 7) : push.call(a, 7, 8); }
