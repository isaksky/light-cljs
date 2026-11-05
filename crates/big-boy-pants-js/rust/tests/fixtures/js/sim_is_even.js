// Synthetic fixture, not light-cljs output.
// One half of a mutually recursive pair, shaped like compiler output for a
// definition that refers to `isOdd`. Calls are not executable yet, so the
// simulated host's stand-in convention applies: returning a number asks the
// host to tail-call the declared callee (`isOdd`) with it; returning a
// boolean is the result. Mutual recursion as in QuickJS
// tests/test_closure.js `test_closure2` (reference only, not adapted).
function isEven(n) {
  if (n === 0) {
    return true;
  }
  return n - 1;
}
