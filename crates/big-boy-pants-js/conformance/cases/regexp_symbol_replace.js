// bbp-authored conformance case (light-cljs-934).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 30905-30926 (regexp_symbol_dispatch, its replace rows), 30929-30957 (regexp_symbol_protocol_edges, its replace rows) and light-cljs-770's planner rows:
// RegExp.prototype[@@replace] as a generic protocol on any object: ToString
// of the argument, then of a non-callable replacement, then Get(rx,
// "flags") and ToString; with "g" Set(rx, "lastIndex", 0) and RegExpExec
// until null (a user `exec`, its result an object or null; an empty match
// advancing lastIndex through Get, ToLength and Set), every exec before
// any replacement; then each result's length, "0", index (clamped),
// captures and groups read and converted in that order; a result whose
// position is before the end of the last one used is dropped; a template
// reads `$<name>` from ToObject(groups) (null is a TypeError). Left out:
// case-insensitive patterns (light-cljs-936) and the Pants engine's work
// limits, which QuickJS does not share. Each row is a `row(...)` check;
// the script completes with the number of rows, or minus the first
// failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
const replace = RegExp.prototype[Symbol.replace];
// regexp_symbol_dispatch (js_source.rs 30905).
{ let r = /a/g; let n = 0; r.exec = function (s) { return n++ === 0 ? { 0: 'x', length: 1, index: 1 } : null; }; row('abc'.replace(r, '[$&]') === 'a[x]c' && n === 2); }
{ let r = /a/; r[Symbol.match] = false; r[Symbol.replace] = () => 7; row('a'.replaceAll(r, 'b') === 7); }
{ let r = /a/; let caught = 0; try { 'a'.replaceAll(r, 'b'); } catch (e) { caught += e instanceof TypeError; } row(caught === 1); }
// regexp_symbol_protocol_edges (js_source.rs 30929).
{ let n = 0; let r = { flags: 'g', exec() { return n++ < 2 ? { 0: 'xx', length: 1, index: n - 1 } : null; } }; row(replace.call(r, 'a', 'X') === 'X'); }
{ let n = 0, calls = 0; let r = { flags: 'g', exec() { return n++ < 2 ? { 0: 'xx', length: 1, index: n - 1 } : null; } }; row(replace.call(r, 'a', () => { calls++; return 'X'; }) === 'X' && calls === 2); }
{ let r = /a/; r[Symbol.replace] = null; row('x/a/y'.replace(r, 'X') === 'xXy'); }
{ let rx = { flags: '', exec() { return { 0: 'XY', 1: { toString() { return 'C'; } }, length: 2, index: 1, groups: { '0': 'N' } }; } }; row(replace.call(rx, 'abcde', '$1-$<0>-$&') === 'aC-N-XYde'); }
{ let n = 0; let rx = { flags: 'g', exec() { return n++ < 2 ? { 0: 'x', length: 1, index: 0 } : null; } }; let calls = 0; row(replace.call(rx, 'ab', () => { calls++; return 'X'; }) === 'Xb' && calls === 2); }
{ let rx = { flags: '', exec() { return { 0: 'x', length: 1, index: 0, groups: null }; } }; row(name(() => replace.call(rx, 'a', '$&')) === "TypeError"); }
{ let d = Object.getOwnPropertyDescriptor(RegExp.prototype, Symbol.replace); row(d.value.name === '[Symbol.replace]' && d.value.length === 2 && d.writable && !d.enumerable && d.configurable); }
// The receiver must be an object; exec's result an object or null; no
// callable exec needs a RegExp.
row(name(() => replace.call('a', 'a', 'b')) === "TypeError" && name(() => replace.call(undefined, 'a', 'b')) === "TypeError");
row(name(() => replace.call({ flags: '', exec() { return 1; } }, 'a', 'b')) === "TypeError" && name(() => replace.call({ flags: '', exec: 3 }, 'a', 'b')) === "TypeError");
row(replace.call({ flags: '', exec() { return null; } }, 'abc', 'x') === 'abc');
{ let r = /b/; r.exec = 'not callable'; row(replace.call(r, 'abc', 'x') === 'axc'); }
// The order of the reads before the first exec.
{
  let log = [];
  let rx = { get flags() { log.push('flags'); return { toString() { log.push('flagsText'); return 'g'; } }; }, set lastIndex(v) { log.push('set' + v); }, get lastIndex() { log.push('get'); return 0; }, get exec() { log.push('exec'); return () => null; } };
  let out = replace.call(rx, { toString() { log.push('text'); return 'ab'; } }, { toString() { log.push('replacement'); return 'X'; } });
  row(out === 'ab' && log.join() === "text,replacement,flags,flagsText,set0,exec");
}
// Every exec before any replacement; then each result's reads in order.
{
  let log = []; let n = 0;
  let result = (i) => ({ get length() { log.push('length' + i); return 3; }, get 0() { log.push('0:' + i); return 'b'; }, get index() { log.push('index' + i); return i * 2 + 1; }, get 1() { log.push('1:' + i); return undefined; }, get 2() { log.push('2:' + i); return 'c'; }, get groups() { log.push('groups' + i); return undefined; } });
  let rx = { flags: 'g', exec() { log.push('exec'); return n < 2 ? result(n++) : null; } };
  let out = replace.call(rx, 'abcbc', function (m, c1, c2, p, s) { log.push('call:' + [m, c1, c2, p, s].join('/')); return '<' + p + '>'; });
  row(out === 'a<1>c<3>c' && log.join() === "exec,0:0,exec,0:1,exec,length0,0:0,index0,1:0,2:0,groups0,call:b//c/1/abcbc,length1,0:1,index1,1:1,2:1,groups1,call:b//c/3/abcbc");
}
// An empty match advances lastIndex through Get, ToLength and Set.
{
  let log = []; let last = 0; let n = 0;
  let rx = { flags: 'gu', get lastIndex() { log.push('get' + last); return { valueOf() { return last; } }; }, set lastIndex(v) { log.push('set' + v); last = v; }, exec(s) { return n++ < 2 ? { 0: '', length: 1, index: last } : null; } };
  let out = replace.call(rx, '\u{1F431}', '-');
  row(out === '-\u{1F431}-' && log.join() === "set0,get0,set2,get2,set3");
}
// Positions clamp; a result before the end of the last one is dropped.
{ let n = 0; let rx = { flags: 'g', exec() { return [{ 0: 'a', index: -5 }, { 0: 'b', index: 99 }, { 0: 'c', index: 1 }][n++] || null; } }; row(replace.call(rx, 'xyz', '[$&]') === '[a]yz[b]'); }
{ let n = 0; let rx = { flags: 'g', exec() { return [{ 0: 'yz', index: 1 }, { 0: '', index: 2 }, { 0: 'q', index: 3 }][n++] || null; } }; row(replace.call(rx, 'xyz', '<$`|$\'>') === 'x<x|>' + '<xyz|>'); }
{ let n = 0; let rx = { flags: 'g', exec() { return [{ 0: 'x', index: 'NaN', length: { valueOf() { return 1.9; } } }, { 0: { toString() { return 'yz'; } }, index: { valueOf() { return 1.7; } } }][n++] || null; } }; row(replace.call(rx, 'xyz', '-') === '--'); }
// The matched string, not the input, gives `$&`, `$'` and the next position.
{ let rx = { flags: '', exec() { return { 0: 'abcdef', index: 1 }; } }; row(replace.call(rx, 'abc', "[$&|$']") === 'a[abcdef|]'); }
// Captures: length - 1 of them, undefined kept, others converted; groups
// converted with ToObject for a template.
{ let rx = { flags: '', exec() { return { 0: 'b', 1: 7, 2: undefined, 3: null, length: 4, index: 1 }; } }; row(replace.call(rx, 'abc', '[$1|$2|$3|$4]') === 'a[7||null|$4]c'); }
{ let got; let rx = { flags: '', exec() { return { 0: 'b', 1: 7, length: 2, index: 1, groups: 'G' }; } }; replace.call(rx, 'abc', function () { got = []; for (let i = 0; i < arguments.length; i++) got.push(arguments[i]); return ''; }); row(got.length === 5 && got[1] === '7' && got[4] === 'G'); }
{ let rx = { flags: '', exec() { return { 0: 'b', length: 1, index: 1, groups: 'xy' }; } }; row(replace.call(rx, 'abc', '[$<length>|$<0>]') === 'a[2|x]c'); }
{ let log = []; let groups = { get a() { log.push('a'); return { toString() { log.push('A'); return 'α'; } }; }, get b() { log.push('b'); return undefined; } }; let rx = { flags: '', exec() { return { 0: 'b', length: 1, index: 1, groups }; } }; row(replace.call(rx, 'abc', '$<b>$<a>$$<a>$<a') === 'aα$<a>$<ac' && log.join() === 'b,a,A'); }
rows = failed === 0 ? rows : -failed;
rows;
