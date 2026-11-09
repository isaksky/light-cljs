// bbp-authored conformance case (light-cljs-893).
// Derives from: none (light-cljs-884 review, finding 2: ArraySetLength,
// ECMAScript 10.4.2.4, converts an object length through ToPrimitive with
// hint number). An array's `length` written with an object value, by
// assignment and by Object.assign: [Symbol.toPrimitive], then valueOf, then
// toString; an invalid result is a RangeError, a thrown conversion
// propagates, the conversion runs before the new length applies. Left out:
// how many times the conversion runs (the specification and QuickJS
// convert twice), and whether a read-only length converts before it
// refuses (QuickJS converts, the specification's OrdinarySet does not).
// Primitive values are array_length_set_converts. Each row is a `row(...)`
// check; the script completes with the number of rows, or minus the first
// failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
{ let a = [1, 2, 3]; a.length = { valueOf() { return 1; } }; row(a.length === 1 && a.join() === "1"); }
{ let a = [1, 2, 3]; a.length = { valueOf() { return '2'; } }; row(a.length === 2); }
{ let a = [1, 2, 3]; a.length = { toString() { return '2'; } }; row(a.length === 2); }
{ let a = [1, 2, 3]; a.length = [2]; row(a.length === 2); }
{ let a = [1, 2, 3]; a['length'] = new Number(1); row(a.length === 1); }
{ let a = [1, 2, 3]; let k = 'length'; a[k] = new String('0'); row(a.length === 0); }
{ let a = [1, 2, 3], calls = 0, other = false; a.length = { [Symbol.toPrimitive](h) { calls++; if (h !== 'number') other = true; return 2; } }; row(a.length === 2 && calls > 0 && !other); }
{ let a = [1, 2, 3]; row(name(() => { a.length = {}; }) === "RangeError" && a.length === 3); }
{ let a = [1, 2, 3]; row(name(() => { a.length = { valueOf() { return 1.5; } }; }) === "RangeError" && a.length === 3); }
{ let a = [1, 2, 3]; row(name(() => { a.length = { valueOf() { throw new SyntaxError('v'); } }; }) === "SyntaxError" && a.length === 3); }
{ let a = [1, 2, 3]; row(name(() => { a.length = { valueOf() { return {}; }, toString() { return {}; } }; }) === "TypeError" && a.length === 3); }
// The conversion runs first; the new length applies to the array as it is then.
{ let a = [1, 2, 3]; a.length = { valueOf() { a.push(4, 5); return 4; } }; row(a.length === 4 && a[3] === 4 && a[4] === undefined); }
// The expression yields the object assigned, not its primitive.
{ let a = [1, 2, 3], v = { valueOf() { return 1; } }; let r = (a.length = v); row(r === v && a.length === 1); }
// A read-only length refuses the write.
{ let a = [1, 2]; Object.defineProperty(a, 'length', { writable: false }); row(name(() => { a.length = { valueOf() { return 1; } }; }) === "TypeError" && a.length === 2); }
// Object.assign onto an array.
row(Object.assign([1, 2, 3], { length: { valueOf() { return 2; } } }).length === 2);
row(Object.assign([1, 2, 3], { length: [1] }).length === 1);
{ let a = [1, 2, 3]; row(name(() => Object.assign(a, { length: { valueOf() { return -1; } } })) === "RangeError" && a.length === 3); }
{ let a = [1, 2, 3]; Object.assign(a, { length: { valueOf() { return 1; } }, x: 5 }, { 3: 'd' }); row(a.length === 4 && a.x === 5 && a[3] === 'd' && a[1] === undefined); }
{ let a = [1, 2, 3]; row(name(() => Object.assign(a, { x: 1, length: { valueOf() { throw new EvalError('e'); } }, y: 2 })) === "EvalError" && a.x === 1 && !('y' in a) && a.length === 3); }
failed === 0 ? rows : -failed;
