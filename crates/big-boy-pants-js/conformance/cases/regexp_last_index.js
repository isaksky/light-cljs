// bbp-authored conformance case (light-cljs-933).
// Derives from: crates/big-boy-pants-js/rust/tests/fixtures/js/regexp_objects.js (js_source.rs line 23002, its lastIndex rows), rust/tests/fixtures/js/regexp_indices.js (line 22796, its lastIndex rows), rust/tests/js_source.rs lines 23132-23180 (regexp_coercions_wait_yield_collect_and_migrate, its lastIndex conversion rows without waits) and light-cljs-770's planner rows:
// RegExpBuiltinExec's lastIndex: ToLength(Get(R, "lastIndex")) read and
// converted on every exec and test (a valueOf runs, also without `g` or
// `y`, where the value is not used and not written); `g` searches from it,
// `y` only at it; a failure with `g` or `y` sets it to 0 (also past the
// end), a match to the match's end; writes use Set, so a non-writable or
// frozen lastIndex is a TypeError. Left out: case-insensitive patterns
// (light-cljs-936) and String.prototype's matching methods. Each row is a
// `row(...)` check; the script completes with the number of rows, or
// minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-770).
{ let r = /a/g; r.exec('aa'); let one = r.lastIndex; r.exec('aa'); let two = r.lastIndex; let third = r.exec('aa'); row(one === 1 && two === 2 && third === null && r.lastIndex === 0); }
row(/a/y.test('ba') === false);
{ let r = /a/y; r.lastIndex = 1; row(r.test('ba') === true && r.lastIndex === 2); }
// regexp_objects.js
{ let r = /a/g; row(r.exec('aba').index === 0 && r.lastIndex === 1 && r.exec('aba').index === 2 && r.lastIndex === 3 && r.exec('aba') === null && r.lastIndex === 0); }
{ let r = /a/y; row(r.exec('ba') === null); r.lastIndex = 1; row(r.test('ba') && r.lastIndex === 2); }
// Without g or y lastIndex is read and converted but neither used nor written.
{ let r = /a/; r.lastIndex = 5; row(r.exec('a').index === 0 && r.lastIndex === 5 && r.test('a') && r.lastIndex === 5); }
{ let r = /a/; r.lastIndex = 'x'; row(r.test('a') && r.lastIndex === 'x'); }
{ let n = 0; let r = /a/; let v = { valueOf() { n++; return 1; } }; r.lastIndex = v; let m = r.exec('aa'); row(n === 1 && m.index === 0 && r.lastIndex === v); }
{ let n = 0; let r = /a/; r.lastIndex = { valueOf() { n++; return 1; } }; r.test('a'); r.test('b'); row(n === 2); }
// With g: read once per exec, through valueOf, from the converted index.
{ let n = 0; let r = /a/g; r.lastIndex = { valueOf() { n++; return 1; } }; let m = r.exec('ba'); row(n === 1 && m.index === 1 && r.lastIndex === 2); }
{ let log = ''; let r = /a/g; r.lastIndex = { valueOf() { log += 'v'; return 0; }, toString() { log += 's'; return '1'; } }; r.exec('aa'); row(log === 'v' && r.lastIndex === 1); }
{ let r = /a/g; r.lastIndex = { [Symbol.toPrimitive](hint) { return hint === 'number' ? 1 : 0; } }; row(r.exec('aa').index === 1); }
{ let r = /a/g; r.lastIndex = { valueOf() { throw new RangeError('x'); } }; row(name(() => r.exec('a')) === "RangeError"); }
{ let r = /a/g; r.lastIndex = Symbol(); row(name(() => r.exec('a')) === "TypeError"); }
// ToLength: negative, NaN, fractional, string and huge values.
{ let r = /a/g; r.lastIndex = -5; row(r.exec('a').index === 0 && r.lastIndex === 1); }
{ let r = /a/g; r.lastIndex = NaN; row(r.exec('a').index === 0); }
{ let r = /a/g; r.lastIndex = 1.9; row(r.exec('aaa').index === 1 && r.lastIndex === 2); }
{ let r = /a/g; r.lastIndex = '2'; row(r.exec('aaa').index === 2 && r.lastIndex === 3); }
{ let r = /a/g; r.lastIndex = 2 ** 60; row(r.exec('a') === null && r.lastIndex === 0); }
{ let r = /a/g; r.lastIndex = Infinity; row(r.test('a') === false && r.lastIndex === 0); }
{ let r = /a/g; r.lastIndex = null; row(r.exec('a').index === 0); }
// Past the end: null and 0, with an empty pattern matching at the end.
{ let r = /a/g; r.lastIndex = 2; row(r.exec('a') === null && r.lastIndex === 0); }
{ let r = /(?:)/g; r.lastIndex = 1; let m = r.exec('a'); row(m.index === 1 && m[0] === '' && r.lastIndex === 1); }
{ let r = /$/g; row(r.exec('ab').index === 2 && r.lastIndex === 2 && r.exec('ab').index === 2); }
// Sticky: only at lastIndex; a failure resets it.
{ let r = /b/y; r.lastIndex = 0; row(r.exec('ab') === null && r.lastIndex === 0); r.lastIndex = 1; row(r.exec('ab')[0] === 'b' && r.lastIndex === 2); }
{ let r = /a/gy; r.lastIndex = 1; row(r.exec('aba') === null && r.lastIndex === 0); }
{ let r = /^a/y; r.lastIndex = 1; row(r.test('aa') === false && r.lastIndex === 0); }
{ let s = /^a/my; s.lastIndex = 1; let first = s.test('a\na'); s.lastIndex = 2; row(first === false && s.test('a\na') && s.lastIndex === 3); }
{ let r = /a|ab/y; r.lastIndex = 1; let m = r.exec('xab'); row(m[0] === 'a' && m.index === 1 && r.lastIndex === 2); }
// A global loop collects every match.
{ let r = /\d+/g; let found = []; let m; while ((m = r.exec('a1b22c333')) !== null) found.push(m[0] + '@' + m.index); row(found.join() === '1@1,22@3,333@6' && r.lastIndex === 0); }
{ let r = /x*/g; let ends = []; for (let i = 0; i < 3; i++) { let m = r.exec('ab'); ends.push(m.index + ':' + r.lastIndex); r.lastIndex = m.index + 1; } row(ends.join() === '0:0,1:1,2:2'); }
// test with g advances and resets lastIndex too.
{ let r = /a/g; row(r.test('aa') && r.lastIndex === 1 && r.test('aa') && r.lastIndex === 2 && !r.test('aa') && r.lastIndex === 0); }
// Writes use Set: non-writable lastIndex throws, the value unchanged.
{ let r = /a/g; Object.defineProperty(r, 'lastIndex', { writable: false }); row(name(() => r.exec('a')) === "TypeError" && r.lastIndex === 0); }
{ let r = /a/g; Object.defineProperty(r, 'lastIndex', { value: 3, writable: false }); row(name(() => r.exec('a')) === "TypeError" && r.lastIndex === 3); }
{ let r = /a/y; Object.freeze(r); row(name(() => r.test('b')) === "TypeError"); }
{ let r = /a/; Object.freeze(r); row(r.exec('a')[0] === 'a' && r.test('b') === false); }
{ let r = /a/g; Object.defineProperty(r, 'lastIndex', { value: 0, writable: false }); row(name(() => r.test('b')) === "TypeError"); }
// lastIndex is per object, also for each evaluation of one literal.
{ let make = () => /a/g; let x = make(); let y = make(); x.exec('aa'); row(x.lastIndex === 1 && y.lastIndex === 0 && y.exec('aa').index === 0); }
// Indices with lastIndex (regexp_indices.js).
{ let a = /a/d; a.lastIndex = 99; let m = a.exec('ba'); row(m.indices[0][0] === 1 && m.indices[0][1] === 2 && a.lastIndex === 99); }
rows = failed === 0 ? rows : -failed;
rows;
