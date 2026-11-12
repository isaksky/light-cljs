// bbp-authored conformance case (light-cljs-905).
// Derives from: rust/tests/fixtures/js/string_methods.js lines 39-48 and 56-67
// (its normalize rows) and rust/tests/js_source.rs lines 30494-30507,
// 30554-30628 and 30751-30770 (string_case_trim_repeat_predicates_at_normalize,
// the JavaScript rows of string_methods_budget_and_gc and
// string_methods_wait_collect_and_migrate).
// Canonical and compatibility decomposition, canonical ordering by
// combining class, composition with exclusions, Hangul syllables, lone
// surrogates and astral composition exclusions, with Unicode 17 data.
// Left out: an LV syllable before U+11A7, which is no trailing consonant
// (`'가ᆧ'.normalize()` is "가ᆧ"; QuickJS drops the
// U+11A7); corpus/serial/string-case.js checks it on Goose.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function error(f) { try { f(); } catch(e) { return e.name; } return ''; }
// string_methods.js
row('é'.normalize('NFD') === 'é' && 'é'.normalize('NFC') === 'é');
row('Å'.normalize() === 'Å' && 'ﬁ①'.normalize('NFKD') === 'fi1');
row('ﬁ①'.normalize('NFKC') === 'fi1' && 'ﬁ①'.normalize('NFC') === 'ﬁ①');
row('à̕'.normalize() === 'à̕');
row('a̅́'.normalize() === 'a̅́');
row('̈́'.normalize() === '̈́');
row('각'.normalize() === '각' && '각'.normalize('NFD') === '각');
row('\ud800é\udfff'.normalize('NFD') === '\ud800é\udfff');
row('𝅗𝅥'.normalize() === '𝅗𝅥');
row(''.normalize('NFKD') === '' && 'abc'.normalize(undefined) === 'abc');
row(typeof String.prototype.normalize === 'function');
row(Object.getOwnPropertyDescriptor(String.prototype, 'normalize').enumerable === false);
row(error(() => String.prototype.normalize.call(null)) === 'TypeError');
row(error(() => new String.prototype.normalize()) === 'TypeError');
for (let form of ['nfc', '', 'NFKX']) row(error(() => 'x'.normalize(form)) === 'RangeError');
row(String.prototype.normalize.name === 'normalize' && String.prototype.normalize.length === 0);
// js_source.rs string_case_trim_repeat_predicates_at_normalize
row('é'.normalize('NFD') === 'é' && 'é'.normalize() === 'é');
// js_source.rs string_methods_budget_and_gc
{ let s = 'à̕'.repeat(1024); row(s.normalize() === s.normalize('NFC')); }
{ let s = 'ﬃ'.repeat(4096); row(s.normalize('NFKD').length === s.length * 3); }
// js_source.rs string_methods_wait_collect_and_migrate
{ let count = 0; row('é'.normalize({toString() { count++; return 'NFD'; }}) === 'é' && count === 1); }
// Planner rows: the forms, Hangul, lone surrogates, compositions.
row('é'.normalize('NFD') === 'é' && 'é'.normalize('NFC') === 'é');
row('ﬁ①'.normalize('NFKD') === 'fi1' && 'ﬁ①'.normalize('NFC') === 'ﬁ①');
row('ẛ̣'.normalize('NFC') === 'ẛ̣' && 'ẛ̣'.normalize('NFD') === 'ẛ̣');
row('ẛ̣'.normalize('NFKC') === 'ṩ' && 'ẛ̣'.normalize('NFKD') === 'ṩ');
row('Å'.normalize() === 'Å' && 'Ω'.normalize() === 'Ω' && 'Å'.normalize('NFD') === 'Å');
row('क़'.normalize() === 'क़' && '⫝̸'.normalize() === '⫝̸' && 'ཱི'.normalize() === 'ཱི');
row('각'.normalize('NFKD') === '각' && '가'.normalize() === '가' && '각'.normalize() === '각');
row('ᄀᅶ'.normalize() === 'ᄀᅶ' && 'ᄓᅡ'.normalize() === 'ᄓᅡ');
row('한글'.normalize('NFD') === '한글' && '한글'.normalize() === '한글');
row('ୋ'.normalize() === 'ୋ' && 'ୋ'.normalize('NFD') === 'ୋ');
row('ạ́'.normalize('NFD') === 'ạ́' && 'ạ́'.normalize() === 'ạ́');
row('ậ'.normalize() === 'ậ' && 'ậ'.normalize() === 'ậ');
row('è́'.normalize() === 'è́' && '́e'.normalize() === '́e');
row('Ǻ'.normalize('NFD') === 'Ǻ' && 'Ǻ'.normalize() === 'Ǻ');
row('Ǟ'.normalize() === 'Ǟ' && 'Ǟ'.normalize('NFD') === 'Ǟ');
row('½'.normalize('NFKD') === '1⁄2' && '½'.normalize('NFKC') === '1⁄2' && '½'.normalize() === '½');
row(' '.normalize('NFKC') === ' ' && '¨'.normalize('NFKD') === ' ̈' && ' '.normalize('NFC') === ' ');
row('ﷺ'.normalize('NFKD').length === 18 && 'ﷺ'.normalize('NFKC') === 'صلى الله عليه وسلم');
row('\u{1d15e}\u{1d165}'.normalize() === '\u{1d157}\u{1d165}\u{1d165}' &&'\u{1d15f}'.normalize('NFD') === '\u{1d158}\u{1d165}');
row('\u{1d400}'.normalize('NFKC') === 'A' && '\u{2f800}'.normalize() === '丽' && '\u{1f100}'.normalize('NFKD') === '0.');
row('\u{110ab}'.normalize('NFC') === '\u{110ab}' && '\u{11099}\u{110ba}'.normalize() === '\u{1109a}');
row('\udc00\ud800'.normalize('NFKD') === '\udc00\ud800' && '\ud800́'.normalize() === '\ud800́');
row('ÀÉÎõü'.normalize('NFD').normalize('NFC') === 'ÀÉÎõü' && 'ÀÉÎõü'.normalize('NFD').length === 10);
row('café'.normalize('NFKD') === 'café' && 'à̀'.normalize() === 'à̀');
row('̈́̀'.normalize('NFD') === '̈́̀' && '̀́̓'.normalize() === '̀́̓');
{ let s = 'a' + '́'.repeat(100) + '̣'.repeat(100); row(s.normalize('NFD') === 'a' + '̣'.repeat(100) + '́'.repeat(100)); }
{ let s = 'é'.repeat(2000); row(s.normalize('NFD').length === 4000 && s.normalize('NFD').normalize() === s); }
row(String.prototype.normalize.call(123) === '123' && String.prototype.normalize.call(true, 'NFKD') === 'true');
row(String.prototype.normalize.call(['é'], 'NFD') === 'é');
{ let log = ''; let r = {toString() { log += 'r'; return 'é'; }}, f = {toString() { log += 'f'; return 'NFD'; }}; row(String.prototype.normalize.call(r, f) === 'é' && log === 'rf'); }
row(error(() => 'a'.normalize(Symbol())) === 'TypeError' && error(() => 'a'.normalize(null)) === 'RangeError' && error(() => 'a'.normalize('NFC ')) === 'RangeError');
row(error(() => String.prototype.normalize.call(undefined, 'NFD')) === 'TypeError');
{ let hit = 0; row(error(() => String.prototype.normalize.call({toString() { throw new EvalError('x'); }}, {toString() { hit++; return 'NFC'; }})) === 'EvalError' && hit === 0); }
failed === 0 ? rows : -failed;
