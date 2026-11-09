// bbp-authored conformance case (light-cljs-885).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 20453-20500 (symbol_values, bbp-127), 20758-20790 (symbol_thrown_values) and 20834-20865 (symbol_numeric_coercion_stops_before_rhs):
// Symbol() values, their identity (===, ==, Object.is), their use as
// property keys of ordinary objects, arrays and functions (own keys list
// symbols after strings in creation order; for-in and Object.keys skip
// them), the TypeErrors of converting one to a number or a string, and
// throwing and catching one, and %Symbol.prototype% changed by JavaScript
// (an @@iterator added, @@toStringTag removed or not a string). Left out: `__rt` rows and the waits of
// `__rt.need`; String.raw (pending in the core-builtins area,
// light-cljs-767). Each row is a `row(...)` check; the script completes with
// the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-766).
{ let s = Symbol('a'); row(typeof s + ',' + s.toString() === "symbol,Symbol(a)"); }
row(String(Symbol('q')) === "Symbol(q)");
{ let o = {}; let s = Symbol('k'); o[s] = 1; row(o[s] === 1); }
row(Reflect.ownKeys({ [Symbol('a')]: 1, b: 2 }).length === 2);
{ let t; try { throw Symbol('t'); } catch (e) { t = typeof e; } row(t === "symbol"); }
row(typeof Symbol.asyncIterator + ',' + String(Symbol.iterator.description) === "symbol,Symbol.iterator");
// symbol_values (js_source.rs line 20453).
row(String(Symbol()) === 'Symbol()' && String(Symbol(undefined)) === 'Symbol()' && String(Symbol('x')) === 'Symbol(x)' && String(Symbol(null)) === 'Symbol(null)' && String(Symbol(42)) === 'Symbol(42)');
row(String(Symbol({toString() { return undefined; }})) === 'Symbol(undefined)' && String(Symbol({toString() { return '\uD800'; }})) === 'Symbol(\uD800)');
{ let a = Symbol('abc'); let b = Symbol('abc'); let obj = {}; obj[a] = 2; row(obj[a] === 2 && obj['abc'] === undefined && a == a && a === a && a != b && a !== b && typeof a === 'symbol'); }
row(Symbol() !== Symbol() && Symbol('x') !== Symbol('x') && !!Symbol());
{ let s = Symbol('x'); function tag(parts, value) { return value === s; } row(tag`${s}`); }
row(Symbol.iterator === Symbol.iterator && Symbol.toPrimitive !== Symbol.hasInstance && typeof Symbol.iterator === 'symbol');
{ let s = Symbol('x'); let o = {x: 1}; o[s] = 2; row(Object.keys(o).join() === 'x' && Object.getOwnPropertyNames(o).join() === 'x'); }
{ let s = Symbol('x'); let o = {}; o[s] = 2; let names = ''; for (let k in o) { names += k; } row(names === '' && s in o && !('x' in o)); }
{ let s = Symbol(); let a = []; a[s] = 3; row(a[s]++ === 3 && a[s] === 4 && a.length === 0 && delete a[s] && !(s in a)); }
{ let s = Symbol(); function f() {} f[s] = 3; row(f[s] === 3 && Object.keys(f).length === 0 && delete f[s] && !(s in f)); }
{ let s = Symbol(); let p = {}; Object.defineProperty(p, s, {get() { return 7; }}); let o = Object.create(p); row(o[s] === 7 && s in o && Object.getOwnPropertyNames(p).length === 0); }
{ let s = Symbol(); let o = {}; Object.defineProperty(o, s, {value: 2, writable: true}); o[s]++; row(Object.getOwnPropertyDescriptor(o, s).value === 3); }
{ let s = Symbol(); row(s != 1 && s != 'x' && s != true && s != null && s != undefined && Object.is(s, s) && !Object.is(s, Symbol())); }
{ let n = 0; row(Symbol({toString() { n++; return 'k'; }}) !== Symbol('k') && n === 1); }
{ let s = Symbol(); let key = {toString() { return s; }}; let o = {}; o[key] = 5; row(o[s] === 5); }
{ let d = Object.getOwnPropertyDescriptor(Symbol, 'iterator'); row(!d.writable && !d.enumerable && !d.configurable && d.value === Symbol.iterator && Symbol.length === 0 && Symbol.name === 'Symbol'); }
row(name(() => new Symbol()) === "TypeError");
row(name(() => '' + Symbol()) === "TypeError");
row(name(() => Symbol() + 1) === "TypeError");
row(name(() => Symbol() * 1) === "TypeError");
row(name(() => Number(Symbol())) === "TypeError");
row(name(() => Symbol(Symbol())) === "TypeError");
row(name(() => new Error(Symbol())) === "TypeError");
row(name(() => `${Symbol()}`) === "TypeError");
row(name(() => String({toString() { return Symbol(); }})) === "TypeError");
row(name(() => new String(Symbol())) === "TypeError");
{ let caught = false; try { new Symbol(); } catch (e) { caught = e instanceof TypeError && e instanceof Error && Object.getPrototypeOf(e) === TypeError.prototype && e.name === 'TypeError'; } row(caught); }
// new Symbol(o) throws before it converts o.
{ let log = ''; row(name(() => new Symbol({toString() { log += 't'; return 'x'; }})) === "TypeError" && log === ''); }
// A spread call converts its object argument too.
row(Symbol(...[{toString() { return 'x'; }}]).description === 'x' && Symbol.for(...[{toString() { return 'y'; }}]) === Symbol.for('y'));
// Own keys: integer indices, then strings, then symbols, each in creation order.
{
  let s1 = Symbol('1'), s2 = Symbol('2');
  let o = {b: 1, [s2]: 2, a: 3, [s1]: 4, 0: 5};
  let k = Reflect.ownKeys(o);
  row(k.length === 5 && k[0] === '0' && k[1] === 'b' && k[2] === 'a' && k[3] === s2 && k[4] === s1);
  let syms = Object.getOwnPropertySymbols(o);
  row(syms.length === 2 && syms[0] === s2 && syms[1] === s1 && Object.keys(o).join() === '0,b,a');
}
// Spread and rest copy symbol keys.
{ let s = Symbol('s'); let o = {a: 1, [s]: 2}; let c = {...o}; let {a, ...r} = o; row(c[s] === 2 && r[s] === 2 && Object.getOwnPropertySymbols(r).length === 1 && a === 1); }
// Object.assign and Object.entries: assign copies symbols, entries skips them.
{ let s = Symbol('s'); let t = Object.assign({}, {x: 1, [s]: 2}); row(t[s] === 2 && Object.entries({x: 1, [s]: 2}).length === 1); }
// A symbol-keyed method's name is its description in brackets.
{ let s = Symbol('m'), u = Symbol(); let o = {[s]() {}, [u]() {}}; row(o[s].name === '[m]' && o[u].name === ''); }
{ let s = Symbol('g'); let o = {get [s]() { return 1; }}; row(Object.getOwnPropertyDescriptor(o, s).get.name === 'get [g]'); }
// Thrown symbols keep their identity (symbol_thrown_values, line 20758).
for (let s of [Symbol(), Symbol('boom')]) { let first = false, caught = false; try { try { throw s; } catch (e) { first = e === s; throw e; } } catch (e) { caught = e === s; } row(first && caught); }
// Numeric operators finish ToNumber on the left before converting the
// right (symbol_numeric_coercion_stops_before_rhs, line 20834).
for (let op of ['-', '*', '/', '%', '**', '<<', '>>', '>>>', '&', '|', '^']) {
  let s = Symbol(), log = '';
  let f = Function('s', 'log', 'r', 'return s ' + op + ' r;');
  row(name(() => f(s, '', {valueOf() { log += 'r'; return 1; }})) === "TypeError" && log === '');
  row(name(() => f({valueOf() { log += 'l'; return s; }}, '', {valueOf() { throw 42; }})) === "TypeError" && log === 'l');
}
// Plus and the relational operators convert both operands first.
for (let src of ['s + r', 's < r', 's > r', 's <= r', 's >= r']) {
  let log = '', s = Symbol(), r = {valueOf() { log += 'r'; return 1; }};
  let f = Function('s', 'r', 'return ' + src + ';');
  row(name(() => f(s, r)) === "TypeError" && log === 'r');
}
// %Symbol.prototype% is a realm object JavaScript may change (the Pants
// engine refuses writes to built-in objects, so these rows are here): a
// symbol is iterable once it gains an @@iterator, a method or a getter.
row((() => { try { for (let x of Symbol()) {} return false; } catch (e) { return e instanceof TypeError; } })());
{ Symbol.prototype[Symbol.iterator] = function () { return [7][Symbol.iterator](); }; let a = [...Symbol()]; delete Symbol.prototype[Symbol.iterator]; row(a.length === 1 && a[0] === 7); }
{ Object.defineProperty(Symbol.prototype, Symbol.iterator, {get() { return function () { return [8, 9][Symbol.iterator](); }; }, configurable: true}); let a = [...Symbol()]; delete Symbol.prototype[Symbol.iterator]; row(a.join() === '8,9'); }
// Without a string @@toStringTag, a symbol and its wrapper are "[object Object]".
{
  let d = Object.getOwnPropertyDescriptor(Symbol.prototype, Symbol.toStringTag);
  delete Symbol.prototype[Symbol.toStringTag];
  row(Object.prototype.toString.call(Symbol()) === '[object Object]' && Object.prototype.toString.call(Object(Symbol())) === '[object Object]');
  Object.defineProperty(Symbol.prototype, Symbol.toStringTag, {value: 7, configurable: true});
  row(Object.prototype.toString.call(Symbol()) === '[object Object]');
  Object.defineProperty(Symbol.prototype, Symbol.toStringTag, d);
  row(Object.prototype.toString.call(Symbol()) === '[object Symbol]');
}
failed === 0 ? rows : -failed;
