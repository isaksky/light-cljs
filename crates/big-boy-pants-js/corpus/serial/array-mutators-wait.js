function arrayMutatorsWait(dep) {
  // Array.prototype.shift across a dependency wait (light-cljs-909): `dep`
  // is a host effect the test answers with "D", called in an element getter
  // above the shift frame after one element moved down, in place or after
  // the job's whole stack was saved as a continuation and restored in
  // another heap (snapshot_tests.goose), so the frame's memos, cursor and
  // first element travel in it. Each getter and setter runs once: nothing
  // is replayed. toSpliced over a getter follows (the Pants engine lacks
  // it, so it is checked here). Completes with "D:true".
  let d = "";
  let log = "";
  const o = { length: 4, 0: "a", 1: "b", get 2() { log += "g"; d = dep(); return "c"; }, set 2(v) { log += "s" + v; }, 3: "z" };
  const first = Array.prototype.shift.call(o);
  const rest = [1, 2, 3].splice(1, 1);
  const copy = Array.prototype.toSpliced.call({ length: 3, get 0() { log += "t"; return 1; }, 1: 2, 2: 3 }, 1, 1, "y");
  return d + ":" + (copy.join() === "1,y,3" && first === "a" && o[0] === "b" && o[1] === "c" && o.length === 3 && !(3 in o) && log === "gszt" && rest[0] === 2);
}
