// A loop inside direct eval code (light-cljs-858; decision 4): it runs in
// the job's dispatch loop on the caller's binding, so serial_tests.goose
// stops and resumes it at every instruction and work limit, as the Pants
// engine's strict_direct_eval runs it under a five-instruction budget.
function directEvalLoop() {
  let n = 0;
  eval("while (n < 25) n++");
  return n;
}
