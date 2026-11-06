// Functions' own properties (light-cljs-805): `f.x = 1` on a closure, read,
// overwritten, deleted and found by `in`; an ordinary function's `prototype`
// is made on its first read, the same object afterwards, whose
// `constructor` is the function, and cannot be deleted; it can be replaced
// by assignment; a property added first, or a read inherited through an
// object whose prototype is the function, makes it too, and `in` reports it
// before it exists. Arrows and methods have no `prototype` but take
// properties; `call` still reaches Function.prototype's.
function functionNames() {
  function counter() { counter.count += 1; return counter.count; }
  counter.count = 0;
  counter(); counter();
  var r1 = counter.count === 2 && ("count" in counter) && counter.missing === undefined;
  function F() {}
  var p = F.prototype;
  // Deleting it is refused: false in sloppy code, a TypeError in strict code.
  var kept = false;
  try { kept = !(delete F.prototype); } catch (e) { kept = true; }
  var r2 = p !== undefined && p !== null && p === F.prototype && p.constructor === F && ("prototype" in F) && kept && F.prototype === p;
  F.prototype = { mark: 7 };
  var r3 = F.prototype.mark === 7 && F.prototype !== p;
  function G() {}
  G.first = 1;
  var r4 = G.prototype.constructor === G && G.first === 1 && (delete G.first) && G.first === undefined;
  var arrow = () => 1;
  var method = { m() { return 2; } }.m;
  arrow.x = 3; method.y = 4;
  var r5 = arrow.prototype === undefined && !("prototype" in arrow) && method.prototype === undefined && arrow.x === 3 && method.y === 4;
  // Inherited before any direct observation, through an object whose
  // prototype is the function; `in` answers before it is made.
  function H() {}
  var present = "prototype" in H;
  var heir = { __proto__: H };
  var r7 = present && heir.prototype.constructor === H && heir.prototype === H.prototype;
  function add(a, b) { return this.base + a + b; }
  add.base = 100;
  var r6 = add.call({ base: 1 }, 2, 3) === 6 && add.base === 100;
  return r1 && r2 && r3 && r4 && r5 && r6 && r7;
}
