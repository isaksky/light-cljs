// bbp-authored conformance case.
// Derives from: none (light-cljs-805: an array's named properties sit beside its elements and `length`: read, overwritten, deleted,
// found by `in`, listed by for-in after the indices, and kept when a far write makes the array sparse and when its length is cut).
// Completes with "0,1,2,tag,self,|reads|0,1,2,5000,tag,self,late,|0,1,tag,self,late,".
(function () {
  var a = [1, 2, 3];
  a.tag = "t"; a.kind = 4; a.self = function () { return this.length; };
  a.tag = "u";
  delete a.kind;
  var keys = ""; for (var k in a) keys += k + ",";
  var reads = a.self() === 3 && a.tag === "u" && ("self" in a) && !("kind" in a) && a.kind === undefined ? "reads" : "wrong";
  a[5000] = "far"; a.late = "l";
  var sparse = ""; for (var s in a) sparse += s + ",";
  a.length = 2;
  var cut = ""; for (var c in a) cut += c + ",";
  return keys + "|" + reads + "|" + sparse + "|" + cut;
})();
