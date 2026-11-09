function runtimeConversions() {
  // Built-in callbacks of the light-cljs runtime's native frames convert
  // their arguments in Coerce frames (light-cljs-900): mapv's and a deferred
  // map's (builtin_callback) and swap!'s (swap_call), with valueOf and
  // toString running loops above the frame, an extra object argument and a
  // valueOf that throws. serial_tests.goose runs it at every budget cut with
  // the runtime booted (setup 9); the Pants engine has no runtime, so this
  // case is Goose's alone.
  "use strict";
  const rt = __lightNativeRuntime, core = rt.core;
  let order = "";
  const spin = (tag) => { let i = 0; while (i < 3) i++; order += tag; };
  const mapped = core.str(core.mapv(isNaN, rt.vectorFromArray([{ valueOf() { spin("a"); return NaN; } }, { valueOf() { spin("b"); return 1; } }])));
  const deferred = core.str(core.vec(core.mapSeq(parseFloat, rt.listFromArray([{ toString() { spin("c"); return "1.5x"; } }]))));
  const symbols = core.mapv(Symbol, rt.vectorFromArray([{ toString() { spin("d"); return "q"; } }]));
  const atom = core.atom({ toString() { spin("s"); return "ff"; } });
  core.swap(atom, parseInt, { valueOf() { spin("r"); return 16; } });
  let thrown = 0;
  try { core.swap(core.atom("7"), parseInt, { valueOf() { spin("t"); throw 5; } }); } catch (e) { thrown = e; }
  // Math.sumPrecise as the callback runs its own native frame above
  // mapv's and a deferred map's (light-cljs-901), over a user iterator.
  const items = { *[Symbol.iterator]() { spin("u"); yield 1; yield 2; } };
  const sums = core.str(core.mapv(Math.sumPrecise, rt.vectorFromArray([[1, 2], items])));
  const lazy = core.str(core.vec(core.mapSeq(Math.sumPrecise, rt.listFromArray([[0.1, 0.2], [3]]))));
  return mapped === "[true false]" && deferred === "[1.5]" && core.str(core.mapv((s) => s.description === "q", symbols)) === "[true]"
    && core.deref(atom) === 255 && thrown === 5 && sums === "[3 3]" && lazy === "[0.30000000000000004 3]" && order === "abcdsrtu";
}
