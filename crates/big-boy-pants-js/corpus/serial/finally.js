// Finally blocks and completion records (light-cljs-786): return, break,
// continue and throw through and out of finally blocks, nested finally.
function finallyBlocks() {
  let log = "";
  function returned() { try { log += "a"; return 1; } finally { log += "b"; } }
  function overridden() { try { return 1; } finally { return 2; } }
  function thrower() { try { throw "1"; } finally { throw "2"; } }
  function nested() {
    try { try { log += "c"; throw "3"; } finally { log += "d"; } } catch (e) { log += e; } finally { log += "e"; }
    return 4;
  }
  function caught() { try { throw "x"; } catch (e) { return e; } finally { log += "g"; } }
  const r = returned();
  const o = overridden();
  const k = nested();
  const c = caught();
  let n = 0;
  for (;;) { try { n += 1; break; } finally { n += 10; } }
  let m = 0;
  while (m < 3) { try { m += 1; continue; } finally { log += "f"; } }
  let dropped = 0;
  for (;;) { try { throw "5"; } finally { dropped = 7; break; } }
  let thrown = "";
  try { thrower(); } catch (e) { thrown = e; }
  let q = 0;
  outer: for (;;) { try { try { q += 1; break outer; } finally { q += 2; } } finally { q += 4; } }
  return r === 1 && o === 2 && k === 4 && c === "x" && n === 11 && m === 3 && dropped === 7 && thrown === "2" && q === 7 && log === "abcd3egfff";
}
