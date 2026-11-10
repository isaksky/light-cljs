// bbp-authored conformance case (light-cljs-933).
// Derives from: crates/big-boy-pants-js/rust/tests/fixtures/js/regexp_objects.js (js_source.rs line 23002, regexp_objects_exec_and_test, and its second assertion), rust/tests/fixtures/js/regexp_indices.js (line 22796, regexp_has_indices), rust/tests/js_source.rs lines 22943-22955 (regexp_property_literals_execute_when_evaluated) and 23132-23180 (regexp_coercions_wait_yield_collect_and_migrate, its exec and test rows without waits), and light-cljs-770's planner rows:
// RegExp.prototype.exec's match results (the groups' strings, undefined
// for unmatched groups, `index`, `input`, `groups` with a null prototype
// and duplicate names, `indices` with `d`, their attributes and key
// order), exec's receiver check before its ToString, and test's RegExpExec
// (a user `exec`, read by a getter, its result checked; a non-callable
// one falls back to the built-in on a RegExp). Left out: case-insensitive
// patterns (light-cljs-936), String.prototype's matching methods
// (light-cljs-934 and -937) and the Pants engine's step-limit rows, which
// QuickJS does not share. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
function same(a, b) {
  if (a.length !== b.length) return false;
  for (let i = 0; i < b.length; i++) { if (a[i] !== b[i] || !(i in a)) return false; }
  return true;
}
// The planner's rows (light-cljs-770).
{ let m = /(b+)(c)?/.exec('abbbd'); row([m.length, m[0], m[1], m[2], m.index, m.input, Array.isArray(m), m.groups].join('|') === "3|bbb|bbb||1|abbbd|true|"); }
{ let m = /(?<y>\d{4})-(?<m>\d\d)/.exec('on 2024-05'); row(m.index === 3 && m.groups.y === '2024' && m.groups.m === '05' && Object.getPrototypeOf(m.groups) === null && Object.keys(m.groups).join('/') === 'y/m'); }
{ let m = /(?:(?<a>x)|(?<a>y))\k<a>/.exec('yy'); row(m.length === 3 && m[0] === 'yy' && m[1] === undefined && 1 in m && m[2] === 'y' && m.groups.a === 'y'); }
{ let m = /a(?<z>b)(c)?/d.exec('xab'); row([m.indices[0], m.indices[1], m.indices[2], m.indices.groups.z, m.indices.length].join('|') === "1,3|2,3||2,3|3"); }
{ let r = /x/; r.exec = function () { return {0: 'z', length: 1, index: 0}; }; row(r.test('q') === true); }
{ let r = /x/; r.exec = 5; row(r.test('x') === true && r.test('y') === false); }
{ let r = /x/; r.exec = function () { return 7; }; row(name(() => r.test('x')) === "TypeError"); }
row(name(() => RegExp.prototype.exec.call({}, 'a')) === "TypeError");
row(RegExp.prototype.test.call({exec: () => null}, 'a') === false);
row([/\1(a)/.exec('aa')[0], /(?:(a)|b)\1/.exec('b')[0], /(a)|\1b/.exec('b')[0]].join() === "a,b,b");
row([/a{2,3}/.exec('aaaa')[0], /a{2,3}?/.exec('aaaa')[0], /(a*)*/.exec('b')[0].length, /(a*?)+?b/.exec('aab')[0]].join() === "aaa,aa,0,aab");
// regexp_objects.js (QuickJS's test_regexp rows there).
{ let str = 'abbbbbc'; let a = /(b+)c/.exec(str); row(a[0] === 'bbbbbc' && a[1] === 'bbbbb' && a.index === 1 && a.input === str && a.groups === undefined && /(b+)c/.test(str)); }
row(/\x61/.exec('a')[0] === 'a' && /a/.exec('a')[0] === 'a' && /\ca/.exec('\x01')[0] === '\x01' && /\\a/.exec('\\a')[0] === '\\a' && /\c0/.exec('\\c0')[0] === '\\c0');
{ let a = /(\.(?=com|org)|\/)/.exec('ah.com'); row(a.index === 2 && a[0] === '.' && /(\.(?!com|org)|\/)/.exec('ah.com') === null); }
{ let a = /(?=(a+))/.exec('baaabac'); row(a.index === 1 && a[0] === '' && a[1] === 'aaa'); }
{ let a = /(z)((a+)?(b+)?(c))*/.exec('zaacbbbcac'); row(same(a, ['zaacbbbcac', 'z', 'ac', 'a', undefined, 'c']) && a.length === 6 && 4 in a && a[4] === undefined); }
{ let a = RegExp('\0a'); row(a.toString() === '/\0a/' && a.exec('\0a')[0] === '\0a'); }
{ let a = /\0a/; row(a.toString() === '/\\0a/' && a.exec('\0a')[0] === '\0a'); }
row(same(/a{1+/.exec('a{11'), ['a{11']) && same(/(?:(?=(abc)))a/.exec('abc'), ['a', 'abc']));
row(same(/(?:(?=(abc)))?a/.exec('abc'), ['a', undefined]) && same(/(?:(?=(abc))){0,2}a/.exec('abc'), ['a', undefined]));
row(same(/(?:|[\w])+([0-9])/.exec('123a23'), ['123a23', '3']) && /()*?a/.exec(',') === null);
row(/[\q{a\b}]/.test('a\b') && /[\b]/.test('\b'));
// The result's own properties: elements, then index, input and groups
// (and indices), each writable, enumerable and configurable.
{ let m = /a(b)/.exec('xab'); row(Object.keys(m).join() === "0,1,index,input,groups"); }
{ let m = /a/d.exec('a'); row(Object.keys(m).join() === "0,index,input,groups,indices" && Object.keys(m.indices).join() === "0,groups"); }
{
  let m = /a(?<n>b)/d.exec('ab'); let ok = true;
  for (let k of ['index', 'input', 'groups', 'indices', '0', '1']) { let d = Object.getOwnPropertyDescriptor(m, k); if (!(d.writable && d.enumerable && d.configurable)) ok = false; }
  let g = Object.getOwnPropertyDescriptor(m.groups, 'n');
  row(ok && g.value === 'b' && g.writable && g.enumerable && g.configurable && m.length === 2 && Object.getPrototypeOf(m) === Array.prototype);
}
{ let r = /a/; row(r.exec('a') !== r.exec('a') && r.exec('b') === null); }
// Named groups: an unmatched one is an own undefined; duplicate names in
// exclusive alternatives give the one that matched, first-declared order.
{ let m = /(?<n>x)?y/.exec('y'); row(m.groups.n === undefined && Object.keys(m.groups).join() === 'n' && m.length === 2); }
{ let m = /(?<a>x)|(?<b>y)|(?<a>z)/.exec('z'); row(Object.keys(m.groups).join() === 'a,b' && m.groups.a === 'z' && m.groups.b === undefined && m[1] === undefined && m[3] === 'z'); }
{ let m = /(?<a>x)|(?<b>y)|(?<a>z)/.exec('x'); row(m.groups.a === 'x' && m[3] === undefined); }
{ let m = /(?<__proto__>a)(?<é>b)/.exec('ab'); row(m.groups.__proto__ === 'a' && m.groups['é'] === 'b' && Object.keys(m.groups).join() === '__proto__,é'); }
{ let m = /(?<n>.)(?<k>\k<n>)/.exec('xyy'); row(m.index === 1 && m.groups.k === 'y'); }
// Indices (regexp_indices.js rows).
{ let r = /a(b)?(c)/d.exec('xac'); row(r.indices.length === 3 && r.indices[1] === undefined && 1 in r.indices && r.indices[0][0] === 1 && r.indices[0][1] === 3 && r.indices[2][0] === 2 && r.indices.groups === undefined); }
{ let r = /(?<n>a)|(?<n>b)/d.exec('b'); row(r.indices[1] === undefined && r.indices.groups.n === r.indices[2] && r.indices.groups.n[0] === 0 && r.indices.groups.n[1] === 1); }
{ let r = /(?=(ab))a/d.exec('xab'); row(r.indices[1][0] === 1 && r.indices[1][1] === 3 && /()/d.exec('x').indices[1][1] === 0 && /(a)+/d.exec('xaa').indices[1][0] === 2); }
{ let a = /(?<n>a)/d; let first = a.exec('a'); let second = a.exec('a'); row(first.indices !== second.indices && first.indices[1] !== second.indices[1] && Array.isArray(first.indices[1])); }
row(/a/.exec('a').indices === undefined && Object.getOwnPropertyDescriptor(/a/.exec('a'), 'indices') === undefined);
// exec checks its receiver, then converts its argument with ToString.
{ let log = ''; row(name(() => RegExp.prototype.exec.call({}, { toString() { log += 't'; return 'a'; } })) === "TypeError" && log === ''); }
{ let log = ''; let m = /b/.exec({ toString() { log += 't'; return 'abc'; } }); row(log === 't' && m.index === 1 && m.input === 'abc'); }
{ let r = /a/g; row(r.exec({ toString() { return 'a'; } })[0] === 'a' && r.lastIndex === 1); }
row(/null/.test(null) && /undefined/.test() && /^undefined$/.exec(undefined)[0] === 'undefined' && /1/.test(1) && /true/.test(true) && /\[object Object\]/.test({}));
row(name(() => /a/.exec(Symbol())) === "TypeError" && name(() => /a/.test(Symbol())) === "TypeError");
row(name(() => RegExp.prototype.exec.call(RegExp.prototype, 'a')) === "TypeError" && name(() => RegExp.prototype.exec.call('a', 'a')) === "TypeError");
// test: an object receiver, Get(R, "exec"), its result checked; a
// non-callable exec needs a RegExp. (QuickJS reads `exec` before ToString
// of the argument, which it passes to a user exec unconverted: the order
// and the converted argument are left out.)
row(name(() => RegExp.prototype.test.call(1, 'a')) === "TypeError" && name(() => RegExp.prototype.test.call(undefined, 'a')) === "TypeError");
{ let log = ''; let o = { get exec() { log += 'e'; return () => ({}); } }; row(RegExp.prototype.test.call(o, 'a') === true && log === 'e'); }
{ let r = /a/; let calls = []; r.exec = function (s) { calls.push(this === r, s); return null; }; row(r.test('5') === false && calls.join() === "true,5"); }
{ let r = /a/; Object.defineProperty(r, 'exec', { get() { return undefined; } }); row(r.test('a') === true && r.test('b') === false); }
row(name(() => RegExp.prototype.test.call({exec: 1}, 'a')) === "TypeError" && name(() => RegExp.prototype.test.call({}, 'a')) === "TypeError");
row(RegExp.prototype.test.call({exec: () => []}, 'a') === true && name(() => RegExp.prototype.test.call({exec: () => undefined}, 'a')) === "TypeError" && name(() => RegExp.prototype.test.call({exec: () => 'x'}, 'a')) === "TypeError");
{ let r = /b/; r.exec = RegExp.prototype.exec; row(r.test('abc') === true); }
{ let r = /b/; r.exec = function (s) { return RegExp.prototype.exec.call(/c/, s); }; row(r.test('abc') === true && r.test('ab') === false); }
// Subclasses: exec is the built-in on the instance; an overriding exec is
// what test calls.
{ class R extends RegExp {} let r = new R('(a)', 'g'); let m = r.exec('ba'); row(m.index === 1 && m[1] === 'a' && r.lastIndex === 2 && r.test('a') === false && r.lastIndex === 0); }
{ class R extends RegExp { exec(s) { return s === 'yes' ? [] : null; } } let r = new R('a'); row(r.test('yes') === true && r.test('a') === false); }
// Property escapes execute when evaluated (regexp_property_literals_execute_when_evaluated).
row(/\p{RGI_Emoji}/v.test('😀') && /\p{Script=Greek}/u.test('α') && !/\p{Script=Greek}/u.test('a') && (false && /\p{Script_Extensions=Grek}/u) === false);
// The function objects.
row(RegExp.prototype.exec.length === 1 && RegExp.prototype.exec.name === 'exec' && RegExp.prototype.test.length === 1 && RegExp.prototype.test.name === 'test');
{ let d = Object.getOwnPropertyDescriptor(RegExp.prototype, 'exec'); let e = Object.getOwnPropertyDescriptor(RegExp.prototype, 'test'); row(d.writable && !d.enumerable && d.configurable && typeof d.value === 'function' && e.writable && !e.enumerable && e.configurable); }
rows = failed === 0 ? rows : -failed;
rows;
