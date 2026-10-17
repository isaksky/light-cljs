"use strict";
const assert = require("node:assert/strict");
const fs = require("node:fs");
const load = path => (0, eval)(fs.readFileSync(path, "utf8"));
const runtimes = [load("/private/tmp/light-cljs-phase102/project/src/js_runtime.js"), load(__dirname + "/project/src/js_runtime.js")];
let id = 1, seed = 731;
const random = n => ((seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) % n);
const atom = () => {
  switch (random(6)) {
    case 0: return ["keyword", id++, null, "shared", String(random(4))];
    case 1: return ["symbol", id++, null, null, "s" + random(4)];
    case 2: return ["number", id++, null, random(6) - 3];
    case 3: return ["string", id++, null, "s" + random(4)];
    case 4: return ["bool", id++, null, !!random(2)];
    default: return ["nil", id++, null];
  }
};
function form(depth) {
  if (!depth || random(3) === 0) return atom();
  const tags = ["list", "vector", "map", "set"], tag = tags[random(tags.length)];
  const children = Array.from({length: random(4) * (tag === "map" ? 2 : 1)}, () => form(depth - 1));
  const metadata = random(2) ? ["map", id++, null, [atom(), atom()]] : null;
  return [tag, id++, metadata, children];
}
function exercise(runtime, descriptor) {
  const bridge = runtime.compilerFormBridge();
  const value = bridge.decode(descriptor, null, true);
  // Model an environment import interleaved before positional argument import.
  bridge.decode(["keyword", 999999, ["map", 999998, null, [["string", 999997, null, "env"], ["number", 999996, null, 7]]], "shared", "0"]);
  const [args, snapshots] = bridge.macroArguments(value, 100000, 1000);
  args.forEach((arg, i) => assert.equal(arg, runtime.core.nth(value, i + 1)));
  assert.throws(() => bridge.macroArguments(value, 100000, 1000), /Missing compiler macro input/);
  return [snapshots, args.map(arg => bridge.encode(arg, 100000, 1000)), bridge.encode(value, 100000, 1000)];
}
for (let i = 0; i < 500; i++) {
  const shared = form(3), nested = ["vector", id++, null, [shared, form(2)]];
  // Include descriptor aliasing across top-level arguments and nested children.
  const args = i % 2 ? [shared, nested, shared] : [form(4), form(4), form(4)];
  const root = ["list", id++, form(1), [["symbol", id++, null, "demo", "m"], ...args]];
  assert.deepEqual(exercise(runtimes[1], root), exercise(runtimes[0], root), "case " + i);
}
for (const runtime of runtimes) {
  const bridge = runtime.compilerFormBridge();
  const root = ["list", 1, null, [["symbol", 2, null, "demo", "m"], ["vector", 3, null, [["number", 4, null, 1]]]]];
  const value = bridge.decode(root, null, true);
  assert.throws(() => bridge.macroArguments(value, 1, 1000), /node limit/);
  assert.throws(() => bridge.check(), /node limit/);
}
console.log("500 replay equivalence cases, shared identity, interleaved metadata/provenance, release and limits passed");
