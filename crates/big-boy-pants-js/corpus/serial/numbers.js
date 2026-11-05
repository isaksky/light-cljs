function numbers() {
  const nan = 0 / 0; const zero = -0;
  let increment = 0; const before = increment++; const after = ++increment;
  return before === 0 && after === 2 && increment === 2 && !(null == 0) && !(undefined == 0) && false == 0 && nan !== nan && !nan && !zero && 1 / zero < 0 && (-7 % 3) === -1 && (+null) === 0 && (+true) === 1 && (+undefined) !== (+undefined) && null == undefined && "a" < "b" && "ab" + "cd" === "abcd";
}
