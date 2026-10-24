// Synthetic fixture, not light-cljs output.
// The default program of `cargo run` (src/main.rs): JavaScript source that
// this crate's frontend compiles and its VM executes; no qjsc, QuickJS or
// other C engine is involved. It exercises recursion, nested calls, an
// escaping closure over a task-owned transient builder, collection
// intrinsics and one host effect through the compiler-reserved binding
// `__rt` (docs/grammar.md). The script completes with 89.

function fib(n) {                       // recursion
  return n < 2 ? n : fib(n - 1) + fib(n - 2);
}

function makeCollector() {
  const items = __rt.vectorBuilder();   // task-owned, mutable in place
  return function (x) {                 // escapes, keeping `items` alive
    __rt.builderConj(items, x);
    return items;
  };
}

const collect = makeCollector();
let i = 0;
while (i < 10) {
  collect(fib(i));                      // nested calls
  i = i + 1;
}
const fibs = __rt.freeze(collect(fib(10)));   // persistent [0 1 1 2 ... 55]
__rt.effect(0, __rt.count(fibs));       // the host prints "effect 0: 11"
__rt.nth(fibs, 10) + __rt.nth(fibs, 9)  // 55 + 34
