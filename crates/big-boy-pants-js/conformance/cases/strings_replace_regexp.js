// bbp-authored conformance case (light-cljs-934).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 24022-24062 (string_replace, its RegExp rows), quickjs/tests/test_builtin.js test_regexp's case-sensitive replacement row, cljs.core's printer, demunge and uuid (research/clojurescript core.cljs quote-string, demunge-str, random-uuid) and light-cljs-770's planner rows:
// String.prototype.replace and replaceAll with a RegExp, through
// %RegExp.prototype%[@@replace]: one match or, with `g`, every match from
// lastIndex 0 (an empty match advancing by a code unit, or a code point
// with `u` or `v`), lastIndex left as the last search set it; replaceAll's
// TypeError for a RegExp without `g`, after reading `flags`;
// GetSubstitution's `$n` and `$nn` (the two-digit form only when that
// capture exists, `$0` and `$00` literal), `$<name>` (a missing name gives
// "", no groups leaves it literal), `$&`, `` $` ``, `$'`; a replacer with
// (matched, captures..., position, string[, groups]), its result
// converted. Left out: case-insensitive patterns (light-cljs-936) and the
// Pants engine's work limits, which QuickJS does not share. Each row is a
// `row(...)` check; the script completes with the number of rows, or
// minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-770).
row(['aaa'.replace(/a/g, (m, i) => i), 'John Smith'.replace(/(\w+)\s(\w+)/, '$2, $1'), '2024-05'.replace(/(?<y>\d+)-(?<m>\d+)/, '$<m>/$<y>'), 'aaa'.replaceAll(/a/g, 'b'), 'abc'.replace(/(b)/, '[$01$2$10]'), 'a.b'.replace(/(?<x>\.)/, '$<x>$<nope>!'), 'abc'.replace(/b/, '$0')].join('|') === "012|Smith, John|05/2024|bbb|a[b$2b0]c|a.!b|a$0c");
{ let got; 'x1y'.replace(/(?<d>\d)/, function () { got = arguments; return ''; }); row(got.length === 5 && got[0] === '1' && got[1] === '1' && got[2] === 1 && got[3] === 'x1y' && typeof got[4] === 'object' && got[4].d === '1' && Object.getPrototypeOf(got[4]) === null); }
// cljs.core's printer (quote-string), demunge and uuid.
{ let escapes = { '"': '\\"', '\\': '\\\\', '\b': '\\b', '\f': '\\f', '\n': '\\n', '\r': '\\r', '\t': '\\t' }; row('a"b\nc'.replace(/[\\"\b\f\n\r\t]/g, ch => escapes[ch]) === 'a\\"b\\nc' && 'plain'.replace(new RegExp("[\\\\\"\b\f\n\r\t]", "g"), ch => escapes[ch]) === 'plain'); }
{ let map = { _PLUS_: '+', _BANG_: '!', _QMARK_: '?' }; let r = new RegExp('_QMARK_|_PLUS_|_BANG_|\\$', 'g'); row('foo_QMARK_$bar_PLUS__BANG_'.replace(r, x => x === '$' ? '/' : map[x]) === 'foo?/bar+!'); }
{ let n = 0; let u = 'xxxx-4xxx-yxxx'.replace(/[xy]/g, c => { n++; return (c === 'x' ? 7 : 11).toString(); }); row(u === '7777-4777-11777' && n === 11); }
row('2024-05-01T00:00:00.000Z'.replace(/Z$/, '-00:00') === '2024-05-01T00:00:00.000-00:00');
// test_builtin.js test_regexp (its case-sensitive row).
row("aAbBcC#4".replace(/\p{Lower}/gu, "X") === "XAXBXC#4");
// string_replace (js_source.rs 24022).
row("aAbB".replace(/a/g, "X") === "XAbB" && "abc".replace(/(?:)/g, "-") === "-a-b-c-" && "abc".replace(/b/, "[$&$`$'$$]") === "a[bac$]c");
row("abc".replace(/(b)/, "$1$2$01") === "ab$2bc" && "abc".replace(/(?<x>b)/, "<$<x>>") === "a<b>c");
row(name(() => "aa".replaceAll(/a/, "b")) === "TypeError");
{ let r = /a/g; r.lastIndex = 99; row("aa".replace(r, "b") === "bb" && r.lastIndex === 0); }
{ let r = /(?:)/g; row("ab".replaceAll(r, "-") === "-a-b-" && r.lastIndex === 0); }
row("b".replace(/(a)?b/, "<$1>") === "<>" && "b".replace(/b/, "$<x>$0$00$01$10$") === "$<x>$0$00$01$10$");
row("ab".replace(/(a)(b)/, "$12-$02-$99") === "a2-b-$99");
row("ab".replace(/(?<x>a)(?<y>z)?/, "$<x>:$<y>:$<absent>:$<x") === "a:::$<xb");
{ let r = /b/y; r.lastIndex = 1; row("abc".replace(r, "X") === "aXc" && r.lastIndex === 2); }
{ let r = /b/; r.lastIndex = 2; row("abc".replace(r, "X") === "aXc" && r.lastIndex === 2); }
{ let r = /a/g; let n = 0; row("aa".replace(r, function (m, p, s) { n++; return m + p + s.length; }) === "a02a12" && n === 2); }
row("ab".replace(/(?<x>a)(z)?/, function (m, a, z, p, s, groups) { return m === a && z === undefined && p === 0 && s === 'ab' && groups.x === a && Object.getPrototypeOf(groups) === null ? '$&' : 'bad'; }) === "$&b");
{ let r = /a/g; let n = 0; row("aa".replace(r, () => { n++; let zero = r.lastIndex === 0; r.lastIndex = 77; return zero ? 'X' : 'Y'; }) === "XY" && n === 2 && r.lastIndex === 77); }
{ let r = /a/g; Object.defineProperty(r, 'lastIndex', { writable: false }); row(name(() => 'a'.replace(r, 'b')) === "TypeError"); }
{ let r = /a/g; r.exec = () => { throw 1; }; Object.defineProperty(r, 'flags', { get() { throw 2; } }); let caught = false; try { 'aa'.replaceAll(r, 'b'); } catch (e) { caught = e === 2; } row(caught); }
// Unicode mode advances an empty match by a code point.
row('\u{1F431}'.replace(/(?:)/gu, '-') === '-\u{1F431}-' && '\u{1F431}'.replace(/(?:)/g, '-') === '-\uD83D-\uDC31-' && 'a\u{1F431}b'.replaceAll(/(?:)/gv, '.') === '.a.\u{1F431}.b.');
{ let ps = []; '\u{1F431}x'.replace(/(?:)/gu, (m, p) => { ps.push(p); return ''; }); row(ps.join() === '0,2,3'); }
// Match positions and captures in UTF-16 strings.
row('é☃é'.replace(/(☃)/, '[$1]') === 'é[☃]é' && 'ab☃'.replace(/b(.)/, '$1$1') === 'a☃☃' && '☃☃'.replace(/☃/g, 'x') === 'xx');
// A global pattern's matches all come before any replacer runs.
{ let r = /a/g; let log = []; let result = 'aXa'.replace(r, (m, p) => { log.push(p + ':' + r.lastIndex); return '' + p; }); row(result === '0X2' && log.join() === '0:0,2:0'); }
// replaceAll with a global RegExp is replace; flags are read from `flags`.
{ let r = /b/g; let reads = 0; Object.defineProperty(r, 'flags', { get() { reads++; return 'g'; } }); row('abcb'.replaceAll(r, 'X') === 'aXcX' && reads === 2); }
{ let r = /b/g; Object.defineProperty(r, 'flags', { value: '' }); row('abcb'.replace(r, 'X') === 'aXcb' && r.lastIndex === 2 && name(() => 'b'.replaceAll(r, 'X')) === "TypeError"); }
row(name(() => 'a'.replaceAll(/a/y, '')) === "TypeError" && 'aa'.replaceAll(/a/gy, '-') === '--');
// No match: the string itself; a sticky search fails at lastIndex.
row('abc'.replace(/z/g, 'x') === 'abc' && 'abc'.replace(/b/y, 'x') === 'abc');
// Templates and replacers with many captures.
row('abcdefghijkl'.replace(/(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)(l)/, '$12$11$10$9$8$7$6$5$4$3$2$1$13') === 'lkjihgfedcbaa3');
{ let n; 'abc'.replace(/(a)(x)?(c)?/, function () { n = arguments.length; return ''; }); row(n === 6); }
rows = failed === 0 ? rows : -failed;
rows;
