function stringScans(n) {
  // Native scan and copy phases longer than one quantum (light-cljs-903).
  const count = n === undefined ? 1100 : n;
  const a = String.fromCharCode.apply(null, Array(count).fill(97));
  const s = a + '\ud800' + 'b';
  const raw = String.raw({raw: [a, 'b']}, 'S');
  return s.indexOf('ab') === -1 && s.lastIndexOf('aa') === count - 2
    && s.includes('\ud800b') && s.endsWith('\ud800b') && s.startsWith(a)
    && !s.isWellFormed() && s.toWellFormed().charCodeAt(count) === 65533
    && raw.length === count + 2 && raw.charAt(count) === 'S';
}
