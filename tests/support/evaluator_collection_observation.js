// Exact typed observation for the complete evaluator collection fixture.
// Preserve every collection kind and traversal order; never sort members.
((inspect, print, value) => {
  const encode = value => {
    if (value === null || typeof value === 'number' || typeof value === 'boolean' || typeof value === 'string') return value;
    const data = inspect(value);
    if (!data) throw new Error('Unsupported evaluator fixture value');
    if (data.kind === 'keyword') return {$keyword: {namespace: data.namespace, name: data.name, spelling: data.spelling}};
    if (data.kind === 'vector') return {$vector: Array.from(data.items, encode)};
    if (data.kind === 'map') return {$map: Array.from(data.entries, pair => Array.from(pair, encode)), count: data.count};
    if (data.kind === 'set') return {$set: Array.from(data.items, encode), count: data.count};
    throw new Error('Unsupported evaluator fixture kind: ' + data.kind);
  };
  return JSON.stringify({printed: print(value), value: encode(value)});
})
