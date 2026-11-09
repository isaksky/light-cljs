// A dependency wait inside newTarget's `prototype` getter after the
// Function constructor compiled (light-cljs-887): `dep` is a host effect
// the test answers with "x", in place or after the job's continuation was
// saved and restored in another heap (snapshot_tests.goose). The
// DynamicFunction frame waits at pc 1 with the compiled function in its
// output; the getter runs once and the function takes its prototype.
// Completes with "1:4:true".
function functionPrototypeWait(dep) {
  let reads = 0;
  const proto = Object.create(Function.prototype);
  const NT = (function () {}).bind(null);
  Object.defineProperty(NT, 'prototype', { get() { reads++; dep('need'); return proto; } });
  const f = Reflect.construct(Function, ['x', 'return x + 1'], NT);
  return reads + ':' + f(3) + ':' + (Object.getPrototypeOf(f) === proto);
}
