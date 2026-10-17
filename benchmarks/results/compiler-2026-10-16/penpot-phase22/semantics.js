(() => {
  const c = rt.core, slot = "cljs$core$ICounted$_count$arity$1", prototype = c.List.prototype;
  const previous = Object.getOwnPropertyDescriptor(prototype, slot);
  let calls = 0, value, observed;
  try {
    Object.defineProperty(prototype, slot, {configurable: true, writable: true,
      value: () => { calls++; return 999; }});
    value = rt.listFromArray([10, 20, 30]);
    observed = c.count(value);
  } finally {
    if (previous) Object.defineProperty(prototype, slot, previous);
    else delete prototype[slot];
  }
  return JSON.stringify([calls, observed, c.count(value), c.prStr(value)]);
})()
