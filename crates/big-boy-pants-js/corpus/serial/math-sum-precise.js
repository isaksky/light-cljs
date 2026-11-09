function mathSumPrecise() {
  // Math.sumPrecise's SumPrecise native frame (light-cljs-901) over a user
  // iterator whose @@iterator, `next`, `done` and `value` run a loop in a
  // frame above it, so serial_tests.goose stops and resumes it at every
  // budget cut, and collects inside the job at every cut while the frame
  // holds the iterator record, the result it reads and its exact sum. A
  // string element is a TypeError that closes the iterator (its `return`
  // runs); an array sums through the built-in iterator; and Math's
  // conversions run in Coerce frames (light-cljs-900).
  "use strict";
  let log = "";
  const spin = (tag) => { let i = 0; while (i < 3) i++; log += tag; };
  function iterable(items) {
    let i = 0;
    return {
      [Symbol.iterator]() {
        spin("i");
        return {
          next() { spin("n"); const k = i++; return { get done() { spin("d"); return k >= items.length; }, get value() { spin("v"); return items[k]; } }; },
          return() { spin("r"); return {}; },
        };
      },
    };
  }
  const a = Math.sumPrecise(iterable([1e20, 0.1, -1e20, -0]));
  let closed = false;
  try { Math.sumPrecise(iterable([1, "2", 3])); } catch (e) { closed = e instanceof TypeError; }
  const b = Math.sumPrecise([1, Number.EPSILON / 2, Number.MIN_VALUE]);
  const z = Math.sumPrecise(iterable([-0, -0]));
  const c = Math.max({ valueOf() { spin("x"); return 2; } }, 1, { valueOf() { spin("y"); return 3; } });
  const d = Math.hypot({ valueOf() { spin("h"); return 3; } }, 4);
  return a === 0.1 && closed && b === 1.0000000000000002 && Object.is(z, -0) && c === 3 && d === 5
    && log === "indvndvndvndvndindvndvrindvndvndxyh";
}
