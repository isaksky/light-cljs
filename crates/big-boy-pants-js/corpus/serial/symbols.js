function symbols() {
  // Symbol rows, symbol keys and the Symbol.for registry (light-cljs-885)
  // at every budget cut and across collections inside the job
  // (serial_tests.goose): each round makes symbols nothing keeps, a symbol
  // key that stays, and a registered symbol only the registry keeps.
  const kept = [];
  const o = {};
  for (let i = 0; i < 400; i++) {
    const s = Symbol('s' + i);
    const junk = {a: Symbol(), b: [i], c: Symbol('t' + i)};
    if (i % 20 === 0) { o[s] = i; kept.push(s); Symbol.for('r' + i); }
  }
  let ok = kept.length === 20;
  for (let j = 0; j < kept.length; j++) {
    const s = kept[j];
    if (o[s] !== j * 20 || s.description !== 's' + j * 20 || Symbol.keyFor(Symbol.for('r' + j * 20)) !== 'r' + j * 20) { ok = false; }
  }
  const w = Object(kept[1]);
  return ok && w.valueOf() === kept[1] && Symbol.for('r0') === Symbol.for('r' + 0) && String(kept[2]) === 'Symbol(s40)' && Symbol.keyFor(kept[3]) === undefined;
}
