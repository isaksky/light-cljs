function numberFormatting() {
  // light-cljs-902: every cut and collection must keep thisNumberValue and
  // each argument conversion, without replaying a getter or callback.
  'use strict';
  let order = '';
  function count(tag, value) {
    return {get valueOf() {
      order += tag;
      return function () { let i = 0; while (i < 3) i++; order += 'v'; return value; };
    }};
  }
  const radix = Number.prototype.toString.call(new Number(255), count('r', 16));
  const fixed = (1.125).toFixed(count('f', 2));
  const exponential = Number.prototype.toExponential.apply(25, [count('e', 0)]);
  const precision = (2.5).toPrecision.bind(2.5)(count('p', 1));
  const special = Infinity.toExponential(count('i', 1000));
  let touched = false, badThis = false, thrown = false, absent = false;
  try { Number.prototype.toFixed.call('1', {valueOf() { touched = true; return 2; }}); }
  catch (e) { badThis = e instanceof TypeError; }
  try { (1).toPrecision({valueOf() {order += 't'; throw 42;}}); }
  catch (e) { thrown = e === 42; }
  try { (1).toString({valueOf() {order += 'u'; return undefined;}}); }
  catch (e) { absent = e instanceof RangeError; }
  return radix === 'ff' && fixed === '1.13' && exponential === '3e+1' && precision === '3'
    && special === 'Infinity' && !touched && badThis && thrown && absent && order === 'rvfvevpvivtu';
}
