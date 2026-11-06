// bbp-authored conformance case.
// Derives from: none (light-cljs-804: ordinary objects' index-keyed data
// properties, by number or by string, are read, overwritten, deleted, found
// through a prototype and listed before names in ascending order; a far
// index joins them, names that only look like indices ("4294967295", "01")
// follow the other names; an index-keyed getter and an inherited
// index-keyed setter run). Completes with
// "0,1,2,10,name,|0,2,10,1000000,name,4294967295,01,|ZERO five seven nine".
(function () {
  var o = {};
  o[2] = "two"; o.name = "n"; o[0] = "zero"; o["1"] = "one"; o[10] = "ten";
  var order = ""; for (var k in o) order += k + ",";
  o[0] = "ZERO";
  delete o[1];
  o[1000000] = "far"; o["4294967295"] = "max"; o["01"] = "lead";
  var later = ""; for (var j in o) later += j + ",";
  var heir = { __proto__: o, 5: "five" };
  var acc = { get 7() { return "seven"; } };
  var p = { set 3(value) { this.got = value; } };
  var q = { __proto__: p };
  q[3] = "nine";
  return order + "|" + later + "|" + heir[0] + " " + heir[5] + " " + acc[7] + " " + q.got;
})();
