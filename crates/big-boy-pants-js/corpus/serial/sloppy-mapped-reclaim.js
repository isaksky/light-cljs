// Mapped parameters a closure keeps after its call returns (light-cljs-867),
// a sloppy script. Each call of keep gets a mapped arguments object that
// also holds a fresh 4,000-element array; once the call returns only the
// closures' parameter cells remain, so in-job reclamation detaches each
// cell (it keeps the parameter's value) and discards the object, its map
// and the array. The 300 arrays would pass the Goose job's 1,048,576-value
// limit if the cells kept their objects; serial_tests.goose runs this
// without an instruction budget and checks that it completes after
// collections. Every tenth call goes through both, which also returns its
// arguments object, and self, which assigns its arguments object to its
// parameter: those cells stay aliased across the collections. The loops
// run in a function, so no global write is logged.
function keep(a, unused) { a++; arguments[0]; return [function () { return a; }, function (x) { a = x; }]; }
function both(a, unused) { return [function () { return a; }, arguments]; }
function self(a) { a = arguments; return function () { return a; }; }
(function () {
  var kept = [], linked = [], selves = [];
  for (var i = 0; i < 300; i++) {
    var big = [];
    for (var j = 0; j < 4000; j++) big.push(j);
    if (i % 10 == 0) { linked.push(both(i, big)); selves.push(self(i)); } else kept.push(keep(i, big));
  }
  var ok = kept.length === 270 && linked.length === 30;
  for (var k = 0; k < kept.length; k++) {
    var n = k + (k - k % 9) / 9 + 1;
    ok = ok && kept[k][0]() === n + 1;
    kept[k][1]("v" + k);
    ok = ok && kept[k][0]() === "v" + k;
  }
  for (var m = 0; m < linked.length; m++) {
    ok = ok && linked[m][0]() === m * 10 && linked[m][1][0] === m * 10 && linked[m][1][1].length === 4000;
    linked[m][1][0] = "w" + m;
    ok = ok && linked[m][0]() === "w" + m;
    var s = selves[m]();
    ok = ok && s[0] === s;
    s[0] = m;
    ok = ok && selves[m]() === m;
  }
  return ok;
})();
