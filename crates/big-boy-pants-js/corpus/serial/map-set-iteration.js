// Live cursors and callbacks through tombstones, compaction, clear and
// append, at every budget cut and reclamation (light-cljs-944).
function mapSetIteration() {
let a = {}, b = Symbol('b');
let m = new Map([[a, 1], [b, 2], [3, 3], [4, 4]]);
let it = m.keys();
let ok = it.next().value === a;
let visited = '';
m.forEach((v, k, self) => {
  visited += v;
  self.delete(k);
  if (v === 1) { self.delete(b); self.set(5, 5); self.set(6, 6); }
});
ok = ok && visited === '13456' && m.size === 0;
m.set(a, 7);
ok = ok && it.next().value === a && it.next().done;
m.set(b, 8);
ok = ok && it.next().done;
let s = new Set([1, 2]); let si = s.entries(); let out = '';
s.forEach((v, k, self) => {
  out += v + ':' + k;
  if (v === 1) { self.clear(); self.add(3); }
});
return ok && out === '1:13:3' && si.next().value.join() === '3,3' && si.next().done;
}
