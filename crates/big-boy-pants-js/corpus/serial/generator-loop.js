// A long generator loop and a long await loop (light-cljs-790): each
// suspension reuses its activation's rows, so no step adds value rows and
// a yield adds no continuation row; an await of a value adds only the
// promise it awaits (serial_tests.goose checks the rows).
function generatorLoop() {
  function* count(n) { var i = 0; while (i < n) { yield i; i = i + 1; } }
  var total = 0;
  var g = count(2000);
  for (var r = g.next(); !r.done; r = g.next()) { total = total + r.value; }
  var awaited = 0;
  async function spin(n) { var i = 0; while (i < n) { await i; awaited = awaited + 1; i = i + 1; } }
  spin(2000);
  return total === 1999000;
}
