// Test-only SCI expression transport. Inspection uses lexical runtime brands.
// Raw sequence categories are retained; the catalog owns the documented finite
// list/seq comparison projection. Serialization failures are harness failures.
(rt => {
  const encode = (value, ancestors = new Set()) => {
    if (value === undefined) return {$value: "undefined"};
    if (typeof value === "number") {
      if (Object.is(value, -0)) return {$number: "-0"};
      if (Number.isNaN(value)) return {$number: "NaN"};
      if (value === Infinity) return {$number: "+Infinity"};
      if (value === -Infinity) return {$number: "-Infinity"};
      return value;
    }
    if (value === null || typeof value === "boolean" || typeof value === "string") return value;
    if (typeof value !== "object" || ancestors.has(value)) throw new Error("Unsupported/cyclic SCI observation");
    ancestors.add(value);
    let result;
    const language = rt.inspect(value);
    if (language?.kind === "keyword" || language?.kind === "symbol") {
      result = {["$" + language.kind]: {
        namespace: language.namespace, name: language.name, spelling: language.spelling
      }};
    } else if (["vector", "list", "seq"].includes(language?.kind)) {
      result = {["$" + language.kind]: language.items.map(item => encode(item, ancestors))};
    } else if (language?.kind === "set") {
      const items = language.items.map(item => encode(item, ancestors));
      items.sort((a, b) => JSON.stringify(a) < JSON.stringify(b) ? -1 : JSON.stringify(a) > JSON.stringify(b) ? 1 : 0);
      result = {$set: items, count: language.count};
    } else if (language?.kind === "map") {
      const entries = language.entries.map(pair => pair.map(item => encode(item, ancestors)));
      entries.sort((a, b) => JSON.stringify(a) < JSON.stringify(b) ? -1 : JSON.stringify(a) > JSON.stringify(b) ? 1 : 0);
      result = {$map: entries, count: language.count};
    } else if (language === null && rt.core.isSequential(value)) {
      // inspect deliberately omits deferred LazySeq wrappers. Use the public
      // branded sequence protocol, just as the production data bridge does.
      const items = [];
      for (let current = rt.core.seq(value); current !== null; current = rt.core.next(current))
        items.push(encode(rt.core.first(current), ancestors));
      result = {$seq: items};
    } else if (Array.isArray(value)) result = value.map(item => encode(item, ancestors));
    else throw new Error("Unsupported SCI observation value");
    ancestors.delete(value);
    return result;
  };
  return (id, fn) => {
    const trace = [];
    let stdout = "", stderr = "", value, error, returned = false;
    rt.core.printFn = text => { stdout += text; };
    rt.core.printErrFn = text => { stderr += text; };
    try { value = fn(item => { trace.push(item); return item; }); returned = true; }
    catch (e) { error = e; }
    const outcome = returned ? {status: "returned", value: encode(value)} : {
      status: "threw", error: {category: error instanceof TypeError ? "type-error" :
        error instanceof RangeError ? "range-error" : error instanceof ReferenceError ? "reference-error" : "error"}
    };
    // Original stderr is empty throughout this cohort; any actual write fails.
    if (stderr !== "") throw new Error("Unexpected SCI stderr: " + stderr);
    return JSON.stringify({id, outcome, trace: encode(trace), stdout});
  };
})
