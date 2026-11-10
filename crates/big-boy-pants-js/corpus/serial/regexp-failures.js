function regexpFailures() {
  // RegExp compilation failures on Goose (light-cljs-932), ported from the
  // Pants engine's regexp_quantifier_capacity_is_a_runtime_limit and
  // regexp_compilation_limit_is_catchable (rust/tests/js_source.rs): a
  // literal the matcher compiler cannot compile fails only when evaluated,
  // a capacity failure as a catchable SyntaxError; a pattern past the
  // compilation step limit is a catchable RangeError; invalid flags are a
  // SyntaxError. Then a live lookbehind literal, which the matcher compiler
  // refuses, fails UnsupportedOperand by name: JavaScript cannot catch it.
  const dead = (false && /a{18446744073709551616}/u) === false && (false && /(?<=a)b/) === false;
  let syntax = false;
  try { /a{18446744073709551616}/u; } catch (e) { syntax = e instanceof SyntaxError; }
  let s = 'a*';
  while (s.length < 262144) s += s;
  let limit = false;
  try { new RegExp(s); } catch (e) { limit = e instanceof RangeError && e.message === 'regular expression step limit exceeded'; }
  let flags = false;
  try { new RegExp('a', 'gg'); } catch (e) { flags = e instanceof SyntaxError; }
  // Matching (light-cljs-933): a search past the Pants engine's
  // per-search allowance (MAX_MATCH_STEPS) is a catchable RangeError,
  // where QuickJS keeps going, and so is a match whose groups' code units
  // would take the allowance past it (regexp_overlapping_capture_output_is_bounded),
  // from exec and from test alike; a search within it matches.
  const isLimit = (e) => e instanceof RangeError && e.message === 'regular expression step limit exceeded';
  let thirty = 'a';
  while (thirty.length < 30) thirty += 'a';
  let steps = 0;
  try { /(a+)+b/.test(thirty); } catch (e) { if (isLimit(e)) steps++; }
  try { /(a*)*b/.exec(thirty); } catch (e) { if (isLimit(e)) steps++; }
  let open = '', close = '', text = 'a';
  for (let i = 0; i < 128; i++) { open += '('; close += ')'; }
  while (text.length < 8192) text += text;
  const nested = new RegExp(open + 'a*' + close, 'g');
  let copies = 0;
  try { nested.exec(text); } catch (e) { if (isLimit(e)) copies++; }
  // The Pants engine writes lastIndex before it charges the copies.
  const written = nested.lastIndex;
  nested.lastIndex = 0;
  try { nested.test(text); } catch (e) { if (isLimit(e)) copies++; }
  const small = new RegExp(open + 'a*' + close).exec('aaa');
  if (!(steps === 2 && copies === 2 && written === 8192 && nested.lastIndex === 8192 && small.length === 129 && small[128] === 'aaa' && /(a+)+b/.test(thirty + 'b'))) return false;
  if (!(dead && syntax && limit && flags)) return false;
  try { return /(?<=a)b/; } catch (e) { return 'caught'; }
}
