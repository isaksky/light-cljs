// bbp-authored conformance case.
// Derives from: none (light-cljs-847, from its review: reclamation inside a
// job on two harder schedules). Bursts: a 65,536-unit string, then twenty
// copies one unit longer, keeping only the last, which pass the Goose VM's
// 1,048,576-unit job limit within a few instructions. High water: 92,000
// objects held at once near the 100,000-node limit, dropped, then 60,000
// short-lived ones that need the held set freed. Completes with
// "65537|91999|59999".
(function () {
  let base = "x";
  for (let i = 0; i < 16; i++) base = base + base;
  let last = "";
  for (let i = 0; i < 20; i++) last = base + "!";
  function hold() { const kept = []; for (let i = 0; i < 92000; i++) kept[i] = {i}; return kept[91999].i; }
  const held = hold();
  let o;
  for (let i = 0; i < 60000; i++) o = {i};
  return last.length + "|" + held + "|" + o.i;
})();
