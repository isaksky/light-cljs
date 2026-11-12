// bbp-authored conformance case (light-cljs-937).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 30905-30926 (regexp_symbol_dispatch, its match and search rows), 30929-30960 (regexp_symbol_protocol_edges, its match and search rows) and light-cljs-770's planner rows:
// String.prototype.match and search and RegExp.prototype[@@match] and
// [@@search]. String's: RequireObjectCoercible, then a regexp neither
// undefined nor null has its method read and, when found, called with the
// receiver (the receiver's ToString is the method's); otherwise ToString of
// the receiver and a RegExp made from the regexp, whose method is invoked.
// @@match: ToString of the argument, `flags`, then RegExpExec once without
// "g", or with it lastIndex 0 and each match's ToString(result[0])
// collected, an empty one advancing lastIndex (Get, ToLength, Set); null
// when none. @@search: lastIndex saved, set to 0 unless SameValue, one
// exec, restored unless SameValue, then -1 or the result's `index`. Left
// out: case-insensitive patterns (light-cljs-936), the Pants engine's work
// limits, which QuickJS does not share, and primitive regexps' method
// reads (QuickJS reads objects only). Each row is a `row(...)` check; the
// script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
const match = RegExp.prototype[Symbol.match], search = RegExp.prototype[Symbol.search];
// light-cljs-770's planner rows.
row(['a1b22c333'.match(/\d+/g).join(), 'abc'.match(/x/), 'abc'.match(/(b)/).index, 'abc'.match('b').index, 'a.b'.search('.'), 'abcb'.search(/b/), 'abc'.match(/x/g)].join('|') === "1,22,333||1|1|0|1|");
{ let o = { [Symbol.search](s) { return 'S' + s; } }; row('x'.search(o) === 'Sx'); }
row('abc'.match(/x/g) === null);
{ let r = /b/g; r.lastIndex = 2; row('abc'.search(r) === 1 && r.lastIndex === 2); }
// regexp_symbol_dispatch (js_source.rs 30905).
row('a1b2'.match(/\d/g).join() === '1,2');
row('abc'.search(/c/) === 2);
{ let x = { flags: 'g', [Symbol.match](s) { return s + 1; }, [Symbol.search](s) { return s + 3; } }; row('a'.match(x) === 'a1' && 'a'.search(x) === 'a3'); }
{ let r = /b/g; r.lastIndex = -0; row('abc'.search(r) === 1 && Object.is(r.lastIndex, -0)); }
{ let x = { exec() { return 3; }, flags: '' }; row(name(() => match.call(x, 'a')) === "TypeError"); }
row('abc'.match('b')[0] === 'b' && 'abc'.search('b') === 1);
row('\u{1F431}'.match(/(?:)/gu).length === 2 && '\u{1F431}'.match(/(?:)/g).length === 3);
// regexp_symbol_protocol_edges (js_source.rs 30929).
{ let r = /a/; r.exec = new Proxy(RegExp.prototype.exec, {}); row('a'.match(r)[0] === 'a'); }
{ let r = /a/; r.exec = new Proxy(RegExp.prototype.exec, { apply() { return null; } }); row('a'.match(r) === null); }
{ let n = 0; let x = { [Symbol.search]: 1 }; row(name(() => String.prototype.search.call({ toString() { n++; return 'a'; } }, x)) === "TypeError" && n === 0); }
{ let r = /a/g; r.exec = 1; row('aba'.match(r).join() === 'a,a'); }
{ let r = { lastIndex: 3, exec() { this.lastIndex = 8; return { index: 'answer' }; } }; row(search.call(r, 'a') === 'answer' && r.lastIndex === 3); }
{ let r = { lastIndex: 3, exec() { throw 42; } }; let caught = false; try { search.call(r, 'a'); } catch (e) { caught = e === 42; } row(caught && r.lastIndex === 0); }
row(String.prototype.match.length === 1 && String.prototype.search.length === 1 && String.prototype.match.name === 'match' && String.prototype.search.name === 'search');
for (const [s, n] of [[Symbol.match, '[Symbol.match]'], [Symbol.search, '[Symbol.search]']]) {
  let d = Object.getOwnPropertyDescriptor(RegExp.prototype, s);
  row(d.value.name === n && d.value.length === 1 && d.writable && !d.enumerable && d.configurable);
}
// Receivers: String's must be coercible; the symbol methods' an object.
row(name(() => String.prototype.match.call(null, /a/)) === "TypeError" && name(() => String.prototype.search.call(undefined, /a/)) === "TypeError");
row(name(() => match.call('a', 'a')) === "TypeError" && name(() => search.call(1, 'a')) === "TypeError");
row(String.prototype.match.call(123, /2/).index === 1 && String.prototype.search.call(true, /u/) === 2);
// No regexp, a primitive or an object without the method: a RegExp is made from it.
row('abc'.match()[0] === '' && 'abc'.match().index === 0 && 'abc'.search() === 0 && 'abc'.match(null) === null && 'a1'.search(1) === 1);
row('a.b'.match({ toString() { return '\\.'; } })[0] === '.' && 'x/a/y'.search({ toString() { return 'a'; } }) === 2);
{ let r = /a/; r[Symbol.match] = 7; row(name(() => 'a'.match(r)) === "TypeError"); }
// Non-global @@match answers RegExpExec's result itself.
{ let o = {}; row(match.call({ flags: '', exec() { return o; } }, 'a') === o); }
row(name(() => match.call({ flags: 'g', exec() { return 'x'; } }, 'a')) === "TypeError");
// The reads of a global @@match, in order.
{
  let log = []; let n = 0; let last = 0;
  let rx = {
    get flags() { log.push('flags'); return { toString() { log.push('text'); return 'g'; } }; },
    set lastIndex(v) { log.push('set' + v); last = v; }, get lastIndex() { log.push('get' + last); return last; },
    get exec() { log.push('exec'); return (s) => { log.push('call:' + s); return n++ < 2 ? { get 0() { log.push('0'); return n === 1 ? 'x' : ''; } } : null; }; }
  };
  let out = match.call(rx, { toString() { log.push('arg'); return 'ab'; } });
  row(out.join() === 'x,' && out.length === 2 && log.join() === 'arg,flags,text,set0,exec,call:ab,0,exec,call:ab,0,get0,set1,exec,call:ab');
}
// An empty match advances by a code point with "u" (lastIndex converted).
{
  let last = 0; let n = 0; let seen = [];
  let rx = { flags: 'gu', get lastIndex() { return { valueOf() { return last; } }; }, set lastIndex(v) { seen.push(v); last = v; }, exec() { return n++ < 2 ? { 0: { toString() { return ''; } } } : null; } };
  row(match.call(rx, '\u{1F431}x').length === 2 && seen.join() === '0,2,3');
}
// The reads of @@search, in order.
{
  let log = []; let last = 5;
  let rx = { get lastIndex() { log.push('get' + last); return last; }, set lastIndex(v) { log.push('set' + v); last = v; }, get exec() { log.push('exec'); return () => { last = 9; return { get index() { log.push('index'); return 7; } }; }; } };
  row(search.call(rx, 'q') === 7 && log.join() === 'get5,set0,exec,get9,set5,index');
}
{ let log = []; let rx = { lastIndex: 0, exec() { log.push('exec'); return null; } }; row(search.call(rx, 'q') === -1 && log.join() === 'exec' && rx.lastIndex === 0); }
// A RegExp's own exec and lastIndex: search leaves lastIndex, match resets it.
{ let r = /o/y; r.lastIndex = 1; row('foo'.search(r) === -1 && r.lastIndex === 1); }
{ let r = /o/g; r.lastIndex = 2; row('foo'.match(r).join() === 'o,o' && r.lastIndex === 0); }
{ let r = /(o)(x)?/; let m = 'foo'.match(r); row(m.index === 1 && m.input === 'foo' && m[1] === 'o' && m[2] === undefined && m.length === 3); }
rows = failed === 0 ? rows : -failed;
rows;
