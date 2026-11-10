// bbp-authored conformance case (light-cljs-933).
// Derives from: crates/big-boy-pants-js/rust/tests/fixtures/js/regexp_unicode.js lines 27-63 (js_source.rs regexp_unicode_mode; its rows without String.prototype.replace and replaceAll), rust/tests/fixtures/js/regexp_indices.js lines 6-12 and 44-48 (regexp_has_indices, its Unicode rows), rust/src/regexp/matcher/tests.rs lines 243-287 and 411-431 (offsets_search_and_code_units, unicode_candidate_positions_and_capture_offsets, without the step limit) and light-cljs-770's planner rows:
// matching over code units without `u` or `v` and over code points with
// them, a lone surrogate being a code point; a lastIndex inside a
// surrogate pair starting the search at the pair's start; candidate
// positions advancing by code point; captures, backreferences, classes,
// dots and lookaheads over pairs; both string encodings (Latin-1 and
// UTF-16 inputs, and captures of a UTF-16 input that fit Latin-1). Left
// out: case-insensitive patterns (light-cljs-936) and String.prototype's
// matching methods (light-cljs-934's replace rows wait there). Each row is
// a `row(...)` check; the script completes with the number of rows, or
// minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
// The planner's rows (light-cljs-770).
row([/^.$/u.test('😀'), /^.$/.test('😀'), /\u{1F600}/u.test('😀'), /^[😀]$/u.test('😀'), /\udc00/.test('🐀'), /\udc00/u.test('🐀')].join() === "true,false,true,true,true,false");
// regexp_unicode.js
{ let a = /(?:)/gu; a.lastIndex = 1; a.exec('🐱'); row(a.lastIndex === 0); a.lastIndex = 1; a.exec('a\udc00'); row(a.lastIndex === 1); }
row(/^.$/u.test('🐱') === true && /^.$/.test('🐱') === false && /^[🐱]$/u.test('🐱') === true && /\u{1F431}/u.exec('x🐱')[0] === '🐱');
row(/\udc00/u.test('𐀀') === false && /\udc00/.test('𐀀') === true && /[\u{10000}-\u{10FFFF}]/u.test('\u{10400}') === true);
row(/^🐱{2}$/u.test('🐱🐱') && !/^🐱{2}$/u.test('🐱') && /^(🐱+)(🐱)$/u.exec('🐱🐱🐱')[1] === '🐱🐱');
row(/^(.)\1$/u.test('🐱🐱') && /^(?=(🐱))\1$/u.test('🐱') && /^[^a]$/u.test('🐱') && /^\D$/u.test('🐱'));
row(!/\ud800/u.test('𐀀') && /^\ud800$/u.test('\ud800') && /^\udc00$/u.test('\udc00'));
row(!/^(\ud800)\1\udc00$/u.test('\ud800𐀀') && !/^(\udc00)\ud800\1$/u.test('\udc00𐀀'));
row(/\udc00/u.exec('🐱\udc00').index === 2 && /\ud800/u.exec('🐱\ud800').index === 2);
{ let a = /./uy; a.lastIndex = 1; row(a.exec('🐱')[0] === '🐱' && a.lastIndex === 2); a.lastIndex = 3; row(a.exec('🐱') === null && a.lastIndex === 0); }
{ let a = /(.)/gu; row(a.exec('x🐱').index === 0 && a.lastIndex === 1); let m = a.exec('x🐱'); row(m.index === 1 && m[1] === '🐱' && a.lastIndex === 3); }
row(new RegExp('\\u{1F431}', 'u').test('🐱') && /a/gu.unicode && /a/gu.flags === 'gu');
// regexp_indices.js's Unicode rows.
{ let a = /\u{10000}/ugd; a.lastIndex = 1; let r = a.exec('\u{10000}_\u{10000}'); row(r.indices[0][0] === 0 && r.indices[0][1] === 2 && a.lastIndex === 2 && r.index === 0 && r[0].length === 2); }
{ let a = /\u{10000}/ugd; a.lastIndex = 2; let r = a.exec('\u{10000}_\u{10000}'); row(r.indices[0][0] === 3 && r.indices[0][1] === 5 && a.exec('\u{10000}_\u{10000}') === null && a.lastIndex === 0); }
row(/./d.exec('\u{10000}').indices[0][1] === 1 && /./du.exec('\udc00').indices[0][1] === 1);
{ let a = /./duy; a.lastIndex = 1; let m = a.exec('\u{10000}'); row(m.indices[0][0] === 0 && m.indices[0][1] === 2 && a.lastIndex === 2); }
// matcher/tests.rs: code units without u, pairs with it.
{ let m = /(.)/.exec('\udc00\udfff'); row(m.index === 0 && m[1] === '\udc00'); }
{ let r = /(.)/y; r.lastIndex = 1; let m = r.exec('𐀀\udfff'); row(m.index === 1 && m[1] === '\udc00'); }
{ let m = /(\uD800)\1/.exec('\ud800\ud800'); row(m[0].length === 2 && m[1] === '\ud800'); }
for (let flags of ['u', 'uy']) { let r = new RegExp('(.)', flags); r.lastIndex = 1; let m = r.exec('🐱\udc31'); row((flags === 'u' ? m.index === 0 : m.index === 0 && r.lastIndex === 2) && m[1] === '🐱'); }
row(/\udc31/u.exec('🐱\udc31').index === 2);
{ let r = /./uy; r.lastIndex = 3; row(r.exec('🐱\udc31') === null); }
// Lone surrogates as code points, dots and classes over pairs.
row(/^.$/u.test('\ud800') && /^..$/u.test('\udc00\ud800') && !/^.$/u.test('𐀀x') && /^[\ud800-\udbff]$/u.test('\ud800') && !/^[\ud800-\udbff]$/u.test('𐀀'));
row(/^[^x]$/u.test('\u{10000}') && /^[^x]{2}$/.test('\u{10000}') && /^\S$/u.test('\u{10000}') && /^\W$/u.test('\u{10000}'));
row(/^\u{10000}+$/u.test('\u{10000}\u{10000}') && !/^𐀀+$/.test('\u{10000}\u{10000}') && /^(?:𐀀)+$/.test('\u{10000}\u{10000}'));
row(/\b/u.exec('\u{10000}a').index === 2 && /\B/u.exec('\u{10000}').index === 0);
// v mode reads code points too; a string literal set.
row(/^.$/v.test('🐱') && /^[\q{🐱|ab}]$/v.test('ab') && /^[\q{🐱|ab}]$/v.test('🐱') && !/^[\q{ab}]$/v.test('a'));
// Inputs in both encodings: a Latin-1 input, a UTF-16 one whose captures
// fit Latin-1 and one whose captures do not.
{ let m = /é(.)ß/.exec('xéyß'); row(m[1] === 'y' && m.index === 1 && m.input === 'xéyß'); }
{ let m = /☃(.)(é)(☃)/.exec('a☃bé☃c'); row(m[1] === 'b' && m[2] === 'é' && m[3] === '☃' && m.index === 1 && m[0] === '☃bé☃'); }
{ let m = /(\w+)$/.exec('☃☃abc'); row(m[1] === 'abc' && m[1].length === 3 && m[1] === 'a' + 'bc'); }
{ let m = /(.)(.)/u.exec('é🐱'); row(m[1] === 'é' && m[2] === '🐱' && m[0].length === 3); }
rows = failed === 0 ? rows : -failed;
rows;
