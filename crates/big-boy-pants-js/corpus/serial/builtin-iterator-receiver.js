// The built-in array iterators over a source serial_tests.goose supplies
// (light-cljs-844, decision 3): arrays and array-likes in rows the job did
// not allocate (below its protected or owned rows, or in a foreign range).
// A cursor only reads its source, so values() and entries() iterate them,
// and for-of iterates a protected array, without changing a source row.
// Only Goose runs it that way.
function iterateOver(a) {
  let s = "";
  for (const x of Array.prototype.values.call(a)) s += x;
  for (const [i, x] of Array.prototype.entries.call(a)) s += i;
  if (Array.isArray(a)) for (const x of a) s += x;
  return s;
}
