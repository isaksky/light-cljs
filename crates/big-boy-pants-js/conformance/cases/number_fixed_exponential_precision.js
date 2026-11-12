// bbp-authored conformance case; light-cljs-902.
// Derives from: rust/tests/js_source.rs lines 6612-6762 (bbp-150 to bbp-151),
// number_to_fixed (bbp-150), and number_to_exponential_and_precision (bbp-151).
// Each row pins a Pants-engine assertion; additional rows cover conversion order
// and binary64 boundaries. Rounding rows also exercise QuickJS test_number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function throwsType(f) { try { f(); } catch (e) { return e instanceof TypeError; } return false; }
function throwsRange(f) { try { f(); } catch (e) { return e instanceof RangeError; } return false; }
row((1000000000000000128).toFixed(0) === '1000000000000000128');
row((1.125).toFixed(2) === '1.13' && (-1.125).toFixed(2) === '-1.13');
row((0.5).toFixed(0) === '1');
row((-0.5).toFixed(0) === '-1');
row((-1e-10).toFixed(0) === '-0');
row((0).toFixed(2) === '0.00' && (-0).toFixed(1) === '0.0');
row((1.005).toFixed(2) === '1.00' && (1.25).toFixed(1) === '1.3' && (-1.25).toFixed(1) === '-1.3');
row((2.5).toFixed(0) === '3' && (123.456).toFixed() === '123');
row((1e21).toFixed(2) === '1e+21' && (-1e21).toFixed(100) === '-1e+21');
row((0.000001).toFixed(7) === '0.0000010' && (2**69).toFixed(0) === '590295810358705651712');
row((0.1).toFixed(100) === '0.1000000000000000055511151231257827021181583404541015625000000000000000000000000000000000000000000000');
row(NaN.toFixed(2) === 'NaN' && Infinity.toFixed(2) === 'Infinity' && (-Infinity).toFixed() === '-Infinity');
row((new Number(1.125)).toFixed(2) === '1.13' && Number.prototype.toFixed(2) === '0.00');
row((12.5).toFixed(undefined) === '13' && (12.5).toFixed(NaN) === '13' && (12.5).toFixed(null) === '13');
row((1.25).toFixed('1') === '1.3' && (1.25).toFixed(1.9) === '1.3' && (1.25).toFixed(-0.9) === '1');
row(Number.prototype.toFixed.name === 'toFixed' && Number.prototype.toFixed.length === 1);
row((function () { let d = Object.getOwnPropertyDescriptor(Number.prototype, 'toFixed'); return  d.writable && !d.enumerable && d.configurable; })());
row((function () { let log = ''; let digits = {valueOf() {log += 'v'; return {};}, toString() {log += 's'; return '2';}}; return  (1.125).toFixed(digits) === '1.13' && log === 'vs'; })());
row((function () { let touched = false; let caught = false; try {Number.prototype.toFixed.call({}, {valueOf() {touched = true; return 2;}});} catch(e) {caught = e instanceof TypeError;} return  caught && !touched; })());
row((function () { let caught = false; try {Number.prototype.toFixed.call({});} catch(e) {caught = e instanceof TypeError;} return  caught; })());
row((function () { let caught = false; try {(1).toFixed({valueOf() {throw 42;}});} catch(e) {caught = e === 42;} return  caught; })());
row((25).toExponential(0) === '3e+1' && (-25).toExponential(0) === '-3e+1');
row((2.5).toPrecision(1) === '3' && (-2.5).toPrecision(1) === '-3' && (25).toPrecision(1) === '3e+1');
row((0).toExponential() === '0e+0' && (-0).toExponential(2) === '0.00e+0');
row((1.5).toExponential() === '1.5e+0' && (123456).toExponential(2) === '1.23e+5' && (1e-7).toExponential() === '1e-7');
row((0).toPrecision(3) === '0.00' && (-0).toPrecision(3) === '0.00' && (123.456).toPrecision(4) === '123.5');
row((0.000123).toPrecision(2) === '0.00012' && (1e-7).toPrecision(1) === '1e-7');
row((123456).toPrecision(2) === '1.2e+5' && (1e21).toPrecision(3) === '1.00e+21');
row((1.5).toPrecision() === '1.5' && (1e21).toPrecision(undefined) === '1e+21');
row((1000000000000000.25).toExponential() === '1.0000000000000002e+15');
row((9.99).toExponential(1) === '1.0e+1' && (9.99).toPrecision(2) === '10' && (99.9).toPrecision(2) === '1.0e+2');
row((0.000000999).toPrecision(1) === '0.000001' && (0.000000999).toPrecision(2) === '0.0000010');
row((5e-324).toExponential() === '5e-324' && (5e-324).toPrecision(3) === '4.94e-324');
row((1.005).toExponential(2) === '1.00e+0' && (1.005).toPrecision(3) === '1.00');
row((1.25).toExponential(1.9) === '1.3e+0' && (1.25).toPrecision('2') === '1.3');
row((1.5).toExponential(NaN) === '2e+0' && (1.5).toExponential(null) === '2e+0' && (1.5).toExponential(-0.9) === '2e+0');
row((new Number(1.25)).toExponential(1) === '1.3e+0' && (new Number(1.25)).toPrecision(2) === '1.3');
row(Number.prototype.toExponential(2) === '0.00e+0' && Number.prototype.toPrecision(2) === '0.0');
row((1.5).toExponential({valueOf() {return undefined;}}) === '2e+0');
row((1).toExponential(100.9).length === 105 && (1).toPrecision(100.9).length === 101);
row((1).toPrecision(true) === '1' && (1).toExponential(false) === '1e+0');
row((5e-324).toFixed(100) === '0.' + Array(101).join('0'));
for (const count of [101, -1, Infinity, -Infinity, {valueOf() {return 101;}}]) {
  for (const x of [1, NaN, Infinity]) row(throwsRange(() => x.toFixed(count)));
}
for (const method of ['toFixed', 'toExponential', 'toPrecision']) {
  for (const receiver of [{}, '1', true, null, undefined]) {
    let calls = 0;
    row(throwsType(() => Number.prototype[method].call(receiver, {valueOf() {calls++; return 2;}})) && calls === 0);
  }
  row(throwsType(() => (1)[method](Symbol())));
  let calls = 0;
  row((1.25)[method]({get valueOf() { calls++; return function() {calls++; return 2;};}}) === (1.25)[method](2) && calls === 2);
  row(throwsType(() => new Number.prototype[method](2)));
}
for (const method of ['toExponential', 'toPrecision']) {
  const invalid = method === 'toExponential' ? [-1, 101, Infinity, -Infinity] : [0, 101, Infinity, -Infinity, NaN, null, {valueOf() {return undefined;}}];
  for (const count of invalid) row(throwsRange(() => (1)[method](count)));
  for (const x of [NaN, Infinity, -Infinity]) {
    for (const count of [1000, -1, Infinity, {valueOf() {return 1000;}}]) row(x[method](count) === String(x));
    row(throwsType(() => x[method](Symbol())));
    let calls = 0, caught = false;
    try { x[method]({valueOf() {calls++; throw 42;}}); } catch (e) { caught = e === 42; }
    row(caught && calls === 1);
  }
  row(Number.prototype[method].name === method && Number.prototype[method].length === 1);
  const d = Object.getOwnPropertyDescriptor(Number.prototype, method);
  row(d.writable && !d.enumerable && d.configurable && !('prototype' in d.value));
  let log = '';
  (1.25)[method]({valueOf() {log += 'v'; return {};}, toString() {log += 's'; return '2';}});
  row(log === 'vs');
}
// The full exact coefficient matters at 100 digits, even for subnormals.
row((0.1).toExponential(100) === '1.000000000000000055511151231257827021181583404541015625' + Array(47).join('0') + 'e-1');
row((1.25 - 2 ** -52).toFixed(1) === '1.2' && (1.25 + 2 ** -52).toFixed(1) === '1.3');
row((1.25 - 2 ** -52).toPrecision(2) === '1.2' && (1.25 + 2 ** -52).toPrecision(2) === '1.3');
row((1.45).toFixed(1) + ',' + (8.345).toFixed(2) + ',' + (1.005).toFixed(2) + ',' + (5e-7).toFixed(7) + ',' + (123.456).toExponential() + ',' + (NaN).toPrecision(3) === '1.4,8.35,1.00,0.0000005,1.23456e+2,NaN');
row((9.999).toFixed(2) === '10.00' && (0.999).toFixed(2) === '1.00');
row((0.00999).toFixed(2) === '0.01' && (0.005).toFixed(2) === '0.01');
row((-5e-324).toFixed(100) === '-0.' + Array(101).join('0'));
row(Number.MAX_VALUE.toExponential() === '1.7976931348623157e+308' && (2 ** -1022).toExponential() === '2.2250738585072014e-308');
row(Number.MAX_VALUE.toPrecision(3) === '1.80e+308' && (2 ** -1022).toPrecision(3) === '2.23e-308');
row((1000000000000000.25).toExponential(16) === '1.0000000000000003e+15');
row((1).toExponential(100) === '1.' + Array(101).join('0') + 'e+0' && (1).toPrecision(100) === '1.' + Array(100).join('0'));
failed === 0 ? rows : -failed;
