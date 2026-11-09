// bbp-authored conformance case.
// Derives from: rust/tests/js_source.rs lines 7043-7110
// (`number_predicates_and_constants`, the isNaN and isFinite rows; bbp-147),
// 12309-12404 (`parse_int_and_parse_float`; bbp-148), the JavaScript rows of
// 12407-12470 (`parse_numbers_coercion_waits_yields_collects_and_migrates`,
// whose waits and collections are serial-test cuts on Goose,
// corpus/serial/number-conversions.js) and 12473-12521
// (`parse_numbers_long_inputs_are_budgeted_and_rounded`), and the isNaN,
// isFinite, parseInt and parseFloat rows of 19830-19885
// (`global_object_descriptors`); light-cljs-900 (ECMAScript 19.2.2 to
// 19.2.5, QuickJS's js_global_isNaN, js_global_isFinite, js_parseInt and
// js_parseFloat). Each row is a `row(...)` check; the script completes with
// the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function throwsType(f) { try { f(); } catch (e) { return e instanceof TypeError; } return false; }
const same = Object.is;
// s repeated n times (String.prototype.repeat is light-cljs-903's).
function rep(s, n) { let r = ''; while (n > 0) { if (n & 1) r += s; s += s; n >>= 1; } return r; }
// isNaN and isFinite: ToNumber of their argument.
row(isNaN(NaN) && isNaN('abc') && !isNaN('12') && isNaN(undefined) && isNaN());
row(isFinite('1e3') && !isFinite(Infinity) && !isFinite(-Infinity) && !isFinite(NaN) && !isFinite());
row(!isNaN(null) && !isNaN(true) && isFinite(null) && isFinite(false) && isFinite(''));
row([isNaN('x'), isNaN(' 1 '), isFinite('1e308'), isFinite('1e309')].join() === 'true,false,true,false');
{ let log = ''; let o = {valueOf() { log += 'v'; return {}; }, toString() { log += 's'; return 'abc'; }}; row(isNaN(o) && !isFinite(o) && log === 'vsvs'); }
{ let o = {n: 7, valueOf() { return this.n; }}; row(!isNaN(o) && isFinite(o) && isFinite(new Number(2)) && isNaN(new String('abc'))); }
{ let caught = false; try { isNaN({valueOf() { throw 42; }}); } catch (e) { caught = e === 42; } row(caught); }
row(throwsType(() => isFinite({valueOf() { return {}; }, toString() { return {}; }})));
row(throwsType(() => isNaN(Symbol())) && throwsType(() => isFinite({valueOf() { return Symbol(); }})));
row(isNaN.name === 'isNaN' && isFinite.name === 'isFinite' && isNaN.length === 1 && isFinite.length === 1);
{ let isNaN = function () { return 7; }; let isFinite = 8; row(isNaN() === 7 && isFinite === 8); }
row(isNaN.call(null, 'abc') && isFinite.apply(null, ['7']) && Reflect.apply(isNaN, undefined, [{}]) && isFinite.bind(null, '1')());
// parseInt: ToString, then ToInt32 of the radix.
row(parseInt('123') === 123 && parseInt('  123r') === 123 && parseInt('0x123') === 0x123 && parseInt('0o123') === 0);
row(parseInt('ff', 16) === 255 && parseInt('0x10', 16) === 16 && parseInt('0x10', 10) === 0 && parseInt('0x10', 36) === 42804);
row(parseInt('-0x10') === -16 && parseInt('123', 0) === 123 && parseInt('11', 4294967298) === 3 && parseInt('11', -4294967294) === 3);
row(parseInt('10', 2.9) === 2 && parseInt('0b11') === 0 && parseInt('010') === 10 && parseInt('10', Infinity) === 10 && parseInt('10', null) === 10 && parseInt('10', '16') === 16);
row(parseInt('z!', 36) === 35 && parseInt(123.9) === 123 && parseInt('﻿  -12x') === -12 && parseInt('1\uD800') === 1);
row(parseInt('9007199254740993') === 9007199254740992 && parseInt('9007199254740995') === 9007199254740996);
row(parseInt('20000000000001', 16) === 9007199254740992 && parseInt('20000000000003', 16) === 9007199254740996);
row(same(parseInt('12', 37), NaN) && same(parseInt('12', 1), NaN) && same(parseInt(''), NaN) && same(parseInt(' 0x'), NaN) && same(parseInt('+ 1'), NaN));
row(same(parseInt(), NaN) && same(parseInt(null), NaN) && same(parseInt('  -0'), -0) && same(parseInt('-0'), -0) && 1 / parseInt('-0') === -Infinity);
row(parseInt('  0x1F') + ',' + parseInt('12px') + ',' + parseInt('z', 36) + ',' + parseInt('-0') + ',' + Object.is(parseInt('-0'), -0) + ',' + parseInt('0b11') === '31,12,35,0,true,0');
row(parseInt(rep('1', 300)) === 1.1111111111111112e299 && parseInt(rep('1', 400), 2) === 2.5822498780869086e120 && parseInt(rep('z', 300), 36) === Infinity);
// parseFloat: ToString, then the longest StrDecimalLiteral prefix.
row(parseFloat('2147483647') === 2147483647 && parseFloat('-2147483648') === -2147483648 && parseFloat('0x1234') === 0);
row(parseFloat('Infinity') === Infinity && parseFloat('-Infinity') === -Infinity && parseFloat('123.2') === 123.2 && parseFloat('123.2e3') === 123200);
row(parseFloat(' .5e1x') === 5 && parseFloat('Infinityx') === Infinity && parseFloat('+Infinityx') === Infinity && parseFloat('1e400') === Infinity);
row(parseFloat('1e+') === 1 && parseFloat('12.e-x') === 12 && parseFloat(123.9) === 123.9 && parseFloat(' 　 12.5x') === 12.5 && parseFloat('1\uD800') === 1);
row(same(parseFloat('e3'), NaN) && same(parseFloat('.'), NaN) && same(parseFloat('infinity'), NaN) && same(parseFloat(), NaN) && same(parseFloat(true), NaN));
row(same(parseFloat('-.0'), -0) && same(parseFloat('-0'), -0) && parseFloat('0.1') === 0.1 && parseFloat('5e-324') === 5e-324 && parseFloat('2.4703282292062328e-324') === 5e-324);
row(parseFloat('  .5e1x') + ',' + parseFloat('-Infinityx') + ',' + parseFloat('1e1000') + ',' + Object.is(parseFloat('-0'), -0) === '5,-Infinity,Infinity,true');
// Long inputs (the Pants engine budgets their scans).
row(parseInt(rep('0', 8192) + '1', 2) === 1 && parseInt(rep(' ', 8192) + 'ff', 16) === 255);
row(parseFloat('0.' + rep('0', 8192) + '1e8193') === 1 && parseFloat('1.' + rep('0', 8192) + '1') === 1);
// Conversions run user code, in order, once each.
row(parseInt({toString: () => 'ff', valueOf: () => 0}, 16) === 255);
{ let order = ''; let s = {toString: () => { order += 's'; return '11'; }}; let r = {valueOf: () => { order += 'r'; return 2; }}; row(parseInt(s, r) === 3 && order === 'sr'); }
{ let n = 0; try { parseInt({toString: () => { throw 42; }}, {valueOf: () => { n++; return 10; }}); } catch (e) { n += e; } row(n === 42); }
{ let n = 0; row(parseFloat('1', {valueOf: () => { n++; return 2; }}) === 1 && n === 0); }
{ let log = ''; let o = {valueOf() { log += 'v'; return 3; }}; row(isNaN(o) + log + parseInt({toString() { log += 's'; return '42'; }}) + log === 'falsev42vs'); }
{ let order = ''; let s = {toString: () => { order += 's'; return 'ff'; }}; let r = {valueOf: () => { order += 'r'; return 16; }}; row(Number.parseInt.call(null, s, r) === 255 && order === 'sr'); }
{ let order = ''; row(parseFloat({toString: () => { order += 'f'; return '12.5x'; }}) === 12.5 && order === 'f'); }
{ let order = ''; row(parseInt({toString: parseFloat.bind(null, {toString: () => { order += 'f'; return '12.5x'; }})}) === 12 && order === 'f'); }
{ let order = ''; let s = {toString: () => { order += 's'; return 'ff'; }}; let caught = 0; try { parseInt(s, {valueOf: () => { order += 'r'; throw 42; }}); } catch (e) { caught = e; } row(caught === 42 && order === 'sr'); }
// A symbol is a TypeError at its position: no later conversion runs.
{ let n = 0; row(throwsType(() => parseInt(Symbol(), {valueOf() { n++; return 10; }})) && n === 0); }
{ let n = 0; row(throwsType(() => parseInt({toString() { n++; return Symbol(); }}, {valueOf() { n += 10; return 10; }})) && n === 1); }
// Functions, not constructors; the descriptors of the global object.
row(parseInt.length === 2 && parseFloat.length === 1 && parseInt.name === 'parseInt' && parseFloat.name === 'parseFloat');
{ let parseInt = () => 7; row(parseInt('123') === 7); }
{ let n = 0; row(throwsType(() => new parseInt({toString() { n++; return '1'; }})) && throwsType(() => new parseFloat('1')) && throwsType(() => new isNaN(1)) && throwsType(() => new isFinite(1)) && n === 0); }
for (const name of ['isNaN', 'isFinite', 'parseInt', 'parseFloat']) {
  let d = Object.getOwnPropertyDescriptor(globalThis, name);
  row(d !== undefined && d.value === globalThis[name] && typeof d.value === 'function' && d.value.name === name && d.writable && !d.enumerable && d.configurable && Object.getPrototypeOf(d.value) === Function.prototype && !('prototype' in d.value));
}
{ let d = Object.getOwnPropertyDescriptor(globalThis, 'parseInt'); row([d.writable, d.enumerable, d.configurable].join() === 'true,false,true' && d.value.length === 2); }
failed === 0 ? rows : -failed;
