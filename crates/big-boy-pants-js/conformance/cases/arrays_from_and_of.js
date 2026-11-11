// bbp-authored conformance case (light-cljs-911).
// Derives from: rust/tests/js_source.rs lines 3570-3618 (array_from_iterables_and_array_likes and array_from_iterator_protocol_errors_and_exhaustion), the rows both oracles pass, and light-cljs-767's planner rows
// (Array.from and Array.of: iterables through the iterator protocol, array-likes, strings, Sets and Maps, the map function with its index and thisArg, the map-function check before any read, the iterator closed when the map function throws and not when next throws, and Array.of's items). Completes with "ok", or the number of the first failed check.
(function () {
  var failed = 0;
  function check(n, ok) { if (!ok && failed === 0) { failed = n; } }
  check(1, (function () { let a = [1, 2, 3], b = Array.from(a); return (b !== a && b.length === 3 && b.join() === '1,2,3'); })());
  check(2, (function () {  return (Array.from({length: 2, 0: 'a', 1: 'b'}).join() === 'a,b'); })());
  check(3, (function () {  return (Array.from([1, 2], function(x, i) { return x * 10 + i; }).join() === '10,21'); })());
  check(4, (function () {  return (Array.from([1, 2], function(x, i) { return this.n + x + i; }, {n: 5}).join() === '6,8'); })());
  check(5, (function () { let n = 0; let a = Array.from({length: 2, 0: 1, 1: 2}, function(x, i) { n++; return this.offset + x * 10 + i; }, {offset: 100}); return (a.join() === '110,121' && n === 2); })());
  check(6, (function () {  return (Array.from.call(null, [1])[0] === 1 && Array.from.call(() => 0, {length: 1, 0: 2})[0] === 2 && Array.from.call(Math.floor, [3])[0] === 3); })());
  check(7, (function () { let n = 0; try { Array.from(null); } catch(e) { if (e instanceof TypeError) n++; } try { Array.from(undefined); } catch(e) { if (e instanceof TypeError) n++; } return (n === 2); })());
  check(8, (function () { let n = 0; let o = {}; o[Symbol.iterator] = function() { return {next() { n++; return {value: n, done: n > 3}; }}; }; let a = Array.from(o, function(x, i) { return n === i + 1 ? x : 99; }); return (a.join() === '1,2,3' && n === 4); })());
  check(9, (function () { let ok = false; try { Array.from([1], 5); } catch(e) { ok = e instanceof TypeError; } return (ok); })());
  check(10, (function () { let n = 0, c = 0; let o = {next() { return {value: ++n}; }, return() { c++; return {}; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o, function(x) { if (x === 2) throw 17; return x; }); } catch(e) { ok = e === 17; } return (ok && n === 2 && c === 1); })());
  check(11, (function () { let reads = 0, o = {}; Object.defineProperty(o, Symbol.iterator, {get() { reads++; throw 99; }}); let ok = false; try { Array.from(o, null); } catch(e) { ok = e instanceof TypeError; } return (ok && reads === 0); })());
  check(12, (function () { let a = Array.from({length: 2.9, 1: 'x'}); return (a.length === 2 && Object.getOwnPropertyDescriptor(a, '0').writable && a[0] === undefined && a[1] === 'x'); })());
  check(13, (function () {  return (Array.from({length: -1}).length === 0 && Array.from({length: NaN}).length === 0 && Array.from(5).length === 0 && Array.from(true).length === 0 && Array.from(Symbol()).length === 0); })());
  check(14, (function () { let ok = false; try { Array.from({length: Infinity}); } catch(e) { ok = e instanceof RangeError; } return (ok); })());
  check(15, (function () { let n = 0, o = {length: {valueOf() { n++; return 2; }}, 0: 3, 1: 4}; return (Array.from(o).join() === '3,4' && n === 1); })());
  check(16, (function () { let o = {length: 1, 0: 3}; o[Symbol.iterator] = null; return (Array.from(o)[0] === 3); })());
  check(17, (function () { let o = {length: 1}; o[Symbol.iterator] = 5; let ok = false; try { Array.from(o); } catch(e) { ok = e instanceof TypeError; } return (ok); })());
  check(18, (function () { let c = 0, o = {next() { throw 17; }, return() { c++; return {}; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o); } catch(e) { ok = e === 17; } return (ok && c === 0); })());
  check(19, (function () { let c = 0, o = {next() { return {get done() { throw 17; }}; }, return() { c++; return {}; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o); } catch(e) { ok = e === 17; } return (ok && c === 0); })());
  check(20, (function () { let c = 0, o = {next() { return {get value() { throw 17; }}; }, return() { c++; return {}; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o); } catch(e) { ok = e === 17; } return (ok && c === 0); })());
  check(21, (function () { let c = 0, o = {next() { return {value: 1}; }, get return() { c++; throw 99; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o, function() { throw 17; }); } catch(e) { ok = e === 17; } return (ok && c === 1); })());
  check(22, (function () { let c = 0, o = {next() { return {value: 1}; }, return() { c++; throw 99; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o, function() { throw 17; }); } catch(e) { ok = e === 17; } return (ok && c === 1); })());
  check(23, (function () { let o = {next() { return {value: 1}; }, return() { return 5; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o, function() { throw 17; }); } catch(e) { ok = e === 17; } return (ok); })());
  check(24, (function () { let d = Object.getOwnPropertyDescriptor(Array, 'from'); return (Array.from.name === 'from' && Array.from.length === 1 && d.writable && d.configurable && !d.enumerable); })());
  check(25, (function () { let c = 0, o = {next() { return 1; }, return() { c++; return {}; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o); } catch(e) { ok = e instanceof TypeError; } return (ok && c === 0); })());
  check(26, (function () { let c = 0, o = {next: 1, return() { c++; return {}; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o); } catch(e) { ok = e instanceof TypeError; } return (ok && c === 0); })());
  check(27, (function () { let n = 0, reads = 0, o = {next() { n++; return {done: true, get value() { reads++; throw 99; }}; }, get return() { reads++; throw 98; }}; o[Symbol.iterator] = function() { return this; }; let a = Array.from(o, function() { throw 97; }); return (a.length === 0 && n === 1 && reads === 0); })());
  check(200, (function () {  return (Array.from([1, 2], function (x) { return x + this.k }, {k: 10}).join() === '11,12'); })());
  check(201, (function () { let d = Object.getOwnPropertyDescriptor(Array, 'from'), e = Object.getOwnPropertyDescriptor(Array, 'of'); return (d.value.name === 'from' && d.value.length === 1 && e.value.name === 'of' && e.value.length === 0 && d.writable && d.configurable && !d.enumerable); })());
  check(202, (function () { let log = ''; let it = {[Symbol.iterator]() { return {i: 0, next() { log += 'n'; return this.i++ < 2 ? {done: false, value: this.i} : {done: true}; }, return() { log += 'r'; return {}; }}; }}; return (Array.from(it, x => x * 2).join() === '2,4' && log === 'nnn'); })());
  check(203, (function () { let log = ''; let it = {[Symbol.iterator]() { return {next() { log += 'n'; throw 7; }, return() { log += 'r'; return {}; }}; }}; let c; try { Array.from(it); } catch (e) { c = e; } return (c === 7 && log === 'n'); })());
  check(204, (function () { let t; try { Array.from([1], 5); } catch (e) { t = e instanceof TypeError; } let u; try { Array.from(null); } catch (e) { u = e instanceof TypeError; } return (t && u && Array.from({length: 3, 1: 'b'}).length === 3); })());
  check(205, (function () { let made; function C(n) { made = n; } let r = Array.of.call(C, 7, 8); return (r instanceof C && made === 2 && r[0] === 7 && r[1] === 8 && r.length === 2); })());
  return failed === 0 ? "ok" : failed;
})();
