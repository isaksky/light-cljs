// Synthetic fixture, not light-cljs output.
// A producer that cannot produce its own result without another one
// (tests/concurrency, cross-feature): it needs result kind 2 of definition
// `needs` and adds one. Two of these needing each other's results form a
// genuine active wait cycle. Host operation 1 is the pre-wait effect.
function produce_after(needs) {
  __rt.effect(1, needs);
  return __rt.need(needs, 2) + 1;
}
