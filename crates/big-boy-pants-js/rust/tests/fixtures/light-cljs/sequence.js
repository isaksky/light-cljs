(() => {
  const truth = (value) => value != null && value !== false;
  const rt = /* LIGHT_CLJS_RUNTIME */;
  rt.core.registerReaderNamespace("cljs.user", {aliases: [
  ], refers: [
  ], excludes: [],
  bindings: []});
  rt.core.currentNs = rt.core.theNs(rt.symbol(null, "\u0063\u006c\u006a\u0073\u002e\u0075\u0073\u0065\u0072"));
  return (rt.core.reduce).call(null, rt.core.add, (rt.core.mapSeq).call(null, (function(l0) {
    return (rt.core.multiply).call(null, l0, l0);
  }), (rt.core.range).call(null, 10)));
})()
