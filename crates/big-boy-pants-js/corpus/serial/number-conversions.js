function numberConversions() {
  // Built-in receiver and argument conversions (light-cljs-900): each
  // valueOf and toString below runs a loop in a frame above the built-in's
  // Coerce frame, so serial_tests.goose stops and resumes it at every budget
  // cut, and collects inside the job at every cut while the frame holds the
  // receiver and arguments it converts. A valueOf that throws ends its call;
  // a symbol ends one before a later argument converts; isNaN as a
  // @@hasInstance method converts there too (the Pants engine refuses that
  // row, so this case is Goose's alone).
  "use strict";
  let order = "";
  const spin = (tag) => { let i = 0; while (i < 3) i++; order += tag; };
  const s = { toString() { spin("s"); return "ff"; } };
  const r = { valueOf() { spin("r"); return 16; } };
  const a = parseInt(s, r);
  const b = parseInt.apply(null, [s, r]);
  const c = [{ valueOf() { spin("m"); return NaN; } }, "1"].map(isNaN).join();
  const d = Number.prototype.toString.call(new Number(255), { valueOf() { spin("x"); return 10; } });
  let thrown = 0;
  try { isFinite({ valueOf() { spin("t"); throw 7; } }); } catch (e) { thrown = e; }
  let symbol = false;
  try { parseInt(Symbol(), r); } catch (e) { symbol = e instanceof TypeError; }
  function C() {}
  Object.defineProperty(C, Symbol.hasInstance, { value: isNaN });
  const inst = ({ valueOf() { spin("h"); return NaN; } }) instanceof C;
  const n = new Number({ valueOf() { spin("n"); return 3; } });
  const sym = [{ toString() { spin("y"); return "q"; } }].map(Symbol)[0].description;
  const f = parseFloat({ toString() { spin("f"); return "1.5e1x"; } });
  return a === 255 && b === 255 && c === "true,false" && d === "255" && thrown === 7 && symbol
    && inst && n.valueOf() === 3 && sym === "q" && f === 15 && order === "srsrmxthnyf";
}
