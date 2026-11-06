// Array.prototype.map and forEach (light-cljs-789) run their callbacks,
// element getters and the length's getter and valueOf in a resumable native
// frame: dense elements, holes, an inherited index, an exception unwinding
// through the frame to a catch, and a non-callable callback's TypeError.
function arrayCallbacks() {
  const ctx = { k: 10 };
  const doubled = [1, 2, 3].map(function (v, i, o) { return v * 2 + i + (this === ctx ? 0 : 100) + (o.length === 3 ? 0 : 1000); }, ctx);
  let sum = 0;
  const each = [4, 5].forEach(function (v) { sum += v * this.k; }, ctx);
  const holes = Array(3); holes[1] = 7;
  const mapped = holes.map(function (v) { return v + 1; });
  let reads = 0;
  const generic = { __proto__: { 1: 20 }, 0: 10, get 2() { return this[0] + 20; }, get length() { reads++; return { valueOf: function () { return 3.5; } }; } };
  const g = Array.prototype.map.call(generic, function (v, i) { return v + i; });
  let caught = 0, calls = 0;
  try { [1, 2, 3].forEach(function (v) { calls++; if (v === 2) { throw v; } }); } catch (e) { caught = e; }
  let typeError = false;
  try { [].map(null); } catch (e) { typeError = e.name === "TypeError"; }
  return doubled[0] === 2 && doubled[1] === 5 && doubled[2] === 8 && each === undefined && sum === 90 &&
    mapped.length === 3 && !(0 in mapped) && mapped[1] === 8 && !(2 in mapped) &&
    g.length === 3 && g[0] === 10 && g[1] === 21 && g[2] === 32 && reads === 1 && caught === 2 && calls === 2 && typeError;
}
