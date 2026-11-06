// A resumption past the call limits (light-cljs-790): with at most three
// frames, next() through one more function fails with a catchable stack
// overflow, and the generator, at its start or at a yield, stays suspended:
// a direct next() afterwards resumes it (serial_tests.goose sets the limit,
// as the Pants engine's generator_rejected_resume_preserves_suspension).
function generatorLimit() {
  function* g() { yield "7"; yield "8"; }
  function nest(it) { return it.next(); }
  var log = "";
  var started = g();
  try { nest(started); } catch (e) { log += e.name + ";"; }
  var r = started.next(); log += (r.done ? "D" : "Y") + r.value + ";";
  try { nest(started); } catch (e) { log += e.name + ";"; }
  r = started.next(); log += (r.done ? "D" : "Y") + r.value + ";";
  return log;
}
