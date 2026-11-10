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
  if (!(dead && syntax && limit && flags)) return false;
  try { return /(?<=a)b/; } catch (e) { return 'caught'; }
}
