// Synthetic fixture, not light-cljs output.
// A producer for tests/need.rs: its result is a VM function compiled by this
// runtime (a closure over a number, so it passes the publication check),
// published as the callable result of a definition.
function producer(offset) {
  return function helper(x) {
    return x * 2 + offset;
  };
}
