// bbp-authored conformance case.
// Derives from: rust/tests/js_source.rs lines 7043-7110 and 12309-12470
// (`number_predicates_and_constants`, `parse_int_and_parse_float` and
// `parse_numbers_coercion_waits_yields_collects_and_migrates`: the
// conversion order, a throwing valueOf, a symbol argument; bbp-147 and
// bbp-148), with light-cljs-894's failure scenario; light-cljs-900
// (ECMAScript 7.1.1 ToPrimitive, 7.1.4 ToNumber, 7.1.17 ToString).
// Built-ins that convert their receiver or arguments do so in plan order,
// each conversion complete before the next starts, on every path that calls
// them: a call and a method call, Function.prototype.call and apply,
// Reflect.apply and construct, bound functions, spread calls, a proxy
// without an apply trap, native callbacks (map, forEach), ToPrimitive's
// methods and a getter (a built-in @@hasInstance method converting its
// argument is a serial-test row, corpus/serial/number-conversions.js: the
// Pants engine refuses it). Each row is a `row(...)` check; the
// script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function throwsType(f) { try { f(); } catch (e) { return e instanceof TypeError; } return false; }
let log = '';
const s = {toString() { log += 's'; return 'ff'; }};
const r = {valueOf() { log += 'r'; return 16; }};
function order(f) { log = ''; return f() === 255 && log === 'sr'; }
// Left to right, each conversion once, on every call path.
row(order(() => parseInt(s, r)));
row(order(() => ({p: parseInt}).p(s, r)));
row(order(() => parseInt.call(null, s, r)));
row(order(() => parseInt.apply(null, [s, r])));
row(order(() => Reflect.apply(parseInt, undefined, [s, r])));
row(order(() => parseInt.bind(null, s)(r)));
row(order(() => parseInt(...[s, r])));
row(order(() => parseInt.call.call(parseInt, null, s, r)));
row(order(() => new Proxy(parseInt, {})(s, r)));
row(order(() => Function.prototype.apply.call(parseInt, null, [s, r])));
// @@toPrimitive receives the hint of the conversion.
{ log = ''; let o = {[Symbol.toPrimitive](h) { log += h; return '7'; }}; row(parseInt(o) === 7 && isNaN(o) === false && log === 'stringnumber'); }
{ log = ''; let o = {[Symbol.toPrimitive](h) { log += h; return 1; }}; row(Number(o) === 1 && String(o) === '1' && log === 'numberstring'); }
// A throwing valueOf or toString ends the call there.
{ let caught = 0; try { isNaN({valueOf() { throw 7; }}); } catch (e) { caught = e; } row(caught === 7); }
{ let caught = 0; try { isFinite.apply(null, [{valueOf() { throw 8; }}]); } catch (e) { caught = e; } row(caught === 8); }
{ log = ''; let caught = 0; try { parseInt({toString() { log += 's'; throw 9; }}, r); } catch (e) { caught = e; } row(caught === 9 && log === 's'); }
{ let caught = 0; try { [1].map(() => 0).forEach.call([{valueOf() { throw 10; }}], isNaN); } catch (e) { caught = e; } row(caught === 10); }
// A symbol is a TypeError at its position, before any later conversion.
{ log = ''; row(throwsType(() => parseInt(Symbol('a'), r)) && log === ''); }
{ log = ''; row(throwsType(() => parseInt.call(null, {toString() { log += 's'; return Symbol(); }}, r)) && log === 's'); }
row(throwsType(() => isNaN(Symbol.iterator)) && throwsType(() => isFinite({valueOf() { return Symbol(); }})));
{ log = ''; let caught = false; try { parseFloat(Symbol()); } catch (e) { caught = e instanceof TypeError; } row(caught); }
// Native callbacks convert their arguments (light-cljs-894).
row([{toString() { return 'x'; }}].map(Symbol)[0].description === 'x');
row([{toString() { return 'k'; }}].map(Symbol.for)[0] === Symbol.for('k'));
row([{valueOf() { return NaN; }}, {valueOf() { return 1; }}, '2', 'z'].map(isNaN).join() === 'true,false,false,true');
row([{toString() { return '12px'; }}, '0x10'].map(parseFloat).join() === '12,0');
{ let seen = []; [{valueOf() { return 5; }}].forEach(function (v) { seen.push(isFinite(v)); }); row(seen[0] === true); }
row([{valueOf() { return 4; }}].map(Number)[0] === 4 && [{toString() { return 'q'; }}].map(String)[0] === 'q');
// map passes (element, index, array): parseInt takes the index as radix.
row(['10', '10', '10'].map(parseInt).join() === '10,NaN,2');
row([{toString() { return '10'; }}, {toString() { return '10'; }}].map(parseInt).join() === '10,NaN');
// ToPrimitive's methods and a getter.
row(({valueOf: parseFloat, toString: () => 'z'}) * 1 !== ({valueOf: parseFloat, toString: () => 'z'}) * 1);
row(({toString: isNaN}) + '' === 'true');
{ let o = {}; Object.defineProperty(o, 'x', {get: isNaN}); row(o.x === true); }
// Number(o), String(o), Symbol(o) and their constructions.
{ log = ''; let n = new Number({valueOf() { log += 'v'; return 3; }}); row(typeof n === 'object' && n.valueOf() === 3 && log === 'v'); }
{ log = ''; let w = Reflect.construct(String, [{toString() { log += 't'; return 'w'; }}]); row(w.valueOf() === 'w' && w.length === 1 && log === 't'); }
{ log = ''; class N extends Number { constructor(v) { super(v); } } let n = new N({valueOf() { log += 'v'; return 6; }}); row(n instanceof N && n + 1 === 7 && log === 'v'); }
{ log = ''; let n = new Number(...[{valueOf() { log += 'v'; return 2; }}]); row(n.valueOf() === 2 && log === 'v'); }
{ log = ''; let n = new (Number.bind(null, {valueOf() { log += 'v'; return 9; }}))(); row(n.valueOf() === 9 && log === 'v'); }
{ log = ''; row(throwsType(() => new Symbol({toString() { log += 't'; return 'x'; }})) && log === ''); }
{ log = ''; row(throwsType(() => new parseInt({toString() { log += 't'; return '1'; }})) && throwsType(() => Reflect.construct(isNaN, [{valueOf() { log += 'v'; return 1; }}])) && log === ''); }
row(Symbol({toString() { return 'd'; }}).description === 'd' && Symbol.for({toString() { return 'reg'; }}) === Symbol.for('reg'));
row(String(Symbol('s')) === 'Symbol(s)' && throwsType(() => String({toString() { return Symbol(); }})));
// Number.prototype.toString: thisNumberValue before its radix converts.
{ let n = 0; row(throwsType(() => Number.prototype.toString.call('x', {valueOf() { n++; return 10; }})) && n === 0); }
{ let n = 0; row((255).toString({valueOf() { n++; return 10; }}) === '255' && n === 1 && new Number(7).toString({valueOf() { return 10; }}) === '7'); }
row((12).toString.call(new Number(12), {[Symbol.toPrimitive]() { return '10'; }}) === '12');
failed === 0 ? rows : -failed;
