// bbp-authored conformance case.
// Derives from: rust/tests/js_source.rs lines 7043-7110
// (`number_predicates_and_constants`, the Number rows; bbp-147) and the
// Number.parseInt and Number.parseFloat rows of 12309-12404 and 12473-12521
// (`parse_int_and_parse_float`, `parse_numbers_long_inputs_are_budgeted_and_rounded`;
// bbp-148); light-cljs-900 (ECMAScript 21.1.2, QuickJS's js_number_funcs,
// js_number_isInteger and js_number_isSafeInteger). Number's predicates
// convert nothing; its constants are neither writable, enumerable nor
// configurable. Each row is a `row(...)` check; the script completes with
// the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function throwsType(f) { try { f(); } catch (e) { return e instanceof TypeError; } return false; }
row(!Number.isNaN('abc') && Number.isNaN(NaN) && !Number.isNaN(undefined) && !Number.isNaN());
row(Number.isNaN(Number('+')) && Number.isNaN(Number('-')) && Number.isNaN(Number('\x00a')));
row(Number.isFinite(0) && Number.isFinite(-0) && Number.isFinite(Number.MAX_VALUE) && !Number.isFinite('1e3') && !Number.isFinite(null) && !Number.isFinite(undefined) && !Number.isFinite(Infinity) && !Number.isFinite(-Infinity) && !Number.isFinite(NaN) && !Number.isFinite());
row(Number.isInteger(5.0) && !Number.isInteger(5.5) && Number.isInteger(-0) && Number.isInteger(-5) && Number.isInteger(Number.MAX_VALUE) && !Number.isInteger(NaN) && !Number.isInteger(Infinity) && !Number.isInteger(-Infinity) && !Number.isInteger('5') && !Number.isInteger());
row(!Number.isSafeInteger(2 ** 53) && Number.isSafeInteger(2 ** 53 - 1) && Number.isSafeInteger(-(2 ** 53 - 1)) && !Number.isSafeInteger(-(2 ** 53)) && Number.isSafeInteger(-0) && !Number.isSafeInteger(5.5) && !Number.isSafeInteger(NaN) && !Number.isSafeInteger(Infinity) && !Number.isSafeInteger('5') && !Number.isSafeInteger());
row([Number.isNaN('x'), Number.isFinite('1'), Number.isInteger(5.0), Number.isInteger(5.5), Number.isSafeInteger(2 ** 53), Number.isSafeInteger(2 ** 53 - 1)].join() === 'false,false,true,false,false,true');
// The predicates convert nothing: no valueOf or toString runs.
{ let o = {valueOf() { throw 1; }, toString() { throw 2; }}; row(!Number.isNaN(o) && !Number.isFinite(o) && !Number.isInteger(o) && !Number.isSafeInteger(o) && !Number.isFinite(new Number(1)) && !Number.isNaN(new Number(NaN))); }
row(!Number.isNaN(Symbol()) && !Number.isInteger(Symbol.iterator));
row(Number.isNaN.name === 'isNaN' && Number.isFinite.name === 'isFinite' && Number.isInteger.name === 'isInteger' && Number.isSafeInteger.name === 'isSafeInteger');
row(Number.isNaN.length === 1 && Number.isFinite.length === 1 && Number.isInteger.length === 1 && Number.isSafeInteger.length === 1);
row(Number.isInteger.call(null, 7) && Number.isSafeInteger.bind(null)(7) && Number.isNaN.apply(null, [NaN]) && Reflect.apply(Number.isFinite, undefined, [1]));
row(throwsType(() => new Number.isInteger(1)) && throwsType(() => new Number.isNaN(1)));
// The constants.
row(Number.EPSILON === 2 ** -52 && Number.EPSILON / 2 + 1 === 1 && Number.MIN_VALUE === 5e-324 && Number.MIN_VALUE / 2 === 0 && Number.MAX_VALUE === 1.7976931348623157e308 && Number.MAX_VALUE * 2 === Infinity);
row(Number.MAX_SAFE_INTEGER === 9007199254740991 && Number.MIN_SAFE_INTEGER === -9007199254740991 && Number.NaN !== Number.NaN && Number.POSITIVE_INFINITY === Infinity && Number.NEGATIVE_INFINITY === -Infinity);
for (const name of ['EPSILON', 'MIN_VALUE', 'MAX_VALUE', 'MAX_SAFE_INTEGER', 'MIN_SAFE_INTEGER', 'POSITIVE_INFINITY', 'NEGATIVE_INFINITY', 'NaN']) {
  let d = Object.getOwnPropertyDescriptor(Number, name);
  row(!d.writable && !d.enumerable && !d.configurable && Object.is(d.value, Number[name]));
}
{ let d = Object.getOwnPropertyDescriptor(Number, 'EPSILON'); row([d.writable, d.enumerable, d.configurable, Number.MAX_SAFE_INTEGER, Number.EPSILON === 2 ** -52].join() === 'false,false,false,9007199254740991,true'); }
{ let caught = false; try { Number.EPSILON = 1; } catch (e) { caught = e instanceof TypeError; } row(caught && Number.EPSILON === 2 ** -52); }
row(throwsType(() => { delete Number.MAX_VALUE; }) && Number.MAX_VALUE === 1.7976931348623157e308);
// Number.parseInt and Number.parseFloat are the global functions.
row(Number.parseInt === parseInt && Number.parseFloat === parseFloat && Number.isNaN !== isNaN && Number.isFinite !== isFinite);
for (const name of ['parseInt', 'parseFloat']) {
  let d = Object.getOwnPropertyDescriptor(Number, name);
  row(d.value === globalThis[name] && d.writable && !d.enumerable && d.configurable && Object.getPrototypeOf(d.value) === Function.prototype);
}
for (const name of ['isNaN', 'isFinite', 'isInteger', 'isSafeInteger']) {
  let d = Object.getOwnPropertyDescriptor(Number, name);
  row(typeof d.value === 'function' && d.writable && !d.enumerable && d.configurable && !('prototype' in d.value));
}
row(Number.parseInt('0x1F') === 31 && Number.parseFloat('1.5e1x') === 15);
failed === 0 ? rows : -failed;
