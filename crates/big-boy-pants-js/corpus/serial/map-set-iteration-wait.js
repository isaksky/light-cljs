function mapSetIterationWait(dep) {
  // Save a forEach callback and an independent live cursor, compact while
  // the callback is suspended, and migrate both to a fresh heap. A second
  // wait exercises Set's forEach too. Every entry is visited exactly once.
  let d = '', out = '';
  let a = {}, b = Symbol('b');
  let m = new Map([[a, 1], [b, 2], [3, 3], [4, 4]]);
  let it = m.values(); let first = it.next().value;
  m.forEach((v, k) => {
    out += v;
    if (v === 2) {
      m.delete(a); m.delete(b); m.set(5, 5); m.set(6, 6);
      d = dep();
    }
  });
  let ok = first === 1 && out === '123456' && [...it].join() === '3,4,5,6';
  let s = new Set([1, 2]); let si = s.values(); out = '';
  s.forEach(v => { out += v; if (v === 1) { s.clear(); s.add(3); d = dep(); } });
  return d + ':' + (ok && out === '13' && [...si].join() === '3');
}
