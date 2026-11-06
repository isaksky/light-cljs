// bbp-authored conformance case.
// Derives from: none (light-cljs-806: closures' own `length` (ExpectedArgumentCount) and `name` (SetFunctionName, inferred
// names included) for declarations, expressions, arrows, methods, accessors and defaults/rest; both configurable only,
// so for-in and object rest skip them; read without being made, then made by a write or a delete).
// Completes with "names:decl,inner,anon,arrow,method,get g,set s,|lengths:2,1,0,1,0,1,1,0|attrs:ff,t,t|own:tttf|lazy:flazy|forin:x,|rest:x|write:refused|delete:true,,f".
(function () {
  function decl(a, b) {}
  var named = function inner(a) {};
  var anon = function () {};
  var arrow = (a, b = 1, c) => a;
  var obj = { method() {}, get g() { return 1; }, set s(v) {} };
  var getter = Object.getOwnPropertyDescriptor(obj, "g").get;
  var setter = Object.getOwnPropertyDescriptor(obj, "s").set;
  var anonymous = [function () {}][0];
  function rest(a, ...more) {}
  function defaults(a, b = 2) {}
  function shape({ a, b }) {}
  var names = decl.name + "," + named.name + "," + anon.name + "," + arrow.name + "," + obj.method.name + "," +
    getter.name + "," + setter.name + "," + anonymous.name;
  function digit(n) { return n === 0 ? "0" : n === 1 ? "1" : n === 2 ? "2" : "?"; }
  var lengths = digit(decl.length) + "," + digit(named.length) + "," + digit(anon.length) + "," + digit(arrow.length) + "," +
    digit(obj.method.length) + "," + digit(rest.length) + "," + digit(defaults.length) + "," + digit((function (...all) {}).length);
  var lengths1 = shape.length;
  var d = Object.getOwnPropertyDescriptor(decl, "name");
  var l = Object.getOwnPropertyDescriptor(arrow, "length");
  var attrs = (d.writable || d.enumerable ? "t" : "f") + (l.writable || l.enumerable ? "t" : "f") + "," +
    (d.configurable && l.configurable ? "t" : "f") + "," + (d.value === "decl" && l.value === 1 ? "t" : "f");
  // Enumeration before anything is made: no enumerable key, and an heir
  // does not own its prototype's lazy `prototype`.
  function lazy(a) {}
  var lazyArrow = () => 0;
  var lazySeen = "";
  for (var lk in lazy) { lazySeen += lk; }
  for (var la in lazyArrow) { lazySeen += la; }
  var { ...lazyRest } = lazy;
  for (var lr in lazyRest) { lazySeen += lr; }
  var lazySpread = { ...lazyArrow };
  for (var ls in lazySpread) { lazySeen += ls; }
  var heir = { __proto__: lazy };
  lazySeen += has(heir, "prototype") + heir.name;
  function keyed() {}
  keyed.x = 1;
  function has(o, k) { return Object.getOwnPropertyDescriptor(o, k) !== undefined ? "t" : "f"; }
  var own = has(keyed, "length") + has(keyed, "name") + has(keyed, "prototype") + has(arrow, "prototype");
  var listed = "";
  for (var k in keyed) { listed += k + ","; }
  var { ...copied } = keyed;
  var restKeys = "";
  for (var c in copied) { restKeys += c; }
  var write = "accepted";
  try { decl.name = "other"; } catch (e) { write = e.name === "TypeError" ? "refused" : "wrong"; }
  var gone = (delete anon.name) ? "true" : "false";
  var deleted = gone + "," + anon.name + "," + has(anon, "name");
  return "names:" + names + "|lengths:" + lengths + (lengths1 === 1 ? "" : "!") + "|attrs:" + attrs + "|own:" + own +
    "|lazy:" + lazySeen + "|forin:" + listed + "|rest:" + restKeys + "|write:" + write + "|delete:" + deleted;
})();
