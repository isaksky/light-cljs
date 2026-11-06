// Index keys of ordinary objects (light-cljs-804): data properties keyed by
// array indices, by number or by string, read, overwritten, deleted, found
// through a prototype, and listed before names in ascending order; an index
// far past the others, an index-keyed accessor, a setter inherited under an
// index, and names that only look like indices ("01", "4294967295").
function indexKeys() {
  const o = {};
  o[2] = "two"; o.name = "n"; o[0] = "zero"; o["1"] = "one"; o[10] = "ten";
  let order = "";
  for (const k in o) order += k + ",";
  const reads = o[0] === "zero" && o["2"] === "two" && o[1] === "one" && o[3] === undefined && 2 in o && !(3 in o) && "10" in o;
  o[0] = "ZERO";
  delete o[1];
  o[1000000] = "far"; o["4294967295"] = "max"; o["01"] = "lead";
  let later = "";
  for (const k in o) later += k + ",";
  const heir = { __proto__: o, 5: "five" };
  const inherited = heir[0] === "ZERO" && heir[5] === "five" && heir[1000000] === "far" && 2 in heir;
  const acc = { get 7() { return "seven"; }, 8: "eight" };
  const p = { set 3(value) { this.got = value; } };
  const q = { __proto__: p };
  q[3] = 9;
  const literal = { 1: "x", 0: "y", b: 1, a: 2 };
  let literalOrder = "";
  for (const k in literal) literalOrder += k + ",";
  return order === "0,1,2,10,name," && reads && later === "0,2,10,1000000,name,4294967295,01," && o[1] === undefined && inherited
    && acc[7] === "seven" && acc[8] === "eight" && q.got === 9 && q[3] === undefined && literalOrder === "0,1,b,a,";
}
