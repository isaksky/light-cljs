// bbp-authored conformance case (light-cljs-886), a sloppy script.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 1189-1229
// (base_classes, bbp-136), 1270-1290 (base_classes_are_strict_in_sloppy_scripts),
// 1475-1530 (non_constructors_reject_new, constructor_kind_survives_code_round_trip;
// bbp-135, rows made for class methods, accessors and static methods) and
// rust/tests/reflect.rs lines 72-86 (constructors_and_new_target, rows made
// for a class), with light-cljs-766's planner rows: class constructors with
// their eager `prototype` (neither writable, enumerable nor configurable)
// and its `constructor`, `length` and `name`, methods, getters, setters and
// static members as non-enumerable configurable properties (methods
// writable), string, number, symbol and computed keys, the immutable inner
// class-name binding, [[Call]] a TypeError, [[Construct]] with new.target
// and the return override, instanceof, Reflect.construct with a newTarget
// (and a newTarget whose `prototype` is a getter), integrity levels,
// and a class body strict in a sloppy script (Function.prototype.toString
// of a class is classes_source_text). Left out: `extends`, `super` and `#x` (light-cljs-887 to
// -889). Each row is a `row(...)` check; the script completes with the
// number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
function message(thunk) { try { thunk(); return "none"; } catch (e) { return e.message; } }
// The planner's rows (light-cljs-766).
{ class C {} row(typeof new C() === 'object'); }
{ class C { constructor() { this.x = 1 } m() { return 2 } static s() { return 3 } get g() { return 4 } } let c = new C(); row(c.x + c.m() + C.s() + c.g === 10); }
{ let C = class D { static f() { return D } }; row(C.f() === C); }
{ class C { get y() { return 1 } } row(Object.getOwnPropertyDescriptor(C.prototype, 'y').get.name === 'get y'); }
{ class C {} row(new C() instanceof C); }
{ class C { constructor() { this.t = new.target === C } } row(new C().t === true); }
{ class C {} row(name(() => C()) === 'TypeError'); }
{ class C { m() {} } let d = Object.getOwnPropertyDescriptor(C.prototype, 'm'); row(d.enumerable + ',' + d.writable + ',' + d.configurable === 'false,true,true'); }
{ class C { static m() {} } row(Object.getOwnPropertyNames(C).join() === 'length,name,prototype,m'); }
{ class C { m() {} } row(name(() => new (new C().m)()) === 'TypeError'); }
{ class C { 'a b'() { return 1 } [Symbol.iterator]() { return 2 } static 3() { return 3 } } row(new C()['a b']() + new C()[Symbol.iterator]() + C[3]() === 6); }
row(Reflect.construct(class { constructor() { this.t = new.target } }, [], Object).t === Object);
// base_classes (js_source.rs line 1189).
{ class C { constructor() { this.x = 10; } f() { return 1; } static F() { return -1; } get y() { return 12; } } row(C.F() === -1 && new C().f() === 1 && new C().x === 10 && new C().y === 12 && Object.getOwnPropertyDescriptor(C.prototype, 'y').get.name === 'get y' && Object.keys(C.prototype).length === 0); }
{ let E1 = class E { static F() { return E; } }; row(E1 === E1.F() && typeof E === 'undefined'); }
{ class P { static() { return 42; } get() { return 1; } set() { return 2; } async() { return 3; } } row(new P().static() === 42 && new P().get() === 1 && new P().set() === 2 && new P().async() === 3); }
{ class C {} let ok = false; try { C(); } catch (e) { ok = e instanceof TypeError; } row(ok); }
{ let ok = false; try { C; } catch (e) { ok = e instanceof ReferenceError; } class C {} row(ok); }
{ class K {} let ok = false; try { (function () { 'use strict'; K.prototype = {}; })(); } catch (e) { ok = e instanceof TypeError; } row(ok); }
{ class C { static f() { C = 1; } } let ok = false; try { C.f(); } catch (e) { ok = e instanceof TypeError; } row(ok && typeof C === 'function'); }
{ class C { get y() { return this.x; } set y(v) { this.x = v; } static get z() { return this.x; } static set z(v) { this.x = v; } } let c = new C(); c.y = 7; C.z = 9; row(c.y === 7 && C.z === 9); }
{ let key = 'm'; class C { [key]() { return 3; } static [key]() { return 4; } } row(new C().m() === 3 && C.m() === 4); }
{ class C {} let d = Object.getOwnPropertyDescriptor(C, 'prototype'); let c = Object.getOwnPropertyDescriptor(C.prototype, 'constructor'); row(!d.writable && !d.enumerable && !d.configurable && c.value === C && c.writable && !c.enumerable && c.configurable && new C() instanceof C); }
{ class C { m(a) {} } let ok = false; try { new C.prototype.m(); } catch (e) { ok = e instanceof TypeError; } row(ok && C.prototype.m.name === 'm' && C.prototype.m.length === 1); }
{ let C = class {}; let D = class Named {}; row(C.name === 'C' && D.name === 'Named' && C.length === 0 && D.length === 0); }
{ let C = class Inner { constructor(x = 3) { this.x = x; } m() { return Inner; } }; let D = C; C = 1; row(new D().m() === D && new D().x === 3 && D.length === 0); }
{ class C { m() { return C; } } let D = C; C = 1; row(new D().m() === D); }
{ class C { m() { return eval('C'); } static s() { return eval('C'); } } let D = C; C = 1; row(new D().m() === D && D.s() === D); }
{ class C { m() { eval('C = 1'); } } let ok = false; try { new C().m(); } catch (e) { ok = e instanceof TypeError; } row(ok && typeof C === 'function'); }
{ let ok = false; try { let C = class C { [eval('C')]() {} }; } catch (e) { ok = e instanceof ReferenceError; } row(ok); }
{ let ok = false; try { let C = class C { [C]() {} }; } catch (e) { ok = e instanceof ReferenceError; } row(ok); }
{ let C = 9; let seen = 0; try { let D = class C { [(seen++, C)]() {} }; } catch (e) { seen += e instanceof ReferenceError ? 10 : 100; } row(C === 9 && seen === 11); }
{ class C { static constructor() { return 7; } ['constructor']() { return 8; } ['__proto__']() { return 9; } } row(C.constructor() === 7 && new C().constructor() === 8 && new C().__proto__() === 9); }
{ class C { *g() { yield 7; } async a() { return 8; } } row(new C().g().next().value === 7 && C.prototype.g.name === 'g' && C.prototype.a.name === 'a'); }
{ class C { static get x() { return 1; } static set x(v) {} m() {} m() { return 9; } } let d = Object.getOwnPropertyDescriptor(C, 'x'); row(typeof d.get === 'function' && typeof d.set === 'function' && !d.enumerable && d.configurable && new C().m() === 9 && Object.keys(C).length === 0); }
{ let s = Symbol('m'); class C { [s]() { return 7; } get ['x']() { return 9; } } row(new C()[s]() === 7 && C.prototype[s].name === '[m]' && Object.getOwnPropertyDescriptor(C.prototype, 'x').get.name === 'get x'); }
{ class C {} let n = 0; for (let i = 0; i < 4; i++) { try { if (i === 0) C.call(null); if (i === 1) C.apply(null, []); if (i === 2) C.bind(null)(); if (i === 3) C(...[]); } catch (e) { if (e instanceof TypeError) n++; } } row(n === 4 && new (C.bind(null))() instanceof C); }
{ class C { constructor() { return {x: 9}; } } row(new C().x === 9); }
{ class C { constructor() { this.x = 7; return 3; } } row(new C().x === 7); }
{ { class C {} } row(typeof C === 'undefined'); }
{ let o = {['C']: class {}}; row(o.C.name === 'C'); }
{ let s = Symbol('C'); let o = {[s]: class {}}; row(o[s].name === '[C]'); }
{ let o = {['C']: class Inner { static self() { return Inner; } }}; row(o.C.name === 'Inner' && o.C.self() === o.C); }
{ let o = {['C']: class { static name() { return 7; } }}; row(o.C.name() === 7 && o.C.name.name === 'name'); }
{ let o = {['C']: class { static get name() { return 'override'; } }}; row(o.C.name === 'override'); }
{ class C { get y() { return 12; } } row(typeof Object.getOwnPropertyDescriptor(C.prototype, 'y').get === 'function' && new C().y === 12); }
{ class C { a() {} if() { return 7; } } row(new C().if() === 7); }
{ function* g() { let D = class C { [yield 'key']() { return C; } }; return D; } let it = g(); let a = it.next(); let C = it.next('m').value; row(a.value === 'key' && new C().m() === C); }
// base_classes_are_strict_in_sloppy_scripts (js_source.rs line 1270): the
// class body is strict code. Its rows that write or delete in a computed
// key are left out: QuickJS evaluates computed keys as sloppy code there,
// so the oracles disagree. (base_classes's `'x' in b` key in a for head is
// left out too: QuickJS does not run it.)
row((function f() { let C = class { [eval('this') === this ? 'm' : 'bad']() { return 7; } }; return new C().m() === 7; }).call({}));
{ class C {} let ok = false; try { C.prototype = {}; } catch (e) { ok = true; } row(!ok && typeof C.prototype === 'object'); }
{ class C { m() { return this; } } let m = C.prototype.m; row(m() === undefined); }
// non_constructors_reject_new (js_source.rs line 1475), class elements:
// methods, accessors and static methods are not constructors, a class is.
{ class C { m() {} } row(message(() => new (new C().m)()) !== 'none' && name(() => new C.prototype.m()) === 'TypeError'); }
{ class C { get x() {} set x(v) {} } let d = Object.getOwnPropertyDescriptor(C.prototype, 'x'); row(name(() => new d.get()) === 'TypeError' && name(() => new d.set()) === 'TypeError'); }
{ class C { static m() {} *g() {} async a() {} } row(name(() => new C.m()) === 'TypeError' && name(() => new C.prototype.g()) === 'TypeError' && name(() => new C.prototype.a()) === 'TypeError'); }
{ class C { static m() { return () => new.target; } } row(name(() => new (C.m())()) === 'TypeError'); }
{ class C { constructor() { this.x = 7; } } let B = C.bind(null); row(new B().x === 7 && new B() instanceof C); }
{ let called = false; class C { m() { called = true; } } let n = 0; try { new (new C().m)(++n); } catch (e) {} row(!called && n === 1); }
// constructor_kind_survives_code_round_trip (js_source.rs line 1520): the
// constructor kind of class code, as exported, keeps [[Construct]] and
// refuses [[Call]].
{ class C { constructor(v) { this.v = v; } } let f = function () { return C(1); }; row(new C(2).v === 2 && name(f) === 'TypeError'); }
// constructors_and_new_target (reflect.rs line 72), with a class.
{ class F { constructor(a, b) { this.sum = a + b; this.target = new.target; } } function G() {} let o = Reflect.construct(F, [2, 3], G); row(o.sum === 5 && o.target === G && Object.getPrototypeOf(o) === G.prototype && !(o instanceof F)); }
{ class F { constructor(x) { this.x = x; } } let log = ''; let list = {get length() { log += 'l'; return 2; }, get 0() { log += 'a'; return 4; }, get 1() { log += 'b'; return 5; }}; let o = Reflect.construct(F, list); row(o.x === 4 && log === 'lab' && o instanceof F); }
{ class C {} let n = 0; try { Reflect.construct(Object, [], C); } catch (e) { n++; } try { Reflect.apply(C, null, []); } catch (e) { if (e instanceof TypeError) n += 10; } row(n === 10 && Object.getPrototypeOf(Reflect.construct(Object, [], C)) === C.prototype); }
// OrdinaryCreateFromConstructor reads newTarget's `prototype` through its
// getter (Reflect.construct with a bound function given one), once, before
// the constructor runs; a non-object result gives %Object.prototype%.
{ class C { x = 1; } const p = {}; let n = 0; const G = (function () {}).bind(null); Object.defineProperty(G, 'prototype', {get() { n++; return p; }}); const o = Reflect.construct(C, [], G); row(n === 1 && Object.getPrototypeOf(o) === p && o.x === 1); }
{ function F(a) { this.a = a; this.t = new.target; } let n = 0; const G = F.bind(null); Object.defineProperty(G, 'prototype', {get() { n++; return 5; }}); const o = Reflect.construct(F, [7], G); row(n === 1 && Object.getPrototypeOf(o) === Object.prototype && o.a === 7 && o.t === G); }
{ let ran = false; class C { constructor() { ran = true; } } const G = (function () {}).bind(null); Object.defineProperty(G, 'prototype', {get() { throw new RangeError('x'); }}); row(name(() => Reflect.construct(C, [], G)) === 'RangeError' && !ran); }
{ class C { constructor(a, b) { this.s = a + b; } } const G = (function () {}).bind(null); Object.defineProperty(G, 'prototype', {get() { return C.prototype; }}); const o = Reflect.construct(C, [2, 3], G); row(o.s === 5 && o instanceof C); }
// Integrity levels of a class and its prototype (light-cljs-882's
// operations on a class's eager properties).
{ class C { static s = 1; m() { return 2 } } Object.freeze(C); let ok = Object.isFrozen(C) && !Object.isExtensible(C); try { C.s = 3 } catch (e) { ok = ok && e instanceof TypeError } row(ok && new C().m() === 2 && C.s === 1); }
{ class C { static s = 1 } Object.seal(C); let d = Object.getOwnPropertyDescriptor(C, 's'); row(Object.isSealed(C) && !d.configurable && d.writable && new C() instanceof C); }
{ class C {} Object.preventExtensions(C); row(name(() => { 'use strict'; C.z = 1; }) === 'TypeError' && !Object.isExtensible(C) && typeof new C() === 'object'); }
{ class C { m() {} } Object.freeze(C.prototype); row(name(() => { 'use strict'; C.prototype.m = 1; }) === 'TypeError' && typeof new C().m === 'function'); }
failed === 0 ? rows : -failed;
