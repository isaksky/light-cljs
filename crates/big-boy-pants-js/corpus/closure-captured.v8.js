rt => rt.core.macroReaderFunction(((__lightSessionRuntime) => (() => {
  const truth = (value) => value != null && value !== false;
  const rt = __lightSessionRuntime;
  return (function l0(l1, l2, l3, l4) {
    {
      const l5 = (rt.core.add).call(null, l3, 3);
      const l7 = (function(l6) {
        return rt.vectorFromArray([l6, (rt.core.add).call(null, l5, l6)]);
      });
      return (rt.core.list).call(null, rt.symbol(null, "quote"), (rt.core.mapv).call(null, l7, l4));
    }
  });
})()
)(rt))