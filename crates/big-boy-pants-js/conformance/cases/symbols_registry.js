// bbp-authored conformance case (light-cljs-885).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 20666-20700 (symbol_registry, bbp-128) and 20502-20531 (dispose_well_known_symbols):
// Symbol.for and Symbol.keyFor: one registered symbol per key string,
// whose key is its ToString (a toString or valueOf runs first), keyFor of
// an unregistered or well-known symbol is undefined and of a non-symbol a
// TypeError, the built-ins' descriptors, and the well-known symbols'
// descriptions and descriptors. Left out: the waits of `__rt.need`. Each
// row is a `row(...)` check; the script completes with the number of rows,
// or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-766).
row(Symbol.for('a') === Symbol.for('a'));
row(Symbol.keyFor(Symbol.iterator) === undefined);
// symbol_registry (js_source.rs line 20666).
{ let a = Symbol('abc'); let b = Symbol.for('abc'); let c = Symbol.for('abc'); row(b === c && b !== a && Symbol.keyFor(b) === 'abc' && Symbol.keyFor(a) === undefined); }
row(Symbol.for(1) === Symbol.for('1') && Symbol.for() === Symbol.for('undefined') && Symbol.for(null) === Symbol.for('null'));
row(Symbol.for('a' + 'bc') === Symbol.for('abc') && Symbol.for('') !== Symbol.for('undefined') && Symbol.keyFor(Symbol.for('')) === '');
row(Symbol.keyFor(Symbol.iterator) === undefined && Symbol.keyFor(Symbol.toPrimitive) === undefined);
// String code units are preserved in registry keys (light-cljs-903).
{ let hi = String.fromCharCode(0xD800); row(Symbol.for(hi + '') === Symbol.for('\uD800')); }
row(Symbol.keyFor(Symbol.for(String.fromCharCode(0xD800))).charCodeAt(0) === 0xD800);
row(Symbol.for('\uD800') !== Symbol.for('�') && Symbol.keyFor(Symbol.for('\uD800')) === '\uD800' && Symbol.keyFor(Symbol.for('\uD800')).length === 1);
{ let n = 0; row(Symbol.for({toString() { n++; return 'abc'; }}) === Symbol.for('abc') && n === 1); }
row(Symbol.for({toString() { return undefined; }}) === Symbol.for('undefined'));
{ let log = ''; let s = Symbol.for({toString() { log += 's'; return {}; }, valueOf() { log += 'v'; return 12; }}); row(s === Symbol.for('12') && log === 'sv'); }
row(Symbol.for.call(null, 'abc') === Symbol.for('abc') && Symbol.keyFor.call(null, Symbol.for('abc')) === 'abc');
row(Symbol.for.length === 1 && Symbol.for.name === 'for' && Symbol.keyFor.length === 1 && Symbol.keyFor.name === 'keyFor');
{ let d = Object.getOwnPropertyDescriptor(Symbol, 'for'); row(d.value === Symbol.for && d.writable && !d.enumerable && d.configurable); }
{ let d = Object.getOwnPropertyDescriptor(Symbol, 'keyFor'); row(d.value === Symbol.keyFor && d.writable && !d.enumerable && d.configurable); }
{ let caught = false; try { Symbol.keyFor(1); } catch (e) { caught = true; } row(caught); }
{ let caught = false; try { Symbol.for({toString() { throw 42; }}); } catch (e) { caught = e === 42; } row(caught); }
row(name(() => Symbol.keyFor(1)) === "TypeError");
row(name(() => Symbol.keyFor()) === "TypeError");
row(name(() => Symbol.keyFor('abc')) === "TypeError");
row(name(() => Symbol.keyFor({toString() { throw 42; }})) === "TypeError");
row(name(() => Symbol.for(Symbol())) === "TypeError");
row(name(() => Symbol.for({toString() { return Symbol(); }})) === "TypeError");
row(name(() => new Symbol.for('abc')) === "TypeError");
row(name(() => new Symbol.keyFor(Symbol.for('abc'))) === "TypeError");
// A registered symbol is a property key like any other, and many keys
// stay distinct.
{ let o = {}; o[Symbol.for('k')] = 1; row(o[Symbol.for('k')] === 1 && Object.getOwnPropertySymbols(o)[0] === Symbol.for('k')); }
{ let a = []; for (let i = 0; i < 50; i++) { a.push(Symbol.for('key' + i)); } let ok = true; for (let i = 0; i < 50; i++) { if (Symbol.for('key' + i) !== a[i] || Symbol.keyFor(a[i]) !== 'key' + i) { ok = false; } } row(ok); }
// dispose_well_known_symbols (js_source.rs line 20502).
row(typeof Symbol.dispose === 'symbol' && typeof Symbol.asyncDispose === 'symbol' && Symbol.dispose !== Symbol.asyncDispose);
row(Symbol.dispose.description === 'Symbol.dispose' && String(Symbol.dispose) === 'Symbol(Symbol.dispose)' && Symbol.dispose.toString() === 'Symbol(Symbol.dispose)');
row(Symbol.asyncDispose.description === 'Symbol.asyncDispose' && String(Symbol.asyncDispose) === 'Symbol(Symbol.asyncDispose)' && Symbol.asyncDispose.toString() === 'Symbol(Symbol.asyncDispose)');
row(Symbol.for('Symbol.dispose') !== Symbol.dispose && Symbol.for('Symbol.asyncDispose') !== Symbol.asyncDispose && Symbol.keyFor(Symbol.dispose) === undefined && Symbol.keyFor(Symbol.asyncDispose) === undefined);
for (let n of ['dispose', 'asyncDispose']) {
  let d = Object.getOwnPropertyDescriptor(Symbol, n);
  row(d.value === Symbol[n] && !d.writable && !d.enumerable && !d.configurable);
  let s = Symbol[n];
  row(name(() => { Symbol[n] = 1; }) === "TypeError" && Symbol[n] === s);
  row(name(() => { delete Symbol[n]; }) === "TypeError" && typeof Symbol[n] === 'symbol');
}
failed === 0 ? rows : -failed;
