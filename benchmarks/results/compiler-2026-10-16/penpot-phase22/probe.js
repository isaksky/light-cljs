(() => {
  const items = Array.from({length: 20}, (_, i) => i);
  let result;
  const start = Date.now();
  for (let i = 0; i < 50000; i++) result = rt.listFromArray(items);
  const elapsed = Date.now() - start;
  if (rt.core.count(result) !== 20 || rt.core.first(result) !== 0 || rt.core.nth(result, 19) !== 19)
    throw Error("incorrect list");
  return JSON.stringify({nodes: 1000000, milliseconds: elapsed});
})()
