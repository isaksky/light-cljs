// Call shapes of light-cljs-212 (`cargo bench --bench compiler -- --callees`;
// see benchmarks/compiler.md, "Direct call shapes"). A function of
// `(rounds, warmups, n)` that returns JSON: for each call shape and target,
// the nanoseconds per call of every timed round. Each round times every pair
// once, so outside load lands on all pairs alike.
//
// Targets are the three function values the emitter builds: one fixed method
// (`single`), an arity dispatcher over fixed methods (`multi`, called with one
// argument), and a dispatcher with a variadic method (`variadic`, called with
// its one fixed argument). Shapes:
// - `call`: `f.call(null, a)`, what every unknown callee gets;
// - `arity`: upstream's `:fn-invoke-direct` for an unknown callee,
//   `f.cljs$core$IFn$_invoke$arity$1 ? f.cljs$core$IFn$_invoke$arity$1(a) : f.call(null, a)`;
// - `direct`: the method itself, `m(a)`, what a known target gets.
(rounds, warmups, n) => {
  const methods = {
    single: function (x) { return x + 1; },
    multi: function (x) { return x + 1; },
    variadic: function (x) { return x + 1; },
  };
  const single = methods.single;
  const multi = (() => {
    const m0 = function () { return 0; };
    const m1 = methods.multi;
    const m2 = function (x, y) { return x + y; };
    const f = function (a0, a1) {
      switch (arguments.length) {
        case 0: return m0.apply(this, arguments);
        case 1: return m1.apply(this, arguments);
        case 2: return m2.apply(this, arguments);
        default: throw new Error("Invalid arity: " + arguments.length);
      }
    };
    f.cljs$core$IFn$_invoke$arity$0 = m0;
    f.cljs$core$IFn$_invoke$arity$1 = m1;
    f.cljs$core$IFn$_invoke$arity$2 = m2;
    return f;
  })();
  const variadic = (() => {
    const m0 = methods.variadic;
    const m1 = function (x, rest) { return x + 1; };
    const f = function (a0, a1) {
      switch (arguments.length) {
        case 1: return m0.apply(this, arguments);
        default: return m1.call(this, arguments[0], null);
      }
    };
    f.cljs$core$IFn$_invoke$arity$1 = m0;
    f.cljs$core$IFn$_invoke$arity$variadic = m1;
    return f;
  })();
  const targets = { single, multi, variadic };
  // Each pair gets a loop compiled on its own, so no call site sees two targets.
  const bodies = {
    call: "total = f.call(null, total);",
    arity: "total = f.cljs$core$IFn$_invoke$arity$1 ? f.cljs$core$IFn$_invoke$arity$1(total) : f.call(null, total);",
    direct: "total = f(total);",
  };
  const pairs = [];
  for (const [shape, body] of Object.entries(bodies)) {
    for (const [name, target] of Object.entries(targets)) {
      const loop = new Function("f", "n", `let total = 0; for (let i = 0; i < n; i++) { ${body} } return total;`);
      pairs.push([shape + "/" + name, loop, shape === "direct" ? methods[name] : target]);
    }
  }
  const now = typeof performance === "undefined" ? () => Date.now() : () => performance.now();
  const samples = {};
  for (let round = 0; round < warmups + rounds; round++) {
    for (const [key, loop, f] of pairs) {
      const start = now();
      const total = loop(f, n);
      const elapsed = now() - start;
      if (total !== n) throw new Error(key + " returned " + total);
      if (round >= warmups) (samples[key] ??= []).push(elapsed * 1e6 / n);
    }
  }
  return JSON.stringify(samples);
}
