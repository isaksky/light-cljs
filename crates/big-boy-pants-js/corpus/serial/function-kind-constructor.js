// The run-time generator, async and async generator constructors belong to
// the generators area (light-cljs-768): reaching one through a generator
// function's `constructor` fails by name on Goose (light-cljs-857, decision
// 1), UnsupportedOperand naming the pending `constructor` of
// %GeneratorFunction.prototype%, which the `catch` cannot swallow; it never
// reaches the Function constructor (serial_tests.goose).
function functionKindConstructor() {
  try { return (function* () {}).constructor("yield 1")().next().value; } catch (e) { return "caught"; }
}
