// Reclamation inside a job (light-cljs-847), two harder schedules. Bursts:
// a 65,536-unit string, then twenty copies of it one unit longer, of which
// only the last is kept, in fewer instructions than a run_claimed slice:
// together they pass the 1,048,576-unit job limit within a few
// instructions, so the collection must come right after the allocation that
// asked for it. High water: 92,000 objects held at once, so a collection
// finds little to free and the margin shrinks near the 100,000-node limit,
// then dropped and followed by 60,000 short-lived ones, which need the
// collection that frees the held set. serial_tests.goose runs it without an
// instruction budget and counts the collections. Completes with true (the
// values otherwise).
function reclaimBursts() {
  let base = "x";
  for (let i = 0; i < 16; i++) base = base + base;
  let last = "";
  for (let i = 0; i < 20; i++) last = base + "!";
  function hold() { const kept = []; for (let i = 0; i < 92000; i++) kept[i] = {i}; return kept[91999].i; }
  const held = hold();
  let o;
  for (let i = 0; i < 60000; i++) o = {i};
  const result = last.length + "|" + held + "|" + o.i;
  return result === "65537|91999|59999" || result;
}
