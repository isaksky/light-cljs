// bbp-authored conformance case; light-cljs-902.
// Derives from: rust/tests/js_source.rs lines 6562-6606 (bbp-149), and
// rust/src/value/string.rs lines 2670-2755 (radix boundaries and round trips).
// Each row pins a Pants-engine assertion; additional rows cover conversion order
// and binary64 boundaries. Rounding rows also exercise QuickJS test_number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function throwsType(f) { try { f(); } catch (e) { return e instanceof TypeError; } return false; }
function throwsRange(f) { try { f(); } catch (e) { return e instanceof RangeError; } return false; }
row((1-2**-53).toString(12) === '0.bbbbbbbbbbbbbba');
row((1.3).toString(7) === '1.2046204620462046205');
row((1.3).toString(35) === '1.ahhhhhhhhhm');
row((1000000000000000128).toString() === '1000000000000000100');
row((10).toString(undefined) === '10' && (10).toString('10') === '10');
row((255).toString(16) === 'ff' && (-255).toString(2) === '-11111111');
row((0.5).toString(2) === '0.1' && (35).toString(36) === 'z');
row((new Number(31)).toString(16) === '1f' && Number.prototype.toString(2) === '0');
row((-0).toString(2) === '0' && NaN.toString(2) === 'NaN' && (-Infinity).toString(16) === '-Infinity');
row((10).toString(2.9) === '1010' && (10).toString({valueOf() {return 16;}}) === 'a');
row((function () { let log = ''; let radix = {valueOf() {log += 'v'; return {};}, toString() {log += 's'; return '2';}}; return  (10).toString(radix) === '1010' && log === 'vs'; })());
row((function () { let touched = false; try {Number.prototype.toString.call('1', {valueOf() {touched = true; return 2;}});} catch(e) {} return  !touched; })());
row((function () { let caught = false; try {(1).toString({valueOf() {throw 42;}});} catch(e) {caught = e === 42;} return  caught; })());
for (const radix of [1, 37, NaN, null, false, Infinity, -Infinity, 0, -2, {}, {valueOf() {return undefined;}}]) {
  row(throwsRange(() => (10).toString(radix)));
}
row(throwsType(() => Number.prototype.toString.call('1')));
row(throwsType(() => (1).toString(Symbol())));
row(throwsRange(() => NaN.toString(1)) && throwsRange(() => Infinity.toString(37)));
// Only an originally undefined argument defaults to decimal.
row(throwsRange(() => (10).toString({[Symbol.toPrimitive](hint) { return undefined; }})));
// Exact power-of-two radices, including both ends of binary64.
row((5e-324).toString(2) === '0.' + Array(1074).join('0') + '1');
row(Number.MIN_VALUE.toString(16) === '0.' + Array(269).join('0') + '4');
row(Number.MAX_VALUE.toString(2) === Array(54).join('1') + Array(972).join('0'));
row((1000000000000000128).toString(16) === 'de0b6b3a7640080');
// Odd-radix ties use the parity of the whole coefficient.
row((1.5).toString(3) === '1.' + Array(34).join('1'));
row((2.5).toString(3) === '2.' + Array(33).join('1') + '2');
row((-1.5).toString(3) === '-1.' + Array(34).join('1'));
row((2 ** -1022).toString(16) === '0.' + Array(256).join('0') + '4');
for (let radix = 2; radix <= 36; radix++) {
  for (const x of [-65535, -255, -1, 0, 1, 255, 65535]) row(parseInt(x.toString(radix), radix) === x);
}
for (let radix = 2; radix <= 36; radix++) {
  for (const x of [5e-324, 2 ** -1022, 0.1, 0.5, 1.3, 1 - 2 ** -53]) {
    const text = x.toString(radix);
    row(text.length <= 1077 && text[text.length - 1] !== '0');
  }
}
failed === 0 ? rows : -failed;
