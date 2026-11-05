// Synthetic fixture, not light-cljs output.
// Occupies a worker of the concurrency suite inside host operation 2 (GATE)
// until the test opens gate `gate`.
function block(gate) {
  __rt.effect(2, gate);
  return gate;
}
