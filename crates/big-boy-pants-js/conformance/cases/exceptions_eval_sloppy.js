// bbp-authored conformance case (light-cljs-869), a sloppy script.
// Derives from: none (light-cljs-765's probe rows for eval code that throws,
// bbp-44): a sloppy direct eval whose code throws, caught by the caller; a
// direct eval of malformed code throwing a SyntaxError; a throw caught inside
// eval code completing with the catch body's value; a finally block inside
// eval code keeping the try block's value; a `var` declared by eval code
// before it throws staying in the caller's scope; the caller's finally block
// running when eval code throws; an indirect eval's ReferenceError.
// Completes with "5|SyntaxError|2|1|declared|f,7|ReferenceError".
(function () {
  var r = [];
  try { eval("throw 5"); } catch (e) { r.push(e); }
  try { eval("("); } catch (e) { r.push(e instanceof SyntaxError ? e.name : "other"); }
  r.push(eval("try { throw 1; } catch (e) { e + 1; }"));
  r.push(eval("try { 1; } finally { 2; }"));
  try { eval("var leaked869 = 'declared'; throw 0;"); } catch (e) { r.push(leaked869); }
  var log = [];
  try { try { eval("throw 7"); } finally { log.push("f"); } } catch (e) { log.push(e); }
  r.push(log.join(","));
  try { (0, eval)("notDefinedInEval869"); } catch (e) { r.push(e.name); }
  return r.join("|");
})();
