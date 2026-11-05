rt => rt.core.macroReaderFunction(((__lightSessionRuntime) => (() => {
  const rt = __lightSessionRuntime;
  return (function l0(l1, l2, l3) {
    let operand0, operand1, operand2, operand3;
    {
      const l4 = 0;
      const l5 = __rt.formVector();
      const l6 = __rt.formReaderCollection(__rt.formLiteralMap(__rt.formVector()));
      let r1_0 = l4;
      let r1_1 = l5;
      let r1_2 = l6;
      recur1: while (true) {
        const l4 = r1_0;
        const l5 = r1_1;
        const l6 = r1_2;
        if (__rt.cljTruth(__rt.formEquals(l4, l3))) {
          {
            const l7 = __rt.cljSeq(l5);
            const l8 = 0;
            let r2_0 = l7;
            let r2_1 = l8;
            recur2: while (true) {
              const l7 = r2_0;
              const l8 = r2_1;
              if (__rt.cljTruth(l7)) {
                {
                  const next0 = __rt.cljNext(l7);
                  const next1 = (operand0 = l8, operand1 = __rt.cljFirst(l7), operand2 = __rt.cljGet(l6, 1, 0), operand3 = __rt.cljCount(__rt.cljRest(__rt.formVector(1, 2, 3))), (__rt.formValue(operand0) + __rt.formValue(operand1) + __rt.formValue(operand2) + __rt.formValue(operand3)));
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
            const next0 = (__rt.formValue(l4) + __rt.formValue(1));
            const next1 = __rt.cljConj(l5, l4);
            const next2 = __rt.cljAssoc(l6, __rt.cljCount(l5), __rt.cljCount(l6));
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