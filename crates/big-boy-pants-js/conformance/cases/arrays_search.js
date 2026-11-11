// bbp-authored conformance case (light-cljs-908).
// Derives from: rust/tests/fixtures/js/array_search.js lines 1-38 (without its two BigInt rows, light-cljs-771), rust/tests/js_source.rs lines 29903-29927 (array_search_observable_order_and_errors, without its Uint8Array row, light-cljs-769, its BigInt index and, in find over a string, the check that the callback's third argument is an object: QuickJS passes the primitive), rust/tests/fixtures/js/proxy_has.js line 119 (at through that proxy is left out: QuickJS's at asks the has trap first), and light-cljs-767's planner row
// (Array.prototype.indexOf, lastIndexOf, includes, at, find, findIndex, findLast and findLastIndex: fromIndex conversions and their order after the length, an empty array converting none, strict equality against SameValueZero, holes, inherited elements, getters, array-likes and strings, huge lengths, mutation during find, and a proxy receiver's has trap). Completes with "ok", or the number of the first failed check.
(function () {
  var failed = 0;
  function check(n, ok) { if (!ok && failed === 0) { failed = n; } }
  check(1, (function () {
    let a = [1, 2, 3, 2];
    let ok = a.indexOf(2) === 1 && a.lastIndexOf(2) === 3 && a.includes(3);
    ok = ok && a.indexOf(2, -2) === 3 && a.lastIndexOf(2, -2) === 1;
    ok = ok && a.indexOf(1, -99) === 0 && a.lastIndexOf(2, 99) === 3;
    ok = ok && a.indexOf(2, Infinity) === -1 && a.lastIndexOf(2, -Infinity) === -1;
    ok = ok && a.lastIndexOf(2, undefined) === -1 && a.lastIndexOf(2) === 3;
    ok = ok && a.includes(1, -Infinity) && !a.includes(1, Infinity);
    ok = ok && a.at(-1) === 2 && a.at(-4) === 1 && a.at(-5) === undefined;
    ok = ok && a.at(4) === undefined && a.at(NaN) === 1 && a.at(1.9) === 2;
    ok = ok && a.at(Infinity) === undefined && a.at(-Infinity) === undefined;
    let holes = new Array(3);
    ok = ok && holes.indexOf(undefined) === -1 && holes.lastIndexOf(undefined) === -1;
    ok = ok && holes.includes(undefined) && [NaN].includes(NaN) && [NaN].indexOf(NaN) === -1;
    ok = ok && [0].includes(-0) && [-0].indexOf(0) === 0;
    ok = ok && [1].indexOf('1') === -1 && ![1].includes('1');
    let seen = '', ctx = {x: 7};
    let found = holes.find(function(v, i, o) { seen += i; return this === ctx && o === holes && i === 2 && v === undefined; }, ctx);
    ok = ok && found === undefined && seen === '012';
    seen = '';
    ok = ok && holes.findLastIndex((v, i) => { seen += i; return i === 0; }) === 0 && seen === '210';
    ok = ok && a.find(x => x > 1) === 2 && a.findIndex(x => x > 1) === 1;
    ok = ok && a.findLast(x => x > 1) === 2 && a.findLastIndex(x => x > 1) === 3;
    ok = ok && a.find(() => false) === undefined && a.findLast(() => false) === undefined;
    ok = ok && a.findIndex(() => false) === -1 && a.findLastIndex(() => false) === -1;
    let o = Object.create({1: 'inherited'}); o.length = 3; o[2] = 'last';
    ok = ok && Array.prototype.indexOf.call(o, 'inherited') === 1;
    ok = ok && Array.prototype.lastIndexOf.call(o, 'inherited') === 1;
    ok = ok && Array.prototype.includes.call(o, undefined) && Array.prototype.at.call(o, -1) === 'last';
    ok = ok && Array.prototype.find.call(o, v => v === 'inherited') === 'inherited';
    ok = ok && Array.prototype.at.call('abc', -1) === 'c' && Array.prototype.indexOf.call('abc', 'b') === 1;
    for (let name of ['indexOf', 'lastIndexOf', 'includes', 'find', 'findIndex', 'findLast', 'findLastIndex', 'at']) {
        let d = Object.getOwnPropertyDescriptor(Array.prototype, name);
        ok = ok && d.value.name === name && d.value.length === 1 && d.writable && d.configurable && !d.enumerable;
    }
    return ok;
  })());
  check(10, (function () { let log = ''; let o = {get length() { log += 'l'; return {valueOf() { log += 'n'; return 2; }}; }, get 0() { log += 'a'; return 7; }, get 1() { log += 'b'; return 8; }}; let f = {valueOf() { log += 'i'; return 0; }}; return (Array.prototype.indexOf.call(o, 8, f) === 1 && log === 'lniab'); })());
  check(11, (function () { let n = 0, f = {valueOf() { n++; return 0; }}; return ([].indexOf(1,f) === -1 && [].lastIndexOf(1,f) === -1 && ![].includes(1,f) && n === 0 && [].at(f) === undefined && n === 1); })());
  check(12, (function () { let n = 0; for (let method of ['find','findIndex','findLast','findLastIndex']) { let o = {get length() { n++; return 0; }}; try { Array.prototype[method].call(o, null); } catch(e) { if (!(e instanceof TypeError)) throw e; n++; } } return (n === 8); })());
  check(13, (function () { let n = 0; for (let method of ['indexOf','lastIndexOf','includes','find','findIndex','findLast','findLastIndex','at']) { for (let receiver of [null,undefined]) { try { Array.prototype[method].call(receiver, () => true); } catch(e) { if (e instanceof TypeError) n++; } } } return (n === 16); })());
  check(14, (function () { let a = [{x:1},{x:2}], n = 0; let r = a.find((v,i) => { n++; delete a[i]; return true; }); return (r.x === 1 && n === 1); })());
  check(15, (function () { let a = [1,2,3], seen = ''; let r = a.find((v,i,o) => { seen += String(v); if (i === 0) { o.length = 1; o.push(7); o.push(8); o.push(9); } return false; }); return (r === undefined && seen === '178'); })());
  check(16, (function () { let a = [1,2,3], seen = ''; a.findLast((v,i,o) => { seen += String(v); if (i === 2) delete o[1]; return false; }); return (seen === '3undefined1'); })());
  check(17, (function () { let n = 0; let o = {length: 3, get 0() { n++; return 1; }, get 1() { throw 42; }}; return (Array.prototype.includes.call(o,1) && n === 1); })());
  check(18, (function () { let n = 0; try { [1,2].find(v => { n++; throw 42; }); } catch(e) { n += e; } return (n === 43); })());
  check(19, (function () { let n = 0; try { [1].includes(1, Symbol()); } catch(e) { if (e instanceof TypeError) n++; } return (n === 1); })());
  check(20, (function () { let o = {length: -3, get 0() { throw 99; }}; return (Array.prototype.at.call(o,0) === undefined && Array.prototype.indexOf.call(o,undefined) === -1); })());
  check(21, (function () { let o = {length: Infinity, 9007199254740990: 'x'}; return (Array.prototype.at.call(o,-1) === 'x' && Array.prototype.lastIndexOf.call(o,'x') === 9007199254740990 && Array.prototype.indexOf.call(o,'x',9007199254740990) === 9007199254740990); })());
  check(22, (function () { let o = {x:2}; return ([1,2,3].find(function(v) { return v === this.x; }.bind(o)) === 2); })());
  check(23, (function () { let a = []; let result = Array.prototype.find.call('xy',function(v,i,o) { a.push(o[i] === v); return i === 1; }); return (result === 'y' && a.join() === 'true,true'); })());
  check(24, (function () { let a = [undefined]; return (a.indexOf(undefined) === 0 && a.lastIndexOf(undefined) === 0 && a.findIndex(v => v === undefined) === 0); })());
  check(25, (function () { let a = [1,2,3]; return (a.indexOf(1,-0.5) === 0 && a.lastIndexOf(1,-0.5) === 0 && a.includes(1,NaN) && a.at(-0.5) === 1); })());
  check(26, (function () { let n=0; for (let method of ['indexOf','lastIndexOf','includes','find','findIndex','findLast','findLastIndex','at']) { try { new Array.prototype[method](); } catch(e) { if (e instanceof TypeError) n++; } } return (n === 8); })());
  check(40, (function () {  return ([[1,2,NaN].indexOf(NaN), [1,2,NaN].includes(NaN), [1,2,1].lastIndexOf(1), [1,2,3].find(x => x > 1), [1,2,3].findLast(x => x < 3), [1,2,3].findIndex(x => x > 5), [1,2,3].at(-1)].join() === '-1,true,2,2,2,-1,3'); })());
  check(41, (function () { function hidden(target, key) {return new Proxy(target, {has(t, k) {return k !== key && k in t;}});} let p = hidden([1, 2, 3], '1'); return (Array.prototype.indexOf.call(p, 2) === -1 && Array.prototype.lastIndexOf.call(p, 2) === -1 && Array.prototype.includes.call(p, 2)); })());
  check(42, (function () { let log = ''; let p = new Proxy([5, 6], {has(t, k) { log += 'h' + String(k); return k in t; }, get(t, k, r) { log += 'g' + String(k); return Reflect.get(t, k, r); }}); return (Array.prototype.indexOf.call(p, 6) === 1 && Array.prototype.includes.call(p, 6) && log === 'glengthh0g0h1g1glengthg0g1'); })());
  check(43, (function () { let a = []; for (let i = 0; i < 5000; i++) a.push(i); return (a.indexOf(4999) === 4999 && a.lastIndexOf(0) === 0 && a.includes(4999) && !a.includes(-1) && Array.prototype.indexOf.call({length: 5000, 4999: 'x'}, 'x') === 4999); })());
  return failed === 0 ? "ok" : failed;
})();
