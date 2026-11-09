// bbp-authored conformance case (light-cljs-880).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 1920-1968, 2899-2989:
// object_destructuring (line 1920, bbp-132), array_destructuring (2899),
// array_destructuring_protocol_and_abrupt_completion (2923) and
// array_destructuring_nested_iterator_close (2957) (bbp-131): defaults,
// nesting, rest, computed keys, targets and their evaluation order, getters
// run once, TDZ in defaults, the iterator protocol, closing on abrupt
// completion and the error that wins. `Object.keys` counts are for-in counts
// here. Left out: rows using `Object.defineProperty` (light-cljs-881) or
// `Symbol()` (light-cljs-885), and three rows QuickJS at 7d1c13c gets wrong
// while the Pants engine and Goose follow the specification: a getter that
// deletes a later key during object rest (QuickJS still copies it), an
// elision `[,]` and a done result (both read `value`, which they should not).
// The last row is the planner's (light-cljs-766). Each row is a `row(...)`
// check; the script completes with the number of rows, or minus the first
// failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// object_destructuring (line 1920, bbp-132).
{
  let {a, b: c, d = 4, ...r} = {a: 1, b: 2, e: 5};
  row(a === 1 && c === 2 && d === 4 && r.e === 5 && !('a' in r));
}
{
  let {b = 10} = {};
  row(b === 10);
}
{
  let o = {}; ({x: o.y} = {x: 3});
  row(o.y === 3);
}
{
  let {length} = 1;
  row(length === undefined);
}
{
  let n = 0; try { let {q} = null; } catch(e) { n = e instanceof TypeError; }
  row(n);
}
{
  let n = 0; let {x} = {get x() { n++; return 7; }};
  row(n === 1 && x === 7);
}
row((({a, b}) => a + b)({a: 1, b: 2}) === 3 && (({a,b}) => 0).length === 1);
{
  let {a: [{b = 7}]} = {a: [{}]};
  row(b === 7);
}
{
  let [{x: [a]}] = [{x: [9]}];
  row(a === 9);
}
{
  var {a = 7} = {};
  row(a === 7);
}
{
  function f({a} = {a: 7}) { return a; }
  row(f() === 7 && f.length === 0);
}
{
  let a; try { throw {x: 7}; } catch({x}) { a = () => x; }
  row(a() === 7);
}
{
  let n = 0; try { let {} = undefined; } catch(e) { n = e instanceof TypeError; }
  row(n);
}
{
  let {x = 7, y = 8} = {x: null, y: false};
  row(x === null && y === false);
}
{
  let {0: a, length, ...r} = 'abc';
  row(a === 'a' && length === 3 && r[1] === 'b' && r[2] === 'c' && !('0' in r) && !('length' in r));
}
{
  let source = {a: 7}, a; let result = ({a} = source);
  row(result === source && a === 7);
}
{
  let a, r; ({a = 7, ...r} = {}); let n = 0; for (let k in r) n++;
  row(a === 7 && n === 0);
}
{
  let a; ({x: {y: [a = 7]}} = {x: {y: []}});
  row(a === 7);
}
{
  let {f = function() {}} = {};
  row(f.name === 'f');
}
{
  let failed = false; try { let {a = b, b} = {}; } catch(e) { failed = e instanceof ReferenceError; }
  row(failed);
}
{
  let log = ''; let k = {toString() { log += 'k'; return 'x'; }}; let source = {get x() { log += 'g'; return undefined; }}; let dest = {}; function lhs() { log += 'l'; return 'y'; } function def() { log += 'd'; return 7; } ({[k]: dest[lhs()] = def()} = source);
  row(log === 'klgd' && dest.y === 7);
}
{
  let n = 0; let key = {toString() { n++; return 'a'; }}; let {[key]: a, ...r} = {a: 1, b: 2};
  row(n === 1 && a === 1 && r.b === 2 && !('a' in r));
}
{
  let o = {__proto__: {a: 7}, b: 8}; let {a, ...r} = o;
  row(a === 7 && r.b === 8 && !('a' in r));
}
{
  let o = {get a() { this.c = 3; return 1; }, b: 2}; let {...r} = o;
  row(r.a === 1 && r.b === 2 && !('c' in r));
}
{
  let proto = {z: 9}; let o = {__proto__: proto, a: 1}; let {...r} = o;
  row(r.a === 1 && !('z' in r) && Object.getPrototypeOf(r) === Object.prototype);
}
{
  let n = 0; let o = {get a() { n++; return 1; }, get b() { n++; return 2; }}; let {a, ...r} = o;
  row(n === 2 && a === 1 && r.b === 2 && Object.getOwnPropertyDescriptor(r, 'b').get === undefined);
}
{
  let o = {}, r; ({...o.x} = {a: 7});
  row(o.x.a === 7);
}
{
  let {['x']: a, ...r} = {x: 1, y: 2};
  row(a === 1 && r.y === 2);
}
{
  let {1: a, ...r} = {1: 7, 2: 8};
  row(a === 7 && !('1' in r) && r[2] === 8);
}
{
  let o = {b: 1, 2: 2, 1: 3, a: 4}; let {...r} = o; let s = ''; for (let k in r) s += k + ',';
  row(s === '1,2,b,a,');
}
{
  let closed = 0; let it = {next() { return {value: {get a() { throw 17; }}}; }, return() { closed++; return {}; }}; it[Symbol.iterator] = function() { return this; }; let error; try { let [{a}] = it; } catch(e) { error = e; }
  row(error === 17 && closed === 1);
}
{
  let log = ''; let k = {toString() { log += 'k'; return 'x'; }}; let o = {}; let src = {get a() { log += 'g'; return undefined; }}; function def() { log += 'd'; return 7; } ({a: o[k] = def()} = src);
  row(o.x === 7 && log === 'gdk');
}
{
  let error = 0; let log = ''; function def() { log += 'd'; return 7; } try { ({a: null.x = def()} = {}); } catch(e) { error = e instanceof TypeError; }
  row(error && log === 'd');
}
{
  let result; for (let {['p' in {p: 1}]: x, a = 'q' in {q: 1}} = {true: 7}; result === undefined;) { result = x === 7 && a; }
  row(result);
}
// array_destructuring (line 2899, bbp-131).
{
  let [a, , b = 3, ...r] = [1, 2, undefined, 4, 5];
  row(a === 1 && b === 3 && r.length === 2 && r[0] === 4);
}
{
  let p = 1, q = 2; [p, q] = [q, p];
  row(p === 2 && q === 1);
}
{
  let it = {next() { return {done: true}; }}; it[Symbol.iterator] = function() { return this; }; let [x] = it;
  row(x === undefined);
}
{
  let n = 0; let [x = ++n] = [2];
  row(x === 2 && n === 0);
}
{
  let closed = 0; let it = {next() { return {value: 1}; }, return() { closed++; return {}; }}; it[Symbol.iterator] = function() { return this; }; let [x] = it;
  row(x === 1 && closed === 1);
}
{
  let closed = 0; let it = {next() { return {done: true}; }, return() { closed++; return {}; }}; it[Symbol.iterator] = function() { return this; }; let [x, y] = it;
  row(closed === 0 && x === undefined && y === undefined);
}
{
  let n = 0; try { let [z] = 1; } catch(e) { if (e instanceof TypeError) n++; } try { let [z] = null; } catch(e) { if (e instanceof TypeError) n++; }
  row(n === 2);
}
row((([a, b]) => a + b)([1, 2]) === 3);
{
  let r = false; try { throw [7]; } catch ([e]) { r = e === 7; }
  row(r);
}
{
  let [[d]] = [[1]];
  row(d === 1);
}
{
  let o = {}, a = []; [o.x, a[0]] = [3, 4];
  row(o.x === 3 && a[0] === 4);
}
{
  var [a, ...r] = [1, 2];
  row(a === 1 && r[0] === 2);
}
{
  function f([a, b] = [2, 3]) { return a + b; }
  row(f() === 5 && f.length === 0 && (([a,b]) => 0).length === 1);
}
{
  function* values([a, b] = [3, 4]) { yield a; yield b; return 9; } let [x, ...r] = values();
  row(x === 3 && r.length === 1 && r[0] === 4);
}
{
  function* finished([a] = [7]) { return a; } let [x] = finished();
  row(x === undefined);
}
// array_destructuring_protocol_and_abrupt_completion (line 2923, bbp-131).
{
  let n = 0; let it = {next() { n++; return {value: n}; }, return() { n += 10; return {}; }}; it[Symbol.iterator] = function() { return this; }; let [] = it;
  row(n === 10);
}
{
  let log = ''; let it = {next() { log += 'n'; return {value: undefined}; }, return() { log += 'r'; throw 99; }}; it[Symbol.iterator] = function() { return this; }; function fail() { throw 17; } let caught; try { let [x = fail()] = it; } catch(e) { caught = e; }
  row(caught === 17 && log === 'nr');
}
{
  let log = ''; let o = {set x(v) { log += 's'; throw 17; }}; let it = {next() { log += 'n'; return {value: 1}; }, return() { log += 'r'; return {}; }}; it[Symbol.iterator] = function() { return this; }; let caught; try { [o.x] = it; } catch(e) { caught = e; }
  row(caught === 17 && log === 'nsr');
}
{
  let log = ''; let o = {}; function key() { log += 'k'; return 'x'; } let it = {next() { log += 'n'; return {value: undefined}; }, return() { log += 'r'; return {}; }}; it[Symbol.iterator] = function() { return this; }; function def() { log += 'd'; return 3; } [o[key()] = def()] = it;
  row(o.x === 3 && log === 'kndr');
}
{
  let n = 0; let it = {next() { throw 17; }, return() { n++; return {}; }}; it[Symbol.iterator] = function() { return this; }; let error; try { let [x] = it; } catch(e) { error = e; }
  row(error === 17 && n === 0);
}
{
  let n = 0; let it = {next() { return {get value() { throw 17; }}; }, return() { n++; return {}; }}; it[Symbol.iterator] = function() { return this; }; let error; try { let [x] = it; } catch(e) { error = e; }
  row(error === 17 && n === 0);
}
{
  let n = 0; let it = {next() { return {value: 1}; }, return() { n++; return 1; }}; it[Symbol.iterator] = function() { return this; }; let error; try { let [x] = it; } catch(e) { error = e instanceof TypeError; }
  row(error && n === 1);
}
{
  let n = 0; let it = {next() { return {value: 1}; }, return() { n++; throw 17; }}; it[Symbol.iterator] = function() { return this; }; let error; try { let [x] = it; } catch(e) { error = e; }
  row(error === 17 && n === 1);
}
{
  let n = 0; function f([a = () => b, b = 2]) { return a(); }
  row(f([]) === 2);
}
{
  let caught = false; try { let [a = b, b] = []; } catch(e) { caught = e instanceof ReferenceError; }
  row(caught);
}
{
  let [f = function() {}] = [];
  row(f.name === 'f');
}
{
  let a = [1, 2]; let x; let v = ([x] = a);
  row(v === a && x === 1);
}
{
  let a; [[a] = [3]] = [];
  row(a === 3);
}
{
  let a; [...a /* , allowed in a comment */] = [1];
  row(a[0] === 1);
}
{
  let n = 0; for (let [a] = [2]; n < a; n++) {}
  row(n === 2);
}
{
  function f([a], c = a) { var a = 5; return c + a; }
  row(f([2]) === 7);
}
{
  let f; try { throw []; } catch([a = 3]) { f = () => a; }
  row(f() === 3);
}
{
  let log = ''; let key = {toString() { log += 'k'; return 'x'; }}; let o = {set x(v) { log += 's'; }}; let it = {next() { log += 'n'; return {}; }, return() { log += 'r'; return {}; }}; it[Symbol.iterator] = function() { return this; }; function def() { log += 'd'; return 7; } [o[key] = def()] = it;
  row(log === 'ndksr');
}
{
  let log = ''; let it = {next() { log += 'n'; return {}; }, return() { log += 'r'; return {}; }}; it[Symbol.iterator] = function() { return this; }; function def() { log += 'd'; return 7; } let caught = false; try { [null['x'] = def()] = it; } catch(e) { caught = e instanceof TypeError; }
  row(caught && log === 'ndr');
}
{
  let caught = 0; try { let [x] = undefined; } catch(e) { if (e instanceof TypeError) caught++; }
  row(caught === 1);
}
{
  let [a, ...r] = 'a\u{1f600}';
  row(a === 'a' && r.length === 1 && r[0].length === 2);
}
row(((c, [a, b] = 1, d) => 0).length === 1);
// array_destructuring_nested_iterator_close (line 2957, bbp-131): its four generated rows.
{
  let log = ''; let inner = {next() { return {}; }, return() { log += 'i'; return {}; }}; inner[Symbol.iterator] = function() { return this; }; let outer = {next() { return {value: inner}; }, return() { log += 'o'; return {}; }}; outer[Symbol.iterator] = function() { return this; }; function fail() { throw 17; } let caught; try { let [[x]] = outer; } catch(e) { caught = e; }
  row(log === 'io' && caught === undefined);
}
{
  let log = ''; let inner = {next() { return {}; }, return() { log += 'i'; return {}; }}; inner[Symbol.iterator] = function() { return this; }; let outer = {next() { return {value: inner}; }, return() { log += 'o'; return {}; }}; outer[Symbol.iterator] = function() { return this; }; function fail() { throw 17; } let caught; try { let [[x = fail()]] = outer; } catch(e) { caught = e; }
  row(log === 'io' && caught === 17);
}
{
  let log = ''; let inner = {next() { return {}; }, return() { log += 'i'; throw 18; }}; inner[Symbol.iterator] = function() { return this; }; let outer = {next() { return {value: inner}; }, return() { log += 'o'; throw 19; }}; outer[Symbol.iterator] = function() { return this; }; function fail() { throw 17; } let caught; try { let [[x = fail()]] = outer; } catch(e) { caught = e; }
  row(log === 'io' && caught === 17);
}
{
  let log = ''; let inner = {next() { return {}; }, return() { log += 'i'; throw 18; }}; inner[Symbol.iterator] = function() { return this; }; let outer = {next() { return {value: inner}; }, return() { log += 'o'; throw 19; }}; outer[Symbol.iterator] = function() { return this; }; function fail() { throw 17; } let caught; try { let [[x]] = outer; } catch(e) { caught = e; }
  row(log === 'io' && caught === 18);
}
// The planner's row (light-cljs-766).
{
  let {a, b: {c} = ({c: 3}), ...r} = {a: 1, x: 2, y: 3}; let s = ''; for (let k in r) s += k;
  row((a + ',' + c + ',' + s) === "1,3,xy");
}
failed === 0 ? rows : -failed;
