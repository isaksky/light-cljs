function mutations() {
  function counter() { let n = 0; return {read: function() { return n; }, bump: function(x) { n = n + x; return n; }}; }
  const c = counter();
  function pair(a, b) { return a * 10 + b; }
  const ordered = pair(c.bump(1), c.bump(2));
  const obj = {x: 2, f: function(a) { this.x = this.x + a; return this.x; }};
  const alias = obj;
  const xs = [obj, c.read(), ordered]; xs[3] = obj.f(4);
  obj["named"] = 9;
  return ordered === 13 && c.read() === 3 && xs.length === 4 && xs[0] === alias && alias.x === 6 && xs[3] === 6 && obj.named === 9 && obj.missing === undefined;
}
