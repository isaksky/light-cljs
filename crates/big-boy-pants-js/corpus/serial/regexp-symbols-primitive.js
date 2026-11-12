function regexpSymbolsPrimitive() {
  // String.prototype.match, matchAll and search on a primitive regexp
  // (light-cljs-937): the symbol method is read from the primitive's
  // prototype (GetV, as V8 and the specification do; a getter there runs
  // with the primitive as `this`) and called with the primitive as `this`
  // and the receiver unconverted; the realm's own method then rejects the
  // primitive, and a null method falls back to a RegExp made from it.
  // QuickJS reads objects only and the Pants engine refuses definitions on
  // its built-in prototypes, so this runs on Goose alone
  // (serial_tests.goose, at every budget cut); Node agrees.
  let log = '';
  const receiver = { toString() { log += 'c'; return 'a7b'; } };
  const hooks = [Symbol.match, Symbol.matchAll, Symbol.search];
  for (const s of hooks) {
    Object.defineProperty(Number.prototype, s, {
      configurable: true,
      get() { 'use strict'; log += 'g' + typeof this; return function (x) { 'use strict'; log += 'h' + typeof this + (x === receiver); return this + 1; }; }
    });
  }
  const hooked = [String.prototype.match.call(receiver, 7), String.prototype.matchAll.call(receiver, 8), String.prototype.search.call(receiver, 9)].join();
  Object.defineProperty(Number.prototype, Symbol.search, { configurable: true, value: RegExp.prototype[Symbol.search] });
  let refused = '';
  try { String.prototype.search.call(receiver, 7); } catch (e) { refused = e.name; }
  Object.defineProperty(Number.prototype, Symbol.search, { configurable: true, value: null });
  const fallback = String.prototype.search.call(receiver, 7);
  for (const s of hooks) delete Number.prototype[s];
  const plain = 'a7b'.search(7) + ':' + 'a7b'.match(7).index;
  return hooked === '8,9,10' && log === 'gnumberhnumbertrue'.repeat(3) + 'c' && refused === 'TypeError' && fallback === 1 && plain === '1:1';
}
