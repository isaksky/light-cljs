(() => {
  const truth = (value) => value != null && value !== false;
  const rt = /* LIGHT_CLJS_RUNTIME */;
  rt.core.registerReaderNamespace("cljs.user", {aliases: [
  ], refers: [
  ], excludes: [],
  bindings: ["\u0049\u0057\u0065\u0069\u0067\u0068\u0074","\u0077\u0065\u0069\u0067\u0068\u0074","\u0042\u006f\u0078","\u002d\u003e\u0042\u006f\u0078"]});
  rt.core.currentNs = rt.core.theNs(rt.symbol(null, "\u0063\u006c\u006a\u0073\u002e\u0075\u0073\u0065\u0072"));
  let g0;
  let g1;
  let g2;
  let g3;
  (() => { g0 = rt.protocol("cljs.user/IWeight"); g1 = rt.protocolMethod("cljs.user/IWeight/weight", "cljs.user/weight", [1], false, null, () => g1); return undefined; })();
  (() => { g2 = rt.makeType(["n"], [["cljs.user/IWeight", 0, (function() {
    return true;
  })], ["cljs.user/IWeight/weight", 1, (function(l3) {
    const self3 = this;
    l3 = this;
    return self3["n"];
  })]], false, null, "cljs.user/Box", null, function(a0) { this["n"] = a0; }); g3 = (...args) => new g2(...args); g2.getBasis = () => rt.core.vector(rt.symbol(null, "n")); g2.cljs$lang$type = true; g2.cljs$lang$ctorStr = "cljs.user/Box"; return g2; })();
  {
    const l0 = 0;
    const l1 = 0;
    let r1_0 = l0;
    let r1_1 = l1;
    recur1: while (true) {
      const l0 = r1_0;
      const l1 = r1_1;
      if (truth((rt.core.less).call(null, l0, 16))) {
        {
          const next0 = (rt.core.inc).call(null, l0);
          const next1 = (rt.core.add).call(null, l1, (g1).call(null, new g2(l0)));
          r1_0 = next0;
          r1_1 = next1;
          continue recur1;
        }
      } else {
        return l1;
      }
    }
  }
})()
