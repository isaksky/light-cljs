rt => rt.core.macroReaderFunction(((__lightSessionRuntime) => (() => {
  const truth = (value) => value != null && value !== false;
  const rt = __lightSessionRuntime;
  return (function l0(l1, l2, l3) {
    {
      const l4 = 0;
      const l5 = rt.core.PersistentVector.EMPTY;
      const l6 = rt.core.PersistentArrayMap.EMPTY;
      let r1_0 = l4;
      let r1_1 = l5;
      let r1_2 = l6;
      recur1: while (true) {
        const l4 = r1_0;
        const l5 = r1_1;
        const l6 = r1_2;
        if (truth((rt.core.equal).call(null, l4, l3))) {
          {
            const l7 = (rt.core.seq).call(null, l5);
            const l8 = 0;
            let r2_0 = l7;
            let r2_1 = l8;
            recur2: while (true) {
              const l7 = r2_0;
              const l8 = r2_1;
              if (truth(l7)) {
                {
                  const next0 = (rt.core.next).call(null, l7);
                  const next1 = (rt.core.add).call(null, l8, (rt.core.first).call(null, l7), (rt.core.get).call(null, l6, 1, 0), (rt.core.count).call(null, (rt.core.rest).call(null, rt.vectorFromArray([1, 2, 3]))));
                  r2_0 = next0;
                  r2_1 = next1;
                  continue recur2;
                }
              } else {
                return l8;
              }
            }
          }
        } else {
          {
            const next0 = (rt.core.add).call(null, l4, 1);
            const next1 = (rt.core.conj).call(null, l5, l4);
            const next2 = (rt.core.assoc).call(null, l6, (rt.core.count).call(null, l5), (rt.core.count).call(null, l6));
            r1_0 = next0;
            r1_1 = next1;
            r1_2 = next2;
            continue recur1;
          }
        }
      }
    }
  });
})()
)(rt))