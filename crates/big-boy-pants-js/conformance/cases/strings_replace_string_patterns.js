// bbp-authored conformance case (light-cljs-934).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 24022-24062 (string_replace, its string-pattern rows), 30905-30926 (regexp_symbol_dispatch, its custom @@replace row), 30929-30957 (regexp_symbol_protocol_edges, its receiver row) and light-cljs-770's planner rows:
// String.prototype.replace and replaceAll with a search value that is no
// RegExp: GetMethod(searchValue, @@replace) first (a getter runs, a method
// found answers with (receiver, replaceValue)), then ToString of the
// receiver, the search value and a non-callable replacement, in that
// order; the first or every occurrence (replaceAll advances by max(1,
// length), so "" matches between every code unit); GetSubstitution's
// `$$`, `$&`, `` $` ``, `$'` and its literal forms (a string pattern has no
// captures and no groups); a replacer called with (matched, position,
// string) and its result converted by ToString. Left out: RegExp patterns
// (strings_replace_regexp, regexp_symbol_replace), the Pants engine's
// work and argument limits, which QuickJS does not share, and a primitive
// search value's @@replace from its prototype, which QuickJS does not read
// (it looks at objects only) and the Pants engine's prototypes refuse to
// take. Each row is a
// `row(...)` check; the script completes with the number of rows, or
// minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-770).
row(['a-b-c'.replaceAll('-', '_'), 'x.y'.replaceAll('.', '$'), 'abc'.replace('b', '$&$&'), 'abc'.replace('b', "$'"), 'abc'.replace('b', '$`'), 'abc'.replace('b', '$$'), 'abc'.replace('b', '$1'), 'xyz'.replace('', '_'), 'xyz'.replaceAll('', '_'), 'aXbX'.replace('X', (m, i, s) => m + i + s.length), 'q?'.replaceAll('?', '_QMARK_')].join('|') === "a_b_c|x$y|abbc|acc|aac|a$c|a$1c|_xyz|_x_y_z_|aX14bX|q_QMARK_");
row('abc'.replace({ toString() { return 'b'; } }, 'X') === "aXc");
row('abc'.replace({ [Symbol.replace](s, r) { return s + r; } }, 'X') === "abcX");
// cljs.core's boot: munged names (boot.js 1415, 8577, 8589).
row('cljs.core.Foo'.replaceAll('.', '$') === "cljs$core$Foo" && 'set-meta!?'.replaceAll('-', '_').replaceAll('?', '_QMARK_') === "set_meta!_QMARK_");
// string_replace (js_source.rs 24022).
row("abcabc".replace("b", "X") === "aXcabc" && "abcabc".replaceAll("b", "X") === "aXcaXc" && "ab".replaceAll("", "-") === "-a-b-");
row("aaa".replaceAll("aa", "X") === "Xa" && "".replaceAll("", "-") === "-" && "abc".replace("", "-") === "-abc" && "abc".replace("z", "x") === "abc");
row("ab".replace("a", "$1$<x>$&") === "$1$<x>ab" && "𐀀".replaceAll("", "-") === "-\uD800-\uDC00-");
row("ab".replaceAll("", (m, p, s) => p) === "0a1b2");
{ let caught = false; try { 'x'.replace('x', () => { throw 42; }); } catch (e) { caught = e === 42; } row(caught); }
{ let log = ''; let r = String.prototype.replace.call({ toString() { log += 't'; return 'aba'; } }, { toString() { log += 's'; return 'a'; } }, { toString() { log += 'r'; return 'X'; } }); row(r === 'Xba' && log === 'tsr'); }
row(String.prototype.replace.call(123, 2, 4) === '143' && 'undefined'.replace() === 'undefined');
{ let d = Object.getOwnPropertyDescriptor(String.prototype, 'replaceAll'); let e = Object.getOwnPropertyDescriptor(String.prototype, 'replace'); row(d.value.length === 2 && d.value.name === 'replaceAll' && d.writable && !d.enumerable && d.configurable && e.value.length === 2 && e.value.name === 'replace' && e.writable && !e.enumerable && e.configurable); }
row(name(() => String.prototype.replace.call(null, '', 'x')) === "TypeError" && name(() => String.prototype.replaceAll.call(undefined, '', 'x')) === "TypeError");
row(name(() => 'x'.replace(Symbol(), '')) === "TypeError" && name(() => 'x'.replace('x', Symbol())) === "TypeError" && name(() => String.prototype.replace.call(Symbol(), 'x', '')) === "TypeError");
// regexp_symbol_protocol_edges (js_source.rs 30929): @@replace before the receiver's ToString.
{ let receiver = { toString() { throw 1; } }; let x = { [Symbol.replace](s, r) { return s === receiver && r === 7; } }; row(String.prototype.replace.call(receiver, x, 7) === true); }
// regexp_symbol_dispatch (js_source.rs 30905): a custom object's @@replace for both.
{ let x = { flags: 'g', [Symbol.replace](s, r) { return s + r; } }; row('a'.replace(x, 'b') === 'ab' && 'a'.replaceAll(x, 'c') === 'ac'); }
// GetMethod: a getter runs; null and undefined fall back to strings; anything else must be callable.
{ let log = ''; let x = { get [Symbol.replace]() { log += 'g'; return undefined; }, toString() { log += 's'; return 'b'; } }; row('abc'.replace(x, 'X') === 'aXc' && log === 'gs'); }
{ let x = { [Symbol.replace]: null, toString() { return 'c'; } }; row('abc'.replaceAll(x, '-') === 'ab-'); }
row(name(() => 'a'.replace({ [Symbol.replace]: 1 }, '')) === "TypeError" && name(() => 'a'.replaceAll({ [Symbol.replace]: {} }, '')) === "TypeError");
{ let args; let x = { [Symbol.replace](...a) { args = a; return this; } }; row('q'.replace(x) === x && args.length === 2 && args[0] === 'q' && args[1] === undefined); }
// The replacer: undefined `this`, three arguments, its result converted (an object by its toString).
{ let calls = []; let r = 'a.b.c'.replaceAll('.', function (m, p, s) { 'use strict'; calls.push([this, m, p, s, arguments.length].join()); return { toString() { return '/'; } }; }); row(r === 'a/b/c' && calls.join(';') === ",.,1,a.b.c,3;,.,3,a.b.c,3"); }
row('abc'.replace('b', () => 5) === 'a5c' && 'abc'.replace('b', () => undefined) === 'aundefinedc' && 'abc'.replace('b', () => null) === 'anullc');
// Occurrences: the first only, or every one without overlap; the template
// once per occurrence, `$'` and `` $` `` from the occurrence.
row('aaaa'.replace('aa', 'b') === 'baa' && 'aaaa'.replaceAll('aa', 'b') === 'bb' && 'aaa'.replaceAll('aa', '[$&]') === '[aa]a');
row('a-b-c'.replaceAll('-', "<$`|$'>") === "a<a|b-c>b<a-b|c>c" && '12'.replaceAll('', '$&$$') === "$1$2$");
row('$1$2'.replace('$1', '$$') === '$$2' && 'x'.replace('x', '$') === '$' && 'x'.replace('x', '$$$') === '$$' && 'x'.replace('x', '$<') === '$<' && 'x'.replace('x', '$0$00$9') === '$0$00$9');
// Non-string receivers, search values and replacements convert.
row(String.prototype.replaceAll.call(1001, 0, 9) === '1991' && 'truefalse'.replace(false, true) === 'truetrue' && 'nullnull'.replaceAll(null, undefined) === 'undefinedundefined');
row(String.prototype.replace.call([1, 2], ',', '+') === '1+2' && 'a,b'.replace([','], ['+']) === 'a+b');
// UTF-16 code units: no code point boundaries.
row('\u{1F431}'.replace('\uDC31', 'x') === '\uD83Dx' && 'é☃é'.replaceAll('é', 'e') === 'e☃e' && 'a☃b'.replace('☃', 'é') === 'aéb');
rows = failed === 0 ? rows : -failed;
rows;
