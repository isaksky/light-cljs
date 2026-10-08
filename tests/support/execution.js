// Shared by light-cljs and the standard ClojureScript compiler's test output.
// Only the host harness supplies effects; the compiled language stays minimal.
function normalized(value) {
  if (Object.is(value, -0)) return { $number: "-0" };
  if (value === undefined) return { $value: "undefined" };
  return value;
}
for (const program of globalThis.lightCases) {
  const trace = [];
  const observe = (value) => { trace.push(value); return value; };
  const value = program(observe);
  console.log(JSON.stringify({ value: normalized(value), trace: trace.map(normalized) }));
}
