// Mapped arguments objects across a dependency wait (light-cljs-868), a
// sloppy script: `dep` is a host effect the test answers with "D", in place
// or after the job's continuation was saved, validated and restored in a
// fresh heap (snapshot_tests.goose). Live across the wait: the entry's own
// mapped cell on the saved stack, an escaped mapped arguments object with
// closures over its parameters (one index moved out of the element run by
// an `enumerable: false` redefinition, which keeps the link), and a sloppy
// async function awaiting with its mapped object. Afterwards aliasing holds
// both ways and a delete still unmaps. Completes with
// "D,s,,t,m:n,qpB:undefined,true" (r3 is undefined, which join prints empty).
async function sloppyMappedWait(dep) {
  var self = arguments;
  function keep(a, b) {
    Object.defineProperty(arguments, "1", { enumerable: false });
    return { args: arguments, get: function () { return a; }, set: function (x) { a = x; }, getB: function () { return b; } };
  }
  var k = keep(1, 2);
  async function af(a, b) {
    var v = await "p";
    arguments[0] = a + v; b = "B";
    var s = a + arguments[1];
    delete arguments[1]; b = "C";
    return s + ":" + arguments[1];
  }
  var pending = af("q", "r");
  var d = dep("need");
  k.args[0] = d; var r1 = k.get();
  k.set("s"); var r2 = k.args[0];
  delete k.args[0]; k.set("t"); var r3 = k.args[0];
  k.args[0] = "u"; var r4 = k.get();
  k.args[1] = "m"; var r5 = k.getB() + ":" + (k.args[1] = "n", k.args[1]);
  var z = await pending;
  dep = 5;
  return [r1, r2, r3, r4, r5, z, self[0] === 5 && Object.keys(k.args).join() === "0"].join(",");
}
sloppyMappedWait(dep);
