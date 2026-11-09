// bbp-authored conformance case.
// Derives from: rust/tests/js_source.rs lines 6271-6321 (`math_sum_precise`,
// bbp-158; its first row adapts QuickJS's test_builtin.js test_math) and
// 6323-6342 (`math_sum_precise_iterator_protocol_errors_and_exhaustion`);
// rust/src/vm/native/sum_precise.rs lines 430-466 (its rounding unit tests);
// light-cljs-901's planner rows (light-cljs-767; ECMAScript 21.3.2.34
// Math.sumPrecise, QuickJS's js_math_sumPrecise). The sum is exact and
// rounds once; it is -0 when nothing but -0 was added, an infinity only for
// an infinite element or an overflowing exact sum, NaN for a NaN or both
// infinities. GetIterator reads `next` once; an element that is not a
// Number is a TypeError that closes the iterator, rethrown whatever
// `return` does; an exception from `next`, `done` or `value` closes
// nothing. Left out: the `[1n]` row (the BigInt area, light-cljs-771). Each
// row is a `row(...)` check; the script completes with the number of rows,
// or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function throwsType(f) { try { f(); } catch (e) { return e instanceof TypeError; } return false; }
// The planner's rows (light-cljs-767).
row(Math.sumPrecise([1e20, 0.1, -1e20]) + ',' + Math.sumPrecise([]) + ',' + Object.is(Math.sumPrecise([-0, -0]), -0) === '0.1,0,true');
row(Math.sumPrecise([1, Number.EPSILON / 2, Number.MIN_VALUE]) + ',' + Math.sumPrecise([Infinity, 1]) + ',' + Math.sumPrecise([1e308, 1e308, -1e308]) === '1.0000000000000002,Infinity,1e+308');
row(throwsType(() => Math.sumPrecise([1, '2'])) && throwsType(() => Math.sumPrecise(1)));
// math_sum_precise (js_source.rs line 6276).
row(Math.sumPrecise([1, Number.EPSILON / 2, Number.MIN_VALUE]) === 1.0000000000000002);
row(1 / Math.sumPrecise([]) === -Infinity && 1 / Math.sumPrecise([-0, -0]) === -Infinity);
row(1 / Math.sumPrecise([-0, 0]) === Infinity && 1 / Math.sumPrecise([1, -1]) === Infinity);
row(Math.sumPrecise([1e16, 1, -1e16]) === 1);
row(Math.sumPrecise([1e308, 1e308, -1e308]) === 1e308);
row(Math.sumPrecise([1e308, 1e308]) === Infinity && Math.sumPrecise([-1e308, -1e308]) === -Infinity);
row(Number.isNaN(Math.sumPrecise([Infinity, -Infinity])) && Number.isNaN(Math.sumPrecise([NaN])));
row(Math.sumPrecise([Infinity, 1]) === Infinity && Math.sumPrecise([-Infinity, 1]) === -Infinity);
row(Math.sumPrecise([0.1, 0.2]) === 0.30000000000000004);
row(Math.sumPrecise([Number.MIN_VALUE, Number.MIN_VALUE]) === 2 * Number.MIN_VALUE);
row(Math.sumPrecise([1, Number.EPSILON / 2]) === 1 && Math.sumPrecise([1, 3 * Number.EPSILON / 2]) === 1 + 2 * Number.EPSILON);
row(Math.sumPrecise([-1, -Number.EPSILON / 2, -Number.MIN_VALUE]) === -1.0000000000000002);
row(Math.sumPrecise.call(null, [1, 2]) === 3 && Math.sumPrecise.apply(null, [[2, 3]]) === 5 && Math.sumPrecise.bind(null, [3, 4])() === 7);
{ let d = Object.getOwnPropertyDescriptor(Math, 'sumPrecise'); row(d.writable && !d.enumerable && d.configurable && d.value.length === 1 && d.value.name === 'sumPrecise'); }
{ let n = 0, c = 0, o = { next() { n++; return { value: n === 1 ? 1 : '2' }; }, return() { c++; throw 99; } }; o[Symbol.iterator] = function () { return this; }; row(throwsType(() => Math.sumPrecise(o)) && n === 2 && c === 1); }
{ let c = 0, o = { next() { throw 42; }, return() { c++; return {}; } }; o[Symbol.iterator] = function () { return this; }; let ok = false; try { Math.sumPrecise(o); } catch (e) { ok = e === 42; } row(ok && c === 0); }
for (const argument of [() => [1, '2'], () => 1, () => null, () => undefined, () => [new Number(1)], () => [NaN, '2'], () => [Infinity, '2'], () => '12']) { row(throwsType(() => Math.sumPrecise(argument()))); }
row(throwsType(() => new Math.sumPrecise([])));
// math_sum_precise_iterator_protocol_errors_and_exhaustion (line 6324).
{ let c = 0, error = {}, o = { next() { return { get done() { throw error; } }; }, return() { c++; return {}; } }; o[Symbol.iterator] = function () { return this; }; let ok = false; try { Math.sumPrecise(o); } catch (e) { ok = e === error; } row(ok && c === 0); }
{ let c = 0, error = {}, o = { next() { return { get value() { throw error; } }; }, return() { c++; return {}; } }; o[Symbol.iterator] = function () { return this; }; let ok = false; try { Math.sumPrecise(o); } catch (e) { ok = e === error; } row(ok && c === 0); }
{ let c = 0, o = { next() { return 1; }, return() { c++; return {}; } }; o[Symbol.iterator] = function () { return this; }; row(throwsType(() => Math.sumPrecise(o)) && c === 0); }
{ let n = 0, o = { next() { n++; return { done: true, get value() { throw 99; } }; }, get return() { throw 98; } }; o[Symbol.iterator] = function () { return this; }; row(1 / Math.sumPrecise(o) === -Infinity && n === 1); }
{ let n = 0, o = { get next() { n++; return function () { Object.defineProperty(o, 'next', { value: function () { throw 99; }, configurable: true }); return { done: n++ > 3, value: 1 }; }; } }; o[Symbol.iterator] = function () { return this; }; row(Math.sumPrecise(o) === 3 && n === 5); }
// The exact sum's rounding (rust/src/vm/native/sum_precise.rs lines
// 430-466: `rounding_at_every_normal_exponent`, every fourth exponent, and
// `overflow_threshold_and_cancellation`): ties to even at every normal
// exponent, a carry into the exponent, the overflow threshold of both signs
// and the smallest normal less the smallest subnormal.
{
  let ok = true;
  for (let e = -1021; e <= 1023; e += 4) {
    const p = 2 ** e, half = p * Number.EPSILON / 2;
    for (const s of [-1, 1]) {
      if (Math.sumPrecise([s * p, s * half]) !== s * p || Math.sumPrecise([s * p, s * half, s * Number.MIN_VALUE]) !== s * (p + 2 * half) || Math.sumPrecise([s * p, -s * p, Number.MIN_VALUE]) !== Number.MIN_VALUE) { ok = false; }
    }
  }
  row(ok);
}
row(Math.sumPrecise([2 - Number.EPSILON, Number.EPSILON / 2]) === 2);
for (const s of [-1, 1]) {
  row(Math.sumPrecise([s * Number.MAX_VALUE, s * 2 ** 970]) === s * Infinity);
  row(Math.sumPrecise([s * Number.MAX_VALUE, s * 2 ** 970, -s * Number.MIN_VALUE]) === s * Number.MAX_VALUE);
  row(Math.sumPrecise([s * Number.MAX_VALUE, s * Number.MAX_VALUE, -s * Number.MAX_VALUE]) === s * Number.MAX_VALUE);
}
row(Math.sumPrecise([2 ** -1022, -Number.MIN_VALUE]) === 2 ** -1022 - Number.MIN_VALUE);
for (const cleanup of [function () { return {}; }, function () { return 1; }, function () { throw 99; }]) {
  let n = 0, c = 0, o = { next() { n++; return { value: 'bad' }; }, return() { c++; return cleanup(); } }; o[Symbol.iterator] = function () { return this; };
  row(throwsType(() => Math.sumPrecise(o)) && n === 1 && c === 1);
}
failed === 0 ? rows : -failed;
