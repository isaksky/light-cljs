// [[Extensible]] and integrity levels (light-cljs-882): objects made
// non-extensible, sealed and frozen in a loop that allocates garbage
// between them, so serial_tests.goose stops and resumes the job at every
// budget cut and collects inside it at every cut while it holds them: the
// node's [[Extensible]] state, a sealed or frozen dense array's flags, the
// restricted shapes, dictionary records and moved elements must survive
// in-job reclamation, and new properties stay refused after it.
function integrityLevels() {
  "use strict";
  const refused = (f) => { try { f(); return false; } catch (e) { return e instanceof TypeError; } };
  const kept = [];
  let junk = 0;
  for (let i = 0; i < 4; i++) {
    const o = { a: i, b: [i] };
    const p = Object.preventExtensions({ x: i });
    const f = Object.freeze(i % 2 === 0 ? { k: i, 0: "e" } : [i, , i + 1]);
    const s = Object.seal([i, i + 1]);
    const g = function named() { return i; };
    Object.freeze(g);
    for (let j = 0; j < 20; j++) { junk += [j, { j: j }].length; }
    Object.freeze(o);
    kept.push(o, p, f, s, g);
  }
  let ok = junk === 160;
  for (let i = 0; i < 4; i++) {
    const o = kept[5 * i], p = kept[5 * i + 1], f = kept[5 * i + 2], s = kept[5 * i + 3], g = kept[5 * i + 4];
    s[0] = 9;
    p.x = 7;
    ok = ok && Object.isFrozen(o) && !Object.isFrozen(o.b) && refused(() => { o.c = 1; }) && refused(() => { o.a = 1; }) && o.a === i
      && !Object.isExtensible(p) && refused(() => { p.y = 1; }) && p.x === 7
      && Object.isFrozen(f) && refused(() => { f[0] = 1; })
      && Object.isSealed(s) && !Object.isFrozen(s) && s[0] === 9 && refused(() => { s.push(1); }) && refused(() => { delete s[1]; })
      && Object.isFrozen(g) && g() === i && g.name === "named" && refused(() => { g.prototype = 1; });
  }
  return ok;
}
