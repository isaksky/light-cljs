// Test-only typed observation of the pinned numeric fixture bodies. The same
// helper runs in QuickJS and Node; fixture expectations remain literal TSV data.
// It deliberately supports only the numeric corpus's primitive/vector results.
(rt => {
  const encode = value => {
    if (value === undefined) return {$value: "undefined"};
    if (typeof value === "number") {
      if (Object.is(value, -0)) return {$number: "-0"};
      if (Number.isNaN(value)) return {$number: "NaN"};
      if (value === Infinity) return {$number: "+Infinity"};
      if (value === -Infinity) return {$number: "-Infinity"};
      return value;
    }
    if (value === null || typeof value === "boolean" || typeof value === "string") return value;
    const language = rt.inspect(value);
    if (language?.kind === "vector") return {$vector: language.items.map(encode)};
    if (Array.isArray(value)) return value.map(encode);
    throw new Error("Unsupported numeric fixture outcome");
  };
  globalThis.lightHost = {
    boundary: name => ({nil: null, undefined, "negative-zero": -0, nan: NaN,
      infinity: Infinity, "negative-infinity": -Infinity})[name]
  };
  globalThis.numericObservation = fn => {
    const trace = [];
    let value, error, returned = false;
    try { value = fn(x => { trace.push(x); return x; }); returned = true; }
    catch (e) { error = e; }
    const outcome = returned ? {status: "returned", value: encode(value)}
      : {status: "threw", error: {category: error instanceof TypeError ? "type-error" : "error"}};
    return JSON.stringify({outcome, trace: encode(trace), stdout: ""});
  };
  return null;
})
