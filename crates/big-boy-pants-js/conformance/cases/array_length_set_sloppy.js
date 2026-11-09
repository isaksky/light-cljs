// bbp-authored conformance case (light-cljs-893), run as a sloppy script.
// Derives from: none (light-cljs-893 review, finding 3): a refused array
// `length` assignment in sloppy code does nothing and throws nothing, for a
// primitive and for an object value converted first (ArraySetLength,
// ECMAScript 10.4.2.4); the expression still yields the value assigned. An
// invalid length is a RangeError in either mode. Each row is a `row(...)`
// check; the script completes with the number of rows, or minus the first
// failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// A non-configurable element stops the shortening without a TypeError.
{ let a = [1, 2, 3]; Object.defineProperty(a, 1, { value: 2, configurable: false }); let r = (a.length = '0'); row(r === '0' && a.length === 2 && !(2 in a) && a[1] === 2); }
{ let a = [1, 2, 3], v = { valueOf() { return 0; } }; Object.defineProperty(a, 1, { value: 2, configurable: false }); let r = (a.length = v); row(r === v && a.length === 2 && !(2 in a) && a[1] === 2); }
// A read-only length refuses silently.
{ let a = [1, 2]; Object.defineProperty(a, 'length', { writable: false }); let r = (a.length = '1'); row(r === '1' && a.length === 2); }
{ let a = [1, 2, 3], v = { valueOf() { Object.defineProperty(a, 'length', { writable: false }); return 1; } }; let r = (a.length = v); row(r === v && a.length === 3); }
// An invalid length is still a RangeError.
{ let a = [1, 2, 3]; row(name(() => { a.length = { valueOf() { return -1; } }; }) === "RangeError" && name(() => { a.length = 'x'; }) === "RangeError" && a.length === 3); }
failed === 0 ? rows : -failed;
