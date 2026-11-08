// The Function constructor (light-cljs-857; decision 5): its arguments
// convert by ToString in a native frame, an object's toString running a loop
// above it, and the function it compiles runs a loop of its own, so
// serial_tests.goose stops and resumes the DynamicFunction frame and the new
// function at every instruction and work limit, as the Pants engine's
// function_constructor_waits_collects_and_migrates runs it under a small
// budget.
function functionConstructor() {
  let order = "";
  const p = { toString() { let i = 0; while (i < 5) i++; order += "p"; return "x"; } };
  const b = { toString() { order += "b"; return "let n = x; while (n < 25) n++; return n"; } };
  const f = Function(p, b);
  return order === "pb" && f.length === 1 && f.name === "anonymous" ? f(3) : -1;
}
