// bbp-authored conformance case (light-cljs-883).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 7102-17317:
// stored_proto_accessor (line 7102, bbp-261), prototype_reflection (line
// 16949, bbp-121), its `__proto__` accessor and literal rows,
// accessor_descriptors_and_integrity (line 7172), its `__proto__` accessor
// row; and the planner's rows (light-cljs-766). Object.prototype's
// `__proto__` is a configurable, non-enumerable accessor whose getter is
// [[GetPrototypeOf]] of ToObject(this) and whose setter is [[SetPrototypeOf]]
// of an object this, ignoring a prototype that is neither an object nor
// null; an own `__proto__` property shadows it, an object without
// Object.prototype on its chain has none, and a literal's `__proto__: v`
// sets the prototype without it. Left out: rows that change
// Object.prototype's `__proto__` or the accessor's functions (the Pants
// engine and Goose refuse to change realm objects, QuickJS allows it), rows
// using Object.getOwnPropertyNames or Object.keys (pending in Goose), and
// `__rt` runtime values. Errors are compared by name. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-766).
{ let p = {x: 7}; let o = {__proto__: p}; row((o.__proto__ === p) + ',' + o.x === "true,7"); }
{ let o = {}; o.__proto__ = Array.prototype; row(Array.isArray(o) + ',' + (o instanceof Array) === "false,true"); }
// prototype_reflection (line 16949): the accessor.
{ let o1 = {f: function () { return this; }}; let o2 = {}; o2.__proto__ = o1; row(o2.f() === o2); }
{ let o = {}; row(o.__proto__ === Object.prototype && o['__proto__'] === Object.prototype); }
row([].__proto__ === Object.getPrototypeOf([]) && Object.prototype.__proto__ === null);
{ let f = function () {}; row(f.__proto__ === Function.prototype && Object.is.__proto__ === Function.prototype); }
{ let p = {x: 5}; let o = {}; let v = (o.__proto__ = p); row(v === p && Object.getPrototypeOf(o) === p && o.x === 5 && o.__proto__ === p && Object.getOwnPropertyDescriptor(o, '__proto__') === undefined); }
{ let p = {}; let o = Object.create(p); let v = (o.__proto__ = 1); row(v === 1 && Object.getPrototypeOf(o) === p && Object.getOwnPropertyDescriptor(o, '__proto__') === undefined); }
{ let o = {}; o.__proto__ = null; row(Object.getPrototypeOf(o) === null && o.__proto__ === undefined); }
{ let o = {}; o['__proto__'] = []; row(o.length === 0 && Object.getPrototypeOf(o) !== null); }
{ let o = {}; row(name(() => { o.__proto__ = o; }) === "TypeError" && Object.getPrototypeOf(o) === Object.prototype); }
row(name(() => { Object.prototype.__proto__ = {}; }) === "TypeError");
// The setter's no-ops on Object.prototype are accepted.
{ Object.prototype.__proto__ = null; Object.prototype.__proto__ = 1; [].constructor.prototype.__proto__ = Object.prototype; row(Object.getPrototypeOf(Object.prototype) === null); }
// Without Object.prototype on the chain, `__proto__` is an ordinary property.
{ let o = Object.create(null); let p = {x: 1}; o.__proto__ = p; row(Object.getPrototypeOf(o) === null && o.x === undefined && o.__proto__ === p && Object.getOwnPropertyDescriptor(o, '__proto__').value === p); }
// An own `__proto__` data property shadows the accessor.
{ let o = {}; Object.defineProperty(o, '__proto__', {value: 7, writable: true}); o.__proto__ = 8; row(o.__proto__ === 8 && Object.getPrototypeOf(o) === Object.prototype); }
{ let d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); row(typeof d.get === 'function' && typeof d.set === 'function' && d.get.name === 'get __proto__' && d.set.name === 'set __proto__' && !d.enumerable && d.configurable && !('value' in d) && !('writable' in d)); }
// accessor_descriptors_and_integrity (line 7172): a literal accessor named __proto__ is an own accessor.
{ let a = {get __proto__() { return 3; }, set __proto__(v) { this.x = v; }}; a.__proto__ = 4; row(a.__proto__ === 3 && a.x === 4 && Object.getPrototypeOf(a) === Object.prototype); }
// A literal's `__proto__: v` (SetProto) and its shorthand.
{ let p = {x: 3}; let o = {__proto__: p, y: 4}; row(o.x === 3 && o.y === 4 && Object.getPrototypeOf(o) === p && Object.getOwnPropertyDescriptor(o, '__proto__') === undefined); }
{ let p = {x: 3}; row(({"__proto__": p}).x === 3); }
row(Object.getPrototypeOf({__proto__: null}) === null && Object.getPrototypeOf({__proto__: 1}) === Object.prototype);
{ let o = {__proto__: undefined}; row(Object.getPrototypeOf(o) === Object.prototype); }
{ let p = {}; row(Object.getPrototypeOf({a: 1, __proto__: p, b: 2}) === p); }
{ let __proto__ = 5; let p = {}; let o = {__proto__: p, __proto__}; row(Object.getPrototypeOf(o) === p && Object.getOwnPropertyDescriptor(o, '__proto__').value === 5 && o.__proto__ === 5); }
{ let log = ''; let p = {}; let o = {a: (log = log + 'a'), __proto__: (log = log + 'p', p), b: (log = log + 'b')}; row(log === 'apb' && Object.getPrototypeOf(o) === p); }
{ let q = {y: 2}; let s = {__proto__: q}; let t = {}; t.__proto__ = q; row(s.y === 2 && t.__proto__ === q && ({}).__proto__ === Object.prototype); }
// stored_proto_accessor (line 7102).
var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__');
row(d.get.length === 0 && d.set.length === 1 && !('prototype' in d.get) && !('prototype' in d.set) && Object.getPrototypeOf(d.get) === Function.prototype && d.get === Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get);
row(d.get.call([]) === Array.prototype && d.get.call({}) === Object.prototype && d.get.call(Object.create(null)) === null && d.get.call(Object.prototype) === null);
row(d.get.call(1) === Number.prototype && d.get.call('abc') === String.prototype && d.get.call(new Number(2)) === Number.prototype && d.get.call(new String('a')) === String.prototype);
row(d.get.apply([], []) === Array.prototype && d.get.bind(1)() === Number.prototype && Reflect.apply(d.get, Object.prototype, []) === null);
{ let b = true; b.__proto__ = {}; (1).__proto__ = null; 'a'.__proto__ = {}; row(b === true); }
{ let o = {}; let p = {x: 7}; row(d.set.call(o, p) === undefined && o.x === 7 && d.get.call(o) === p && d.set.call(o, null) === undefined && d.get.call(o) === null); }
{ let o = {}; let p = d.get.call(o); let v = {valueOf() { throw 7; }, toString() { throw 8; }}; row(d.set.call(o) === undefined && d.set.call(o, 1) === undefined && d.set.call(1, v) === undefined && d.set.call('a', null) === undefined && d.set.call(true, v) === undefined && d.get.call(o) === p); }
{ let o = {}; Object.defineProperty(o, 'link', d); let p = {}; row((o.link = p) === p && o.link === p && Object.getPrototypeOf(o) === p); }
{ let p = {}; Object.defineProperty(p, 'link', d); let o = Object.create(p); let q = {}; row((o.link = q) === q && Object.getPrototypeOf(o) === q && Object.getOwnPropertyDescriptor(o, 'link') === undefined && Object.getPrototypeOf(p) === Object.prototype); }
{ let a = {x: 3, __proto__: null}; let o = {__proto__: a}; row(!('__proto__' in o) && Object.getOwnPropertyDescriptor(o, '__proto__') === undefined && o.x === 3); }
row('__proto__' in {} && !('__proto__' in Object.create(null)));
row(name(() => d.get.call(null)) === "TypeError" && name(() => d.get.call(undefined)) === "TypeError" && name(() => d.set.call(null, 1)) === "TypeError" && name(() => d.set.call(undefined)) === "TypeError");
row(name(() => d.set.call(Object.prototype, {})) === "TypeError" && name(() => d.set.call(Object.preventExtensions({}), {})) === "TypeError");
{ let o = {}; row(name(() => d.set.call(o, o)) === "TypeError"); }
row(name(() => new d.get()) === "TypeError" && name(() => new d.set({})) === "TypeError" && name(() => { d.set.caller = 1; }) === "TypeError");
failed === 0 ? rows : -failed;
