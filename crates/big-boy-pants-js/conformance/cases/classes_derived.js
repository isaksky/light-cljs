// bbp-authored conformance case (light-cljs-887).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 760-789
// (derived_classes, bbp-137), 1045-1053 (derived_classes_new_target),
// 1124-1127 (derived_classes_inherited_function_names), 1136-1139
// (derived_classes_bound_static_inheritance) and 1150-1154
// (derived_classes_enumerate_inherited_statics), with light-cljs-766's
// planner rows: `extends` (a constructor or null, both inheritance links),
// super() with the shared new.target, spread and forwarded arguments,
// `this` before super() and a second super() (ReferenceError), super() in
// arrows, eval and parameter defaults, the derived [[Construct]]'s return
// rules, instance fields after super(), new.target in derived constructors
// and arrows, inherited statics and function names. Subclasses of
// built-ins are classes_builtin_parents; `super.x` is light-cljs-888's and
// `#x` light-cljs-889's. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-766).
{ class A { constructor() { this.a = 1 } } class B extends A { constructor() { super(); this.b = 2 } } let b = new B(); row(b.a + b.b === 3); }
{ class A { constructor() { this.t = new.target.name } } class B extends A {} row(new B().t === "B"); }
{ class N extends null { constructor() { return Object.create(N.prototype) } } row(Object.getPrototypeOf(N.prototype) === null); }
{ class A {} class B extends A { constructor() { try { this.x } catch (e) { super(); return } } } row(typeof new B() === "object"); }
{ class A {} class B extends A { constructor() { super(); try { super() } catch (e) { this.e = e instanceof ReferenceError } } } row(new B().e === true); }
{ function F() {}; class B extends F { constructor(...a) { super(...a) } } row(new B(1, 2) instanceof F); }
// Fields run after super() returns, seeing the bound `this`; an arrow's
// super() binds the constructor's `this`; the return rules.
{ class A { x = 1 } class B extends A { y = this.x + 1 } let b = new B(); row(b.x + b.y === 3 && Object.keys(b).join() === "x,y"); }
{ class A { constructor() { this.a = 1 } } class B extends A { constructor() { let f = () => super(); f(); this.b = 2 } } let b = new B(); row(b.a + b.b === 3); }
{ class A {} class B extends A { constructor() { return {k: 3} } } row(new B().k === 3); }
{ class A {} class B extends A { constructor() { super(); return undefined } } row(new B() instanceof B); }
{ class A {} class B extends A { constructor() { super(); return null } } row(name(() => new B()) === "TypeError"); }
{ class A {} class B extends A { constructor() { super(); return 1 } } row(name(() => new B()) === "TypeError"); }
{ class A {} class B extends A { constructor() {} } row(name(() => new B()) === "ReferenceError"); }
{ class A { constructor(a, b) { this.s = a + b } } class B extends A {} class C extends B {} row(new C(2, 3).s === 5 && C.length === 0 && C.name === "C"); }
{ class A { constructor() { this.args = arguments.length } } class B extends A {} row(new B(1, undefined, 3).args === 3); }
{ class A { constructor() { this.t = new.target } } class B extends A {} row(Reflect.construct(B, [], Object).t === Object && Object.getPrototypeOf(Reflect.construct(B, [], Object)) === Object.prototype); }
{ class A { static s() { return this.name } } class B extends A {} row(B.s() === "B" && Object.getOwnPropertyNames(B).join() === "length,name,prototype"); }
{ class A { m() { return 1 } } class B extends A {} row(new B().m() === 1 && Object.getOwnPropertyNames(B.prototype).join() === "constructor"); }
{ class A {} class B extends A {} row(name(() => B()) === "TypeError"); }
// js_source.rs derived_classes (bbp-137).
{ class C { constructor() { this.x = 10; } f() { return 1; } static F() { return -1; } } class D extends C { constructor() { super(); this.z = 20; } g() { return 2; } static G() { return -2; } } let o = new D(); row(D.F() === -1 && D.G() === -2 && o.f() === 1 && o.g() === 2 && o.x === 10 && o.z === 20 && Object.getPrototypeOf(D) === C && Object.getPrototypeOf(D.prototype) === C.prototype && o instanceof D && o instanceof C); }
{ class A { constructor(a,b) { this.s = a+b; } } class B extends A {} class C extends B {} let c = new C(1,2); row(c.s === 3 && Object.getPrototypeOf(c) === C.prototype && B.length === 0); }
{ class N extends null {} row(Object.getPrototypeOf(N.prototype) === null && Object.getPrototypeOf(N) === Function.prototype); }
{ let n = 0; for (let p of [1, {}, () => 1]) { try { class X extends p {} } catch(e) { if (e instanceof TypeError) n++; } } row(n === 3); }
{ function C() {} C.prototype = 1; let ok = false; try { class D extends C {} } catch(e) { ok = e instanceof TypeError; } row(ok); }
{ class C {} class D extends C { constructor() { this.x = 1; super(); } } let ok = false; try { new D(); } catch(e) { ok = e instanceof ReferenceError; } row(ok); }
{ let n = 0; class C { constructor() { n++; } } class D extends C { constructor() { super(); super(); } } let ok = false; try { new D(); } catch(e) { ok = e instanceof ReferenceError; } row(ok && n === 2); }
{ class C {} class D extends C { constructor() {} } let ok = false; try { new D(); } catch(e) { ok = e instanceof ReferenceError; } row(ok); }
{ class C {} class D extends C { constructor() { return 1; } } let ok = false; try { new D(); } catch(e) { ok = e instanceof TypeError; } row(ok); }
{ class N extends null { constructor() { return {x: 7}; } } row(new N().x === 7); }
{ class N extends null {} let ok = false; try { new N(); } catch(e) { ok = e instanceof TypeError; } row(ok); }
{ class D extends Object {} let o = new D(7); row(Object.getPrototypeOf(o) === D.prototype && o instanceof Object); }
{ class C {} class D extends C {} let ok = false; try { D(); } catch(e) { ok = e instanceof TypeError; } row(ok); }
{ function C(x) { this.x = x; } class D extends C {} row(new D(7).x === 7); }
{ class C { constructor(x) { this.x = x; } } let parent = C.bind(null, 2); let n = 0; Object.defineProperty(parent, 'prototype', {get() { n++; return C.prototype; }}); class D extends parent {} let d = new D(); row(d.x === 2 && n === 1 && Object.getPrototypeOf(D) === parent && Object.getPrototypeOf(d) === D.prototype); }
{ let n = 0; class C { constructor() { if (++n === 1) throw 1; this.x = 7; } } class D extends C { constructor() { try { super(); } catch(e) {} super(); } } row(new D().x === 7 && n === 2); }
{ class C {} class D extends C { constructor() { this.f = () => this; } } let ok = false; try { new D(); } catch(e) { ok = e instanceof ReferenceError; } row(ok); }
{ class C {} class D extends C { constructor() { super(); this.f = () => this; } } let d = new D(); row(d.f() === d); }
{ function C() {} C.prototype = null; class D extends C {} row(Object.getPrototypeOf(D.prototype) === null && new D() instanceof D); }
{ class C {} class D extends C { constructor() { try { return; } finally { super(); this.x = 7; } } } row(new D().x === 7); }
{ class C {} class D extends C { constructor() { try { super(); return 1; } finally { return {x: 7}; } } } row(new D().x === 7); }
{ let ok = false; try { let C = class C extends C {}; } catch(e) { ok = e instanceof ReferenceError; } row(ok); }
{ class C {} class D extends C { constructor(x = super()) { this.ok = x === this; } } row(new D().ok); }
{ class C {} class D extends C { constructor() { let f = () => this; super(); this.x = 7; this.same = f() === this; } } let d = new D(); row(d.x === 7 && d.same); }
{ class C {} class D extends C { constructor() { let f = () => this; let ok = false; try { f(); } catch(e) { ok = e instanceof ReferenceError; } super(); this.ok = ok; } } row(new D().ok); }
{ class C {} class D extends C { constructor() { let ok = false; try { eval('this'); } catch(e) { ok = e instanceof ReferenceError; } super(); this.ok = ok && eval('this') === this; } } row(new D().ok); }
{ class C { constructor(a,b) { this.s = a+b; } } class D extends C { constructor() { super(...[1,2]); } } row(new D().s === 3); }
{ class C {} class D extends C { constructor() { (() => super())(); } } row(new D() instanceof D); }
{ class C { constructor() { return {x: 7}; } } class D extends C { constructor() { super(); this.z = 8; } } let d = new D(); row(d.x === 7 && d.z === 8); }
{ class C {} class D extends C { constructor() { try { new (super())(); } catch(e) {} this.x = 7; } } row(new D().x === 7); }
// js_source.rs derived_classes_new_target.
{ class C { constructor() { this.target = new.target; } } let c = new C(); row(c.target === C); }
{ class C { constructor() { this.base = new.target; } } class D extends C { constructor() { let target = new.target; super(); this.derived = target; } } let d = new D(); row(d.base === D && d.derived === D && Object.getPrototypeOf(d) === D.prototype); }
{ class C { constructor(a,b) { this.target = new.target; this.sum = a+b; } } class D extends C {} class E extends D { constructor() { let target = new.target; super(...[1,2]); this.same = target === new.target; } } let e = new E(); row(e.target === E && e.sum === 3 && e.same && Object.getPrototypeOf(e) === E.prototype); }
{ class C { constructor(a = () => new.target) { this.target = a; } } class D extends C { constructor(f = () => () => new.target) { let before = f()(); super(); this.same = before === new.target && f()() === new.target; this.arrow = () => new.target; } } let d = new D(); row(d.same && d.target() === D && d.arrow.call(null) === D); }
{ class C { constructor() { this.target = new.target; } } class D extends C { constructor() { let target = () => new.target; let make = () => super(); let before = target(); make(); this.same = before === target() && target() === D; this.targetArrow = target; } } let d = new D(); row(d.same && d.target === D && d.targetArrow() === D); }
{ class C { constructor() { this.target = eval('new.target'); } } class D extends C { constructor() { let target = eval('() => eval("new.target")'); let before = target(); super(); this.same = before === new.target && target() === new.target; this.arrow = target; } } let d = new D(); row(d.same && d.target === D && d.arrow() === D); }
{ class C { constructor() { this.target = new.target; } } class D extends C {} let Bound = D.bind(null); let d = new Bound(); row(d.target === D && Object.getPrototypeOf(d) === D.prototype); }
{ class C { constructor() { this.target = new.target; } } let parent = C.bind(null); parent.prototype = C.prototype; class D extends parent {} let d = new D(); row(d.target === D && Object.getPrototypeOf(d) === D.prototype); }
{ class C { constructor() { this.arrow = () => new.target; } } class D extends C { constructor() { function f() { return new.target; } let before = f(); super(); this.same = before === undefined && f() === undefined && new f() === f; } } let d = new D(); row(d.same && d.arrow() === D); }
// js_source.rs derived_classes_inherited_function_names.
{ class A { static toString() { return 7; } static caller() { return 8; } static arguments() { return 9; } } class B extends A {} row(B.toString() === 7 && B.caller() === 8 && B.arguments() === 9 && 'toString' in B && 'caller' in B && 'arguments' in B); }
{ class A { static toString() { return 7; } static caller() { return 8; } static arguments() { return 9; } } class B extends A {} B.toString = 10; B.caller = 11; B.arguments = 12; row(B.toString === 10 && B.caller === 11 && B.arguments === 12 && A.toString() === 7 && A.caller() === 8 && A.arguments() === 9); }
{ class A { static get toString() { return this.x; } static set toString(v) { this.x = v; } } class B extends A {} B.toString = 7; row(B.toString === 7 && A.x === undefined && !Object.getOwnPropertyDescriptor(B, 'toString')); }
{ class A {} Object.defineProperty(A, 'caller', {value:7}); class B extends A {} let ok = false; try { B.caller = 8; } catch(e) { ok = e instanceof TypeError; } row(ok && B.caller === 7); }
// js_source.rs derived_classes_bound_static_inheritance.
{ class A { static f() { return this.x; } } class B extends A {} let bound = B.bind(null); bound.x = 7; row(Object.getPrototypeOf(bound) === A && bound.f() === 7 && A.x === undefined && new bound() instanceof B); }
{ class A { static f() { return 7; } } class B extends A {} let bound = B.bind(null); let before = Object.getPrototypeOf(bound) === A && bound.f() === 7; bound.own = 1; row(before && Object.getPrototypeOf(bound) === A && bound.f() === 7 && bound.own === 1); }
{ class A { static toString() { return 7; } } class B extends A {} let bound = B.bind(null).bind(null); let before = bound.toString() === 7 && Object.getPrototypeOf(bound) === A; bound.toString = 8; row(before && bound.toString === 8 && B.toString() === 7 && Object.getPrototypeOf(bound) === A); }
{ class A { static toString() { return 'ok'; } } class B extends A {} let bound = B.bind(null); row('' + bound === 'ok'); }
// js_source.rs derived_classes_enumerate_inherited_statics.
{ class C {} C.x = 1; class D extends C {} let keys = ''; for (let k in D) keys += k; row(keys === 'x'); }
{ class C {} C.x = 1; class D extends C {} class E extends D {} let Bound = E.bind(null); let keys = ''; for (let k in Bound) keys += k; row(keys === 'x'); }
{ class C {} C.x = 1; C.y = 2; class D extends C { static x() {} } let keys = ''; for (let k in D) keys += k; row(keys === 'y'); }
{ class C {} C.x = 1; C.y = 2; class D extends C {} let Bound = D.bind(null); Object.defineProperty(Bound, 'x', {value:3}); let keys = ''; for (let k in Bound) keys += k; row(keys === 'y'); }
{ class C {} C.x = 1; class D extends C {} D.y = 2; let keys = ''; for (let k in D) keys += k; row(keys === 'yx'); }
failed === 0 ? rows : -failed;
