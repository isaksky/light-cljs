// bbp-authored conformance case.
// Derives from: rust/tests/js_source.rs lines 5885-5964 (`math_pow` and
// `math_sqrt_and_log2`, the rows that convert no object) and 6344-6400
// (`math_basic_functions`); rust/tests/math.rs lines 30-78
// (`remaining_math_members_and_coercions`, the descriptor, `new` and
// no-argument rows), 79-111 (`remaining_math_edges`, but for its two
// conversion rows, which math_conversions has), 112-132 (`math_constants`)
// and 194-227 (`math_random_is_task_local_and_deterministic`, its range and
// distinctness; QuickJS seeds its stream from the clock, so no value is
// pinned); light-cljs-901's planner rows (light-cljs-767; ECMAScript 21.3,
// QuickJS's js_math_funcs). Left out: `__rt` rows, BigInt arguments (the
// BigInt area, light-cljs-771), and the Linux-only libm bit pins of
// math_libm_pinned_bits; the transcendental functions are the platform's
// libm on both oracles, so their rows pin only exact cases. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function throwsType(f) { try { f(); } catch (e) { return e instanceof TypeError; } return false; }
// The planner's rows (light-cljs-767).
row([Math.max(), Math.min(), Math.max(1, NaN, 3), Object.is(Math.max(-0, 0), 0), Object.is(Math.min(0, -0), -0), Math.round(-2.5), Math.round(2.5), Object.is(Math.round(-0.4), -0)].join() === '-Infinity,Infinity,NaN,true,true,-2,3,true');
row([Math.sign(-3), Math.cbrt(27), Math.clz32(1), Math.trunc(-4.7), Math.expm1(0), Math.log1p(0), Math.atan2(1, 1) === Math.PI / 4, Math.hypot(3, 4)].join() === '-1,3,31,-4,0,0,true,5');
{ let r = Math.random(); row((typeof r) + ',' + (r >= 0 && r < 1) + ',' + (Object.getPrototypeOf(Math) === Object.prototype) + ',' + (Math.random() !== r) === 'number,true,true,true'); }
row(Object.prototype.toString.call(Math) + ',' + Math[Symbol.toStringTag] + ',' + Math.f16round(1.337) === '[object Math],Math,1.3369140625');
row(Object.getOwnPropertyNames(Math).length + ',' + Math.E + ',' + Math.LN2 + ',' + Math.SQRT1_2 === '45,2.718281828459045,0.6931471805599453,0.7071067811865476');
// math_pow (js_source.rs line 5887): SameValue against `**`.
row(Math.pow(2, 32) - 1 === 4294967295 && Object.is(Math.pow(2, 32), 2 ** 32));
row(Math.pow(2, -1) === 0.5 && Object.is(Math.pow(2, -1), 2 ** -1));
row(Math.pow(NaN, 0) === 1 && Object.is(Math.pow(NaN, 0), NaN ** 0));
row(Number.isNaN(Math.pow(1, Infinity)) && Object.is(Math.pow(1, Infinity), 1 ** Infinity));
row(Number.isNaN(Math.pow(-8, 1 / 3)) && Object.is(Math.pow(-8, 1 / 3), (-8) ** (1 / 3)));
row(Math.pow(-0, -1) === -Infinity && Object.is(Math.pow(-0, -1), (-0) ** -1));
row(1 / Math.pow(-0, 3) === -Infinity && Object.is(Math.pow(-0, 3), (-0) ** 3));
row(Number.isNaN(Math.pow(-1, -Infinity)) && Object.is(Math.pow(-1, -Infinity), (-1) ** -Infinity));
row(Number.isNaN(Math.pow(1, NaN)) && Object.is(Math.pow(1, NaN), 1 ** NaN));
row(Math.pow(NaN, -0) === 1 && Object.is(Math.pow(NaN, -0), NaN ** -0));
row(1 / Math.pow(-0, 2) === Infinity && Object.is(Math.pow(-0, 2), (-0) ** 2));
row(1 / Math.pow(-Infinity, -3) === -Infinity && Object.is(Math.pow(-Infinity, -3), (-Infinity) ** -3));
{ let d = Object.getOwnPropertyDescriptor(Math, 'pow'); row(d.value === Math.pow && d.writable && !d.enumerable && d.configurable && Math.pow.length === 2 && Math.pow.name === 'pow'); }
row(Number.isNaN(Math.pow()) && Number.isNaN(Math.pow(2)) && Number.isNaN(Math.pow(undefined, 2)));
row(Math.pow('2', '3') === 8 && Math.pow(null, false) === 1);
row(Math.pow.call(null, 2, 3) === 8 && Math.pow.apply(null, [2, 3]) === 8 && Math.pow.bind(null, 2)(3) === 8);
row(throwsType(() => new Math.pow(2, 3)));
// math_sqrt_and_log2 (line 5926).
row(Math.sqrt(4) === 2 && Math.sqrt(2) === 1.4142135623730951);
row(1 / Math.sqrt(-0) === -Infinity && 1 / Math.sqrt(0) === Infinity);
row(Number.isNaN(Math.sqrt(-1)) && Number.isNaN(Math.sqrt(-Infinity)) && Number.isNaN(Math.sqrt(NaN)));
row(Math.sqrt(Infinity) === Infinity && Math.sqrt(Number.MIN_VALUE) === 2 ** -537);
row(Math.log2(8) === 3 && Math.log2(2 ** -1074) === -1074);
row(Math.log2(10) === 3.321928094887362 && Math.ceil(2000 * Math.log2(10)) === 6644);
row(Math.log2(0) === -Infinity && Math.log2(-0) === -Infinity);
row(Number.isNaN(Math.log2(-1)) && Number.isNaN(Math.log2(-Infinity)) && Number.isNaN(Math.log2(NaN)));
row(Math.log2(Infinity) === Infinity && 1 / Math.log2(1) === Infinity);
row(Number.isNaN(Math.sqrt()) && Number.isNaN(Math.log2()) && Number.isNaN(Math.sqrt(undefined)));
row(Math.sqrt('9') === 3 && Math.sqrt(null) === 0 && Math.log2(true) === 0 && Math.log2(null) === -Infinity);
row(Math.sqrt.call(null, '16') === 4 && Math.log2.apply(null, [32]) === 5 && Math.sqrt.bind(null, 25)() === 5);
{ let ok = true; for (let e = -1074; e <= 1023; e++) { if (Math.log2(2 ** e) !== e) { ok = false; } } row(ok); }
// math_basic_functions (line 6344); the first rows adapt QuickJS's test_math.
row(Math.floor(1.4) === 1 && Math.ceil(1.4) === 2);
row(Math.imul(0x12345678, 123) === -1088058456);
row(Math.imul(0xB505, 0xB504) === 2147441940 && Math.imul(0xB505, 0xB505) === -2147479015);
row(Math.imul((-2) ** 31, (-2) ** 31) === 0 && Math.imul(2 ** 31 - 1, 2 ** 31 - 1) === 1);
row(Math.fround(0.1) === 0.10000000149011612);
row(Math.hypot() === 0 && Math.hypot(-2) === 2 && Math.hypot(3, 4) === 5);
row(Math.abs(Math.hypot(3, 4, 5) - 7.0710678118654755) <= 1e-15);
row(Math.floor(-0.5) === -1 && 1 / Math.floor(-0) === -Infinity && 1 / Math.ceil(-0.5) === -Infinity);
row(Math.abs(-2) === 2 && 1 / Math.abs(-0) === Infinity && Math.floor('2.5') === 2);
row(Math.imul(-1, 8) === -8 && Math.imul(3) === 0 && Math.imul(4294967297.9, '3') === 3);
row(Number.isNaN(Math.fround(NaN)) && Math.fround(2 ** 128) === Infinity && Math.fround(1.5) === 1.5);
row(Math.fround(1 + 2 ** -24) === 1 && Math.fround(1 + 3 * 2 ** -24) === 1 + 2 ** -22);
row(1 / Math.fround(-1e-100) === -Infinity && 1 / Math.fround(-0) === -Infinity);
row(Math.hypot(NaN, Infinity) === Infinity && Math.hypot(Infinity, NaN, -Infinity) === Infinity);
row(Number.isNaN(Math.hypot(NaN, 1)) && 1 / Math.hypot(-0) === Infinity);
row(Math.hypot(1e200, 1e200) < Infinity && Math.hypot(1e-200, 1e-200) > 0);
row(Math.hypot(3e200, 4e200) / 1e200 === 5 && Math.hypot(3e-200, 4e-200) / 1e-200 === 5);
row(Math.hypot(Number.MIN_VALUE) === Number.MIN_VALUE && Math.hypot(Number.MAX_VALUE) === Number.MAX_VALUE);
row(Number.isNaN(Math.floor()) && Number.isNaN(Math.ceil()) && Number.isNaN(Math.abs()) && Number.isNaN(Math.fround()) && Math.imul() === 0);
row(Math.floor(Infinity) === Infinity && Math.ceil(-Infinity) === -Infinity && Math.abs(-Infinity) === Infinity);
row(typeof Math === 'object' && Object.getPrototypeOf(Math) === Object.prototype && Object.keys(Math).length === 0);
row(Math.floor.call(null, '2.9') === 2 && Math.imul.apply(null, [-1, 8]) === -8 && Math.hypot.bind(null, 3)(4) === 5);
{ let Math = 7; row(Math === 7); }
for (const [name, length] of [['floor', 1], ['ceil', 1], ['abs', 1], ['imul', 2], ['fround', 1], ['hypot', 2]]) {
  let f = Math[name]; let d = Object.getOwnPropertyDescriptor(Math, name);
  row(d.value === f && d.writable && !d.enumerable && d.configurable && f.length === length && f.name === name);
  row(throwsType(() => new Math[name]()));
}
row(throwsType(() => Math()) && throwsType(() => new Math()));
// remaining_math_members_and_coercions (math.rs line 31): every other
// function's descriptor, name, length, prototype and [[Construct]], and its
// value without an argument.
const UNARY = ['round', 'trunc', 'sign', 'log', 'log10', 'log1p', 'exp', 'expm1', 'sin', 'cos', 'tan', 'asin', 'acos', 'atan', 'sinh', 'cosh', 'tanh', 'asinh', 'acosh', 'atanh', 'cbrt', 'clz32', 'f16round'];
for (const name of [...UNARY, 'atan2', 'min', 'max', 'random', 'sqrt', 'log2', 'pow', 'sumPrecise']) {
  let length = name === 'atan2' || name === 'min' || name === 'max' || name === 'pow' ? 2 : name === 'random' ? 0 : 1;
  let f = Math[name], d = Object.getOwnPropertyDescriptor(Math, name);
  row(typeof f === 'function' && d.value === f && d.writable && !d.enumerable && d.configurable && f.name === name && f.length === length && Object.getPrototypeOf(f) === Function.prototype && !('prototype' in f));
  row(throwsType(() => new Math[name]()));
}
for (const name of UNARY) { row(name === 'clz32' ? Math.clz32() === 32 : Number.isNaN(Math[name]())); }
row(Number.isNaN(Math.atan2()) && Number.isNaN(Math.atan2(1)) && Math.atan2('0', null) === 0);
// remaining_math_edges (math.rs line 80).
row(Math.min() === Infinity && Math.max() === -Infinity);
row(Object.is(Math.min(0, -0), -0) && Object.is(Math.min(-0, 0), -0) && Object.is(Math.max(0, -0), 0) && Object.is(Math.max(-0, 0), 0));
row(Math.min(3, 2, -7, Infinity) === -7 && Math.max(3, 2, -7, -Infinity) === 3 && Number.isNaN(Math.min(NaN, Infinity)) && Number.isNaN(Math.max(-Infinity, NaN)));
row(Math.round(0.5) === 1 && Object.is(Math.round(-0.5), -0) && Math.round(-1.5) === -1 && Math.round(1.5) === 2);
row(Object.is(Math.round(-0), -0) && Object.is(Math.round(-Number.MIN_VALUE), -0) && Math.round(0.49999999999999994) === 0 && Math.round(4503599627370495.5) === 4503599627370496);
row(Math.round(Infinity) === Infinity && Math.round(-Infinity) === -Infinity && Math.round(Number.MAX_VALUE) === Number.MAX_VALUE);
row(Object.is(Math.trunc(-0.9), -0) && Math.trunc(-1.9) === -1 && Math.trunc(Infinity) === Infinity);
row(Object.is(Math.sign(-0), -0) && Object.is(Math.sign(0), 0) && Math.sign(-Infinity) === -1 && Math.sign(Infinity) === 1 && Number.isNaN(Math.sign(NaN)));
row(Math.clz32(0) === 32 && Math.clz32(-1) === 0 && Math.clz32(1) === 31 && Math.clz32(4294967297.9) === 31 && Math.clz32(Infinity) === 32 && Math.clz32(NaN) === 32);
row(Math.f16round(1 + 2 ** -11) === 1 && Math.f16round(1 + 3 * 2 ** -11) === 1 + 2 ** -9 && Math.f16round(1 + 2 ** -11 + 2 ** -52) === 1 + 2 ** -10);
row(Math.f16round(65504) === 65504 && Math.f16round(65520) === Infinity && Math.f16round(-65520) === -Infinity && Math.f16round(2 ** -24) === 2 ** -24);
row(Object.is(Math.f16round(-(2 ** -25)), -0) && Math.f16round(2 ** -25) === 0 && Math.f16round(2 ** -25 + 2 ** -76) === 2 ** -24 && Object.is(Math.f16round(-0), -0));
row(Math.log(0) === -Infinity && Math.log(-0) === -Infinity && Number.isNaN(Math.log(-1)) && Math.log(Infinity) === Infinity);
row(Math.log10(0) === -Infinity && Number.isNaN(Math.log10(-1)) && Math.log10(Infinity) === Infinity);
row(Object.is(Math.log1p(-0), -0) && Math.log1p(-1) === -Infinity && Number.isNaN(Math.log1p(-2)) && Math.log1p(Infinity) === Infinity);
row(Math.exp(-Infinity) === 0 && Math.exp(Infinity) === Infinity && Math.expm1(-Infinity) === -1 && Object.is(Math.expm1(-0), -0));
row(Object.is(Math.sin(-0), -0) && Object.is(Math.tan(-0), -0) && Math.cos(-0) === 1 && Number.isNaN(Math.sin(Infinity)) && Number.isNaN(Math.cos(Infinity)) && Number.isNaN(Math.tan(Infinity)));
row(Object.is(Math.asin(-0), -0) && Object.is(Math.atan(-0), -0) && Number.isNaN(Math.asin(2)) && Number.isNaN(Math.acos(2)) && Math.acos(1) === 0 && Math.atan(Infinity) === Math.PI / 2);
row(Object.is(Math.atan2(-0, 0), -0) && Math.atan2(0, -0) === Math.PI && Math.atan2(-0, -0) === -Math.PI && Math.atan2(Infinity, Infinity) === Math.PI / 4 && Math.atan2(-Infinity, Infinity) === -Math.PI / 4);
row(Object.is(Math.sinh(-0), -0) && Object.is(Math.tanh(-0), -0) && Math.sinh(-Infinity) === -Infinity && Math.cosh(-Infinity) === Infinity && Math.cosh(-0) === 1 && Math.tanh(-Infinity) === -1);
row(Object.is(Math.asinh(-0), -0) && Math.asinh(-Infinity) === -Infinity && Math.acosh(1) === 0 && Number.isNaN(Math.acosh(0)) && Math.acosh(Infinity) === Infinity);
row(Object.is(Math.atanh(-0), -0) && Math.atanh(1) === Infinity && Math.atanh(-1) === -Infinity && Number.isNaN(Math.atanh(2)));
row(Object.is(Math.cbrt(-0), -0) && Math.cbrt(-Infinity) === -Infinity && Math.cbrt(Infinity) === Infinity);
// math_constants (math.rs line 113).
for (const [name, value] of [['E', 2.718281828459045], ['LN2', 0.6931471805599453], ['LN10', 2.302585092994046], ['LOG2E', 1.4426950408889634], ['LOG10E', 0.4342944819032518], ['PI', 3.141592653589793], ['SQRT1_2', 0.7071067811865476], ['SQRT2', 1.4142135623730951]]) {
  let d = Object.getOwnPropertyDescriptor(Math, name);
  row(d.value === value && !d.writable && !d.enumerable && !d.configurable);
}
{ let d = Object.getOwnPropertyDescriptor(Math, Symbol.toStringTag); row(d.value === 'Math' && !d.writable && !d.enumerable && d.configurable && Object.prototype.toString.call(Math) === '[object Math]' && Object.keys(Math).length === 0); }
// math_random_is_task_local_and_deterministic (math.rs line 195): sixteen
// distinct numbers in [0, 1), the argument never converted.
{
  let out = [];
  for (let i = 0; i < 16; i++) { let x = Math.random({ valueOf() { throw 42; } }); if (!(x >= 0 && x < 1)) { out = null; break; } out.push(x); }
  let distinct = true;
  if (out !== null) { for (let i = 0; i < out.length; i++) { for (let j = 0; j < i; j++) { if (out[i] === out[j]) { distinct = false; } } } }
  row(out !== null && out.length === 16 && distinct);
}
failed === 0 ? rows : -failed;
