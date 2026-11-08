// Mapped arguments objects (light-cljs-860), a sloppy script: parameters
// and arguments elements aliased both ways, through closures, after the
// calls return, across a generator's yields, with a duplicate parameter
// name and after deletes. Objects kept across calls stay linked at every
// cut and collection; a parameter only its closure keeps survives the
// collection that discards its arguments object (light-cljs-867), and so
// does one holding its own arguments object, and ones holding a fresh
// object, a string made at run time and another such closure.
function pair(a, b) { return [function () { return a + "," + b; }, arguments, function (x) { a = x; }]; }
var kept = [];
for (var i = 0; i < 20; i++) kept.push(pair(i, i + 1));
function* walk(a) { arguments[0] = a + 1; yield a; delete arguments[0]; a = 10; yield arguments[0]; arguments[0] = 5; yield a; }
(function () {
  for (var i = 0; i < 20; i++) { kept[i][1][1] = "b" + i; kept[i][2]("a" + i); }
  var ok = true;
  for (var j = 0; j < 20; j++) ok = ok && kept[j][0]() === "a" + j + ",b" + j && kept[j][1][0] === "a" + j;
  return ok;
})() &&
(function () { var it = walk(1); return it.next().value + ":" + it.next().value + ":" + it.next().value === "2:undefined:10"; })() &&
(function () { function f(a, a) { a = 9; arguments[0] = 7; return arguments[0] + "," + arguments[1] + "," + a; } return f(1, 2) === "7,9,9"; })() &&
(function () { function f(a, b) { arguments[1] = 3; b += 1; var s = ""; for (var k in arguments) s += arguments[k]; return s + b; } return f(1, 2) === "144"; })() &&
(function () { function f(a) { var g = function () { return a; }; delete arguments[0]; arguments[0] = 2; a = 3; return g() + "," + arguments[0]; } return f(1) === "3,2"; })() &&
(function () { function keep(a, b) { a++; arguments[0]; return function (x) { if (x !== undefined) a = x; return a; }; } var fs = []; for (var i = 0; i < 3; i++) fs.push(keep(i, [i])); fs[1](7); return fs[0]() + "," + fs[1]() + "," + fs[2]() === "1,7,3"; })() &&
(function () { function self(a) { a = arguments; return function () { return a; }; } var g = self(1); var s = g(); var same = s[0] === s; s[0] = 4; return same && g() === 4; })() &&
(function () { function hold(a, b) { if (b === 0) a = 0; arguments[0]; return function () { return a; }; } var n = 7; var g1 = hold({n: 1}, [1]), g2 = hold("s" + n + "-" + n, [2]), g3 = hold(hold({m: 2}, [3]), [4]); var o = g1(); o.n = 5; return g1() === o && g1().n === 5 && g2() === "s7-7" && g3()().m === 2; })();
