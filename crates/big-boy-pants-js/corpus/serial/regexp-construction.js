function regexpConstruction() {
  // RegExp objects (light-cljs-932) at every budget cut and across
  // collections inside the job (serial_tests.goose): the constructor's
  // frame over a pattern object whose @@match, source and flags are
  // getters and whose conversions run JavaScript, with a newTarget whose
  // `prototype` is a getter; the flags getter's and toString's frames;
  // literals evaluated as new objects; and run-time patterns, whose
  // programs are job code rows.
  let log = '';
  const B = RegExp.bind(null);
  Object.defineProperty(B, 'prototype', { get() { log += 'p'; return RegExp.prototype; } });
  const kept = [];
  for (let i = 0; i < 40; i++) {
    const pattern = {
      get [Symbol.match]() { log += 'm'; return true; },
      get source() { log += 's'; return { toString() { log += 'x'; return 'a' + i; } }; },
      get flags() { log += 'f'; return i % 2 === 0 ? 'g' : 'iy'; }
    };
    const r = Reflect.construct(RegExp, [pattern], B);
    const junk = [new RegExp('b' + i, 'm'), /c/u, { d: RegExp(r) }];
    if (i % 10 === 0) { r.lastIndex = i; kept.push(r); }
  }
  let ok = log.length === 40 * 5;
  for (let j = 0; j < kept.length; j++) {
    const r = kept[j];
    if (r.source !== 'a' + j * 10 || r.flags !== 'g' || r.lastIndex !== j * 10 || String(r) !== '/a' + j * 10 + '/g') { ok = false; }
  }
  const get = Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get;
  const generic = get.call({ get global() { return true; }, get sticky() { return 1; } });
  const text = RegExp.prototype.toString.call({ get source() { return { toString() { return 'S'; } }; }, flags: 'F' });
  class R extends RegExp {}
  const sub = new R('z', 'dv');
  return ok && generic === 'gy' && text === '/S/F' && sub instanceof R && sub.flags === 'dv' && /a\/b/.source === 'a\\/b' && RegExp(kept[1]) === kept[1];
}
