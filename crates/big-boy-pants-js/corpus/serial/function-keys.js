// Key enumeration of functions before their own properties are made
// (light-cljs-806): for-in, object rest and spread find no enumerable key
// on a fresh ordinary function or arrow, then the added one; `length` and
// `name` read from the code row; an inherited lazy `prototype` is not an
// own property of the heir.
function functionKeys() {
  function fresh(a, b) {}
  var arrow = (x) => x;
  var seen = 0;
  for (var k in fresh) { seen += 1; }
  for (var k2 in arrow) { seen += 1; }
  var { ...rest } = fresh;
  var spread = { ...arrow };
  var empty = seen === 0 && Object.getOwnPropertyDescriptor(rest, "length") === undefined && Object.getOwnPropertyDescriptor(spread, "name") === undefined;
  var heir = { __proto__: fresh };
  var inherited = Object.getOwnPropertyDescriptor(heir, "prototype") === undefined && heir.name === "fresh" && heir.length === 2;
  fresh.tag = 1;
  var listed = "";
  for (var k3 in fresh) { listed += k3; }
  var { ...after } = fresh;
  return empty && inherited && listed === "tag" && after.tag === 1 && fresh.name === "fresh" && arrow.name === "arrow";
}
