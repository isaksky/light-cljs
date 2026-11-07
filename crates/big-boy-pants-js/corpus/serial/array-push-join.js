// Array.prototype.push, join and toString (light-cljs-842) as resumable
// native frames: a push onto an array-like whose `length` getter returns an
// object converted by valueOf, through index and `length` setters; a join
// whose elements run toString (one of them pushing onto the array being
// joined), a getter element, an object separator, ToPrimitive of an array
// through toString and join, and an exception unwinding through the frames.
function arrayPushJoin() {
  let log = "";
  const like = { get length() { log += "l"; return { valueOf: function () { log += "v"; return 1; } }; }, set 1(v) { log += "s" + v; }, set length(v) { log += "n" + v; } };
  const pushed = Array.prototype.push.call(like, 7, 8);
  const dense = [1]; const grown = dense.push(2, 3);
  const a = [1, 2];
  a[0] = { toString: function () { a.push(3); return "x"; } };
  const joined = a.join({ toString: function () { log += "j"; return "-"; } });
  const generic = Array.prototype.join.call({ length: 2, get 0() { log += "g"; return null; }, 1: { toString: function () { return "y"; } } });
  const text = "" + [[1, 2], { toString: function () { return "z"; } }];
  let caught = 0;
  try { [1, { toString: function () { throw 9; } }].join(); } catch (e) { caught = e; }
  return pushed === 3 && like[2] === 8 && grown === 3 && dense[2] === 3 && joined === "x-2" && a.length === 3 &&
    generic === ",y" && text === "1,2,z" && caught === 9 && log === "lvs7n3jg";
}
