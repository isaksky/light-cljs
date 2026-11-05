(() => {
  const truth = (value) => value != null && value !== false;
  const rt = /* LIGHT_CLJS_RUNTIME */;
  rt.core.registerReaderNamespace("cljs.user", {aliases: [
  ], refers: [
  ], excludes: [],
  bindings: []});
  rt.core.currentNs = rt.core.theNs(rt.symbol(null, "\u0063\u006c\u006a\u0073\u002e\u0075\u0073\u0065\u0072"));
  {
    const l0 = 0;
    const l1 = rt.core.PersistentArrayMap.EMPTY;
    let r0_0 = l0;
    let r0_1 = l1;
    recur0: while (true) {
      const l0 = r0_0;
      const l1 = r0_1;
      if (truth((rt.core.less).call(null, l0, 16))) {
        {
          const next0 = (rt.core.inc).call(null, l0);
          const next1 = (rt.core.assoc).call(null, l1, l0, (rt.core.multiply).call(null, l0, 2));
          r0_0 = next0;
          r0_1 = next1;
          continue recur0;
        }
      } else {
        return (rt.core.reduce).call(null, rt.core.add, (rt.core.vals).call(null, l1));
      }
    }
  }
})()
