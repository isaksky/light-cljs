// bbp-authored conformance case (light-cljs-904).
// Derives from: rust/tests/js_source.rs lines 7689-8105 and 30494-30507
// bbp-authored slice (light-cljs-904); ported from rust/tests/js_source.rs
// string_substring_slice_concat_pad and string_extract_combine_coercion_and_boundaries.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function error(f) { try { f(); } catch(e) { return e.name; } return ''; }
row((function(){ return 'abcd'.substring(1, 3) === 'bc'; })());
row((function(){ return 'a'.concat('b', 'c') === 'abc'; })());
row((function(){ return 'abc'.padStart(Infinity, '') === 'abc'; })());
row((function(){ return 'abcd'.substring(3, 1) === 'bc'; })());
row((function(){ return 'abcd'.substring(-5, 2) === 'ab'; })());
row((function(){ return 'abcd'.substring(1) === 'bcd'; })());
row((function(){ return 'abcd'.slice(-1) === 'd'; })());
row((function(){ return 'abcd'.slice(0, -1) === 'abc'; })());
row((function(){ return 'abcd'.slice(2, 1) === ''; })());
row((function(){ return 'x@y'.slice(0, 1) + 'x@y'.slice(2) === 'xy'; })());
row((function(){ return 'a'.concat(1, null, undefined) === 'a1nullundefined'; })());
row((function(){ return 'abc'.padStart(6, '12') === '121abc'; })());
row((function(){ return 'abc'.padEnd(5) === 'abc  '; })());
row((function(){ return 'abc'.padStart(2, 'x') === 'abc'; })());
row((function(){ let caught = false; try { 'abc'.padStart(Infinity, 'x'); } catch (e) { caught = true; } return caught; })());
row((function(){ return '\u{10000}'.slice(0, 1) === '\ud800'; })());
row((function(){ return 'abcd'.substring(Infinity, -Infinity) === 'abcd' && 'abcd'.slice(-Infinity, Infinity) === 'abcd'; })());
row((function(){ return 'abcd'.substring(NaN, 2.9) === 'ab' && 'abcd'.slice(-2.9) === 'cd'; })());
row((function(){ return 'abcd'.substring(1, undefined) === 'bcd' && 'abcd'.slice(1, undefined) === 'bcd'; })());
row((function(){ return 'abcd'.substring(1, {valueOf: () => undefined}) === 'a' && 'abcd'.slice(1, {valueOf: () => undefined}) === ''; })());
row((function(){ return 'abc'.padStart(5, undefined) === '  abc' && 'abc'.padEnd(5, {toString: () => undefined}) === 'abcun'; })());
row((function(){ return 'abc'.padEnd(Infinity, '') === 'abc' && 'abc'.padStart(NaN) === 'abc' && 'abc'.padEnd(-Infinity) === 'abc'; })());
row((function(){ return 'abc'.padStart(5.9, '12') === '12abc' && 'abc'.padEnd('6.9', '12') === 'abc121'; })());
row((function(){ return 'a'.concat() === 'a' && ''.concat('', '') === '' && ''.padStart(0, '') === ''; })());
row((function(){ return 'abc'.slice(99) === '' && 'abc'.substring(99) === '' && 'abc'.slice(2, null) === ''; })());
row((function(){ return 'a'.padStart(2, '\u{10000}') === '\ud800a' && 'a'.padEnd(2, '\udc00') === 'a\udc00'; })());
row((function(){ return String.prototype.substring.call(123, 1) === '23' && String.prototype.slice.call(false, -2) === 'se'; })());
row((function(){ return String.prototype.concat.call(true, 1) === 'true1' && String.prototype.padStart.call(1, 2, 0) === '01'; })());
row((function(){ return String.prototype.substring(0) === '' && new String('abc').slice(1) === 'bc'; })());
row((function(){ let s = new String('abc'); s.toString = () => 'xyz'; return s.substring(1) === 'yz' && s.padEnd(5, '!') === 'xyz!!'; })());
row((function(){ let order = ''; let r = {toString: () => {order += 'r'; return 'abc'; }}; let a = {valueOf: () => {order += 'a'; return 1; }}; let b = {valueOf: () => {order += 'b'; return 2; }}; return String.prototype.substring.call(r, a, b) === 'b' && order === 'rab'; })());
row((function(){ let order = ''; let r = {toString: () => {order += 'r'; return 'a'; }}; let a = {toString: () => {order += 'a'; return 1; }, valueOf: () => {throw 1; }}; return String.prototype.concat.call(r, a, a) === 'a11' && order === 'raa'; })());
row((function(){ let n = 0; let f = {toString: () => {n++; throw 1; }}; return 'abc'.padStart(2, f) === 'abc' && 'abc'.padEnd(NaN, f) === 'abc' && n === 0; })());
row((function(){ let n = 0; let f = {toString: () => {n++; return ''; }}; return 'abc'.padEnd(Infinity, f) === 'abc' && n === 1; })());
row((function(){ let order = ''; let f = {toString: () => {order += 'f'; return 'x'; }}; let caught = false; try { 'abc'.padStart(Infinity, f); } catch(e) { caught = true; } return caught && order === 'f'; })());
row((function(){ let n = 0; let bad = {valueOf: () => {throw 42; }}; let end = {valueOf: () => {n++; return 2; }}; let caught = 0; try { 'abc'.slice(bad, end); } catch(e) { caught = e; } return caught === 42 && n === 0; })());
row((function(){ let n = 0; let bad = {toString: () => {throw 42; }}; let later = {toString: () => {n++; return 'x'; }}; let caught = 0; try { 'abc'.concat(bad, later); } catch(e) { caught = e; } return caught === 42 && n === 0; })());
row((function(){ let f = String.prototype.concat.bind({toString: () => 'a'}, {toString: () => 'b'}); return f.apply(null, ['c']) === 'abc'; })());
row((function(){ let s = {toString: String.prototype.concat.bind('a', {toString: () => 'b'})}; return String.prototype.slice.call(s, 1) === 'b'; })());
// Trim and repeat rows from string_case_trim_repeat_predicates_at_normalize.
row('  a\t\n'.trim() === 'a' && ' a '.trimStart() === 'a ' && ' a '.trimEnd() === ' a');
row('\u00a0\u1680\u2000\u200a\u2028\u2029\u202f\u205f\u3000\ufeffx\r\n'.trim() === 'x');
row('\u0085\u200bx\u200b\u0085'.trim() === '\u0085\u200bx\u200b\u0085');
{ let ws = String.fromCharCode(9, 10, 11, 12, 13, 32, 160, 0x1680, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200a, 0x2028, 0x2029, 0x202f, 0x205f, 0x3000, 0xfeff); row((ws + 'x' + ws).trim() === 'x' && ws.trim() === ''); }
row('ab'.repeat(3) === 'ababab' && 'x'.repeat(0) === '' && 'x'.repeat(NaN) === '' && 'x'.repeat(-0.9) === '');
row(''.repeat(1e100) === '' && 'a'.repeat({valueOf() { return 2.9; }}) === 'aa');
row(error(() => 'x'.repeat(-1)) === 'RangeError' && error(() => 'x'.repeat(Infinity)) === 'RangeError');
// Result length errors agree with QuickJS; Pants reports its StringTooLong
// as InternalError, so assert only that the operation rejects after coercion.
for (let n of [1073741824, 2 ** 63]) { let hit = 0, caught = false; try { 'ab'.repeat({valueOf() { hit++; return n; }}); } catch(e) { caught = true; } row(caught && hit === 1); }
for (let pair of [['substring', 2], ['slice', 2], ['concat', 1], ['padStart', 1], ['padEnd', 1], ['repeat', 1], ['trim', 0], ['trimStart', 0], ['trimEnd', 0]]) {
  let f = pair[0], d = Object.getOwnPropertyDescriptor(String.prototype, f);
  row(d.value.name === f && d.value.length === pair[1] && d.writable && !d.enumerable && d.configurable);
  row(error(() => String.prototype[f].call(null)) === 'TypeError' && error(() => String.prototype[f].call(undefined)) === 'TypeError' && error(() => new String.prototype[f]()) === 'TypeError');
}
row('😀'.slice(0, 1) === '\ud83d' && 'x€y'.substring(0, 1) === 'x' && 'x€y'.slice(2) === 'y');
row('a'.padStart(2, '😀') === '\ud83da' && 'a'.padEnd(2, '\udc00') === 'a\udc00');
row('z'.padStart(8192, 'a').length === 8192 && ''.padEnd(8192, 'a').slice(-1) === 'a');
row('a'.repeat(4096).concat('z').slice(-2) === 'az');
{ let hit = 0; let n = {valueOf() { hit++; return 5; }}; row(error(() => String.prototype.padStart.call(Symbol(), n)) === 'TypeError' && hit === 0); }
failed === 0 ? rows : -failed;
