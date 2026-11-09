// bbp-authored conformance case.
// Derives from: rust/tests/js_source.rs lines 5885-5964 (`math_pow` and
// `math_sqrt_and_log2`, their conversion rows), 6402-6427
// (`math_coercion`) and the JavaScript rows of 6471-6560
// (`math_coercion_waits_yields_collects_and_migrates`, whose waits and
// collections are serial-test cuts on Goose,
// corpus/serial/math-sum-precise.js); rust/tests/math.rs lines 30-78
// (`remaining_math_members_and_coercions`, its conversion rows), the two
// conversion rows of 79-111 (`remaining_math_edges`) and the JavaScript rows
// of 133-193 (`remaining_math_waits_collects_and_migrates`); light-cljs-901's
// planner row (light-cljs-767; ECMAScript 7.1.4 ToNumber, 21.3.2, QuickJS's
// js_math_funcs). Every argument a function reads converts with ToNumber
// (ToUint32 for imul and clz32) in order, before the function computes,
// even after a NaN or an infinity; min, max and hypot read every argument,
// the others never one past their own. Left out: `__rt` rows and BigInt
// arguments (the BigInt area, light-cljs-771); the atan2, min and max row
// that passes 1n first passes a symbol, which throws there too. Each row is
// a `row(...)` check; the script completes with the number of rows, or
// minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function throwsType(f) { try { f(); } catch (e) { return e instanceof TypeError; } return false; }
function throws42(f) { try { f(); } catch (e) { return e === 42; } return false; }
// The planner's row (light-cljs-767).
{ let log = ''; row(Math.max({ valueOf() { log += 'a'; return 1; } }, { valueOf() { log += 'b'; return 2; } }) + log === '2ab'); }
// math_pow (js_source.rs line 5887).
{ let log = ''; function v(n) { return { valueOf() { log += n; return n; } }; } row(Math.pow(v(2), v(3)) === 8 && log === '23'); }
{ let log = ''; let a = { valueOf() { log += 'a'; return NaN; } }; let b = { valueOf() { log += 'b'; return 0; } }; row(Math.pow(a, b) === 1 && log === 'ab'); }
{ let log = ''; let a = { valueOf() { throw 42; } }; let b = { valueOf() { log += 'b'; return 0; } }; row(throws42(() => Math.pow(a, b)) && log === ''); }
row(throws42(() => Math.pow(NaN, { valueOf() { throw 42; } })));
{ let o = { valueOf() { throw 42; } }; row(Math.pow(2, 3, o) === 8); }
{ let o = { valueOf() { return {}; }, toString() { return '3'; } }; row(Math.pow(new Number(2), o) === 8); }
// math_sqrt_and_log2 (line 5926).
{ let log = ''; let o = { valueOf() { log += 'v'; return {}; }, toString() { log += 's'; return '16'; } }; row(Math.sqrt(o) === 4 && Math.log2(o) === 4 && log === 'vsvs'); }
{ let o = { valueOf() { throw 42; } }; row(Math.sqrt(4, o) === 2 && Math.log2(8, o) === 3); }
for (const name of ['sqrt', 'log2']) {
  row(throwsType(() => Math[name](Object.create(null))));
  row(throws42(() => Math[name]({ valueOf() { throw 42; } })));
}
// math_coercion (line 6402): ToNumber is ordered and does not stop at
// Infinity or NaN; unused operands are never converted.
{ let log = ''; function v(n) { return { valueOf() { log += n; return n; } }; } row(Math.imul(v(2), v(3)) === 6 && log === '23'); }
{ let log = ''; let o = { valueOf() { log += 'v'; return {}; }, toString() { log += 's'; return '-2.5'; } }; row(Math.floor(o) === -3 && Math.ceil(o) === -2 && Math.abs(o) === 2.5 && Math.fround(o) === -2.5 && log === 'vsvsvsvs'); }
{ let log = ''; let o = { valueOf() { log += 'v'; return 4; } }; row(Math.hypot(NaN, Infinity, o) === Infinity && log === 'v'); }
row(throws42(() => Math.hypot(Infinity, { valueOf() { throw 42; } })));
{ let log = ''; let o = { valueOf() { log += 'v'; return NaN; } }; let p = { valueOf() { log += 'p'; return Infinity; } }; row(Math.hypot(o, p) === Infinity && log === 'vp'); }
row(throws42(() => Math.imul({ valueOf() { throw 42; } }, { valueOf() { throw 43; } })));
{ let o = { valueOf() { throw 42; } }; row(Math.floor(2, o) === 2 && Math.imul(2, 3, o) === 6); }
row(Math.hypot({ valueOf: Math.abs.bind(null, -3) }, new Number(4)) === 5);
{ let log = ''; let o = { get valueOf() { log += 'g'; return function () { log += 'v'; return this.n; }; }, n: -2.5 }; row(Math.floor(o) === -3 && log === 'gv'); }
{ let log = ''; let o = { get valueOf() { log += 'v'; return 0; }, get toString() { log += 's'; return () => '4'; } }; row(Math.hypot(3, o) === 5 && log === 'vs'); }
row(throws42(() => Math.hypot(Infinity, { get valueOf() { throw 42; } })));
row(throwsType(() => Math.hypot(Infinity, Object.create(null))));
// math_coercion_waits_yields_collects_and_migrates (line 6474): its
// expressions and conversion orders, without the waits.
{
  let order = '';
  function v(n, id) { return { n, valueOf() { order += id + ','; return this.n; } }; }
  const cases = [
    [() => Math.floor(v(-2.5, 152)) === -3, '152,'],
    [() => Math.ceil.call(null, v(-2.5, 152)) === -2, '152,'],
    [() => Math.abs.apply(null, [v(-2.5, 152)]) === 2.5, '152,'],
    [() => Math.fround.bind(null, v(1.5, 152))() === 1.5, '152,'],
    [() => Math.sqrt(v(4, 152)) === 2, '152,'],
    [() => Math.log2.call(null, v(8, 152)) === 3, '152,'],
    [() => Math.imul(v(-2, 152), v(4, 153)) === -8, '152,153,'],
    [() => Math.pow(v(2, 152), v(3, 153)) === 8, '152,153,'],
    [() => Math.pow(NaN, v(0, 152)) === 1, '152,'],
    [() => Math.hypot(v(3, 152), v(4, 153)) === 5, '152,153,'],
    [() => Math.hypot(NaN, Infinity, v(4, 152)) === Infinity, '152,'],
    [() => Math.floor({ n: -2.5, get valueOf() { order += '152,'; return function () { return this.n; }; } }) === -3, '152,'],
    [() => Math.hypot(3, { get valueOf() { order += '152,'; return () => ({}); }, get toString() { order += '153,'; return () => '4'; } }) === 5, '152,153,'],
    [() => { let caught = false; try { Math.hypot(Infinity, { get valueOf() { order += '152,'; throw 42; } }); } catch (e) { caught = e === 42; } return caught && Math.floor(v(2.5, 153)) === 2; }, '152,153,'],
    [() => Math.hypot({ valueOf: Math.abs.bind(null, v(-3, 152)) }, v(4, 153)) === 5, '152,153,'],
    [() => { let caught = false; try { Math.hypot(Infinity, v(1, 152), { valueOf() { throw 42; } }); } catch (e) { caught = e === 42; } return caught && Math.floor(v(2.5, 153)) === 2; }, '152,153,'],
  ];
  for (const [f, expected] of cases) { order = ''; row(f() === true && order === expected); }
}
// remaining_math_members_and_coercions (math.rs line 31): each one-argument
// function's ToPrimitive (a valueOf getter, then toString) and its throw,
// a symbol and an object without methods.
const UNARY = ['round', 'trunc', 'sign', 'log', 'log10', 'log1p', 'exp', 'expm1', 'sin', 'cos', 'tan', 'asin', 'acos', 'atan', 'sinh', 'cosh', 'tanh', 'asinh', 'acosh', 'atanh', 'cbrt', 'clz32', 'f16round'];
for (const name of UNARY) {
  { let log = ''; let o = { get valueOf() { log += 'g'; return function () { log += 'v'; return {}; }; }, toString() { log += 's'; return '0.25'; } }; row(Object.is(Math[name](o), Math[name](0.25)) && log === 'gvs'); }
  { let n = 0; let o = { valueOf() { n++; throw 42; } }; let caught = false; try { Math[name](o); } catch (e) { caught = e === 42; } row(caught && n === 1 && Object.is(Math[name].call(null, 0.25, o), Math[name](0.25))); }
  row(throwsType(() => Math[name](Symbol())) && throwsType(() => Math[name](Object.create(null))));
}
{ let log = ''; let a = { valueOf() { log += 'a'; return NaN; } }, b = { valueOf() { log += 'b'; return 0; } }; row(Number.isNaN(Math.atan2(a, b)) && log === 'ab'); }
for (const name of ['atan2', 'min', 'max']) {
  let log = ''; function v(n) { return { valueOf() { log += n; return n; } }; }
  row(Object.is(Math[name](v(1), v(2)), Math[name](1, 2)) && log === '12');
  log = ''; let b = { valueOf() { log += 'b'; return 0; } };
  row(throwsType(() => Math[name](Symbol(), b)) && log === '');
}
// remaining_math_edges (math.rs line 80): min and max convert after a NaN.
{ let n = 0; let o = { valueOf() { n++; return 7; } }; row(Number.isNaN(Math.min(NaN, o)) && Number.isNaN(Math.max(NaN, o)) && n === 2); }
row(throws42(() => Math.min(NaN, { valueOf() { throw 42; } })));
// remaining_math_waits_collects_and_migrates (math.rs line 134): its
// expressions and conversion counts, without the waits.
{
  let n = 0;
  function v(x) { return { get valueOf() { n++; return () => x; } }; }
  const cases = [
    [() => Math.sin(v(0)) === 0, 1], [() => Math.f16round(v(1.5)) === 1.5, 1], [() => Math.clz32(v(1)) === 31, 1],
    [() => Math.atan2(v(0), v(1)) === 0, 2], [() => Number.isNaN(Math.min(NaN, v(1), v(2))), 2], [() => Number.isNaN(Math.max(NaN, v(1), v(2))), 2],
    [() => Math.min(-7, v(2)) === -7, 1], [() => Math.max(7, v(2)) === 7, 1], [() => Math.max(v(1), v(2)) === 2, 2],
  ];
  for (const [f, count] of cases) { n = 0; row(f() === true && n === count); }
}
failed === 0 ? rows : -failed;
