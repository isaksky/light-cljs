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
row("abcabc".indexOf("cab") === 2);
row("abcabc".indexOf("cab2") === -1);
row("abc".indexOf("c") === 2);
row("aaa".indexOf("a") === 0);
row("aaa".indexOf("a", NaN) === 0);
row("aaa".indexOf("a", -Infinity) === 0);
row("aaa".indexOf("a", -1) === 0);
row("aaa".indexOf("a", -0) === 0);
row("aaa".indexOf("a", 0) === 0);
row("aaa".indexOf("a", 1) === 1);
row("aaa".indexOf("a", 2) === 2);
row("aaa".indexOf("a", 3) === -1);
row("aaa".indexOf("a", 4) === -1);
row("aaa".indexOf("a", Infinity) === -1);
row("aaa".indexOf("") === 0);
row("aaa".indexOf("", NaN) === 0);
row("aaa".indexOf("", -Infinity) === 0);
row("aaa".indexOf("", -1) === 0);
row("aaa".indexOf("", -0) === 0);
row("aaa".indexOf("", 0) === 0);
row("aaa".indexOf("", 1) === 1);
row("aaa".indexOf("", 2) === 2);
row("aaa".indexOf("", 3) === 3);
row("aaa".indexOf("", 4) === 3);
row("aaa".indexOf("", Infinity) === 3);
row("aaa".lastIndexOf("a") === 2);
row("aaa".lastIndexOf("a", NaN) === 2);
row("aaa".lastIndexOf("a", -Infinity) === 0);
row("aaa".lastIndexOf("a", -1) === 0);
row("aaa".lastIndexOf("a", -0) === 0);
row("aaa".lastIndexOf("a", 0) === 0);
row("aaa".lastIndexOf("a", 1) === 1);
row("aaa".lastIndexOf("a", 2) === 2);
row("aaa".lastIndexOf("a", 3) === 2);
row("aaa".lastIndexOf("a", 4) === 2);
row("aaa".lastIndexOf("a", Infinity) === 2);
row("aaa".lastIndexOf("") === 3);
row("aaa".lastIndexOf("", NaN) === 3);
row("aaa".lastIndexOf("", -Infinity) === 0);
row("aaa".lastIndexOf("", -1) === 0);
row("aaa".lastIndexOf("", -0) === 0);
row("aaa".lastIndexOf("", 0) === 0);
row("aaa".lastIndexOf("", 1) === 1);
row("aaa".lastIndexOf("", 2) === 2);
row("aaa".lastIndexOf("", 3) === 3);
row("aaa".lastIndexOf("", 4) === 3);
row("aaa".lastIndexOf("", Infinity) === 3);
row("a\u{10000}b".indexOf("b") === 3);
row("x".indexOf() === -1);
row('abcabc'.indexOf('abc', 1.9) === 3 && 'abcabc'.lastIndexOf('abc', 2.9) === 0);
row('abcabc'.lastIndexOf('abc', 3.9) === 3 && 'aaa'.lastIndexOf('aa', 1) === 1);
row('abc'.indexOf('abcd') === -1 && 'abc'.lastIndexOf('abcd') === -1);
row(''.indexOf('') === 0 && ''.lastIndexOf('') === 0 && ''.lastIndexOf('x') === -1);
row('abc'.indexOf('', -0.9) === 0 && 'abc'.lastIndexOf('', -0.9) === 0);
row('abc'.indexOf('b', '1.9') === 1 && 'abc'.lastIndexOf('', undefined) === 3);
row('abc'.lastIndexOf('b', null) === -1 && 'abc'.indexOf('a', false) === 0);
row('undefined'.indexOf() === 0 && 'undefined'.lastIndexOf() === 0);
row('a\u{10000}b'.lastIndexOf('b') === 3 && '\ud800x\ud800'.indexOf('\ud800', 1) === 2 && '\ud800x\ud800'.lastIndexOf('\ud800', 1) === 0);
row('\ud800\udc00'.indexOf('\udc00') === 1 && '\ud800\udc00'.lastIndexOf('\ud800') === 0);
row(String.prototype.indexOf.call(123, 2) === 1 && String.prototype.lastIndexOf.call(false, 'a') === 1);
row(new String('abcabc').indexOf(new String('abc'), new Number(1)) === 3 && new String('abcabc').lastIndexOf(new String('abc')) === 3);
row((function(){ let s = new String('abc'); s.toString = () => 'xyz'; return  s.indexOf('y') === 1; })());
row((function(){ let r = {toString: () => 'abc'}; return  String.prototype.indexOf.call(r, {toString: () => 'b'}, {valueOf: () => 1}) === 1; })());
row((function(){ let order = ''; let r = {toString: () => {order += 'r'; return 'abc'; }}; let s = {toString: () => {order += 's'; return 'b'; }, valueOf: () => {throw 1; }}; let p = {valueOf: () => {order += 'p'; return NaN; }, toString: () => {throw 2; }}; return  String.prototype.lastIndexOf.call(r, s, p) === 1 && order === 'rsp'; })());
row((function(){ let n = 0; let bad = {toString: () => {n++; throw 42; }}; let p = {valueOf: () => {n += 10; return 0; }}; let caught = 0; try { 'abc'.indexOf(bad, p); } catch (e) { caught = e; } return  caught === 42 && n === 1; })());
row((function(){ let n = 0; let s = {toString: () => {n++; return 'x'; }}; let caught = false; try { String.prototype.lastIndexOf.call(undefined, s); } catch (e) { caught = true; } return  caught && n === 0; })());
row((function(){ let bad = {toString: () => ({}), valueOf: () => ({})}; let caught = false; try { 'x'.lastIndexOf(bad); } catch (e) { caught = true; } return  caught; })());
row((function(){ let s = {toString: String.prototype.indexOf.bind('abc', {toString: () => 'b'})}; return  '1'.indexOf(s) === 0; })());
row((function(){ let f = String.prototype.indexOf.bind('abcabc', {toString: () => 'abc'}); return  f.apply(null, [{valueOf: () => 1}]) === 3; })());
row((function(){ let x = {[Symbol.match]:true, toString(){throw 77;}}; let caught=false; try {'a'.includes(x);} catch(e){caught=e instanceof TypeError;} return  caught; })());
row((function(){ let x=/a/; x[Symbol.match]=false; return  '/a/'.startsWith(x) && '/a/'.endsWith(x) && '/a/'.includes(x); })());
row((function(){ let log=''; let x={get [Symbol.match](){log+='m';return false;},toString(){log+='s';return 'a';}}; return  'a'.startsWith(x,{valueOf(){log+='p';return 0;}}) && log==='msp'; })());
row((function(){ let log=''; let x=new Proxy({get [Symbol.match](){log+='m';return false;},toString(){log+='s';return 'a';}}, {}); return  'a'.includes(x) && log==='ms'; })());
row((function(){ let x={get [Symbol.match](){throw 77;},toString(){throw 88;}}; let caught; try {'a'.endsWith(x);}catch(e){caught=e;} return  caught===77; })());
for (let f of ['indexOf', 'lastIndexOf', 'includes', 'startsWith', 'endsWith']) {
  let d = Object.getOwnPropertyDescriptor(String.prototype, f);
  row(d.value.name === f && d.value.length === 1 && d.writable && !d.enumerable && d.configurable);
  row(error(() => String.prototype[f].call(null, '')) === 'TypeError' && error(() => new String.prototype[f]()) === 'TypeError');
  row(error(() => 'abc'[f](Symbol())) === 'TypeError' && error(() => 'abc'[f]('', Symbol())) === 'TypeError');
}
for (let f of ['includes', 'startsWith', 'endsWith']) {
  row(error(() => 'abc'[f](/b/)) === 'TypeError');
  row(error(() => 'abc'[f]({[Symbol.match]: true, toString() { throw 7; }})) === 'TypeError');
  row('abc'[f]({[Symbol.match]: false, toString() { return ''; }}, 9));
}
row('abcabc'.includes('bc', 2) && !'abcabc'.includes('bc', 5) && 'abc'.includes('', 9));
row('abc'.startsWith('bc', 1) && 'abc'.endsWith('ab', 2) && 'abc'.endsWith('c', undefined));
row('abc'.startsWith('', Infinity) && 'abc'.endsWith('', -Infinity) && !'abc'.startsWith('a', Infinity));
row('abc'.includes('a', NaN) && 'abc'.startsWith('a', -Infinity) && 'abc'.endsWith('c', Infinity));
row('😀x'.indexOf('\ude00') === 1 && '😀x'.lastIndexOf('\ud83d') === 0 && '😀x'.startsWith('\ude00', 1));
{ let log = ''; let r = {toString() { log += 'r'; return 'abc'; }}; let s = {get [Symbol.match]() { log += 'm'; return false; }, toString() { log += 's'; return 'b'; }}; let p = {valueOf() { log += 'p'; return 1; }}; row(String.prototype.startsWith.call(r, s, p) && log === 'rmsp'); }
{ let n = 0; row(error(() => String.prototype.includes.call(null, {get [Symbol.match]() { n++; }})) === 'TypeError' && n === 0); }
{ let s = ''; for (let i = 0; i < 200; i++) s += 'aaaaa'; s += 'b'; row(s.indexOf('aaaaab') === 995 && s.lastIndexOf('aaaaab') === 995 && s.includes('aaaaab') && !s.includes('aaaaac')); }
failed === 0 ? rows : -failed;
