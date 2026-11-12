// bbp-authored conformance case (light-cljs-903).
// Derives from: rust/tests/js_source.rs lines 7241-7905 and 30630-30643;
// QuickJS js_string_isWellFormed and js_string_toWellFormed for the gap slice.
// bbp-authored conformance slice (light-cljs-903).
// Ported from rust/tests/js_source.rs: string_code_unit_access (7241),
// string_code_unit_coercion (7292), string_code_unit_method_descriptors
// (7595), string_index_of_and_last_index_of (7621),
// string_search_coercion_and_boundaries (7868), and
// string_search_predicates_symbol_match (30630); the at and predicate
// rows of string_case_trim_repeat_predicates_at_normalize (30494).
// QuickJS-derived rows retain the MIT attribution in docs/attribution.md.
// Every row is an assertion; return its count or minus the first failure.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function error(f) { try { f(); } catch (e) { return e.name; } return ''; }
row((function(){ let a = String('abc'); return  a.length === 3 && a[1] === 'b' && a.charCodeAt(1) === 0x62 && a.charAt(1) === 'b' && a.charAt(-1) === '' && a.charAt(3) === ''; })());
row(String.fromCharCode(65) === 'A' && String.fromCharCode.apply(null, [65, 66, 67]) === 'ABC');
row((function(){ let a = String.fromCharCode(0x20ac); return  a === '€' && a.charCodeAt(0) === 0x20ac; })());
row((function(){ let a = '\u{10ffff}'; return  a.codePointAt(0) === 0x10ffff && String.fromCodePoint(0x10ffff) === a && a.codePointAt(1) === 0xdfff && a.codePointAt(2) === undefined; })());
row('abc'.charCodeAt() === 0x61 && Object.is('abc'.charCodeAt(3), NaN) && 'abc'.charAt(NaN) === 'a');
row(String.fromCharCode() === '' && String.fromCodePoint() === '' && String.fromCharCode(0x10041) === 'A');
row(String.fromCharCode(0xd800).length === 1 && String.fromCharCode(0xd800) === '\ud800' && String.fromCharCode(0xd800).charCodeAt(0) === 0xd800 && String.fromCodePoint(0x41, 0x1f600).length === 3 && String.fromCodePoint(0x41, 0x1f600) === 'A\ud83d\ude00');
row('abc'.charAt(1.9) === 'b' && 'abc'.charAt(-0.9) === 'a' && 'abc'.charAt('2') === 'c' && 'abc'.charCodeAt(null) === 97 && 'abc'.codePointAt(true) === 98);
row('abc'.charAt(Infinity) === '' && 'abc'.charAt(-Infinity) === '' && Object.is('abc'.charCodeAt(-1), NaN) && 'abc'.codePointAt(-1) === undefined && 'abc'.charAt(4294967296) === '');
row(String.fromCharCode(-1, 65536, 65.9, NaN, Infinity, undefined, null, true, '66') === '\uffff\0A\0\0\0\0\u0001B');
row(String.fromCodePoint('65', null, true, -0, 0xd800) === 'A\0\u0001\0\ud800');
row('\ud800x'.codePointAt(0) === 0xd800 && '\udc00\ud800'.codePointAt(0) === 0xdc00 && '\ud800'.codePointAt(0) === 0xd800 && '\ud800\udc00'.charAt(0) === '\ud800');
row(String.prototype.charAt.call(123, 1) === '2' && String.prototype.charCodeAt.call(false, 0) === 102 && String.prototype.charAt() === '');
row(String.prototype.charAt.length === 1 && String.prototype.charCodeAt.length === 1 && String.prototype.codePointAt.length === 1 && String.fromCharCode.length === 1 && String.fromCodePoint.length === 1);
row((function(){ let ok = true; for (let i = 0; i < 65536; i++) { let s = String.fromCharCode(i & 0xffff); if (s.charCodeAt(0) !== i || s.codePointAt(0) !== i || s.charCodeAt(1) === s.charCodeAt(1)) { ok = false; break; } } return  ok; })());
row((function(){ let units = []; for (let i = 0; i < 4096; i++) { units.push(i); } let s = String.fromCharCode.apply(null, units); let ok = true; for (let i = 0; i < 4096; i++) { if (s.charCodeAt(i) !== (i & 0xffff)) { ok = false; break; } } return  ok; })());
row('abc'.charAt({valueOf: () => 1}) === 'b');
row(String.fromCharCode({valueOf: () => 65}) === 'A');
row(String.fromCodePoint({valueOf: () => 65}) === 'A');
row(String.prototype.charAt.call({toString: () => 'abc'}, 0) === 'a');
row(new String('abc').charAt(1) === 'b' && new String('abc').charCodeAt(2) === 99 && new String('😀').codePointAt(0) === 0x1f600);
row(String.fromCharCode(new Number(65)) === 'A' && String.fromCodePoint(new String('128512')) === '😀');
row((function(){ let order = ''; let receiver = {toString: () => {order += 's'; return 123; }}; let pos = {valueOf: () => {order += 'p'; return 1; }}; return  String.prototype.charAt.call(receiver, pos) === '2' && order === 'sp'; })());
row((function(){ let order = ''; let receiver = {toString: () => {order += 's'; receiver.valueOf = () => {order += 'v'; return 'abc'; }; return {}; }}; return  String.prototype.charCodeAt.call(receiver, 1) === 98 && order === 'sv'; })());
row((function(){ let calls = 0; let pos = {valueOf: () => {calls++; return 0; }}; let caught = false; try { String.prototype.charAt.call(null, pos); } catch (e) { caught = true; } return  caught && calls === 0; })());
row((function(){ let calls = 0; let later = {valueOf: () => {calls++; return 65; }}; let caught = false; try { String.fromCodePoint(0x110000, later); } catch (e) { caught = true; } return  caught && calls === 0; })());
row((function(){ let order = ''; let bad = {valueOf: () => {order += 'a'; return 1.5; }}; let later = {valueOf: () => {order += 'b'; return 65; }}; let caught = false; try { String.fromCodePoint(bad, later); } catch (e) { caught = true; } return  caught && order === 'a'; })());
row((function(){ let calls = 0; let bad = {valueOf: () => {calls++; throw 42; }}; let caught = 0; try { String.fromCharCode(bad, bad); } catch (e) { caught = e; } return  caught === 42 && calls === 1; })());
row((function(){ let bad = {toString: () => ({}), valueOf: () => ({})}; let caught = false; try { String.prototype.codePointAt.call(bad); } catch (e) { caught = true; } return  caught; })());
row((function(){ let f = String.fromCharCode.bind(null, {valueOf: () => 65}); return  f.apply(null, [66]) === 'AB' && Reflect.apply(String.fromCodePoint, null, [{valueOf: () => 0x1f600}]) === '😀'; })());
row((function(){ let receiver = {toString: String.fromCharCode.bind(null, {valueOf: () => 65})}; return  String.prototype.charAt.call(receiver, 0) === 'A'; })());
// Range checks stop before the next conversion, and all entry points agree.
for (let x of [-1, 1.5, 0x110000, NaN, Infinity, -Infinity, undefined]) row(error(() => String.fromCodePoint(x)) === 'RangeError');
for (let f of ['charAt', 'charCodeAt', 'codePointAt', 'at']) {
  row(error(() => String.prototype[f].call(null)) === 'TypeError');
  row(error(() => String.prototype[f].call(undefined)) === 'TypeError');
  row(error(() => new String.prototype[f]()) === 'TypeError');
  row(error(() => 'abc'[f](Symbol())) === 'TypeError');
  let d = Object.getOwnPropertyDescriptor(String.prototype, f);
  row(d.value.name === f && d.value.length === 1 && d.writable && !d.enumerable && d.configurable);
}
for (let f of ['fromCharCode', 'fromCodePoint', 'raw']) {
  let d = Object.getOwnPropertyDescriptor(String, f);
  row(d.value.name === f && d.value.length === 1 && d.writable && !d.enumerable && d.configurable && error(() => new String[f]()) === 'TypeError');
}
row('abc'.at(-1) === 'c' && 'abc'.at(-3) === 'a' && 'abc'.at(-4) === undefined && 'abc'.at(3) === undefined);
row('abc'.at(NaN) === 'a' && 'abc'.at(-0.9) === 'a' && 'abc'.at(Infinity) === undefined && 'abc'.at(-Infinity) === undefined);
row(String.prototype.at.call(123, 1) === '2' && new String('abc').at(-1) === 'c');
row('😀'.at(0) === '\ud83d' && '😀'.at(-1) === '\ude00');
row(error(() => String.fromCharCode(Symbol())) === 'TypeError' && error(() => String.fromCodePoint(Symbol())) === 'TypeError');
row(String.raw`a\n${1}b` === 'a\\n1b' && String.raw({raw: ['x', 'y', 'z']}, 1) === 'x1yz');
row(String.raw({raw: []}, {toString() { throw 1; }}) === '' && String.raw({raw: {length: -1}}) === '');
row(error(() => String.raw()) === 'TypeError' && error(() => String.raw({})) === 'TypeError');
row(String.raw({raw: 'abc'}, 1, 2) === 'a1b2c');
{ let log = ''; let raw = {get length() { log += 'l'; return {valueOf() { log += 'n'; return 2.9; }}; }, get 0() { log += '0'; return {toString() { log += 'a'; return 'a'; }}; }, get 1() { log += '1'; return 'b'; }}; let template = {get raw() { log += 'r'; return raw; }}; row(String.raw(template, {toString() { log += 's'; return 'S'; }}) === 'aSb' && log === 'rln0as1'); }
{ let log = ''; let raw = {length: 2, get 0() { log += '0'; return Symbol(); }, get 1() { log += '1'; return 'x'; }}; row(error(() => String.raw({raw}, {toString() { log += 's'; return 's'; }})) === 'TypeError' && log === '0'); }
{ let raw = {length: 3, 0: 'x', 1: 'y', 2: 'z'}; row(String.raw({raw}, {toString() { raw[1] = 'Y'; raw.length = 1; return 's'; }}) === 'xsYz'); }
// Direct native callbacks, conversion methods and accessors.
row(({toString: String.fromCharCode}) + '' === '');
row(({toString: String.fromCodePoint}) + '' === '');
row([65].map(String.fromCharCode)[0] === String.fromCharCode(65, 0, 65));
{ let a = [1]; a.join = String.fromCharCode; row(a.toString() === ''); }
{ let o = {}; Object.defineProperty(o, 'x', {get: String.fromCharCode}); row(o.x === ''); }
failed === 0 ? rows : -failed;
