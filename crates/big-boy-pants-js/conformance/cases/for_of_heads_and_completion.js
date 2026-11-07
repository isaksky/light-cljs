// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 3771-3793
// (`for_of_builtin_iterators`, bbp-237: the `var` and identifier heads) and
// lines 3902-3917 (`for_of_assigns_member_targets`, light-cljs-843), with
// generators and user iterables in place of arrays (array iterators are
// light-cljs-844), each row run in its own function and its result appended
// to one log; and lines 9176-9190 (`script_completion_value_follows_javascript`)
// for the completion value. Heads: `var` (visible after the loop with the
// last value), `let`, `const`, an outer identifier, a member target and a
// computed target, each evaluated anew every iteration after `next` and
// before the body (object, key, key conversion, then the store), a target
// whose base changes in the body, a null base's TypeError closing the
// iterator, `for (let of of g())` and `for (of of g())`. The script's
// completion value is the loop body's last value: the log, which the last
// iteration yields.
// Completes with
// "12:2,12,12,2,12:2,2,no1no2n:2,okca!okcb!,1:2,TypeError:1,12,12".
var log = "";
(function () {
  function row(f) {
    var v;
    try { v = f(); } catch (e) { v = e instanceof Error ? e.name : "thrown:" + e; }
    log += (log === "" ? "" : ",") + v;
  }
  function* g() { yield 1; yield 2; }
  row(function () { var s = ""; for (var x of g()) s += x; return s + ":" + x; });
  row(function () { var s = ""; for (let x of g()) s += x; return s; });
  row(function () { var s = ""; for (const x of g()) s += x; return s; });
  row(function () { var x = 0; for (x of g()) {} return x; });
  row(function () { var a = { x: 0 }, s = ""; for (a.x of g()) s += a.x; return s + ":" + a.x; });
  row(function () { var a = [0]; for (a[0] of g()) ; return a[0]; });
  row(function () {
    var out = "", a = {};
    function obj() { out += "o"; return a; }
    var it = { n: 0, next() { out += "n"; this.n++; return { value: this.n, done: this.n > 2 }; } };
    it[Symbol.iterator] = function () { return this; };
    for (obj().k of it) out += a.k;
    return out + ":" + a.k;
  });
  row(function () {
    var out = "", a = { set k(v) { out += v; } };
    function obj() { out += "o"; return a; }
    function key() { out += "k"; return { toString() { out += "c"; return "k"; } }; }
    function* letters() { yield "a"; yield "b"; }
    for (obj()[key()] of letters()) out += "!";
    return out;
  });
  row(function () { var a = {}, b = {}, target = a; for (target.x of g()) target = b; return a.x + ":" + b.x; });
  row(function () {
    var closed = 0, caught;
    var it = { next() { return { value: 1, done: false }; }, return() { closed++; return {}; } };
    it[Symbol.iterator] = function () { return this; };
    var a = null;
    try { for (a.x of it) ; } catch (e) { caught = e.name; }
    return caught + ":" + closed;
  });
  row(function () { var s = ""; for (let of of g()) s += of; return s; });
  row(function () { var of, s = ""; for (of of g()) s += of; return s; });
})();
function* tail() { yield 1; yield log; }
5; for (const x of tail()) x;
