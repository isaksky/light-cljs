// A dependency wait inside the Function constructor's conversions
// (light-cljs-857): `dep` is a host effect the test answers with "x", in
// place or after the job's continuation was saved and restored in another
// heap (snapshot_tests.goose). The DynamicFunction frame keeps its
// arguments, its conversion index and Reflect.construct's newTarget across
// the wait; the later argument converts once, after it. Completes with
// "pb:4:true".
function functionConstructorWait(dep) {
  let order = "";
  const p = { toString() { order += "p"; return dep("need"); } };
  const b = { toString() { order += "b"; return "return x + 1"; } };
  function NT() {}
  NT.prototype = { marker: 1 };
  const f = Reflect.construct(Function, [p, b], NT);
  return order + ":" + f(3) + ":" + (Object.getPrototypeOf(f) === NT.prototype);
}
