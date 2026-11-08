// A loop inside code an indirect eval compiled (light-cljs-856; decision 4):
// it runs in the job's dispatch loop, so serial_tests.goose stops and
// resumes it at every instruction and work limit, as the Pants engine's
// indirect_eval_waits_collects_and_migrates runs it under a small budget.
function evalLoop() {
  return (0, eval)("let n = 0; while (n < 25) n++; n");
}
