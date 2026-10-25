// Synthetic fixture, not light-cljs output.
// The companion scenario of the end-to-end example (examples/e2e,
// `cargo run --example e2e -- companion`): one task that reaches the
// helper of definition 7 from inside a resumable native fold, with a live
// closure-captured local and two pending finally blocks. Written for this
// crate (no QuickJS text is adapted); the nested-function and finally
// ordering shapes follow the patterns of QuickJS tests/test_closure.js and
// tests/test_loop.js.
//
// `__rt.reduce` calls `step` (native to JavaScript); the first step's
// `__rt.need` finds the helper pending and suspends the whole task inside
// the fold, inside both try blocks; after the wake (on another worker) the
// fold continues with the same step, never repeating it. Each step calls
// `bump`, which reassigns the captured `visits` (one binding cell shared
// with `visited`), and the helper (a nested call: fold -> step -> select),
// collecting the subtree the helper selects from each node.
//
// Host operations: 7 = VISIT (once per fold step, before its need; the
// argument is `visits` after the bump), 8 = INNER (inner finally block,
// with `visits`), 9 = OUTER (outer finally block, with `tag`).
function companion(form, tag) {
  let visits = 0;
  const bump = function () {
    visits = visits + 1;
    return visits;
  };
  const visited = function () {
    return visits;
  };
  const step = function (picked, node) {
    __rt.effect(7, bump());
    const select = __rt.need(7, 2);
    return __rt.conj(picked, select(node, 1));
  };
  try {
    try {
      return __rt.reduce(step, __rt.vector(), __rt.vector(form, __rt.nth(form, 2)));
    } finally {
      __rt.effect(8, visited());
    }
  } finally {
    __rt.effect(9, tag);
  }
}
