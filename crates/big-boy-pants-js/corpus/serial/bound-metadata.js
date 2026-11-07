// bind's `length` and `name` reads (light-cljs-852) from targets
// serial_tests.goose installs on the global object, as
// Object.defineProperty would, which Goose does not run yet: each is a
// function `target`, then `getters`' own `length` and `name` are
// accessors with lenGet and nameGet as getters, `throwing`'s `length` an
// accessor with lenThrow, and `odd`, `wide`, `fraction`, `nan` and
// `negative` have the own `length` "3", Infinity, 4.8, NaN and -Infinity,
// and `odd` the own `name` 7. bind reads `length` (HasOwnProperty, then
// Get, a getter running above its frame) before `name`; a non-number
// `length` gives 0, a non-string `name` "bound ", and a getter's exception
// propagates. Completes with true.
var enabled;
if (enabled) {
  globalThis.unused = [function target(a, b) {}, function lenGet() { log += "l"; return 5.5; }, function nameGet() { log += "n"; return "got"; }, function lenThrow() { throw "len"; }];
}
var log = "";
var g = getters.bind(null, 1);
var o = odd.bind(null, 1);
var caught = "";
try { throwing.bind(); } catch (e) { caught = e; }
g.length === 4 && g.name === "bound got" && log === "ln" && o.length === 0 && o.name === "bound " &&
  wide.bind(null, 1).length === Infinity && fraction.bind(null, 1).length === 3 && fraction.bind(null, 1, 2, 3, 4, 5).length === 0 &&
  nan.bind().length === 0 && negative.bind().length === 0 && caught === "len";
