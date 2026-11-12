// bbp-authored conformance case (light-cljs-937).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 30905-30926 (regexp_symbol_dispatch, its matchAll rows), 30929-30960 (regexp_symbol_protocol_edges, its matchAll and iterator rows) and light-cljs-770's planner rows:
// String.prototype.matchAll and RegExp.prototype[@@matchAll], and the
// RegExp String Iterator. matchAll: an object regexp that IsRegExp says is
// one needs `flags` with "g" (undefined or null flags, or none without "g",
// a TypeError); then its @@matchAll is called, or a RegExp is made with
// "g". @@matchAll: ToString of the argument, SpeciesConstructor (the
// `constructor`, then @@species: undefined and null give %RegExp%, a
// non-constructor is a TypeError), `flags`, the matcher constructed from
// the receiver and the flags, the receiver's lastIndex converted by
// ToLength onto the matcher, then the iterator. `next` runs RegExpExec on
// the matcher: null ends it, a match without "g" ends it after answering,
// and with "g" an empty match advances the matcher's lastIndex (code points
// with "u"). Left out: the order of the @@matchAll read and the flags
// check (QuickJS reads the method first), case-insensitive patterns
// (light-cljs-936) and the Pants engine's work limits. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
const matchAll = RegExp.prototype[Symbol.matchAll];
const IteratorPrototype = Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()));
// light-cljs-770's planner rows.
{ let it = 'a1b2'.matchAll(/\d/g); row(Object.prototype.toString.call(it) === "[object RegExp String Iterator]" && [...it].map(m => m[0] + m.index).join() === "11,23"); }
row([...'aXbX'.matchAll('X')].length === 2);
// regexp_symbol_dispatch (js_source.rs 30905).
{ let r = /a/g; r.lastIndex = 1; let it = 'aba'.matchAll(r); let a = it.next(), b = it.next(); row(a.value.index === 2 && !a.done && b.done && it.next().done && r.lastIndex === 1 && it[Symbol.iterator]() === it && it[Symbol.toStringTag] === 'RegExp String Iterator'); }
{ let x = { flags: 'g', [Symbol.match]() { return 1; }, [Symbol.matchAll](s) { return s + 2; } }; row('a'.matchAll(x) === 'a2'); }
{ let r = /a/; row(name(() => 'a'.matchAll(r)) === "TypeError"); }
row('ab'.matchAll('').next().value[0] === '');
row([...'\u{1F431}'.matchAll(/(?:)/gu)].length === 2 && [...'\u{1F431}'.matchAll(/(?:)/g)].length === 3);
// regexp_symbol_protocol_edges (js_source.rs 30929).
{ let p = Object.getPrototypeOf(''.matchAll(/x/g)); let count = 0; for (let receiver of [p, Object.prototype, {}, 1, undefined]) { try { p.next.call(receiver); } catch (e) { count += e instanceof TypeError; } } row(count === 5 && p.next.length === 0 && p.next.name === 'next'); }
{ let r = /a/g; r.constructor = { [Symbol.species]: null }; row([...'aba'.matchAll(r)].length === 2); }
{ let r = /a/; let it = matchAll.call(r, 'aba'); row(it.next().value.index === 0 && it.next().done); }
{ let rx; let r = /a/g; r.constructor = { [Symbol.species]: function () { rx = { lastIndex: 0, exec() { return { 0: 'a' }; } }; return rx; } }; let it = 'a'.matchAll(r); rx.exec = () => null; row(it.next().done); }
// The prototype: %IteratorPrototype%'s child, `next` and @@toStringTag.
{
  let p = Object.getPrototypeOf('a'.matchAll(/a/g));
  let d = Object.getOwnPropertyDescriptor(p, Symbol.toStringTag); let n = Object.getOwnPropertyDescriptor(p, 'next');
  row(Object.getPrototypeOf(p) === IteratorPrototype && d.value === 'RegExp String Iterator' && !d.writable && !d.enumerable && d.configurable && n.writable && !n.enumerable && n.configurable && Object.getOwnPropertyNames(p).join() === 'next');
}
{ let d = Object.getOwnPropertyDescriptor(RegExp.prototype, Symbol.matchAll); row(d.value.name === '[Symbol.matchAll]' && d.value.length === 1 && d.writable && !d.enumerable && d.configurable && String.prototype.matchAll.length === 1 && String.prototype.matchAll.name === 'matchAll'); }
// Receivers.
row(name(() => String.prototype.matchAll.call(null, /a/g)) === "TypeError" && name(() => matchAll.call('a', 'a')) === "TypeError");
row([...String.prototype.matchAll.call(1213, /1/g)].map(m => m.index).join() === '0,2');
// The flags check of an IsRegExp regexp: RequireObjectCoercible, ToString, "g".
row(name(() => 'a'.matchAll({ [Symbol.match]: true, flags: undefined })) === "TypeError" && name(() => 'a'.matchAll({ [Symbol.match]: true, flags: null })) === "TypeError");
row(name(() => 'a'.matchAll({ [Symbol.match]: true, flags: 'i' })) === "TypeError" && [...'ab'.matchAll({ [Symbol.match]: true, flags: 'g', source: 'b' })][0].index === 1);
{ let r = /a/; r[Symbol.match] = false; row([...'aa'.matchAll(r)].length === 1); }
{ let log = []; let x = { get [Symbol.match]() { log.push('match'); return true; }, get flags() { log.push('flags'); return { toString() { log.push('text'); return 'g'; } }; }, [Symbol.matchAll](s) { log.push('call:' + s); return 'ok'; } }; row('q'.matchAll(x) === 'ok' && log.join() === 'match,flags,text,call:q'); }
// No regexp, a primitive, or an object without @@matchAll: a RegExp with "g".
row([...'abc'.matchAll()].map(m => m.index).join() === '0,1,2,3' && [...'a1a'.matchAll(1)][0].index === 1 && [...'a.b'.matchAll({ toString() { return '\\.'; } })].length === 1);
// The reads of @@matchAll, in order; the matcher is the species' object.
{
  let log = []; let matcher;
  let C = function (r, f) { log.push('new:' + f); matcher = this; this.exec = function () { log.push('exec'); return null; }; };
  let rx = {
    get constructor() { log.push('ctor'); return { get [Symbol.species]() { log.push('species'); return C; } }; },
    get flags() { log.push('flags'); return 'gu'; },
    get lastIndex() { log.push('last'); return { valueOf() { log.push('valueOf'); return 3.5; } }; }
  };
  let it = matchAll.call(rx, { toString() { log.push('arg'); return 'xy'; } });
  let first = it.next();
  row(first.done && first.value === undefined && matcher.lastIndex === 3 && log.join() === 'arg,ctor,species,flags,new:gu,last,valueOf,exec' && it.next().done && log.length === 8);
}
row(name(() => matchAll.call({ constructor: 1 }, 'a')) === "TypeError" && name(() => matchAll.call({ constructor: { [Symbol.species]: () => 0 } }, 'a')) === "TypeError");
{ let r = /a/g; r.constructor = undefined; row([...'aa'.matchAll(r)].length === 2); }
// The matcher is a copy: the receiver's lastIndex is read, never written.
{ let r = /a/g; r.lastIndex = 2; let it = r[Symbol.matchAll]('aaaa'); row(it.next().value.index === 2 && r.lastIndex === 2); }
// next: a user exec's result is the value; empty matches advance lastIndex.
{
  let n = 0; let seen = [];
  let C = function () { this.lastIndex = 0; this.exec = function (s) { return n++ < 3 ? { 0: n === 2 ? 'x' : '', index: n } : null; }; Object.defineProperty(this, 'lastIndex', { get() { return 4; }, set(v) { seen.push(v); } }); };
  let it = matchAll.call({ constructor: { [Symbol.species]: C }, flags: 'g', lastIndex: 0 }, 'abcdef');
  let values = [...it].map(m => m.index + m[0]);
  row(values.join() === '1,2x,3' && seen.join() === '0,5,5');
}
{ let it = 'abc'.matchAll(/(?:)/g); row([...it].map(m => m.index).join() === '0,1,2,3' && it.next().done); }
{ let it = 'aXbX'.matchAll(/(?<x>X)/g); let m = it.next().value; row(m.index === 1 && m.groups.x === 'X' && m.input === 'aXbX' && it.next().value.index === 3 && it.next().done); }
// A non-global iterator answers once; an exec that throws leaves it running.
{ let n = 0; let C = function () { this.exec = function () { if (n++ === 0) throw new RangeError('x'); return { 0: 'y' }; }; }; let it = matchAll.call({ constructor: { [Symbol.species]: C }, flags: '', lastIndex: 0 }, 's'); row(name(() => it.next()) === "RangeError" && it.next().value[0] === 'y' && it.next().done && n === 2); }
row(name(() => matchAll.call({ constructor: { [Symbol.species]: function () { this.exec = () => 1; } }, flags: 'g', lastIndex: 0 }, 's').next()) === "TypeError");
rows = failed === 0 ? rows : -failed;
rows;
