// Tagged templates (light-cljs-862): a site's template object is made once
// and returned every later time the site runs, from a loop, a resumed
// generator, a closure, a direct eval's function and a Function body; each
// eval compiles new sites. The objects are frozen arrays of the cooked and
// raw strings. serial_tests.goose runs it at every budget cut, and with a
// forced collection at every cut, where the only holder of a site's first
// object is the job's template cache.
function taggedTemplates() {
  "use strict";
  function tag(s) { return s; }
  function site() { return tag`k${0}v\x41`; }
  function garbage(n) { var a = []; for (var i = 0; i < n; i++) a.push({ i: i, s: "g" + i }); return a.length; }
  var ok = true;
  // The cache alone holds the first object across the garbage.
  site(); garbage(40);
  var first = site();
  ok = ok && first[0] === "k" && first[1] === "vA" && first.raw[1] === "v\\x41" && first.length === 2;
  for (var i = 0; i < 12; i++) { garbage(3); ok = ok && site() === first; }
  // Distinct sites, one in a loop.
  var seen = [];
  for (var j = 0; j < 5; j++) seen.push(tag`loop${j}`);
  ok = ok && seen[0] === seen[4] && seen[0] !== tag`loop${0}` && seen[0] !== first;
  // A generator's site across its suspensions.
  function* gen() { for (;;) yield tag`y`; }
  var it = gen(), y = it.next().value; garbage(10);
  ok = ok && it.next().value === y && it.next().value === y && y[0] === "y";
  // Closures of one function share its site.
  function make() { return function () { return tag`c`; }; }
  ok = ok && make()() === make()();
  // Each eval compiles new sites; a function made there keeps its site.
  var e1 = eval("tag`e`"), e2 = eval("tag`e`");
  ok = ok && e1 !== e2 && e1[0] === "e" && e2.raw[0] === "e";
  var h = eval("(function () { return tag`h`; })");
  garbage(10);
  ok = ok && h() === h() && h()[0] === "h";
  var f = new Function("t", "return () => t`z`")(tag);
  ok = ok && f() === f() && f() !== new Function("t", "return () => t`z`")(tag)();
  // Frozen: strict writes, deletes and pushes throw and change nothing.
  var refused = 0;
  try { first[0] = "x"; } catch (e) { refused++; }
  try { first.raw[0] = "x"; } catch (e) { refused++; }
  try { first.extra = 1; } catch (e) { refused++; }
  try { first.length = 0; } catch (e) { refused++; }
  try { delete first[1]; } catch (e) { refused++; }
  try { first.push(1); } catch (e) { refused++; }
  ok = ok && refused === 6 && first[0] === "k" && first.length === 2 && site() === first;
  // Invalid escapes cook to undefined.
  var bad = tag`\unicode${1}\u{110000}`;
  return ok && bad[0] === undefined && bad[1] === undefined && bad.raw[0] === "\\unicode" && bad.raw[1] === "\\u{110000}";
}
