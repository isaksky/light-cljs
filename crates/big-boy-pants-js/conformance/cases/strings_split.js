// bbp-authored conformance case (light-cljs-904).
// Derives from: rust/tests/js_source.rs lines 7689-8105 and 30494-30507
// bbp-authored slice (light-cljs-904); string_split and
// string_split_coercion_and_boundaries in rust/tests/js_source.rs.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function error(f) { try { f(); } catch(e) { return e.name; } return ''; }
for (let pair of [
  ['a,b,c'.split(','), ['a','b','c']], [',b,c'.split(','), ['', 'b', 'c']], ['a,b,'.split(','), ['a','b','']],
  ['aaaa'.split(), ['aaaa']], ['aaaa'.split(undefined, 0), []], ['aaaa'.split(''), ['a','a','a','a']],
  ['aaaa'.split('', 0), []], ['aaaa'.split('', 1), ['a']], ['aaaa'.split('', 2), ['a','a']],
  ['aaaa'.split('a'), ['','','','','']], ['aaaa'.split('a', 2), ['','']], ['aaaa'.split('aa'), ['','','']],
  ['aaaa'.split('aa', 0), []], ['aaaa'.split('aa', 1), ['']], ['aaaa'.split('aa', 2), ['','']],
  ['aaaa'.split('aaa'), ['','a']], ['aaaa'.split('aaaa'), ['','']], ['aaaa'.split('aaaaa'), ['aaaa']],
  ['aaaa'.split('aaaaa', 0), []], ['aaaa'.split('aaaaa', 1), ['aaaa']], [''.split(''), []], [''.split(','), ['']],
  ['a,b'.split(',', -1), ['a','b']], ['a1b1c'.split(1), ['a','b','c']]
]) { let a = pair[0], b = pair[1]; row(Object.getPrototypeOf(a) === Array.prototype && a.length === b.length && a.join('|') === b.join('|')); }
row('abababa'.split('aba').join('|') === '|b|' && 'aaaaa'.split('aa').join('|') === '||a');
row('a,b,c'.split(',', 2.9).join('|') === 'a|b' && 'a,b'.split(',', 4294967297).join('|') === 'a');
row('a,b'.split(',', NaN).length === 0 && 'a,b'.split(',', Infinity).length === 0 && 'a,b'.split(',', null).length === 0);
row('a,b'.split(',', -4294967296).length === 0 && 'a,b'.split(',', -1.9).length === 2);
row('undefinedxundefined'.split(undefined).join('|') === 'undefinedxundefined' && 'undefinedxundefined'.split({toString() { return undefined; }}).join('|') === '|x|');
row('a,b'.split(',', {valueOf() { return undefined; }}).length === 0);
row(String.prototype.split.call(121, 2).join('|') === '1|1' && String.prototype.split.call(true, 'r').join('|') === 't|ue');
{ let log = ''; let r = {toString() { log += 'r'; return 'a,b'; }}, l = {valueOf() { log += 'l'; return 0; }}, s = {toString() { log += 's'; return ','; }}; row(String.prototype.split.call(r, s, l).length === 0 && log === 'rls'); }
{ let hit = 0, caught = false; try { String.prototype.split.call(undefined, {toString() { hit++; }}, {valueOf() { hit++; }}); } catch(e) { caught = true; } row(caught && hit === 0); }
{ let hit = 0; row(error(() => 'a,b'.split({toString() { hit++; return ','; }}, {valueOf() { throw 43; }})) !== '' && hit === 0); }
row(error(() => 'a,b'.split({toString() { throw 42; }}, 0)) !== '');
{ let a = 'a,b'.split(','), b = 'a,b'.split(','); a[0] = 'c'; let d = Object.getOwnPropertyDescriptor(b, '0'); row(a !== b && b[0] === 'a' && Object.keys(b).join('|') === '0|1' && d.writable && d.enumerable && d.configurable); }
row('😀'.split('')[0] === '\ud83d' && '😀'.split('')[1] === '\ude00');
{ let log = ''; let receiver = {toString() { throw 9; }}; let limit = {}; let sep = {get [Symbol.split]() { log += 'g'; return function(s, lim) { log += 'c'; return this === sep && s === receiver && lim === limit; }; }}; row(String.prototype.split.call(receiver, sep, limit) && log === 'gc'); }
{ let log = ''; let sep = {[Symbol.split](s, lim) { log += s + lim; return 7; }}; row('ab'.split(sep, 3) + log === '7ab3'); }
row(error(() => 'a'.split({[Symbol.split]: 1})) === 'TypeError');
row('a,b'.split({[Symbol.split]: null, toString() { return ','; }}).join('|') === 'a|b');
for (let x of [null, undefined]) row(error(() => String.prototype.split.call(x, ',')) === 'TypeError');
row(error(() => 'a'.split(Symbol())) === 'TypeError' && error(() => 'a'.split(',', Symbol())) === 'TypeError');
{ let d = Object.getOwnPropertyDescriptor(String.prototype, 'split'); row(d.writable && !d.enumerable && d.configurable && d.value.name === 'split' && d.value.length === 2); }
row('a'.repeat(2048).concat('b').split('aaaab')[0].length === 2044);
row('x'.repeat(1024).split('').length === 1024);
failed === 0 ? rows : -failed;
