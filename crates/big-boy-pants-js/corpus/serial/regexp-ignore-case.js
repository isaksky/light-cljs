function regexpIgnoreCase() {
  // Case-insensitive matching on Goose (light-cljs-933): until
  // Canonicalize lands (light-cljs-936), a case-insensitive instruction
  // that runs fails UnsupportedOperand by name, which JavaScript cannot
  // catch. One the search never reaches, and the assertions, which need no
  // Canonicalize (their word characters gain only U+017F and U+212A under
  // `iu`), match; then a live `i` test fails.
  const s = 'ſa';
  const unreached = /x|(?i:a)/.exec('x')[0] === 'x' && /^$/i.test('') && /\b/i.test('a') && /\B/iu.exec(s).index === 1 && /\B/i.exec(s).index === 0 && /\B/u.exec(s).index === 0;
  if (!unreached) return false;
  try { return /a/i.test('A'); } catch (e) { return 'caught'; }
}
