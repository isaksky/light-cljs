function regexpBoundaries(mode) {
  // RegExp matching's limits and ownership on Goose (light-cljs-933;
  // serial_tests.goose regexp_boundaries runs each mode once):
  //   0  a search past the job's remaining work, which serial_tests makes
  //      smaller than the search's allowance: WorkLimit, which JavaScript
  //      cannot catch, with lastIndex (1) unwritten;
  //   1  a match whose groups' code units take the job's remaining work
  //      past it: WorkLimit after lastIndex was written (8192);
  //   2  RegExp objects a later job does not own (an image's rows): a
  //      global or sticky one's lastIndex write faults NativeBoundary,
  //      in the template's layout and in a dictionary one, a plain one
  //      matches;
  //   3  the d flag's reserve (regexp_indices_limit_is_catchable,
  //      rust/tests/js_source.rs): a match that fits the allowance alone
  //      is a RangeError with d; and many distinct group names.
  function repeat(s, n) {
    let out = '', piece = s;
    while (n > 0) { if (n % 2 === 1) out += piece; piece += piece; n = Math.floor(n / 2); }
    return out;
  }
  if (mode === 0) {
    const r = /(a+)+b/g;
    r.lastIndex = 1;
    try { return r.test(repeat('a', 25)); } catch (e) { return 'caught'; }
  }
  if (mode === 1) {
    const r = new RegExp(repeat('(', 128) + 'a*' + repeat(')', 128), 'g');
    try { return r.exec(repeat('a', 8192)).length; } catch (e) { return 'caught'; }
  }
  if (mode === 2) {
    const dict = /a/g;
    for (let i = 0; i < 70; i++) dict['p' + i] = i;
    return { g: /a/g, y: /a/y, plain: /(a)/, dict: dict, run(r, s) { return r.exec(s); } };
  }
  const isLimit = (e) => e instanceof RangeError && e.message === 'regular expression step limit exceeded';
  const groups = repeat('()', 40000);
  let ok = true;
  for (const length of [100000, 120000]) {
    const pattern = '^' + groups + 'a{' + length + '}$';
    const text = repeat('a', length);
    let ordinary, ordinaryCaught = false;
    try { ordinary = RegExp(pattern).exec(text); } catch (e) { ordinaryCaught = isLimit(e); }
    let caught = false;
    try { RegExp(pattern, 'd').exec(text); } catch (e) { caught = isLimit(e); }
    if (!((length === 120000 ? ordinaryCaught : !ordinaryCaught && ordinary.length === 40001 && ordinary[0] === text) && caught)) ok = false;
  }
  const names = [];
  for (let i = 0; i < 5000; i++) names.push('(?<n' + i + '>)');
  const m = new RegExp(names.join('') + '(?:(?<d>a)|(?<d>b))').exec('b');
  return ok && Object.keys(m.groups).length === 5001 && m.groups.n4999 === '' && m.groups.d === 'b' && m[5001] === undefined && m[5002] === 'b';
}
