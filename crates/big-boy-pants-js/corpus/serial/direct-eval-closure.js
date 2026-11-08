// A direct eval in a reused worker (light-cljs-858): the image's eval site
// travels in the worker's snapshot, the eval code compiled against it joins
// the worker's code with a direct-eval site of its own, and a captured
// snapshot carries both, with the closure, to another program, where the
// closure's nested direct eval compiles and runs. snapshot_tests.goose runs
// it and checks that the worker's reset drops the code and its site.
function directEvalClosure() {
  let made = 0;
  var bump = eval("(function bump() { return eval('++made'); })");
  bump();
  return bump;
}
