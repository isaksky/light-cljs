// bbp-authored conformance case (light-cljs-905).
// Derives from: rust/tests/fixtures/js/string_methods.js lines 3-10 and 56-67
// (its toUpperCase and toLowerCase rows) and rust/tests/js_source.rs lines
// 30494-30507, 30554-30628 and 30751-30770
// (string_case_trim_repeat_predicates_at_normalize, the JavaScript rows of
// string_methods_budget_and_gc and string_methods_wait_collect_and_migrate).
// Full case mapping with Unicode 17 data: SpecialCasing's unconditional
// rows, Final_Sigma, surrogate pairs as one code point, lone surrogates
// kept, and the receiver's conversion.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function error(f) { try { f(); } catch(e) { return e.name; } return ''; }
// string_methods.js
row(''.toUpperCase() === '' && ''.toLowerCase() === '');
row('ßﬃΐ'.toUpperCase() === 'SSFFIΪ́');
row('İ'.toLowerCase() === 'i̇');
row('ΟΣ́'.toLowerCase() === 'ος́');
row('ΟΣ́Α'.toLowerCase() === 'οσ́α');
row('Σ'.toLowerCase() === 'σ' && 'AΣͅ'.toLowerCase() === 'aςͅ');
row('𐐨'.toUpperCase() === '𐐀');
row('\ud800x\udfff'.toUpperCase() === '\ud800X\udfff');
for (let name of ['toUpperCase', 'toLowerCase']) {
  row(typeof String.prototype[name] === 'function');
  row(Object.getOwnPropertyDescriptor(String.prototype, name).enumerable === false);
  row(error(() => String.prototype[name].call(null)) === 'TypeError');
  row(error(() => new String.prototype[name]()) === 'TypeError');
  row(String.prototype[name].name === name && String.prototype[name].length === 0);
}
// js_source.rs string_case_trim_repeat_predicates_at_normalize
row('abc'.toUpperCase() === 'ABC' && 'ABC'.toLowerCase() === 'abc');
row('straße'.toUpperCase() === 'STRASSE' && 'ΟΣ'.toLowerCase() === 'ος');
// js_source.rs string_methods_budget_and_gc
{ let s = 'ß'.repeat(4096); row(s.toUpperCase().length === s.length * 2); }
{ let s = 'Α'.repeat(4096) + 'Σ'; row(s.toLowerCase().at(-1) === 'ς'); }
// js_source.rs string_methods_wait_collect_and_migrate
{ let count = 0; row(String.prototype.toUpperCase.call({get toString() { count++; return () => 'ß'; }}) === 'SS' && count === 1); }
{ let count = 0; row(String.prototype.toLowerCase.call({toString() { count++; return 'ΟΣ'; }}) === 'ος' && count === 1); }
// Planner rows and boundaries.
row('ÿ'.toUpperCase() === 'Ÿ' && 'Ÿ'.toLowerCase() === 'ÿ');
row('µ'.toUpperCase() === 'Μ' && 'ŉ'.toUpperCase() === 'ʼN' && 'ǰ'.toUpperCase() === 'J̌');
row('ﬀﬁﬂﬃﬄﬅﬆ'.toUpperCase() === 'FFFIFLFFIFFLSTST' && 'ﬓ'.toUpperCase() === 'ՄՆ');
row('ΐΰ'.toUpperCase() === 'Ϊ́Ϋ́' && 'ᾀ'.toUpperCase() === 'ἈΙ' && 'ᾈ'.toLowerCase() === 'ᾀ');
row('Ǆǅǆ'.toLowerCase() === 'ǆǆǆ' && 'Ǆǅǆ'.toUpperCase() === 'ǄǄǄ');
row('ΑΣ'.toLowerCase() === 'ας' && 'ΑΣ.'.toLowerCase() === 'ας.' && 'ΑΣΑ'.toLowerCase() === 'ασα');
row('Σ.Α'.toLowerCase() === 'σ.α' && 'Α.Σ'.toLowerCase() === 'α.ς' && 'Α.Σ.Α'.toLowerCase() === 'α.σ.α');
row('ΑΣͅΑ'.toLowerCase() === 'ασͅα' && ' Σ'.toLowerCase() === ' σ' && 'ΑΣ1'.toLowerCase() === 'ας1');
row('ΑΣΣ'.toLowerCase() === 'ασς' && 'ΣΣ'.toLowerCase() === 'σς');
row('𐐀𐐁'.toLowerCase() === '𐐨𐐩' && '😀'.toUpperCase() === '😀');
row('\udc28\ud801'.toUpperCase() === '\udc28\ud801' && '\ud801'.toLowerCase() === '\ud801');
row('ı'.toUpperCase() === 'I' && 'ſ'.toUpperCase() === 'S' && 'K'.toLowerCase() === 'k' && 'Ω'.toLowerCase() === 'ω');
row('ꞔ'.toUpperCase() === 'Ꞔ' && 'Ꟁꟁ'.toLowerCase() === 'ꟁꟁ' && 'ꟁ'.toUpperCase() === 'Ꟁ');
row('ƛ'.toUpperCase() === 'Ƛ' && 'Ƛ'.toLowerCase() === 'ƛ' && '\u{10d50}'.toLowerCase() === '\u{10d70}');
row('\u{1e900}'.toLowerCase() === '\u{1e922}' && '\u{1e922}'.toUpperCase() === '\u{1e900}');
row('\u{16e40}'.toLowerCase() === '\u{16e60}' && '\u{10c80}'.toLowerCase() === '\u{10cc0}');
row('İ'.toUpperCase() === 'İ' && 'i̇'.toUpperCase() === 'İ' && 'I'.toLowerCase() === 'i');
row(String.prototype.toUpperCase.call(123) === '123' && String.prototype.toUpperCase.call(true) === 'TRUE' && String.prototype.toLowerCase.call(-0) === '0');
row(String.prototype.toUpperCase.call(['a', 'b']) === 'A,B' && String.prototype.toLowerCase.call({}) === '[object object]');
row(error(() => String.prototype.toUpperCase.call(undefined)) === 'TypeError' && error(() => String.prototype.toLowerCase.call(Symbol())) === 'TypeError');
row(error(() => String.prototype.toUpperCase.call({toString() { throw new RangeError('x'); }})) === 'RangeError');
{ let s = 'x'.repeat(1000); row(s.toUpperCase() === 'X'.repeat(1000) && s.toUpperCase().toLowerCase() === s); }
{ let s = 'aÀ'.repeat(300); row(s.toUpperCase() === 'AÀ'.repeat(300) && s.toUpperCase().toLowerCase() === 'aà'.repeat(300)); }
{ let a = 'abc', b = a.toLowerCase(); row(a === b && 'ABC'.toUpperCase() === 'ABC'); }
failed === 0 ? rows : -failed;
