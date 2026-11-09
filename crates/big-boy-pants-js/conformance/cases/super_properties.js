// bbp-authored conformance case (light-cljs-888).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 814-846
// (super_property_access, bbp-138; its class h/H and delete rows adapt
// QuickJS test_language.js test_class and test_delete), 1015 and 1029
// (super_property_access_in_suspended_methods; its async row awaited by an
// async check, which rejects unhandled on a wrong value), 8855-8856
// (logical_assignment_dynamic_bindings_and_super), 29254-29255
// (class_fields_super_in_nested_class_keys; its two super() computed-key rows
// are left out: QuickJS throws a ReferenceError there) and 29267
// (class_fields_super_in_parameter_defaults), with light-cljs-766's planner
// rows: `super.x` and `super[k]` reads, calls, writes, updates, compound and
// logical assignments in object-literal methods, accessors, class methods,
// static methods, constructors, field initializers, static blocks, arrows,
// parameter defaults, generators and async methods; the home object's
// prototype read at each access (it follows Object.setPrototypeOf); getters
// and setters running with the method's `this`; a write defining the
// receiver's own property, refused by an inherited read-only property, an
// own accessor on the receiver or a non-extensible receiver (TypeError in
// strict code); key conversion order; `delete super.x` (ReferenceError).
// Sloppy rows are super_properties_sloppy. Each row is a `row(...)` check;
// the script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-766).
{ let p = {m() { return 1 }}; let o = {__proto__: p, m() { return super.m() + 1 }}; row(o.m() === 2); }
{ let p = {n: 1}; let o = {__proto__: p, m() { let k = 'n'; return super[k]++; }}; row(o.m() + ',' + o.n + ',' + p.n === "1,2,1"); }
{ class C { m() { return () => super.toString() } }; row(typeof new C().m()() === "string"); }
{ class A { static s() { return 'A' } } class B extends A { static s() { return super.s() + 'B' } } row(B.s() === "AB"); }
{ class A { get v() { return this.k } } class B extends A { get v() { return super.v + 1 } } let b = new B(); b.k = 1; row(b.v === 2); }
{ class C extends Object { m() { return super.toString === Object.prototype.toString } } row(new C().m() === true); }
// Writes with a separate receiver, constructors, fields and static blocks.
{ let p = {x: 1}; let o = {__proto__: p, m() { super.x = 5; return o.x + ',' + p.x + ',' + Object.keys(o) }}; row(o.m() === "5,1,m,x"); }
{ let p = {set x(v) { this.y = v }}; let o = {__proto__: p, m() { super.x = 5; return o.y }}; row(o.m() === 5); }
{ let p = {}; Object.defineProperty(p, 'x', {value: 1, writable: false}); let o = {__proto__: p, m() { super.x = 5 }}; row(name(() => o.m()) === "TypeError"); }
{ let p = {x: 1}; let o = {__proto__: p, get x() { return 9 }, m() { super.x = 5 }}; row(name(() => o.m()) === "TypeError"); }
{ let o = {__proto__: {}, m() { return super.x = 5 }}; Object.freeze(o); row(name(() => o.m()) === "TypeError"); }
{ let o = {m() { return super.x }}; Object.setPrototypeOf(o, {x: 3}); row(o.m() === 3); }
{ let k = {toString() { return 'n' }}; let p = {n: 7}; let o = {__proto__: p, m() { return super[k] }}; row(o.m() === 7); }
{ let k = {toString() { return 'n' }}; let o = {__proto__: {}, m() { super[k] = 3; return o.n }}; row(o.m() === 3); }
{ let o = {__proto__: {n: 1}, m() { super.n += 2; return o.n }}; row(o.m() === 3); }
{ let o = {__proto__: {n: null}, m() { super.n ??= 4; return o.n }}; row(o.m() === 4); }
{ class A { constructor() { this.a = 1 } m() { return 'A' } } class B extends A { constructor() { super(); this.b = super.m() } } row(new B().b === "A"); }
{ class A { m() { return 'A' } } class B extends A { f = super.m(); } row(new B().f === "A"); }
{ class A { static m() { return 'A' } } class B extends A { static { this.x = super.m() } } row(B.x === "A"); }
{ let o = {__proto__: {m() { return this.v }}, v: 5, *g() { yield super.m() }}; row(o.g().next().value === 5); }
{ let o = {m() { return super.x = 1; }}; row(name(() => o.m.call(5)) === "TypeError"); }
{ let seen; let p = {set x(v) { seen = typeof this + v; }}; let o = {__proto__: p, m() { return super.x = 1; }}; row(o.m.call(5) === 1 && seen === "number1"); }
{ let p = {get x() { return typeof this; }}; let o = {__proto__: p, m() { return super.x; }}; row(o.m.call('s') === "string"); }
{ let o = {m() { return super.x }}; Object.setPrototypeOf(o, null); row(name(() => o.m()) === "TypeError"); }
// A null base is a TypeError before an assignment's key converts.
{ let n = 0; let o = {__proto__: null, m() { super[{toString() { n++; throw 7 }}] = 1; }}; row(name(() => o.m()) === "TypeError" && n === 0); }
// super_property_access (js_source.rs 814-845).
{ let k={[Symbol.toPrimitive](){return 'x';},toString(){return 'y';}}; let p={get x(){return this.a;},set x(v){this.a=v;},get y(){return this.b;},set y(v){this.b=v;}}; let o={__proto__:p,m(){super[k]=7;return super[k];}}; row(o.m()===7 && o.a===7 && o.b===undefined); }
{ class C { f() { return 1; } static F() { return -1; } } class D extends C { h() { return super.f(); } static H() { return super['F'](); } } let o = new D(); row(o.h() === 1 && D.H() === -1); }
{ let p = {x: 1, m() { return this.y; }}; let o = {__proto__: p, y: 2, f() { return super.x + super.m(); }, g() { return super['x'] + super['m'](); }}; row(o.f() === 3 && o.g() === 3); }
{ let p = {get x() { return this; }, set z(v) { this.y = v; }}; let o = {__proto__: p, get x() { return super.x; }, set z(v) { super.z = v; }}; o.z = 5; row(o.x === o && o.y === 5 && p.y === undefined); }
{ let p = {}; let o = {__proto__: p, f() { super.z = 5; super['w'] = 6; }}; o.f(); row(o.z === 5 && o.w === 6 && p.z === undefined && p.w === undefined); }
{ let a = { f() { delete super.a; } }; let ok = false; try { a.f(); } catch(e) { ok = e instanceof ReferenceError; } row(ok); }
{ let n = 0; let expr = 0; let key = {toString() { n++; throw 7; }}; let o = {m() { delete super[(expr++, key)]; }}; let ok = false; try { o.m(); } catch(e) { ok = e instanceof ReferenceError; } row(ok && expr === 1 && n === 0); }
{ let p = {x: 1}; let o = {__proto__: p, m() { return super.x; }}; let other = {x: 9, m: o.m}; Object.setPrototypeOf(other, {x: 8}); let before = other.m(); Object.setPrototypeOf(o, {x: 2}); row(before === 1 && other.m() === 2); }
{ let p = {m(a,b) { return this.x + a + b; }}; let o = {__proto__: p, x: 1, f() { return (super.m)(...[2,3]); }}; row(o.f() === 6); }
{ let p = {x: 1}; let o = {__proto__: p, m() { let a = super.x++; let b = ++super['x']; super.x += 3; return a === 1 && b === 2 && this.x === 4; }}; row(o.m() && p.x === 1); }
{ let p = {}; Object.defineProperty(p, 'x', {value: 1}); let o = {__proto__: p, m() { super.x = 2; }}; let ok = false; try { o.m(); } catch(e) { ok = e instanceof TypeError; } row(ok && o.x === 1); }
{ let p = {x: 1}; let o = {__proto__: p, set x(v) { throw 9; }, m() { super.x = 2; }}; let ok = false; try { o.m(); } catch(e) { ok = e instanceof TypeError; } row(ok); }
{ let p = {m() { return this; }}; let o = {__proto__: p, f() { return super.m?.(); }}; row(o.f() === o); }
{ let o = {__proto__: null, m() { return super.x; }}; let ok = false; try { o.m(); } catch(e) { ok = e instanceof TypeError; } row(ok); }
{ let n = 0; class C {} class D extends C { constructor() { let ok = false; try { super[n++]; } catch(e) { ok = e instanceof ReferenceError; } super(); this.ok = ok; }} row(new D().ok && n === 0); }
{ class C { get x() { return this; } set y(v) { this.value = v; } m() { return this.value; }} class D extends C { constructor() { super(); this.same = super.x === this; super.y = 7; this.result = super.m(); }} let d = new D(); row(d.same && d.value === 7 && d.result === 7 && C.prototype.value === undefined); }
{ class C { constructor() { this.same = super.x === this; super.y = 7; this.result = super.m(); }} let p = {get x() { return this; }, set y(v) { this.value = v; }, m() { return this.value; }}; Object.setPrototypeOf(C.prototype, p); let c = new C(); row(c.same && c.value === 7 && c.result === 7 && p.value === undefined); }
{ class C { static get x() { return this; } static set z(v) { this.y = v; }} class D extends C { static get x() { return super.x; } static set z(v) { super.z = v; }} D.z = 5; row(D.x === D && D.y === 5 && C.y === undefined); }
{ let key = Symbol('x'); let p = {[key]: 7}; let o = {__proto__: p, m() { super[key] = super[key] + 1; }}; o.m(); row(o[key] === 8 && p[key] === 7); }
{ let p = {x: 7}; let o = {__proto__: p, m(x = super.x) { return x; }}; row(o.m() === 7); }
{ let p = {C: function(v) { this.x = v; }}; let o = {__proto__: p, m() { return new super.C(7).x; }}; row(o.m() === 7); }
{ let p = {tag(a) { return this.x + a[0]; }}; let o = {__proto__: p, x: 'a', m() { return super.tag`b`; }}; row(o.m() === 'ab'); }
{ let p = {get x() { return this; }}; let o = {__proto__: p, m() { return super.x; }}; row(o.m.call(7) === 7 && o.m.call(null) === null); }
{ let n = 0; let p = {set x(v) { n++; }}; let o = {__proto__: p, m() { super.x = 7; }}; Object.preventExtensions(o); o.m(); row(n === 1); }
{ let p = {}; let o = {__proto__: p, m() { super.x = 7; }}; Object.preventExtensions(o); let ok = false; try { o.m(); } catch(e) { ok = e instanceof TypeError; } row(ok); }
{ let n = 0; let key = {toString() { n++; return 'x'; }}; let p = {x: 1}; let o = {__proto__: p, m() { super[key] += 2; }}; o.m(); row(n === 1 && o.x === 3 && p.x === 1); }
{ let log = ''; let key = {toString() { log += 'k'; return 'x'; }}; let o = {m() { super[key] = (log += 'v', 7); }}; o.m(); row(log === 'vk' && o.x === 7); }
{ let log = ''; let key = {toString() { log += 'k'; return 'x'; }}; let value = {get x() { log += 'v'; return undefined; }}; let o = {m() { ({x: super[key] = (log += 'd', 7)} = value); }}; o.m(); row(log === 'vdk' && o.x === 7); }
{ let log = ''; let key = {toString() { log += 'k'; return 'x'; }}; let it = {[Symbol.iterator]() { return this; }, next() { log += 's'; return {value: undefined, done: false}; }, return() { return {done: true}; }}; let o = {m() { [super[(log += 'e', key)] = (log += 'd', 7)] = it; }}; o.m(); row(log === 'esdk' && o.x === 7); }
{ let n = 0; let key = {toString() { n++; return 'x'; }}; let o = {m() { try { super[key] = (() => { throw 7; })(); } catch(e) { return e === 7; } }}; row(o.m() && n === 0); }
{ let n = 0; let o = {m() { let inner = {m() { return super.x; }}; Object.setPrototypeOf(inner, {x: 7}); n = inner.m(); return super.x; }}; Object.setPrototypeOf(o, {x: 8}); row(o.m() === 8 && n === 7); }
{ let p = {x: 1}; let o = {__proto__: p, m() { [super.x] = [7]; ({x: super['y']} = {x: 8}); }}; o.m(); row(o.x === 7 && o.y === 8 && p.x === 1 && p.y === undefined); }
// Suspended methods (js_source.rs 1015, 1029), logical
// assignments (8855-8856) and super() in nested class heritage and
// parameter defaults (29254-29255, 29267).
{ let p = {x: 1}; let o = {__proto__: p, *m() { super.x += yield 2; return this.x; }}; let it = o.m(); let a = it.next(); let b = it.next(7); row(a.value === 2 && !a.done && b.value === 8 && b.done && o.x === 8 && p.x === 1); }
{ let p = {x: 1}; let calls = 0; let o = {__proto__: p, async m() { calls++; super.x += await 7; return this.x; }}; (async function () { let v = await o.m(); if (!(v === 8 && o.x === 8 && p.x === 1 && calls === 1)) throw new Error("async super row"); })(); row(calls === 1); }
{ let stored, log = ''; class B { get p() { log += 'g'; return stored; } set p(v) { log += 's'; stored = v; } } class D extends B { f() { let x = super.p ??= 3; let y = super.p ||= 4; let z = super.p &&= 5; return x === 3 && y === 3 && z === 5; } } let d = new D(); row(d.f() && stored === 5 && log === 'gsggs'); }
{ let n = 0; class B { get p() { return 1; } } class D extends B { f() { return (super[(n++, 'p')] ||= 9) === 1 && (super.p ??= 8) === 1; } } row(new D().f() && n === 1); }
{ class B {} class D extends B { x = 7; constructor() { class Inner extends (super(), B) {} } } row(new D().x === 7); }
{ class B {} class D extends B { x = 7; constructor() { class Inner extends ((() => super())(), B) {} } } row(new D().x === 7); }
{ let n = 0; class B { constructor() { this.x = 7; } } class C extends B { y = (++n, this.x); constructor(a = super(), b = this.y) { this.z = b; this.same = a === this; } } let c = new C(); row(c.y === 7 && c.z === 7 && c.same && n === 1); }
failed === 0 ? rows : -failed;
