// Reclamation inside a job (light-cljs-847): loops that allocate objects,
// arrays, strings in both encodings and closures, keeping only the last.
// Each passes a table's job limit (100,000 nodes, 1,048,576 code units)
// many times over, so the job reclaims while it runs; serial_tests.goose
// runs it without an instruction budget, checks that collections ran and
// that the heap validates as a snapshot afterwards. Completes with true
// (the values otherwise).
function reclaimLoops() {
  let o;
  for (let i = 0; i < 150000; i++) o = {i};
  let pairs = 0;
  for (let i = 0; i < 150000; i++) { const p = [i, {j: i}]; pairs += p.length; }
  let narrow = "", wide = "", total = 0;
  for (let i = 0; i < 150000; i++) { narrow = "item-" + i; wide = "€" + i; total += narrow.length + wide.length; }
  let f, sum = 0;
  for (let i = 0; i < 150000; i++) { const k = i * 2; f = () => k + i; sum += f(); }
  const result = o.i + "|" + pairs + "|" + narrow + "|" + wide + "|" + total + "|" + f() + "|" + sum;
  return result === "149999|300000|item-149999|€149999|2477780|449997|33749775000" || result;
}
