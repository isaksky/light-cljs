// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/src/vm/native/iterator.rs lines 286-310, 490-606
// (State::next, Stage::Length: `Kind::ArrayKeys` gives the index,
// `Kind::ArrayEntries` a new [index, value] array, LengthOfArrayLike read on
// every call) and ECMAScript 23.1.3.19 keys, 23.1.3.5 entries and 23.1.5.1
// %ArrayIteratorPrototype%.next: keys() and entries() over a dense array, a
// holey array (a hole is an index whose value reads undefined) and an
// array-like whose `length` getter is observed once per `next` (also after
// the end, while the cursor is not done, and never once it is); a
// `for (const [i, v] of a.entries())` loop; each entry a fresh array; the
// iterators' `next` is %ArrayIteratorPrototype%.next, shared with values().
// Completes with
// "012,0:a;1:b;2:c;,0123,0:5;1:undefined;2:undefined;3:8;,01:3,0:x;1:y;:3,3,0=p1=q,false,true".
(function () {
  var log = "";
  function row(f) {
    var v;
    try { v = f(); } catch (e) { v = e instanceof Error ? e.name : "thrown:" + e; }
    log += (log === "" ? "" : ",") + v;
  }
  function pairs(it) { let s = '', r; while (!(r = it.next()).done) s += r.value[0] + ":" + r.value[1] + ";"; return s; }
  // A dense array.
  row(function () { let s = ''; for (const k of ['a', 'b', 'c'].keys()) s += k; return s; });
  row(function () { return pairs(['a', 'b', 'c'].entries()); });
  // A holey array: every index, holes as undefined.
  row(function () { let s = ''; for (const k of [5, , , 8].keys()) s += k; return s; });
  row(function () { return pairs([5, , , 8].entries()); });
  // An array-like with a `length` getter: read once per `next` until done.
  row(function () {
    let reads = 0; const o = { get length() { reads++; return 2; } };
    const it = Array.prototype.keys.call(o);
    let s = '' + it.next().value + it.next().value;
    it.next(); it.next(); it.next();
    return s + ":" + reads;
  });
  row(function () {
    let reads = 0; const o = { get length() { reads++; return 2; }, 0: 'x', 1: 'y' };
    return pairs(Array.prototype.entries.call(o)) + ":" + reads;
  });
  // The length getter of a keys() iterator over a growing array-like.
  row(function () {
    let n = 1; const o = { get length() { return n; } };
    let c = 0; for (const k of Array.prototype.keys.call(o)) { c++; if (n < 3) n++; }
    return c;
  });
  // Destructuring each entry in the loop head.
  row(function () { let s = ''; for (const [i, v] of ['p', 'q'].entries()) s += (s ? '' : '') + i + '=' + v; return s; });
  // Each entry is a new array.
  row(function () { const it = [1, 1].entries(); return it.next().value === it.next().value; });
  // keys, entries and values share %ArrayIteratorPrototype%.next.
  row(function () { return [].keys().next === [].entries().next && [].entries().next === [].values().next; });
  return log;
})();
