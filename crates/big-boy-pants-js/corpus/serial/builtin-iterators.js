// The built-in array and string iterators (light-cljs-844): for-of over an
// array-like whose `length` getter, the `length` value's valueOf (a Convert
// frame) and element getters run JavaScript above
// %ArrayIteratorPrototype%.next's native frame; keys() and entries() over
// it; an element getter that throws (its index was consumed); an array with
// a hole and an element pushed during the loop; and a string's code points
// (a surrogate pair, a lone surrogate) through String.prototype[@@iterator],
// also of a String object whose ToString runs in a Convert frame; a `next`
// call made by a length getter, which leaves the outer call's index alone;
// and arguments objects, whose @@iterator is %Array.prototype.values%.
// serial_tests.goose checks it at every budget cut and limit point.
// Completes with true.
function builtinIterators() {
  let log = "";
  const like = {
    get length() { log += "L"; return { valueOf() { log += "v"; return 3; } }; },
    get 0() { log += "g"; return "a"; },
    1: "b",
    get 2() { log += "h"; return "c"; }
  };
  let s = "";
  for (const x of Array.prototype.values.call(like)) s += x;
  const values = s === "abc" && log === "LvgLvLvhLv";
  log = "";
  let keys = "";
  for (const k of Array.prototype.keys.call(like)) keys += k;
  const keyed = keys === "012" && log === "LvLvLvLv";
  let entries = "";
  for (const [i, v] of Array.prototype.entries.call(like)) entries += i + v;
  const entered = entries === "0a1b2c";
  const throwing = { length: 2, get 0() { throw new RangeError("element"); }, 1: "z" };
  const it = Array.prototype.values.call(throwing);
  let caught = "";
  try { it.next(); } catch (e) { caught = e.name; }
  const after = it.next().value === "z" && it.next().done;
  const a = [1, , 3];
  let held = "";
  for (const x of a) { held += String(x); if (x === 1) a.push(4); }
  let points = "";
  for (const c of "A\u{1f600}\ud800B") points += c.length;
  let boxed = "";
  const text = { toString() { return "xy"; } };
  for (const c of String.prototype[Symbol.iterator].call(text)) boxed += c;
  for (const c of new String("pq")) boxed += c;
  // A `next` call inside the length getter does not move the outer call's
  // index, read when it started; nor does one that runs the cursor to its
  // end, which stays done.
  let re, once = true;
  const reentered = { 0: "a", 1: "b", get length() { if (once) { once = false; re.next(); } return { valueOf() { return 2; } }; } };
  re = Array.prototype.values.call(reentered);
  const first = re.next().value;
  const reentrant = first === "a" && re.next().value === "b" && re.next().done;
  let ex, inner = "", drained = true;
  const exhausting = { 0: "a", get length() { if (drained) { drained = false; let r; while (!(r = ex.next()).done) inner += r.value; } return 1; } };
  ex = Array.prototype.values.call(exhausting);
  const outer = ex.next();
  const exhausted = inner === "a" && outer.value === "a" && !outer.done && ex.next().done;
  // Arguments objects iterate with %Array.prototype.values%, also after
  // the function that made the cursor returned.
  function args() { let s = ""; for (const x of arguments) s += x; return s + (arguments[Symbol.iterator] === Array.prototype.values); }
  function none() { let n = 0; for (const x of arguments) n++; return n; }
  function keep() { return arguments[Symbol.iterator](); }
  const kept = keep(5, 6);
  const argued = args(1, 2) === "12true" && none() === 0 && kept.next().value === 5 && kept.next().value === 6 && kept.next().done;
  return values && keyed && entered && caught === "RangeError" && after && held === "1undefined34" && points === "1211" && boxed === "xypq" && reentrant && exhausted && argued;
}
