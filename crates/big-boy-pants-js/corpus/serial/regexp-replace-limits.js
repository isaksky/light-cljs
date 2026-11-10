function regexpReplaceLimits(mode) {
  // Replacement's limits and ownership on Goose (light-cljs-934;
  // serial_tests.goose regexp_replace_limits runs each mode once):
  //   0  the Pants engine's per-operation allowance (MAX_REPLACE_WORK;
  //      string_replace_limits_and_budgeted_scans,
  //      string_replace_capture_storage_limit and regexp_symbol_limits in
  //      rust/tests/js_source.rs): past it a catchable RangeError, where
  //      QuickJS keeps going: a template's output, a global search's
  //      collected groups (before any replacer runs), a template's
  //      unterminated `$<` scans, a result's capture count; overlapping
  //      captures past a search's own allowance are its RangeError
  //      (string_replace_overlapping_capture_output_is_bounded); within
  //      the allowance replacements complete;
  //   1  a replacement whose output takes the job's remaining work, which
  //      serial_tests makes smaller than the allowance, past it: WorkLimit,
  //      which JavaScript cannot catch;
  //   2  RegExp objects a later job does not own (an image's rows): a
  //      global one's lastIndex reset faults NativeBoundary, a plain one
  //      replaces;
  //   3  a search string's scan past the job's remaining work (about 900
  //      million comparisons unbounded): WorkLimit within the work left.
  // Mode 0 also checks that a replacement's units are spent once (a
  // string template's and a replacer's result's, Sol's review of
  // light-cljs-934), that a template's numbered captures run the allowance
  // out before a later `$<name>` is read, and long scans.
  function repeat(s, n) {
    let out = '', piece = s;
    while (n > 0) { if (n % 2 === 1) out += piece; piece += piece; n = Math.floor(n / 2); }
    return out;
  }
  if (mode === 1) {
    const s = repeat('a', 2048);
    try { return s.replaceAll('', s).length; } catch (e) { return 'caught'; }
  }
  if (mode === 2) return { g: /a/g, plain: /(a)/, run(r, s) { return s.replace(r, '[$1]'); } };
  if (mode === 3) {
    try { return repeat('a', 60000).replace(repeat('a', 29999) + 'b', 'x'); } catch (e) { return 'caught'; }
  }
  const isWork = (e) => e instanceof RangeError && e.message === 'string replacement work limit exceeded';
  const isLimit = (e) => e instanceof RangeError && e.message === 'regular expression step limit exceeded';
  let work = 0, calls = 0, steps = 0;
  const wide = repeat('a', 2048);
  try { wide.replaceAll('', wide); } catch (e) { if (isWork(e)) work++; }
  try { wide.replace(new RegExp(repeat('()', 1000), 'g'), () => { calls++; return ''; }); } catch (e) { if (isWork(e)) work++; }
  try { wide.replaceAll(new RegExp(repeat('()', 1000), 'g'), ''); } catch (e) { if (isWork(e)) work++; }
  try { 'a'.replace(/(?<x>a)/, repeat('$<', 2000)); } catch (e) { if (isWork(e)) work++; }
  try { RegExp.prototype[Symbol.replace].call({ flags: '', exec() { return { 0: 'a', length: 1000002, index: 0 }; } }, 'a', ''); } catch (e) { if (isWork(e)) work++; }
  const long = repeat('x', 2000);
  const throwing = { flags: '', exec() { return { 0: 'a', 1: long, length: 2, index: 0, groups: { get x() { throw 42; } } }; } };
  try { RegExp.prototype[Symbol.replace].call(throwing, 'a', repeat('$1', 600) + '$<x>'); } catch (e) { if (isWork(e)) work++; }
  const thousand = repeat('a', 1000), six = repeat('x', 600);
  const once = thousand.replaceAll('a', repeat('y', 400)).length === 400000 && thousand.replaceAll('a', () => six).length === 600000;
  const trailing = (repeat('a', 65535) + 'b').replace('b', 'X') === repeat('a', 65535) + 'X' && (repeat('a', 300000) + 'b').replace('b', 'X').length === 300001;
  const nested = new RegExp(repeat('(', 128) + 'a*' + repeat(')', 128), 'g');
  const text = repeat('a', 8000);
  try { text.replace(nested, () => { calls++; return ''; }); } catch (e) { if (isLimit(e)) steps++; }
  try { text.replaceAll(nested, ''); } catch (e) { if (isLimit(e)) steps++; }
  const r = new RegExp(repeat('()', 1000), 'g');
  const small = repeat('a', 10);
  let n = 0;
  const kept = small.replace(r, () => { n++; return ''; }) === small && n === 11 && r.lastIndex === 0;
  const swapped = repeat('ab', 50000).replace(/b/g, 'c') === repeat('ac', 50000) && repeat('a-', 50000).replaceAll('-', '') === repeat('a', 50000);
  return work === 6 && calls === 0 && steps === 2 && kept && swapped && once && trailing;
}
