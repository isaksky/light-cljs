// Shared by light-cljs and the standard ClojureScript compiler's test output.
// Only the host harness supplies effects; the compiled language stays minimal.
function normalized(value) {
  if (Object.is(value, -0)) return { $number: "-0" };
  if (value === undefined) return { $value: "undefined" };
  return value;
}
if (!Array.isArray(globalThis.lightCases) || globalThis.lightCases.length === 0) {
  throw new Error("compiler produced no executable cases");
}
for (const program of globalThis.lightCases) {
  const trace = [];
  // Reset host state for every case, identically for both compilers.
  globalThis.lightHost = {
    value: 40,
    get method() {
      trace.push("method-get");
      return function(a, b) { trace.push(["call", a, b]); return this.value; };
    },
    receiver() { trace.push("receiver"); return this; },
    get path() { trace.push("path"); return this; },
  };
  const observe = (value) => { trace.push(value); return value; };
  const value = program(observe);
  console.log(JSON.stringify({ value: normalized(value), trace: trace.map(normalized) }));
}
