// Bound functions (light-cljs-852): bind's frame, calls and constructions
// through Forward frames (nested binds, call and apply of one, map and
// forEach callbacks, a ToPrimitive method, a spread call), new.target and
// instanceof through the target, and bound arguments and `this` that stay
// live across every budget cut and collection (serial_tests.goose).
function boundFunctions() {
  const ctx = { k: 10 };
  function add(a, b, c) { return this.k * 100 + a * 10 + b + (c === undefined ? 0 : c * 1000); }
  const one = add.bind(ctx, 1);
  const two = one.bind(null, 2);
  let sum = 0;
  for (let i = 0; i < 2; i++) { sum += two(i); }
  const mapped = [3, 4].map(add.bind({ k: 1 }, 5));
  let each = 0;
  [1, 2].forEach(function (v) { each += v * this.k; }.bind(ctx));
  const prim = { valueOf: function () { return this.k; }.bind(ctx) };
  let target;
  function Point(x, y) { this.x = x; this.y = y; target = new.target; }
  const AtOne = Point.bind(null, 1);
  const p = new AtOne(...[2]);
  return sum === 2 * 1012 + 1000 && two.length === 1 && two.name === "bound bound add" &&
    one.call({ k: 7 }, 2) === 1012 && one.apply(null, [3, 4]) === 5013 &&
    mapped[0] === 153 && mapped[1] === 1154 && each === 30 && prim + 1 === 11 &&
    p.x === 1 && p.y === 2 && target === Point && p instanceof AtOne && p instanceof Point &&
    typeof two === "function" && AtOne.prototype === undefined;
}
