(() => {
  const c = rt.core;
  rt.setDeclarationDemand(() => {});
  const pairs = [];
  for (let i = 0; i < 100; i++) pairs.push(c.symbol("alias" + i), c.symbol("target" + i));
  const value = rt.mapFromArray(pairs);
  let count = 0;
  const start = Date.now();
  for (let i = 0; i < 1000; i++) {
    for (let s = c.seq(value); s != null; s = c.next(s)) {
      const entry = c.first(s);
      if (c.str(c.nth(entry, 0)).startsWith("alias") && c.str(c.nth(entry, 1)).startsWith("target")) count++;
    }
  }
  if (count !== 100000) throw Error("incorrect traversal: " + count);
  return JSON.stringify({count, milliseconds: Date.now() - start});
})()
