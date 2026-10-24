// Synthetic fixture, not light-cljs output.
// Shaped like compiler output that lowers known collection operations and a
// host effect to the compiler-reserved intrinsic binding `__rt`
// (docs/grammar.md, "Reserved intrinsic binding"). Host operation 7 is
// expected to return its argument plus 1000. The script completes with 1342.
function sumVector(v) {
  let i = 0, total = 0;
  const n = __rt.count(v);
  while (i < n) {
    total = total + __rt.nth(v, i);
    i = i + 1;
  }
  return total;
}

function range(n) {
  const b = __rt.vectorBuilder();     // task-owned transient builder
  let i = 0;
  while (i < n) {
    __rt.builderConj(b, i);
    i = i + 1;
  }
  return __rt.freeze(b);              // persistent vector
}

const v = __rt.conj(range(5), 10);    // [0 1 2 3 4 10]
const m = __rt.assoc(__rt.map(1, 100, 2, 200), 3, 300);
const logged = __rt.effect(7, sumVector(v));   // the host sees 20

sumVector(v)                          // 20
  + __rt.get(m, 3)                    // 300
  + __rt.getOr(m, 4, 0)               // 0 (absent)
  + __rt.count(__rt.dissoc(m, 1))     // 2
  + logged                            // 1020
