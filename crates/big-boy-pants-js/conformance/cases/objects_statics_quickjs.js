// bbp-authored conformance case (light-cljs-884).
// Derives from: none (ECMAScript 20.1.2.13 Object.groupBy, 20.1.2.11
// Object.getOwnPropertySymbols, 20.1.3.4 Object.prototype.propertyIsEnumerable
// and 20.1.3.5 toLocaleString; QuickJS's js_object_groupBy,
// JS_GetOwnPropertyNames2 and js_object_propertyIsEnumerable
// (~/src/quickjs/quickjs.c); and the planner's rows (light-cljs-766)). The
// Pants engine lacks these four methods, so the case is scored against
// QuickJS alone (areas.edn :pants-gap). Left out: Symbol() (light-cljs-885),
// so the symbol rows use the well-known symbols; Map.groupBy
// (light-cljs-769). Errors are compared by name. Each row is a `row(...)`
// check; the script completes with the number of rows, or minus the first
// failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-766).
row(Object.groupBy([1, 2, 3], x => x % 2 ? 'odd' : 'even').odd.length === 2);
row(({}).toLocaleString() === "[object Object]");
row(({x: 1}).propertyIsEnumerable('x') === true);
row(Object.getOwnPropertySymbols({[Symbol.iterator]: 1}).length === 1);
// Object.groupBy: a null-prototype object of arrays in first-seen order,
// the callback called with (value, index) and its key converted.
{
  let r = []; let g = Object.groupBy('abca', (c, i) => { r.push(i); return c; });
  row(Object.getPrototypeOf(g) === null && g.a.join() === 'a,a' && r.join() === '0,1,2,3' && Object.keys(g).join() === 'a,b,c');
}
{ let g = Object.groupBy([1, 2, 3, 4], x => ({toString() { return x > 2 ? 'big' : 'small'; }})); row(g.big.join() === '3,4' && g.small.join() === '1,2'); }
{ let g = Object.groupBy([1.5, 2.5], x => x > 2); row(g.true.join() === '2.5' && g.false.join() === '1.5'); }
{ let g = Object.groupBy([], x => x); row(Object.keys(g).length === 0 && Object.groupBy.length === 2 && Object.groupBy.name === 'groupBy'); }
row(name(() => Object.groupBy([1], 1)) === "TypeError" && name(() => Object.groupBy(null, x => x)) === "TypeError" && name(() => Object.groupBy(1, x => x)) === "TypeError");
{
  let closed = 0, ok = false;
  let it = {[Symbol.iterator]() { return this; }, next() { return {value: 1}; }, return() { closed++; return {}; }};
  try { Object.groupBy(it, () => { throw 5; }); } catch (e) { ok = e === 5; }
  row(ok && closed === 1);
}
{
  let closed = 0, ok = false;
  let it = {[Symbol.iterator]() { return this; }, next() { return {value: 1}; }, return() { closed++; return {}; }};
  try { Object.groupBy(it, () => ({toString() { throw 6; }})); } catch (e) { ok = e === 6; }
  row(ok && closed === 1);
}
{ function* g() { yield 1; yield 2; } row(Object.groupBy(g(), x => 'k').k.join() === '1,2'); }
// Object.getOwnPropertySymbols: the symbol keys only, in creation order.
{
  let o = {[Symbol.iterator]: 1, a: 2, [Symbol.toStringTag]: 3, 1: 4};
  let s = Object.getOwnPropertySymbols(o);
  row(s.length === 2 && s[0] === Symbol.iterator && s[1] === Symbol.toStringTag && Object.getOwnPropertySymbols({a: 1}).length === 0);
}
row(Object.getOwnPropertySymbols('ab').length === 0 && name(() => Object.getOwnPropertySymbols(null)) === "TypeError" && Object.getOwnPropertySymbols.length === 1);
{ let o = {}; Object.defineProperty(o, Symbol.iterator, {value: 1}); row(Object.getOwnPropertySymbols(o).length === 1); }
// Object.prototype.propertyIsEnumerable: own and enumerable.
{
  let o = {x: 1, __proto__: {y: 2}}; Object.defineProperty(o, 'h', {value: 3});
  row(o.propertyIsEnumerable('x') && !o.propertyIsEnumerable('y') && !o.propertyIsEnumerable('h') && !o.propertyIsEnumerable('z'));
}
row([5].propertyIsEnumerable(0) && ![5].propertyIsEnumerable('length') && 'ab'.propertyIsEnumerable(1) && !'ab'.propertyIsEnumerable('length') && !(1).propertyIsEnumerable('x'));
row(!(function () {}).propertyIsEnumerable('prototype') && !Object.prototype.propertyIsEnumerable('toString'));
{ let n = 0, k = {toString() { n++; return 'x'; }}; row(({x: 1}).propertyIsEnumerable(k) && n === 1); }
row(name(() => Object.prototype.propertyIsEnumerable.call(null, 'x')) === "TypeError" && Object.prototype.propertyIsEnumerable.length === 1);
{ let o = {[Symbol.iterator]: 1}; row(o.propertyIsEnumerable(Symbol.iterator)); }
{
  let log = '', p = new Proxy({a: 1}, {getOwnPropertyDescriptor(t, k) { log += k; return Object.getOwnPropertyDescriptor(t, k); }});
  row(Object.prototype.propertyIsEnumerable.call(p, 'a') && !Object.prototype.propertyIsEnumerable.call(p, 'b') && log === 'ab');
}
// Object.prototype.toLocaleString: Invoke(this, "toString").
{ let o = {toString() { return 'T'; }}; row(o.toLocaleString() === 'T' && Object.prototype.toLocaleString.call(o) === 'T'); }
{ let seen; let o = {toString() { seen = this; return 1; }}; row(o.toLocaleString() === 1 && seen === o); }
row([1, 2].toString() === Object.prototype.toLocaleString.call([1, 2]) && Object.prototype.toLocaleString.call(true) === 'true');
row(name(() => Object.prototype.toLocaleString.call(null)) === "TypeError" && name(() => Object.prototype.toLocaleString.call({toString: 1})) === "TypeError");
{ let o = {get toString() { return () => 'G'; }}; row(o.toLocaleString() === 'G' && Object.prototype.toLocaleString.length === 0); }
failed === 0 ? rows : -failed;
