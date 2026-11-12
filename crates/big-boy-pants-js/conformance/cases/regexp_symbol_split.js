// bbp-authored conformance case (light-cljs-937).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 23249-23256 (string_split_regexp_separators), 30905-30926 (regexp_symbol_dispatch, its split rows), 30929-30960 (regexp_symbol_protocol_edges, its split rows) and light-cljs-770's planner rows:
// RegExp.prototype[@@split] and String.prototype.split's delegation to it.
// @@split: ToString of the argument, SpeciesConstructor (`constructor`,
// then @@species), `flags`, the splitter constructed from the receiver and
// the flags with "y", ToUint32 of the limit (undefined: 2^32 - 1, 0: []),
// then the sticky scan: an empty string is [] when the splitter matches
// it, else [S]; otherwise at each position lastIndex is set and the
// splitter's exec runs; a match whose lastIndex (ToLength, at most the
// length) is past the last piece's end appends the piece before it and
// each capture below the result's `length`, the limit stopping it after
// any element; the rest is the last piece. An empty match advances by a
// code point with "u" or "v". Left out: case-insensitive patterns
// (light-cljs-936) and the Pants engine's work limits. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
const split = RegExp.prototype[Symbol.split];
// light-cljs-770's planner rows.
row(['a, b ,c'.split(/\s*,\s*/).join('|'), 'a1b2c'.split(/(\d)/).join('|'), 'abc'.split(/(?:)/u).length, 'test'.split(/t/, 1).length, 'abc'.split(/b/y).join('|'), ''.split(/x/).length, ''.split(/(?:)/).length, 'ab'.split(/(?:)/, 0).length].join('/') === "a|b|c/a|1|b|2|c/3/1/a|c/1/0/0");
{ let log = 0; class R extends RegExp { static get [Symbol.species]() { log++; return RegExp; } } row('a,b'.split(new R(',')).join('|') === 'a|b' && log === 1); }
// string_split_regexp_separators (js_source.rs 23249).
row('a,b'.split(/,/).join('|') === 'a|b' && 'a,b'.split(RegExp(',')).join('|') === 'a|b' && 'a,b'.split(new RegExp(',')).join('|') === 'a|b');
// regexp_symbol_dispatch (js_source.rs 30905).
row('a1b2'.split(/\d/).join('|') === 'a|b|');
{ let x = { [Symbol.split](s, n) { return s + n; } }; row('a'.split(x, 4) === 'a4'); }
row('ab'.split(/(b)/).join('|') === 'a|b|' && 'ab'.split(/(?:)/).join('|') === 'a|b' && ''.split(/a/).length === 1 && ''.split(/(?:)/).length === 0);
{ let seen = ''; let r = /,/; r.constructor = { [Symbol.species]: function (rx, f) { seen = f; return /,/y; } }; row('a,b'.split(r).join('|') === 'a|b' && seen === 'y'); }
// regexp_symbol_protocol_edges (js_source.rs 30929).
{ let r = /a/; r[Symbol.split] = null; row('x/a/y'.split(r).join('|') === 'x|y'); }
{ let seen = ''; let r = /a/; r.constructor = { [Symbol.species]: () => {} }; Object.defineProperty(r, 'flags', { get() { seen += 'f'; return ''; } }); row(name(() => 'a'.split(r, 0)) === "TypeError" && seen === ''); }
{ let r = /,/g; r.lastIndex = 2; row('a,b,c'.split(r, 3).join('|') === 'a|b|c' && r.lastIndex === 2); }
{ let a = 'ab'.split(/(z)?b/); row(a.length === 3 && a[0] === 'a' && a[1] === undefined && a[2] === ''); }
row('\u{1F431}x'.split(/(?:)/u).join('|') === '\u{1F431}|x' && 'ab'.split(/(b)/, 2).join('|') === 'a|b' && '\u{1F431}x'.split(/(?:)/).length === 3);
{ let d = Object.getOwnPropertyDescriptor(RegExp.prototype, Symbol.split); row(d.value.name === '[Symbol.split]' && d.value.length === 2 && d.writable && !d.enumerable && d.configurable); }
// Receivers and limits.
row(name(() => split.call('a', 'a')) === "TypeError" && name(() => split.call(undefined, 'a')) === "TypeError");
row('a1b2c3'.split(/(\d)/, 3).join('|') === 'a|1|b' && 'a1b2c3'.split(/(\d)/, 4).join('|') === 'a|1|b|2' && 'a1b'.split(/\d/, -1).length === 2 && 'abc'.split(/(?:)/, 2.9).join() === 'a,b');
row('abc'.split(/b/, { valueOf() { return 1; } }).join() === 'a' && 'aXbXc'.split(/X/, 4294967297).join() === 'a');
row(split.call(/-/, 'a-b-c').join() === 'a,b,c' && split.call(/-/, 'a-b-c', 2).join() === 'a,b' && split.call(/(-)/, 7).join() === '7');
// A separator at the ends, and matches that are empty or end at the last piece.
row(',a,'.split(/,/).join('|') === '|a|' && 'aaa'.split(/a*/).join('|') === '|' && 'aaa'.split(/a*?/).join('|') === 'a|a|a' && 'abc'.split(/$/).join('|') === 'abc' && 'abc'.split(/^/m).join('|') === 'abc');
row('a\nb'.split(/^/m).join('|') === 'a\n|b' && 'one two  three'.split(/\s+/).length === 3 && 'a1b22c'.split(/(\d)(\d)?/).join('|') === 'a|1||b|2|2|c');
// The reads of @@split on a generic receiver, in order.
{
  let log = []; let last = 0;
  let splitter = {
    set lastIndex(v) { log.push('set' + v); last = v; }, get lastIndex() { log.push('get' + last); return last; },
    exec(s) { log.push('exec' + last); if (last === 1) { last = 2; return { length: 2, get 1() { log.push('cap'); return 'C'; } }; } return null; }
  };
  let rx = { get constructor() { log.push('ctor'); return { [Symbol.species]: function (r, f) { log.push('new:' + f); return splitter; } }; }, get flags() { log.push('flags'); return 'u'; } };
  let out = split.call(rx, { toString() { log.push('arg'); return 'abc'; } }, { valueOf() { log.push('limit'); return 10; } });
  row(out.join() === 'a,C,c' && log.join() === 'arg,ctor,flags,new:uy,limit,set0,exec0,set1,exec1,get2,cap,set2,exec2');
}
// lastIndex past the end is the end; at the last piece's end it advances.
{ let n = 0; let s = { lastIndex: 0, exec() { if (n++ === 1) { this.lastIndex = 99; return {}; } return null; } }; row(split.call({ constructor: { [Symbol.species]: function () { return s; } }, flags: '' }, 'abcd').join() === 'a,'); }
{ let s = { lastIndex: 0, exec() { this.lastIndex = 0; return { length: 1 }; } }; row(split.call({ constructor: { [Symbol.species]: function () { return s; } }, flags: '' }, 'ab').join() === 'ab'); }
// An empty string: one exec without a lastIndex write.
{ let log = []; let s = { set lastIndex(v) { log.push('set'); }, get lastIndex() { return 0; }, exec(t) { log.push('exec:' + t); return null; } }; let out = split.call({ constructor: { [Symbol.species]: function () { return s; } }, flags: '' }, ''); row(out.length === 1 && out[0] === '' && log.join() === 'exec:'); }
// Exec results must be objects or null; captures are taken as they are.
row(name(() => split.call({ constructor: { [Symbol.species]: function () { return { exec() { return 1; } }; } }, flags: '' }, 'ab')) === "TypeError");
{ let o = {}; let n = 0; let s = { lastIndex: 0, exec() { if (n++ === 0) { this.lastIndex = 1; return { length: 3, 1: o, 2: 5 }; } return null; } }; let out = split.call({ constructor: { [Symbol.species]: function () { return s; } }, flags: '' }, 'xy'); row(out.length === 4 && out[0] === '' && out[1] === o && out[2] === 5 && out[3] === 'y'); }
// The species constructor is consulted once, and its value constructed with (rx, flags + "y").
{ let args; let C = function (r, f) { args = [r, f]; return /b/y; }; let rx = /b/g; rx.constructor = { [Symbol.species]: C }; row('abc'.split(rx).join() === 'a,c' && args[0] === rx && args[1] === 'gy'); }
{ let rx = /b/; rx.constructor = { [Symbol.species]: 1 }; row(name(() => 'abc'.split(rx)) === "TypeError"); }
{ let rx = /b/; rx.constructor = 5; row(name(() => 'abc'.split(rx)) === "TypeError"); }
rows = failed === 0 ? rows : -failed;
rows;
