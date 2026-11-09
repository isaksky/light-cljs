// bbp-authored conformance case (light-cljs-888).
// Derives from: crates/big-boy-pants-js/rust/tests/reflect.rs lines 52, 59
// and 56 (receivers_and_resumable_user_code, bbp-320; line 56's
// Reflect.get and Reflect.set with a key object, without its
// Reflect.defineProperty and deleteProperty, whose key conversion is
// light-cljs-892's), 65 (non_object_targets_and_bad_descriptors_throw,
// for get and set) and 105 (method_metadata_and_symbol_keys, for get and
// set), with light-cljs-766's planner rows: Reflect.get and Reflect.set
// with and without a receiver (an absent receiver is the target; an
// explicit undefined one is not), getters and setters running with the
// receiver, a write defining or updating the receiver's own data property
// (keeping its attributes), refused by an inherited read-only property, by
// the receiver's own accessor or read-only property, by a non-extensible
// receiver or a primitive one, key objects converted once, symbol and index
// keys, arrays and functions as target or receiver, and a TypeError for a
// target that is not an object. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
// The planner's rows (light-cljs-766).
row(Reflect.get({ get x() { return this.n } }, 'x', {n: 4}) === 4);
{ let r = {}; let o = { set x(v) { this.y = v } }; Reflect.set(o, 'x', 5, r); row(r.y === 5); }
row(Reflect.get({x: 1}, 'x') === 1);
{ let o = {a: 1}; row(Reflect.set(o, 'a', 2) && o.a === 2); }
// receivers_and_resumable_user_code (reflect.rs 52, 59, 56).
{ let o={get x(){return this.n+1;},set y(v){this.n=v;}}, r={n:4}; row(Reflect.get(o,'x',r) === 5 && Reflect.set(o,'y',7,r) && r.n === 7 && Reflect.set(o,'z',8,r) && r.z === 8 && !Reflect.has(o,'z') && !Reflect.set(o,'z',9,1)); }
{ let o={x:1}, r={set x(v){throw 'wrong setter';}}; row(!Reflect.set(o,'x',2,r) && o.x === 1); }
{ let o={get x(){throw 23;},set y(v){throw 24;}}, n=0; try{Reflect.get(o,'x');}catch(e){if(e===23)n++;} try{Reflect.set(o,'y',1);}catch(e){if(e===24)n++;} row(n===2); }
{ let n=0, key={toString(){n++;return 'x';}}; let o={x:3}; row(Reflect.get(o,key) === 3 && Reflect.set(o,key,4) && o.x === 4 && n === 2); }
// Targets that are not objects, metadata, symbol and index keys.
{ let n=0; for (let f of [Reflect.get, Reflect.set]) { try { f(1, 'x', {}); } catch (e) { if (e instanceof TypeError) n++; } } row(n === 2); }
row(Reflect.get.name + Reflect.get.length + Reflect.set.name + Reflect.set.length === "get2set3");
{ let d = Object.getOwnPropertyDescriptor(Reflect, 'get'), e = Object.getOwnPropertyDescriptor(Reflect, 'set'); row(d.writable && !d.enumerable && d.configurable && e.writable && !e.enumerable && e.configurable); }
{ let s = Symbol('s'); let o = {[s]: 1}; row(Reflect.get(o, s) === 1 && Reflect.set(o, s, 2) && o[s] === 2); }
{ let a = [1,2]; row(Reflect.set(a, 'length', 1) && a.length === 1 && Reflect.get(a, 0) === 1 && Reflect.get(a, 'length') === 1); }
{ let r = {}; row(Reflect.set([], 0, 'a', r) && r[0] === 'a' && !Array.isArray(r)); }
{ let a = []; row(Reflect.set({}, 0, 'a', a) && a[0] === 'a' && a.length === 1); }
// Writes with a separate receiver.
{ let r={}; let ok = Reflect.set({x:1}, 'x', 2, r); row(ok && r.x === 2 && Object.getOwnPropertyDescriptor(r,'x').enumerable); }
{ let r={}; Object.defineProperty(r,'x',{value:1,writable:true,enumerable:false,configurable:true}); Reflect.set({}, 'x', 2, r); row(r.x === 2 && !Object.getOwnPropertyDescriptor(r,'x').enumerable); }
{ let r = {}; Object.defineProperty(r, 'x', {value: 1}); row(!Reflect.set({}, 'x', 2, r) && r.x === 1); }
{ let p = {}; Object.defineProperty(p, 'x', {value: 1}); let r = {}; row(!Reflect.set(p, 'x', 2, r) && r.x === undefined); }
{ let r = {}; Object.freeze(r); row(Reflect.set({}, 'x', 1, r) === false); }
{ let o = {}; Object.freeze(o); row(Reflect.set(o, 'x', 1) === false); }
row(Reflect.set({}, 'x', 1, 5) === false);
row(Reflect.set({}, 'x', 1, undefined) === false);
{ let o = {x: 1}; row(Reflect.get(o, 'x', undefined) === 1); }
{ let o = {get x() { return this; }}; row(Reflect.get(o, 'x', 7) === 7); }
{ let o = {set x(v) { this.v = v; }}; let r = {}; row(Reflect.set(o, 'x', 3, r) && r.v === 3 && o.v === undefined); }
{ let r = {}; row(Reflect.set(Object.create({set x(v){ throw 1 }}), 'y', 1, r) && r.y === 1); }
{ function F() {} let r = {}; row(Reflect.set(F, 'prototype', 1, r) && r.prototype === 1 && typeof F.prototype === "object"); }
{ function F() {} row(Reflect.set({}, 'x', 1, F) && F.x === 1); }
{ function F(a, b) {} row(Reflect.get(F, 'name') + Reflect.get(F, 'length') === "F2"); }
row(Reflect.get('abc'.constructor.prototype, 'length', 'abcd') === 0);
// An array's `length` set to an object converts it twice (ArraySetLength),
// on the target or a separate receiver; a throw or a bad length propagates.
{ let a = [1,2,3], n = 0, v = {valueOf() { n++; return 2; }}; row(Reflect.set(a, 'length', v) && a.length === 2 && n === 2); }
{ let a = [1,2,3], n = 0, v = {valueOf() { n++; return 2; }}; row(Reflect.set({}, 'length', v, a) && a.length === 2 && n === 2); }
{ let a = [1,2,3], v = {valueOf() { throw 7; }}; let r; try { Reflect.set(a, 'length', v); r = 'none'; } catch (e) { r = e; } row(r === 7 && a.length === 3); }
{ let a = [1,2,3]; let r; try { Reflect.set(a, 'length', {valueOf() { return -1; }}); r = 'none'; } catch (e) { r = e.name; } row(r === "RangeError"); }
{ let a = [1,2,3]; Object.defineProperty(a, 'length', {writable: false}); row(Reflect.set(a, 'length', {valueOf() { return 1; }}) === false); }
failed === 0 ? rows : -failed;
