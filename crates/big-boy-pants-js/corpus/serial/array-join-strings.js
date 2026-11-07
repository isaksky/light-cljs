// Array.prototype.join of an array and separator serial_tests.goose supplies
// (light-cljs-842): long Latin-1 and UTF-16 strings, whose code units join
// charges to the work limit and checks against the unit limit. Only Goose
// runs it with those arguments.
function joinAll(a, s) { return a.join(s); }
