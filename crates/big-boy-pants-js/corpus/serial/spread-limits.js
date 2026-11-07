// Spread-argument limits (light-cljs-850): collection stops with a RangeError
// after exactly 65,536 `next` calls, 65,535 arguments are accepted, and
// arguments that pass the value limit are refused before a built-in callee
// converts its first argument (serial_tests.goose runs it under a 128-value
// stack limit too).
function spreadLimits() {
  'use strict';
  var n = 0;
  var it = { next() { n++; return { value: n, done: n > 70000 }; } };
  it[Symbol.iterator] = function () { return this; };
  var cut = 'none';
  try { (function () {})(...it); } catch (e) { cut = e.name; }
  var accepted;
  try { accepted = (function () { return arguments.length; })(...new Array(65535)); } catch (e) { accepted = e.name; }
  var hook = 0;
  var args = [{ valueOf() { hook++; return 1; } }];
  for (var i = 1; i < 200; i++) args.push(i);
  var converted;
  try { converted = Number(...args); } catch (e) { converted = e.name; }
  return cut + ':' + n + ':' + accepted + ':' + converted + ':' + hook;
}
