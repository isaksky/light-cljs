// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 3771-3794
// (`for_of_builtin_iterators`, bbp-130, its rows without typed arrays) and
// lines 3708-3713 (`for_of_loops` row 2): the code points of a string with
// a surrogate pair and a lone surrogate; an array hole and an element pushed
// during the loop; an array iterator is its own @@iterator and stays done
// after its array grows; Array.prototype.values over an array-like and over
// a string; Array.prototype.values is Array.prototype[@@iterator]; the two
// @@toStringTag values; `var`, `let` and identifier heads over arrays;
// `for (let x of x)` a ReferenceError; closures over the per-iteration
// bindings; assigning a `const` binding a TypeError; iterating `true` and a
// symbol (Symbol.iterator stands for js_source.rs's `Symbol()`, since
// creating symbols is the objects area's, light-cljs-766) TypeErrors; a
// string iterator's `next` called on an array iterator a TypeError.
// The rows `Uint8Array.prototype.values === Uint8Array.prototype[Symbol.iterator]`,
// `for (let x of new Uint16Array(...))` and a detached typed array's `next`
// (and js_source.rs `for_of_borrowed_array_iterator_on_typed_array`) wait for
// the typed-array iterators of the collections area (light-cljs-769).
// Completes with
// "1211,1undefined34,true:1:true:true,5:true,a:b:true,true,a:b:true,Array Iterator:String Iterator,12:2,2,ReferenceError,23,1:3,2,TypeError,TypeError".
(function () {
  var log = "";
  function row(f) {
    var v;
    try { v = f(); } catch (e) { v = e instanceof Error ? e.name : "thrown:" + e; }
    log += (log === "" ? "" : ",") + v;
  }
  // Code points: 'A', a surrogate pair, a lone lead surrogate, 'B'.
  row(function () { let s = ''; for (let x of 'A\u{1f600}\ud800B') s += x.length; return s; });
  // A hole reads undefined; an element pushed during the loop is visited.
  row(function () { let a = [1, , 3], s = ''; for (let x of a) { s += String(x); if (x === 1) a.push(4); } return s; });
  // An array iterator is its own @@iterator; done, it stays done.
  row(function () {
    let a = [1], it = a.values();
    let self = it[Symbol.iterator]() === it, first = it.next().value, done = it.next().done;
    a.push(2);
    return self + ":" + first + ":" + done + ":" + it.next().done;
  });
  row(function () { let it = Array.prototype.values.call({0: 5, length: 1}); return it.next().value + ":" + it.next().done; });
  row(function () { let it = Array.prototype.values.call('ab'); return it.next().value + ":" + it.next().value + ":" + it.next().done; });
  row(function () { return Array.prototype.values === Array.prototype[Symbol.iterator]; });
  row(function () { let it = 'ab'[Symbol.iterator](); return (it[Symbol.iterator]() === it) && it.next().value + ":" + it.next().value + ":" + it.next().done; });
  row(function () { return [1].values()[Symbol.toStringTag] + ":" + 'a'[Symbol.iterator]()[Symbol.toStringTag]; });
  // var, let and identifier heads.
  row(function () { let s = ''; for (var x of [1, 2]) s += x; return s + ":" + x; });
  row(function () { let x = 0; for (x of [1, 2]) {} return x; });
  row(function () { for (let x of x) {} return "no error"; });
  // Each iteration's binding is its own: a closure sees its iteration's.
  row(function () { let n = 0; for (let x of [1, 2]) { let f = () => x; x += 10; n += f(); } return n; });
  row(function () { let fs = []; for (let x of [1, 2, 3]) fs.push(() => x); return fs[0]() + ":" + fs[2](); });
  row(function () { let n = 0; for (const x of [1, 2]) { try { x = 3; } catch (e) { if (e instanceof TypeError) n++; } } return n; });
  row(function () {
    let c = 0;
    try { for (let x of true) {} } catch (e) { if (e instanceof TypeError) c++; }
    try { for (let x of Symbol.iterator) {} } catch (e) { if (e instanceof TypeError) c++; }
    return c === 2 ? "TypeError" : "c=" + c;
  });
  row(function () { 'a'[Symbol.iterator]().next.call([1].values()); return "no error"; });
  return log;
})();
