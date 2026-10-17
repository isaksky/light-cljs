(() => {
  const run = found => {
    const value = {};
    rt.extendInstance(value, [["ILookup", 0, null]]);
    Object.defineProperty(value, "cljs$core$ILookup$_lookup$arity$3", {
      get() {
        delete value.cljs$core$ILookup$;
        return found ? () => "found" : null;
      }
    });
    try { return rt.core.get(value, "key", "fallback"); }
    catch (error) { return error.message; }
  };
  return JSON.stringify([run(false), run(true)]);
})()
