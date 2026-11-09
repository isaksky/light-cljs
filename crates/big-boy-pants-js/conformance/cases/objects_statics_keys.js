// bbp-authored conformance case (light-cljs-884).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 17640-17829 (own_property_key_order, bbp-123); reflect.rs lines 34-201:
// [[OwnPropertyKeys]] order through Object.keys, Object.getOwnPropertyNames
// and Reflect.ownKeys (integer indices ascending, then strings, then
// symbols, in creation order) for ordinary objects, arrays, functions,
// String wrappers and primitives after ToObject; EnumerableOwnProperties'
// enumerability; Reflect.has ([[HasProperty]]) and the methods' TypeErrors;
// and proxies' ownKeys and getOwnPropertyDescriptor traps (light-cljs-845's
// frame). Left out: `__rt` rows; Object.getOwnPropertyNames of
// Object.prototype, whose names differ between the engines, and the order of
// Object's own names after `name`; an array element at index 4294967294,
// which Goose's dense array model cannot hold; Symbol()
// (light-cljs-885), so the symbol rows use Symbol.iterator; a proxy's [[Get]]
// and [[HasProperty]] (light-cljs-890). Each row is a `row(...)` check; the
// script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// "length:k0,k1,...," with a `!` after any element that is not a string.
function listed(k) { let s = k.length + ':'; for (let i = 0; i < k.length; i++) { s = s + String(k[i]) + (typeof k[i] === 'string' ? ',' : '!,'); } return s; }
// The planner's rows (light-cljs-766).
row(Object.keys({b: 1, a: 2, 1: 3}).join() === "1,b,a");
row(Object.getOwnPropertyNames([1]).join() === "0,length");
row(Object.getOwnPropertyNames(function f(a, b) {}).join() === "length,name,prototype");
row(Object.keys('ab').join() + ';' + Object.entries([7]).join() === "0,1;0,7");
{ let {a, b: {c} = ({c: 3}), ...r} = {a: 1, x: 2, y: 3}; row(a + ',' + c + ',' + Object.keys(r).join() === "1,3,x,y"); }
row(Reflect.ownKeys({a: 1, b: 2}).join() === "a,b");
row(Reflect.has({a: 1}, 'a') === true);
// own_property_key_order (js_source.rs line 17640): the upstream rows.
{
  let a = {x: 1, "18014398509481984": 1, "9007199254740992": 1, "9007199254740991": 1, "4294967296": 1, "4294967295": 1, y: 1, "4294967294": 1, "1": 2};
  row(listed(Object.keys(a)) === "9:1,4294967294,x,18014398509481984,9007199254740992,9007199254740991,4294967296,4294967295,y,");
}
row(listed(Object.getOwnPropertyNames([ , 1])) === "2:1,length,");
// Index keys first, ascending; the rest in creation order.
row(listed(Object.keys({b: 1, a: 2, 1: 3, 0: 4})) === "4:0,1,b,a,");
{ let o = {}; o.x = 1; o[2] = 1; o['1'] = 1; o[-1] = 1; o[1.5] = 1; o['01'] = 1; o[4294967295] = 1; o[4294967294] = 1; row(listed(Object.keys(o)) === "8:1,2,4294967294,x,-1,1.5,01,4294967295,"); }
{
  let o = {}; for (let i = 20; i > 0; i--) { o['p' + i] = i; o[i] = i; }
  let expected = '40:'; for (let i = 1; i <= 20; i++) expected += i + ','; for (let i = 20; i >= 1; i--) expected += 'p' + i + ',';
  row(listed(Object.keys(o)) === expected);
}
// A redefinition keeps the first position; inherited keys are not own.
{ let o = {a: 1, b: 2}; o.a = 3; Object.defineProperty(o, 'a', {value: 4}); row(listed(Object.keys(o)) === "2:a,b,"); }
{ let o = Object.create({a: 1}); o.b = 2; row(listed(Object.keys(o)) === "1:b,"); }
// Non-enumerable properties are names but not keys.
{ let o = {a: 1}; Object.defineProperty(o, 'h', {value: 2}); o.z = 3; row(listed(Object.keys(o)) === "2:a,z," && listed(Object.getOwnPropertyNames(o)) === "3:a,h,z,"); }
row(listed(Object.keys(Object.freeze({a: 1, 1: 2}))) === "2:1,a,");
row(listed(Object.keys(Object.freeze([5, , 6]))) === "2:0,2,");
// Arrays: elements (holes absent), `length`, names.
row(listed(Object.keys([1, , 3])) === "2:0,2,");
{
  let a = [1]; a.x = 2; a[5] = 3; a[3000] = 5;
  row(listed(Object.getOwnPropertyNames(a)) === "5:0,5,3000,length,x," && listed(Object.keys(a)) === "4:0,5,3000,x,");
}
{ let a = [1, 2]; Object.defineProperty(a, 0, {enumerable: false}); row(listed(Object.keys(a)) === "1:1,"); }
row(listed(Object.getOwnPropertyNames([])) === "1:length,");
// The statics as callbacks of Array.prototype.map.
row([{a: 1}, {b: 2, c: 3}].map(Object.keys).join(";") === "a;b,c" && name(() => [1].map(Reflect.ownKeys)) === "TypeError");
// Results are fresh arrays.
{ let o = {a: 1}; let k = Object.keys(o); row(k !== Object.keys(o) && Object.getPrototypeOf(k) === Object.getPrototypeOf([]) && (k[1] = 'b', k.length === 2) && Object.keys(o).length === 1); }
row(Object.keys.length === 1 && Object.keys.name === 'keys' && Object.getOwnPropertyNames.length === 1 && Object.getOwnPropertyNames.name === 'getOwnPropertyNames');
// Built-in objects and functions.
row(listed(Object.keys(Object)) === "0:" && listed(Object.keys(Object.prototype)) === "0:");
{
  let n = Object.getOwnPropertyNames(Object); let has = false, proto = false;
  for (let i = 0; i < n.length; i++) { if (n[i] === 'keys') has = true; if (n[i] === 'prototype') proto = true; }
  row(n[0] === 'length' && n[1] === 'name' && proto && has);
}
{ let f = function () {}; f.b = 1; f.a = 2; f.prototype; row(listed(Object.keys(f)) === "2:b,a,"); }
row(listed(Object.keys(() => 1)) === "0:" && listed(Object.getOwnPropertyNames(function () {})) === "3:length,name,prototype,");
// ToObject: undefined and null are TypeErrors; a String's indices, then
// its non-enumerable `length`; Number and Boolean have none.
row(name(() => Object.keys(null)) === "TypeError" && name(() => Object.keys()) === "TypeError" && name(() => Object.getOwnPropertyNames(undefined)) === "TypeError");
row(listed(Object.keys('ab')) === "2:0,1," && listed(Object.getOwnPropertyNames('ab')) === "3:0,1,length," && listed(Object.keys('')) === "0:" && listed(Object.getOwnPropertyNames('')) === "1:length,");
row(listed(Object.keys(1)) === "0:" && listed(Object.getOwnPropertyNames(true)) === "0:");
{ let k; try { null.x; } catch (e) { k = Object.keys(e); } row(k.length === 0); }
// Deleting and re-adding a key moves it to the end.
{ let o = {a: 1, b: 2}; delete o.a; o.a = 3; row(listed(Object.keys(o)) === "2:b,a,"); }
// Symbols: Reflect.ownKeys lists them last; keys and names skip them.
{
  let s = Symbol.iterator, o = {[s]: 0, b: 1, 1: 2};
  row(listed(Reflect.ownKeys(o)) === "3:1,b,Symbol(Symbol.iterator)!," && listed(Object.keys(o)) === "2:1,b," && listed(Object.getOwnPropertyNames(o)) === "2:1,b,");
}
{ let p = {inherited: 1}, o = Object.create(p), s = Symbol.iterator; o.a = 3; o[s] = 4; let keys = Reflect.ownKeys(o); row(Reflect.has(o, 'inherited') && keys.length === 2 && keys[0] === 'a' && keys[1] === s); }
// A key object converts once, with hint string (reflect.rs line 108).
{ let n = 0, key = {toString() { n++; return 'x'; }}; let o = {x: 1}; row(Reflect.has(o, key) && !Reflect.has({}, key) && n === 2); }
row(Reflect.has([], 'length') && Reflect.has([1], 0) && !Reflect.has([1], 1) && Reflect.has(function () {}, 'prototype') && Reflect.has({}, 'toString'));
// Reflect's methods require an object target, before the key converts.
{
  let n = 0, key = {toString() { throw 'key ran'; }};
  for (let f of [Reflect.has, Reflect.ownKeys]) { try { f(1, 'x'); } catch (e) { if (e instanceof TypeError) n++; } try { f(null, key); } catch (e) { if (e instanceof TypeError) n++; } }
  row(n === 4 && Reflect.ownKeys.length === 1 && Reflect.has.length === 2 && Reflect.ownKeys.name === 'ownKeys' && Reflect.has.name === 'has');
}
// Proxies: [[OwnPropertyKeys]] and [[GetOwnProperty]] run their traps.
{
  let s = Symbol.iterator, o = {a: 1}; o[s] = 2; let log = '';
  let p = new Proxy(o, {ownKeys(t) { log += 'k'; return ['a', s]; }, getOwnPropertyDescriptor(t, k) { log += 'd'; return Object.getOwnPropertyDescriptor(t, k); }});
  let keys = Reflect.ownKeys(p);
  row(keys.length === 2 && keys[1] === s && log === 'k');
}
{
  let log = '', s = Symbol.iterator;
  let p = new Proxy({a: 1, b: 2, [s]: 3}, {ownKeys() { log += 'k'; return ['b', s, 'a']; }, getOwnPropertyDescriptor(t, k) { log += 'd' + String(k).length; return k === 'b' ? undefined : {value: 1, enumerable: true, configurable: true}; }});
  row(Object.keys(p).join() === 'a' && log === 'kd1d1' && Object.getOwnPropertyNames(p).join() === 'b,a');
}
{ let p = new Proxy({x: 1}, {}); row(Object.keys(p).join() === 'x' && Reflect.ownKeys(p).join() === 'x'); }
{ let ok = false, p = new Proxy({}, {ownKeys() { throw 17; }}); try { Object.keys(p); } catch (e) { ok = e === 17; } row(ok); }
failed === 0 ? rows : -failed;
