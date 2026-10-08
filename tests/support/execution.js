// Shared outcome protocol for both compilers. Harness/serialization failures
// escape this runner; only exceptions from the compiled case are outcomes.
(() => {
  function encode(value, ancestors = new Set()) {
    if (value === undefined) return { $value: "undefined" };
    if (typeof value === "number") {
      if (Object.is(value, -0)) return { $number: "-0" };
      if (Number.isNaN(value)) return { $number: "NaN" };
      if (value === Infinity) return { $number: "+Infinity" };
      if (value === -Infinity) return { $number: "-Infinity" };
      return value;
    }
    if (value === null || typeof value === "boolean" || typeof value === "string") return value;
    if (typeof value !== "object" || ancestors.has(value)) {
      throw new Error("unsupported or cyclic outcome value");
    }
    ancestors.add(value);
    let result;
    if (Array.isArray(value)) {
      result = Array.from(value, item => encode(item, ancestors));
    } else if (Object.getPrototypeOf(value) === Object.prototype || Object.getPrototypeOf(value) === null) {
      // Tag plain objects so a host object cannot impersonate a special value.
      result = { $object: Object.fromEntries(Object.entries(value).map(([key, item]) => [key, encode(item, ancestors)])) };
    } else {
      throw new Error("unsupported outcome object");
    }
    ancestors.delete(value);
    return result;
  }

  function failure(error) {
    // Native categories, never engine-specific messages, names, or stacks.
    for (const [type, category] of [
      [TypeError, "type-error"], [RangeError, "range-error"],
      [ReferenceError, "reference-error"], [SyntaxError, "syntax-error"],
      [URIError, "uri-error"], [EvalError, "eval-error"], [Error, "error"],
    ]) {
      if (error instanceof type) return { category };
    }
    return { category: "thrown-value", value: encode(error) };
  }

  const programs = globalThis.lightCases;
  const metadata = globalThis.lightCaseMetadata;
  if (!Array.isArray(programs) || programs.length === 0 ||
      !Array.isArray(metadata) || programs.length !== metadata.length ||
      metadata.some(row => !Array.isArray(row) || row.length !== 2 || row.some(value => typeof value !== "string")) ||
      new Set(metadata.map(([id]) => id)).size !== programs.length ||
      programs.some(program => typeof program !== "function")) {
    throw new Error("compiler produced missing, invalid, or duplicate executable cases");
  }

  for (let index = 0; index < programs.length; index++) {
    const [id, source] = metadata[index];
    process.stderr.write(`CASE START ${JSON.stringify({ id, source })}\n`);
    const trace = [];
    const stdout = [];
    globalThis.lightHost = {
      value: 40,
      get method() {
        trace.push("method-get");
        return function(a, b) { trace.push(["call", a, b]); return this.value; };
      },
      receiver() { trace.push("receiver"); return this; },
      get path() { trace.push("path"); return this; },
      boundary(name) {
        return { nil: null, undefined, "negative-zero": -0, nan: NaN,
          infinity: Infinity, "negative-infinity": -Infinity }[name];
      },
      fail(kind) {
        trace.push(["throw", kind]);
        if (kind === "type") throw new TypeError("host failure");
        if (kind === "range") throw new RangeError("host failure");
        if (kind === "reference") throw new ReferenceError("host failure");
        if (kind === "nil") throw null;
        if (kind === "undefined") throw undefined;
        throw new Error("host failure");
      },
    };
    const observe = value => { trace.push(value); return value; };
    const write = process.stdout.write;
    // Capture the stream, including console.log and direct host writes. Buffer
    // chunks are joined before decoding so split UTF-8 characters survive.
    process.stdout.write = function(chunk, encoding, callback) {
      stdout.push(Buffer.isBuffer(chunk) ? Buffer.from(chunk) : Buffer.from(chunk, typeof encoding === "string" ? encoding : "utf8"));
      if (typeof encoding === "function") encoding();
      else if (typeof callback === "function") callback();
      return true;
    };
    let returned = false, value, error;
    try {
      value = programs[index](observe);
      returned = true;
    } catch (thrown) {
      error = thrown;
    } finally {
      process.stdout.write = write;
    }
    const outcome = returned ? { status: "returned", value: encode(value) }
      : { status: "threw", error: failure(error) };
    write.call(process.stdout, JSON.stringify({ id, outcome, trace: encode(trace), stdout: Buffer.concat(stdout).toString("utf8") }) + "\n");
    process.stderr.write(`CASE END ${JSON.stringify({ id })}\n`);
  }
})();
