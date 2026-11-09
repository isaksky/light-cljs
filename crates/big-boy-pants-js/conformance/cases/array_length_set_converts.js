// bbp-authored conformance case (light-cljs-893).
// Derives from: none (light-cljs-884 review, finding 2: ArraySetLength,
// ECMAScript 10.4.2.4, converts the new length before it validates it, for
// [[Set]] as for [[DefineOwnProperty]]). An array's `length` written by
// assignment (`a.length = v`, `a['length'] = v`, compound assignment) and by
// Object.assign, with a primitive value: a string, boolean or null is
// ToNumber'd; a fractional, negative, NaN or too large number is a
// catchable RangeError that leaves the array unchanged; a symbol is a
// TypeError; a read-only length refuses the write. Object values are
// array_length_set_to_primitive (the Pants engine does not convert them).
// Each row is a `row(...)` check; the script completes with the number of
// rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// Primitives through assignment.
{ let a = [1, 2, 3]; a.length = '2'; row(a.length === 2 && a[2] === undefined && a.join() === "1,2"); }
{ let a = [1, 2, 3]; a.length = ' 0x2 '; row(a.length === 2); }
{ let a = [1, 2, 3]; a.length = ''; row(a.length === 0); }
{ let a = [1, 2, 3]; a.length = true; row(a.length === 1 && a[0] === 1); }
{ let a = [1, 2, 3]; a.length = false; row(a.length === 0); }
{ let a = [1, 2, 3]; a.length = null; row(a.length === 0); }
{ let a = [1, 2, 3]; a.length = '5'; row(a.length === 5 && !(3 in a) && a[4] === undefined); }
{ let a = [1, 2, 3]; a.length = -0; row(a.length === 0 && Object.is(a.length, 0)); }
{ let a = [1, 2, 3]; a.length = '1e1'; row(a.length === 10); }
{ let a = [1, 2, 3]; a['length'] = '1'; row(a.length === 1); }
{ let a = [1, 2, 3]; let k = 'length'; a[k] = '2'; row(a.length === 2); }
// Invalid lengths: a RangeError, the array unchanged.
for (let v of [1.5, -1, '1.5', '-1', 'x', undefined, NaN, Infinity, 4294967296, '4294967296']) {
  let a = [1, 2, 3]; row(name(() => { a.length = v; }) === "RangeError" && a.length === 3 && a[2] === 3);
}
{ let a = [1, 2, 3], m = ''; try { a.length = '-1'; } catch (e) { m = e.message; } row(m === "invalid array length"); }
{ let a = [1, 2, 3]; row(name(() => { a.length = Symbol.iterator; }) === "TypeError" && a.length === 3); }
// The assignment expression's value is the value assigned, not the length.
{ let a = [1, 2, 3]; let r = (a.length = '1'); row(r === '1' && a.length === 1); }
// Compound assignment reads the length and writes the result.
{ let a = [1, 2, 3]; a.length += '0'; row(a.length === 30); }
{ let a = [1, 2, 3]; a.length -= true; row(a.length === 2); }
// A read-only length refuses the write.
{ let a = [1, 2]; Object.defineProperty(a, 'length', { writable: false }); row(name(() => { a.length = '1'; }) === "TypeError" && a.length === 2); }
{ let a = Object.freeze([1, 2]); row(name(() => { a.length = '0'; }) === "TypeError" && a.length === 2); }
// A non-configurable element stops the shortening: a TypeError in strict code.
{ let a = [1, 2, 3]; Object.defineProperty(a, 1, { value: 2, configurable: false }); row(name(() => { a.length = '0'; }) === "TypeError" && a.length === 2); }
// An array with another prototype converts the same way.
{ let a = [1, 2, 3]; Object.setPrototypeOf(a, null); a.length = '1'; row(a.length === 1); }
// Object.assign onto an array.
row(Object.assign([1, 2, 3], { length: '1' }).length === 1);
row(Object.assign([1, 2, 3], { length: true }).length === 1);
row(Object.assign([1, 2, 3], { length: null }).length === 0);
row(name(() => Object.assign([1, 2, 3], { length: -1 })) === "RangeError");
row(name(() => Object.assign([1, 2, 3], { length: 1.5 })) === "RangeError");
row(name(() => Object.assign([1, 2, 3], { length: 'x' })) === "RangeError");
{ let a = [1, 2, 3]; Object.assign(a, { length: '1', x: 5 }, { 3: 'd' }); row(a.length === 4 && a.x === 5 && a[3] === 'd' && a[1] === undefined); }
{ let a = [1, 2, 3]; row(name(() => Object.assign(a, { x: 1, length: -1, y: 2 })) === "RangeError" && a.x === 1 && !('y' in a) && a.length === 3); }
failed === 0 ? rows : -failed;
