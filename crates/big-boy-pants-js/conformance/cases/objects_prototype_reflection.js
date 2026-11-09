// bbp-authored conformance case (light-cljs-883).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 10355-17256:
// prototype_reflection (line 16949, bbp-121), its getPrototypeOf, create
// and setPrototypeOf rows; for_in_walks_the_prototype_chain (line 10355),
// its rows that set a prototype before or during a walk; rust/tests/reflect.rs
// (lines 40 and 46), its getPrototypeOf and setPrototypeOf rows; and the
// planner's rows (light-cljs-766). [[SetPrototypeOf]] accepts the prototype
// an object has, refuses a cycle, a non-extensible object and a new
// prototype for %Object.prototype%; an array keeps its exotic behaviour
// under another prototype, and a hole inherits from it. Left out: rows on
// functions, Error objects and primitive wrappers, Object.create with a
// properties argument (the Pants engine refuses them; QuickJS-only, in
// objects_prototype_quickjs), `__rt` runtime values (the Pants engine
// answers them, Goose fails by name), realm objects other than
// %Object.prototype% as targets (the Pants engine and Goose refuse to change
// them, QuickJS allows it), rows using Object.getOwnPropertyNames or
// Object.keys (pending in Goose), and the `__proto__` accessor's rows
// (objects_proto_accessor). Errors are compared by name. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-766).
{ let p = {x: 7}; let o = Object.create(p); row(o.x + ',' + (Object.getPrototypeOf(o) === p) === "7,true"); }
{ let p = {x: 7}; let o = {}; Object.setPrototypeOf(o, p); row(o.x === 7); }
{ let o = Object.create(null); row(Object.getPrototypeOf(o) === null && !('toString' in o)); }
row(name(() => Object.setPrototypeOf(Object.prototype, {})) === "TypeError");
{ let a = {}, b = {__proto__: a}; row(name(() => Object.setPrototypeOf(a, b)) === "TypeError"); }
row(Reflect.getPrototypeOf([]) === Array.prototype);
// prototype_reflection (line 16949): the upstream rows.
{ let a = Object(); a.x = 1; row(Object.getPrototypeOf(a) === Object.prototype); }
{ let a = {x: 1}; let b = Object.create(a); row(Object.getPrototypeOf(b) === a); }
{ let a = {x: 1}; let b = Object.create(a); let c = {u: 2}; Object.setPrototypeOf(a, c); row(Object.getPrototypeOf(a) === c && b.u === 2); }
// getPrototypeOf.
row(Object.getPrototypeOf({}) === Object.prototype && Object.getPrototypeOf(Object.prototype) === null);
row(Object.getPrototypeOf(Object.getOwnPropertyDescriptor({x: 1}, 'x')) === Object.prototype);
{ let ap = Object.getPrototypeOf([]); row(ap === [].constructor.prototype && ap !== Object.prototype && Object.getPrototypeOf(ap) === Object.prototype && Object.getPrototypeOf([1, 2]) === ap); }
row(name(() => Object.getPrototypeOf(undefined)) === "TypeError" && name(() => Object.getPrototypeOf(null)) === "TypeError" && name(() => Object.getPrototypeOf()) === "TypeError");
row(Object.getPrototypeOf(function () {}) === Function.prototype && Object.getPrototypeOf(() => 1) === Function.prototype);
row(Object.getPrototypeOf(Object) === Function.prototype && Object.getPrototypeOf(Function.prototype) === Object.prototype);
// A function keeps the prototype it has.
{ let f = () => 1; row(Object.setPrototypeOf(f, Function.prototype) === f); }
// create.
{ let o = Object.create(null); row(Object.getPrototypeOf(o) === null && o.toString === undefined && o.x === undefined); }
{ let a = {x: 1}; let b = Object.create(a); let r1 = b.x; a.x = 2; let r2 = b.x; b.x = 3; row(r1 === 1 && r2 === 2 && b.x === 3 && a.x === 2 && Object.getOwnPropertyDescriptor(b, 'x').value === 3 && b.toString === Object.prototype.toString); }
{ let a = {}; row(Object.create(a) !== Object.create(a)); }
{ let o = {}; Object.defineProperty(o, 'k', Object.create({value: 7, writable: true})); let d = Object.getOwnPropertyDescriptor(o, 'k'); row(d.value === 7 && d.writable && !d.enumerable); }
row(typeof Object.create({}) === 'object' && Object.create.length === 2 && Object.create.name === 'create');
row(Object.getPrototypeOf.length === 1 && Object.setPrototypeOf.length === 2 && Object.getPrototypeOf.name === 'getPrototypeOf' && Object.setPrototypeOf.name === 'setPrototypeOf');
row(name(() => Object.create(1)) === "TypeError" && name(() => Object.create()) === "TypeError" && name(() => Object.create('p')) === "TypeError" && name(() => Object.create(undefined)) === "TypeError");
row(Object.getPrototypeOf(Object.create({}, undefined)) !== null);
row(Object.getPrototypeOf(Object.create(Object.prototype)) === Object.prototype && Object.getPrototypeOf(Object.create(Array.prototype)) === Array.prototype && !Array.isArray(Object.create(Array.prototype)));
// A function as a prototype: its own properties are inherited, attributes included.
{ let f = function () {}; f.y = 2; let o = Object.create(f); row(o.x === undefined && o.y === 2 && o.prototype === f.prototype); }
{ function F() {} let o = Object.create(F); o.x = 1; o.prototype = 5; row(o.x === 1 && o.prototype === 5 && F.prototype !== 5 && F.x === undefined && Object.getOwnPropertyDescriptor(o, 'prototype').value === 5); }
{ let f = function () {}; Object.defineProperty(f, 'k', {value: 1}); let o = Object.create(f); row(name(() => { o.k = 2; }) === "TypeError" && o.k === 1); }
{ function F() {} Object.defineProperty(F, 'prototype', {writable: false}); let o = Object.create(F); row(name(() => { o.prototype = 2; }) === "TypeError"); }
row(Object.create(function () {}).call === Function.prototype.call && Object.create(function () {}).bind === Function.prototype.bind);
{ let o = Object.create(function () {}); o.bind = 1; row(o.bind === 1); }
// setPrototypeOf.
{ let a = {}; let p = {y: 4}; row(Object.setPrototypeOf(a, p) === a && a.y === 4); }
{ let a = {x: 1}; Object.setPrototypeOf(a, null); row(Object.getPrototypeOf(a) === null && a.toString === undefined && a.x === 1); }
row(Object.setPrototypeOf(1, null) === 1 && Object.setPrototypeOf('s', {}) === 's');
{ let a = [1, 2]; let p = {z: 9}; Object.setPrototypeOf(a, p); row(a.z === 9 && a.length === 2 && a[1] === 2 && Array.isArray(a) && !(a instanceof Array)); }
{ let a = {}; let b = Object.create(a); row(name(() => Object.setPrototypeOf(a, b)) === "TypeError"); }
{ let a = {}; row(name(() => Object.setPrototypeOf(a, a)) === "TypeError" && Object.getPrototypeOf(a) === Object.prototype); }
{ let a = {}; let b = Object.create(a); let c = Object.create(b); row(name(() => Object.setPrototypeOf(a, c)) === "TypeError" && Object.getPrototypeOf(a) === Object.prototype); }
{ let a = {}; let b = Object.create(a); row(Object.setPrototypeOf(b, a) === b); }
row(Object.setPrototypeOf(Object.prototype, null) === Object.prototype);
row(name(() => Object.setPrototypeOf(undefined, {})) === "TypeError" && name(() => Object.setPrototypeOf(null, null)) === "TypeError");
row(name(() => Object.setPrototypeOf({}, 1)) === "TypeError" && name(() => Object.setPrototypeOf({}, undefined)) === "TypeError" && name(() => Object.setPrototypeOf({})) === "TypeError");
row(Object.setPrototypeOf(Object, Function.prototype) === Object);
// A non-extensible object keeps its prototype; the same one again is accepted.
{ let p = {}; let o = Object.preventExtensions(Object.create(p)); row(name(() => Object.setPrototypeOf(o, {})) === "TypeError" && Object.setPrototypeOf(o, p) === o && Object.getPrototypeOf(o) === p); }
{ let o = Object.freeze({}); row(name(() => Object.setPrototypeOf(o, null)) === "TypeError" && Object.getPrototypeOf(o) === Object.prototype); }
// Set back to its realm prototype, an object behaves as one never changed.
{ let a = [1]; Object.setPrototypeOf(a, {}); Object.setPrototypeOf(a, Array.prototype); a.push(2); a[3] = 4; row(a.length === 4 && a.join() === "1,2,,4" && a instanceof Array); }
// Inherited elements: a read-only one refuses a write of its index, on an
// array receiver and an ordinary one; a writable one is shadowed; a hole
// reads through to the prototype.
{ let p = []; Object.defineProperty(p, 0, {value: 1}); let a = []; Object.setPrototypeOf(a, p); let o = Object.create(p); row(name(() => { a[0] = 2; }) === "TypeError" && name(() => { o[0] = 2; }) === "TypeError" && name(() => { a['0'] = 2; }) === "TypeError" && a.length === 0 && a[0] === 1 && o[0] === 1); }
{ let p = [5]; let a = []; Object.setPrototypeOf(a, p); a[0] = 6; row(a[0] === 6 && p[0] === 5 && a.length === 1); }
{ let a = [0, , 2]; Object.setPrototypeOf(a, {1: 'p', get 3() { return 'g'; }}); row(a[1] === 'p' && a[3] === 'g' && a.length === 3 && 1 in a && Object.getOwnPropertyDescriptor(a, 1) === undefined); }
{ let log = []; let a = []; Object.setPrototypeOf(a, {set 0(v) { log.push(v); }}); a[0] = 9; row(log.join() === "9" && a.length === 0 && Object.getOwnPropertyDescriptor(a, 0) === undefined); }
// Reflect.getPrototypeOf and Reflect.setPrototypeOf (reflect.rs lines 40 and 46).
{ let p = {inherited: 1}, o = Object.create(p); row(Reflect.getPrototypeOf(o) === p && Reflect.isExtensible(o) && Reflect.preventExtensions(o) && !Reflect.setPrototypeOf(o, {}) && Reflect.setPrototypeOf(o, p)); }
{ let o = {}, p = {}; row(Reflect.setPrototypeOf(o, p) && !Reflect.setPrototypeOf(p, o) && Object.getPrototypeOf(p) === Object.prototype && Reflect.setPrototypeOf(o, null) && Reflect.getPrototypeOf(o) === null); }
row(Reflect.setPrototypeOf(Object.prototype, null));
row(name(() => Reflect.getPrototypeOf(1)) === "TypeError" && name(() => Reflect.setPrototypeOf(1, {})) === "TypeError" && name(() => Reflect.setPrototypeOf({}, 1)) === "TypeError" && name(() => Reflect.setPrototypeOf({})) === "TypeError");
row(Reflect.getPrototypeOf.length === 1 && Reflect.setPrototypeOf.length === 2 && Reflect.getPrototypeOf.name === 'getPrototypeOf' && Reflect.setPrototypeOf.name === 'setPrototypeOf');
// for_in_walks_the_prototype_chain (line 10355): rows that set a prototype.
{ let a = {x: 2, y: 2, "1": 3}; let b = {"4": 3}; Object.setPrototypeOf(a, b); let tab = ""; for (let i in a) tab = tab + i + ","; row(tab === "1,x,y,4,"); }
{ let a = {y: 2, "1": 3}; Object.defineProperty(a, "x", {value: 1}); let b = {"x": 3}; Object.setPrototypeOf(a, b); let tab = ""; for (let i in a) tab = tab + i + ","; row(tab === "1,y,"); }
{ const c = {d: "c", 2: "c", z: "c"}; Object.defineProperty(c, "w", {value: "c", enumerable: true}); const b = Object.create(c); b.d = "b"; b.y = "b"; b[2] = "b"; Object.defineProperty(b, "z", {value: "b", enumerable: false}); const a = Object.create(b); a.x = "a"; a.d = "a"; let s = ""; for (const k in a) s = s + k + "=" + a[k] + ";"; row(s === "x=a;d=a;2=b;y=b;w=c;"); }
{ const top = {h: 1, t: 1}; const mid = Object.create(top); Object.defineProperty(mid, "h", {value: 1}); const o = Object.create(mid); Object.defineProperty(o, "own", {value: 1}); let s = ""; for (const k in o) s = s + k; row(s === "t"); }
{ const o = Object.create(null); o.k = 1; o[0] = 1; let s = ""; for (const k in o) s = s + k; row(s === "0k"); }
{ const top = {t: 1}; const p = {p: 1, p2: 1, __proto__: top}; const o = {o: 1, __proto__: p}; let s = ""; for (const k in o) { if (k === "p") Object.setPrototypeOf(o, null); s = s + k; } row(s === "opp2t"); }
{ let s = ""; const old = {gone: 1}; const o = {a: 1, b: 1, __proto__: old}; for (const k in o) { if (k === "a") Object.setPrototypeOf(o, {fresh: 1, b: 2}); s = s + k + ","; } const n = {a: 1, __proto__: {gone: 1}}; for (const k in n) { Object.setPrototypeOf(n, null); s = s + k + ","; } row(s === "a,b,fresh,a,"); }
{ function f() {} f.fx = 1; const o = Object.create(f); o.own = 1; let s = ""; for (const k in o) s = s + k; row(s === "ownfx"); }
failed === 0 ? rows : -failed;
