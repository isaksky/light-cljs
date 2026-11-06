// Arrays' named properties (light-cljs-805): beside the elements and
// `length`, read, overwritten, deleted, found by `in`, listed after the
// indices and kept when the array turns sparse, when its length is cut and
// when a delete moves it to dictionary mode; a name can hold a function that
// is called as a method.
function arrayNames() {
  var a = [1, 2, 3];
  a.tag = "t"; a["kind"] = 4; a.self = function () { return this.length; };
  var r1 = a.tag === "t" && a.kind === 4 && a.length === 3 && a[1] === 2 && a.self() === 3 && ("tag" in a) && !("nope" in a);
  var keys = ""; for (var k in a) keys += k + ",";
  a.tag = "u";
  var r2 = keys === "0,1,2,tag,kind,self," && a.tag === "u" && a.missing === undefined;
  var removed = delete a.kind;
  var after = ""; for (var j in a) after += j + ",";
  var r3 = removed && a.kind === undefined && after === "0,1,2,tag,self," && delete a.never;
  a[5000] = "far"; a.late = "l";
  var sparse = ""; for (var s in a) sparse += s + ",";
  var r4 = sparse === "0,1,2,5000,tag,self,late," && a.length === 5001 && a[5000] === "far" && a.tag === "u";
  a.length = 2;
  var cut = ""; for (var c in a) cut += c + ",";
  var r5 = cut === "0,1,tag,self,late," && a.length === 2 && a[2] === undefined && a.late === "l";
  var b = [], name = "n"; for (var i = 0; i < 70; i++) { b[name] = i; name = name + "x"; }
  b.push2 = 1; b[3] = "three";
  var r6 = b.n === 0 && b.nxxx === 3 && b.push2 === 1 && b.length === 4 && b[3] === "three";
  return r1 && r2 && r3 && r4 && r5 && r6;
}
