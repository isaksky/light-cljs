// Dependency waits inside built-in iteration (light-cljs-844; the Pants
// engine's for_of_builtin_iterator_getters_survive_waits and
// for_of_builtin_cursor_survives_body_wait): `dep` is a host effect the test
// answers with "D", in place or after the job's whole stack was saved as a
// continuation and restored in another heap (snapshot_tests.goose). It waits
// in an element getter, above %ArrayIteratorPrototype%.next's native frame,
// in the body of a loop over an array and in the body of a loop over a
// string; each cursor resumes at the index where it waited. Completes with
// "agDbc1wD23xsDy".
function builtinIteratorWait(dep) {
  let log = "";
  const like = { length: 3, 0: "a", get 1() { log += "g" + dep("getter"); return "b"; }, 2: "c" };
  for (const x of Array.prototype.values.call(like)) log += x;
  for (const x of [1, 2, 3]) { if (x === 2) log += "w" + dep("body"); log += x; }
  for (const c of "xy") { log += c; if (c === "x") log += "s" + dep("string"); }
  return log;
}
