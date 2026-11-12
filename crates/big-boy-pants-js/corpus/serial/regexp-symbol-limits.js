function regexpSymbolLimits(mode) {
  // The symbol methods' limits and ownership on Goose (light-cljs-937;
  // serial_tests.goose regexp_symbol_limits runs each mode once):
  //   0  the Pants engine's per-operation allowance (MAX_WORK in
  //      vm/native/string_regexp.rs; regexp_symbol_limits in
  //      rust/tests/js_source.rs), a unit an exec and, for @@match and
  //      @@split, a unit and a string's units for each element appended: a
  //      global match that spends 999,999 units before its last exec
  //      completes, one that spends 1,000,000 throws the catchable
  //      RangeError "RegExp symbol work limit exceeded" at that exec
  //      (before calling it), and one that would spend 1,000,001 at its
  //      last string; a split that spends exactly 1,000,000 completes, one
  //      more at its last piece or its last capture throws; QuickJS keeps
  //      going;
  //   1  a global match whose strings take the job's remaining work, which
  //      serial_tests makes smaller than the allowance, past it: WorkLimit,
  //      which JavaScript cannot catch;
  //   2  objects a later job does not own (an image's rows): a global
  //      RegExp's lastIndex reset faults NativeBoundary, a plain one
  //      matches and searches; a live RegExp String Iterator's next faults
  //      NativeBoundary, a done one answers.
  function repeat(s, n) {
    let out = '', piece = s;
    while (n > 0) { if (n % 2 === 1) out += piece; piece += piece; n = Math.floor(n / 2); }
    return out;
  }
  const match = RegExp.prototype[Symbol.match];
  const big = repeat('x', 65536);
  if (mode === 1) {
    try { return match.call({ flags: 'g', exec() { return { 0: big }; } }, 'a').length; } catch (e) { return 'caught'; }
  }
  if (mode === 2) {
    const done = 'a'.matchAll(/b/g); done.next();
    return {
      g: /a/g, plain: /(a)/, live: 'aa'.matchAll(/a/g), done: done,
      match(r, s) { const m = s.match(r); return m.index + ':' + m[1]; },
      search(r, s) { return s.search(r); },
      next(it) { return String(it.next().done); }
    };
  }
  const isWork = (e) => e instanceof RangeError && e.message === 'RegExp symbol work limit exceeded';
  // Fifteen strings of 65,536 units (65,538 units with their exec), a
  // sixteenth of `last` units, then null: 983,071 + last + 1 units before
  // the seventeenth exec, which spends one more.
  function run(last) {
    let execs = 0;
    const tail = repeat('y', last);
    const r = { flags: 'g', exec() { execs++; return execs <= 15 ? { 0: big } : execs === 16 ? { 0: tail } : null; } };
    try { return match.call(r, 'a').length + ':' + execs; } catch (e) { return (isWork(e) ? 'work' : 'other') + ':' + execs; }
  }
  // @@split spends a unit an exec and an element and a string's units: on
  // 'x', one exec (1), the empty piece before the match (1), fifteen
  // captures of 65,536 units (983,055), a sixteenth of `last` units, then
  // the empty last piece (1): 983,059 + last units.
  function split(last) {
    const tail = repeat('y', last);
    const splitter = { lastIndex: 0, exec() { this.lastIndex = 1; const z = { length: 17, 16: tail }; for (let i = 1; i < 16; i++) z[i] = big; return z; } };
    const rx = { constructor: { [Symbol.species]: function () { return splitter; } }, flags: '' };
    try { return String(RegExp.prototype[Symbol.split].call(rx, 'x').length); } catch (e) { return isWork(e) ? 'work' : 'other'; }
  }
  return run(16927) === '16:17' && run(16928) === 'work:16' && run(16929) === 'work:16' && split(16941) === '18' && split(16942) === 'work' && split(16943) === 'work';
}
