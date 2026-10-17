"use strict";
const assert = require("node:assert/strict");
const fs = require("node:fs");
const load = path => (0, eval)(fs.readFileSync(path, "utf8"));
const runtimes = [load("/private/tmp/light-cljs-phase102/project/src/js_runtime.js"), load(__dirname + "/project/src/js_runtime.js")];
const cases = [
  (rt, b) => rt.vectorFromArray([1, rt.listFromArray([2, undefined])]),
  (rt, b) => rt.mapFromArray([rt.keyword(null, "ok"), rt.vectorFromArray([1, rt.core.atom(0)])]),
  (rt, b) => rt.mapFromArray([rt.core.atom(1), 2]),
  (rt, b) => rt.core.withMeta(rt.vectorFromArray([1]), rt.mapFromArray([rt.keyword(null, "bad"), () => 0])),
  (rt, b) => b.withMeta(b.jsArray([1, rt.symbol(null, "s")]), rt.mapFromArray(["m", 2])),
  (rt, b) => b.jsObject(["a.b", b.jsArray([undefined])]),
  (rt, b) => { const a = b.jsArray([]); a.push(a); return a; },
  (rt, b) => { const a = b.jsArray([1]); return rt.vectorFromArray([a, a]); },
  (rt, b, trace) => {
    const a = b.jsArray([0]);
    Object.defineProperty(a, 0, {get() { trace.push("get"); return 1; }});
    return rt.vectorFromArray([a]);
  },
  (rt, b, trace) => {
    const a = b.jsObject(["x", 0]);
    Object.defineProperty(a, "x", {get() { trace.push("get"); return 1; }});
    return a;
  },
  (rt, b, trace) => new rt.core.LazySeq(null, () => {
    trace.push("realized"); return rt.listFromArray([1, rt.vectorFromArray([() => 0])]);
  }, null, null),
  (rt, b, trace) => new rt.core.LazySeq(null, () => {
    trace.push("thrown"); throw new Error("thunk failure");
  }, null, null),
  (rt, b) => rt.queueFromArray([1, rt.vectorFromArray([2, undefined])]),
  (rt, b) => rt.setFromArray([1, rt.core.atom(2)]),
  (rt, b) => rt.vectorFromArray([new Date(0), /x/gi, new Date(NaN)]),
  (rt, b) => b.jsObject(["a", 1, "b", b.jsArray([2, 3])]),
  (rt, b) => rt.vectorFromArray([Symbol("host")]),
  (rt, b) => rt.vectorFromArray([1n]),
];
function run(rt, create, nodes, depth) {
  const b = rt.compilerFormBridge(), trace = [];
  const value = create(rt, b, trace);
  let result, terminal = null;
  try { result = ["value", b.encode(value, nodes, depth)]; }
  catch (e) { result = ["error", e.message]; }
  try { b.check(); } catch (e) { terminal = e.message; }
  return {result, terminal, trace};
}
let count = 0;
for (let i = 0; i < cases.length; i++) {
  for (const nodes of [0, 1, 2, 3, 4, 5, 8, 12, 50, 1000]) {
    for (const depth of [0, 1, 2, 3, 10]) {
      assert.deepEqual(run(runtimes[1], cases[i], nodes, depth), run(runtimes[0], cases[i], nodes, depth), `${i}: nodes=${nodes} depth=${depth}`);
      count++;
    }
  }
}
console.log(`${count} exact conversion/error-path/terminal-limit/getter/realization comparisons passed`);
