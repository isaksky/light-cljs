// bbp-authored conformance case.
// Derives from: none (light-cljs-840: loops longer than the Goose VM's
// default instruction budget). A 300,000-iteration `for` and a
// `while (true)` left by `break` after 100,000 iterations. The Pants command
// line and QuickJS run scripts without an instruction budget; the
// conformance runner (src/conformance.goose) lifts the VM's default steps
// and work limits so that a legitimate loop is never cut short, and the
// harness's per-engine timeout bounds a runaway one. Completes with
// "300000,100001".
(function () {
  var n = 0;
  for (var j = 0; j < 300000; j++) n++;
  let i = 0;
  while (true) { if (++i > 1e5) break; }
  return n + "," + i;
})();
