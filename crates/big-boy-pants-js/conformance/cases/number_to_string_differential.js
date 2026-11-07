// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/src/value/string.rs lines 2801-2826,
// number_to_string_round_trips_with_shortest_length (bbp-66, Number::toString
// in radix 10). A differential slice:
// it converts about 2,300 doubles it generates itself and completes with
// their ToString, comma-separated, which the harness compares with QuickJS
// and the Pants engine. The doubles: repeated multiplication and division,
// powers of ten up past the largest double and down through the subnormals,
// neighbours of the 1e-6 and 1e21 layout boundaries, subnormals, and
// pseudo-random significands (a Park-Miller sequence, exact in doubles) at
// scales from 1e-30 to 1e30. The text is joined pairwise (a binary counter),
// so each unit is copied about log2(count) times.
var parts = [];
function emit(x) {
  var s = x + ",";
  var i = 0;
  while (parts[i] !== undefined) { s = parts[i] + s; parts[i] = undefined; i++; }
  parts[i] = s;
}
function joined() {
  var out = "";
  for (var i = 0; i < parts.length; i++) { if (parts[i] !== undefined) out = parts[i] + out; }
  return out;
}
var x, i;
// Repeated multiplication and division.
x = 1; for (i = 0; i < 300; i++) { x = x * 1.7; emit(x); }
x = 1; for (i = 0; i < 300; i++) { x = x / 1.7; emit(x); }
x = 1; for (i = 0; i < 100; i++) { x = x * -3.3; emit(x); }
// Powers of ten, inexact as they accumulate, past the largest double and
// into and below the subnormals.
x = 1; for (i = 0; i < 312; i++) { x = x * 10; emit(x); }
x = 1; for (i = 0; i < 330; i++) { x = x / 10; emit(x); }
// The layout boundaries: steps of about one unit in the last place around
// 1e21 and 1e-6, and the decimal neighbours of 1e-7 and 1e20.
x = 1e21; for (i = 0; i < 30; i++) { x = x - 131072; emit(x); }
x = 1e21; for (i = 0; i < 30; i++) { x = x + 131072; emit(x); }
x = 1e-6; for (i = 0; i < 30; i++) { x = x - 2.117582368135751e-22; emit(x); }
x = 1e-6; for (i = 0; i < 30; i++) { x = x + 2.117582368135751e-22; emit(x); }
x = 1e-7; for (i = 1; i < 20; i++) { emit(x * (1 + i / 1e15)); emit(x * (1 - i / 1e15)); }
x = 1e20; for (i = 1; i < 20; i++) { emit(x * (1 + i / 1e15)); emit(x * (1 - i / 1e15)); }
// Subnormals.
x = 5e-324; for (i = 0; i < 70; i++) { x = x * 3; emit(x); }
x = 2.2250738585072014e-308; for (i = 0; i < 70; i++) { x = x / 2.3; emit(x); }
// Pseudo-random significands at scales from 1e-30 to 1e30.
var scales = [];
x = 1e-30; for (i = 0; i < 61; i++) { scales[i] = x; x = x * 10; }
var seed = 42;
for (i = 0; i < 700; i++) {
  seed = (seed * 48271) % 2147483647;
  emit(seed / 2147483647 * scales[i % 61]);
}
joined();
