// Synthetic fixture, not light-cljs output.
// The producer of definition 7's CALLABLE result in the end-to-end scenario
// (examples/e2e). It runs as its own VM task and returns a helper compiled by
// this runtime. The helper closes over `slot` (a parameter never reassigned,
// so it is copied and the closure passes the publication check) and descends
// `depth` levels through that slot of a form, returning the subtree it
// reaches unchanged. A negative slot is a producer failure: the task throws
// and fails (the `failure` variant), and the host publishes that failure.
function produce_select(slot) {
  if (slot < 0) {
    throw slot;
  }
  return function select(form, depth) {
    let node = form;
    let left = depth;
    while (left > 0) {
      node = __rt.nth(node, slot);
      left = left - 1;
    }
    return node;
  };
}
