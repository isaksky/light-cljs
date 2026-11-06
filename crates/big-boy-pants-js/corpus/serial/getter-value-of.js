// ToPrimitive through accessors (light-cljs-805): `valueOf` or `toString`
// behind a getter, own or inherited, runs the getter with the object as
// `this` and then calls the method it returns; a getter giving a
// non-callable skips that method, and an exception the getter throws
// unwinds through the native frame to a catch block.
function getterValueOf() {
  var reads = 0;
  var o = { n: 41, get valueOf() { reads += 1; var self = this; return function () { return self.n + (this === self ? 1 : 0); }; } };
  var base = { get toString() { return function () { return "b" + this.k; }; } };
  var heir = { __proto__: base, k: "2", valueOf: 0 };
  var skip = { get valueOf() { return 5; }, toString: function () { return "s"; } };
  var thrower = { get valueOf() { throw "boom"; } };
  var caught = "";
  try { caught = thrower + 1; } catch (e) { caught = e; }
  return o + 0 === 42 && 1 + o === 43 && reads === 2 && heir + "!" === "b2!" && skip + "" === "s" && caught === "boom";
}
