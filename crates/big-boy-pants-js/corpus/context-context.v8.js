rt => rt.core.macroReaderFunction(((__lightSessionRuntime) => (() => {
  const truth = (value) => value != null && value !== false;
  const rt = __lightSessionRuntime;
  return (function l0(l1, l2, l3) {
    return (rt.core.list).call(null, rt.symbol(null, "quote"), rt.vectorFromArray([l3, l1, (rt.core.get).call(null, (rt.core.get).call(null, l2, rt.keyword(null, "ns")), rt.keyword(null, "name")), (rt.core.meta).call(null, l1)]));
  });
})()
)(rt))