// Dependency waits inside the Descriptor native frame (light-cljs-881):
// `dep` is a host effect the test answers with "D", in place or after the
// job's whole stack was saved as a continuation and restored in another
// heap (snapshot_tests.goose). Every phase waits once: a property getter of
// Object.defineProperties's properties object (pc 1) and a field getter of
// the descriptor it gives (pc 2), then Object.defineProperty's object key
// in its Convert frame (pc 0) and a field getter of its descriptor (pc 2):
// 4 waits. Completes with "aDvDkDwD:1:true".
function definePropertyWait(dep) {
  let log = "";
  const t = {};
  const props = { get a() { log += "a" + dep("930"); return { get value() { log += "v" + dep("931"); return 1; }, enumerable: true }; } };
  Object.defineProperties(t, props);
  const o = {};
  Object.defineProperty(o, { toString() { log += "k" + dep("932"); return "b"; } }, { get writable() { log += "w" + dep("933"); return true; } });
  return log + ":" + t.a + ":" + Object.getOwnPropertyDescriptor(o, "b").writable;
}
