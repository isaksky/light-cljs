// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 18093-18168,
// methods_on_primitive_bases (bbp-73), its read and call rows without
// `__proto__` (pending on Goose, the objects area's), and
// crates/big-boy-pants-js/rust/tests/boolean_bigint_wrappers.rs lines 215-224,
// boolean_primitive_reads_allocate_no_wrappers (bbp-302), as Boolean rows:
// property reads and method calls on number, string and boolean primitives
// through their prototypes (light-cljs-833). Each row is a `row(...)` check;
// the script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function throws(f) { try { f(); } catch (e) { return e.name === 'TypeError'; } return false; }
row((19686109595169230000).toString() === '19686109595169230000');
row((1.5).toString() === '1.5' && (-0).toString() === '0' && (1e21).toString() === '1e+21');
row('abc'.toString() === 'abc' && ''.toString() === '' && `a${1}`.toString() === 'a1');
row((1).toString === Number.prototype.toString && (1).valueOf() === 1);
row((2.5).valueOf() + 1 === 3.5 && Object.is((-0).valueOf(), -0) && 'ab'.valueOf() === 'ab');
{ let u; row(throws(() => u.x) && throws(() => null.toString()) && throws(() => { (1).x = 2; })); }
{ let n = 255; row(n.toString() === '255' && (0 / 0).toString() + (-1 / 0).toString() === 'NaN-Infinity' && (123e-20).toString() === '1.23e-18'); }
row(1..toString() + 2 .toString() === '12' && (1).toString(10) + (2).toString(undefined) === '12');
{ let s = 'x'; row((7)['toString']() === '7' && s['valueOf']() + s.toString() === 'xx'); }
row(Number.prototype.toString() === '0' && String.prototype.toString() + String.prototype.valueOf() === '');
row((1).toString.name + (1).valueOf.name + 'a'.toString.name === 'toStringvalueOftoString');
row('a'.toString === String.prototype.toString && 'a'.valueOf === String.prototype.valueOf);
row((1).toString !== 'a'.toString && (1).valueOf !== Object.prototype.valueOf && (1).hasOwnProperty === Object.prototype.hasOwnProperty);
row((1).constructor === Number && 'a'.constructor === String && true.constructor === Boolean);
row((1).x === undefined && 'abc'.x === undefined && 'abc'[3] === undefined && 'abc'[-1] === undefined && 'abc'['1.5'] === undefined);
row('abc'[1] === 'b' && 'abc'.length === 3 && 'abc'['length'] === 3 && ''.length === 0);
row(String.prototype.length === 0 && 'toString' in Number.prototype && !('length' in Number.prototype));
{ let f = (1).toString; row(f === (2).toString && f === Number.prototype.toString); }
{ let f = (1).toString; row(throws(() => f()) && throws(() => ({ m: (1).valueOf }).m()) && throws(() => ({ m: 'a'.toString }).m()) && throws(() => ({ m: 'a'.valueOf }).m())); }
// Boolean rows.
row(true.toString() === 'true' && false.toString() === 'false' && true.valueOf() === true && false.valueOf() === false);
row(true.toString === Boolean.prototype.toString && false.valueOf === Boolean.prototype.valueOf && true.x === undefined);
{ let b = false; let ok = true; for (let i = 0; i < 100; i++) { if (b.valueOf() !== false || b.toString() !== 'false') ok = false; } row(ok); }
{ let f = true.toString; row(throws(() => f()) && throws(() => ({ m: false.valueOf }).m())); }
// Symbol-keyed reads and computed keys on primitive bases.
row((1)[Symbol.toPrimitive] === undefined && true[Symbol.iterator] === undefined);
{ let k = { toString() { return 'toString'; } }; row((3)[k]() === '3' && 'q'[k]() === 'q'); }
// Methods on a primitive see it as their receiver, without a wrapper.
{ let r = 0; for (let i = 0; i < 200; i++) { r = r + i.valueOf(); if (('' + i).toString() === i.toString()) { r = r + 1; } } row(r === 19900 + 200); }
failed === 0 ? rows : -failed;
