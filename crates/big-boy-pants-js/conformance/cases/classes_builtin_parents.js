// bbp-authored conformance case (light-cljs-887).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 1163-1187
// (derived_classes_builtin_parents, bbp-309: the Array and Error parents)
// and rust/tests/builtin_subclasses.rs lines 36-46
// (array_subclass_preserves_exotic_length_and_forwarding), 49-55
// (error_subclasses_preserve_brand_stack_name_and_cause, without its stack
// terms), 85-91 (the Boolean, Number, String and Symbol rows of
// date_regexp_and_wrapper_subclasses_keep_internal_slots), 271-277 (the
// Function row of buffer_typed_array_and_function_subclasses), 285-288
// (Proxy is no heritage), 302-309 (bound_builtin_super_and_argument_forwarding),
// 477 (an Error subclass converting its message), 61-75
// (collection_subclasses_use_derived_set_and_add, its Map and Set rows;
// light-cljs-943) and 630-637
// (new_target_prototype_getter_resumes_once_and_propagates_errors, with
// Reflect.construct and a bound newTarget for its `new Proxy(B, {})`, whose
// [[Construct]] is light-cljs-891's), with light-cljs-766's planner rows
// and light-cljs-765's `class ... extends Error` rows: every built-in
// constructor this VM has takes its object's prototype from newTarget
// (GetPrototypeFromConstructor, through a getter once, before its steps),
// arrays stay exotic, errors keep their brand, wrappers their primitive.
// Left out: WeakMap and WeakSet (light-cljs-945), Promise, Date, RegExp,
// typed array and ArrayBuffer parents (light-cljs-768 to -770), SuppressedError (light-cljs-772),
// BigInt (light-cljs-771) and Intl.Segmenter (light-cljs-774). Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// A bound function whose `prototype` is a getter counting its reads.
function getter(get) { const G = (function () {}).bind(null); Object.defineProperty(G, "prototype", {get}); return G; }
// The planner's rows (light-cljs-766, light-cljs-765).
{ class L extends Array {} let l = new L(); l.push(1, 2); row(l.length + "," + (l instanceof L) + "," + Array.isArray(l) === "2,true,true"); }
{ class F extends Function {} let f = new F("return 7"); row(f() === 7); }
{ class E extends Error { constructor(m) { super(m); this.name = "E" } } let e = new E("x"); row((e instanceof Error) + "," + e.message + "," + String(e) === "true,x,E: x"); }
{ class E extends Error {} let e = new E("m"); row(Object.getPrototypeOf(e) === E.prototype && e.name === "Error" && Object.prototype.toString.call(e) === "[object Error]" && Object.getOwnPropertyDescriptor(e, "name") === undefined && Object.getOwnPropertyDescriptor(e, "message").value === "m"); }
{ class E extends RangeError { constructor() { super("r"); this.k = 1 } } let e = new E(); row(e instanceof RangeError && e.message === "r" && e.k === 1 && String(e) === "RangeError: r"); }
{ class E extends Error {} try { throw new E("t") } catch (e) { row(e instanceof E && e.message === "t"); } }
{ class B extends Object { constructor() { super(5) } } let b = new B(); row(typeof b === "object" && b instanceof B && Object.getPrototypeOf(b) === B.prototype); }
{ class O extends Object {} row(Object.getPrototypeOf(new O(7)) === O.prototype && Object.keys(new O({a: 1})).length === 0); }
{ class S extends String {} let s = new S("ab"); row(s.length === 2 && s[1] === "b" && s instanceof S && s.toString() === "ab" && typeof s === "object"); }
{ class N extends Number { twice() { return this.valueOf() * 2 } } row(new N(4).twice() === 8 && new N("3") + 1 === 4); }
{ class B extends Boolean {} row(new B(1).valueOf() === true && new B(1) instanceof B && Object.getPrototypeOf(new B()) === B.prototype); }
{ class F extends Function {} let f = new F("a", "b", "return a * b"); row(f(3, 4) === 12 && f instanceof F && Object.getPrototypeOf(f) === F.prototype && f.name === "anonymous"); }
{ class A extends Array { sum() { let s = 0; for (let i = 0; i < this.length; i++) s += this[i]; return s } } let a = new A(1, 2, 3); a.length = 2; row(a.sum() === 3 && a.length === 2 && a[0] + "," + a[1] + "," + a[2] === "1,2,undefined"); }
{ class A extends Array {} let a = new A(); a[5] = 1; row(a.length === 6 && Array.isArray(a) && Object.getPrototypeOf(a) === A.prototype); }
// Built-ins with newTarget's `prototype` through a getter (once, before the
// steps; a non-object gives the constructor's realm prototype; a throw).
{ let n = 0; const p = Object.create(Array.prototype); let a = Reflect.construct(Array, [1, 2], getter(() => { n++; return p })); row(n === 1 && Object.getPrototypeOf(a) === p && Array.isArray(a) && a.length === 2); }
{ let n = 0; let a = Reflect.construct(Array, [3], getter(() => { n++; return null })); row(n === 1 && Object.getPrototypeOf(a) === Array.prototype && a.length === 3); }
{ let n = 0; let caught; try { Reflect.construct(Array, [1, 2], getter(() => { n++; throw 17 })) } catch (e) { caught = e } row(caught === 17 && n === 1); }
{ let n = 0; const p = {}; let e = Reflect.construct(TypeError, ["m"], getter(() => { n++; return p })); row(n === 1 && Object.getPrototypeOf(e) === p && e.message === "m"); }
{ let log = []; let e = Reflect.construct(Error, [{ toString() { log.push("message"); return "m" } }], getter(() => { log.push("prototype"); return Error.prototype })); row(log.join() === "prototype,message" && e.message === "m"); }
{ let n = 0; const p = {}; let o = Reflect.construct(Object, [], getter(() => { n++; return p })); row(n === 1 && Object.getPrototypeOf(o) === p); }
{ let n = 0; let w = Reflect.construct(Number, [5], getter(() => { n++; return 3 })); row(n === 1 && Object.getPrototypeOf(w) === Number.prototype && w.valueOf() === 5); }
{ let log = []; let w = Reflect.construct(String, [{ toString() { log.push("value"); return "s" } }], getter(() => { log.push("prototype"); return String.prototype })); row(log.join() === "value,prototype" && w.valueOf() === "s"); }
{ row(name(() => Reflect.construct(Boolean, [1], getter(() => { throw new RangeError("r") }))) === "RangeError"); }
{ let n = 0; const G = getter(() => { n++; throw 17 }); row(name(() => Reflect.construct(Number, [Symbol()], G)) === "TypeError" && name(() => Reflect.construct(String, [Symbol("s")], G)) === "TypeError" && n === 0); }
{ let log = []; const p = Object.create(Function.prototype); let f = Reflect.construct(Function, [{ toString() { log.push("body"); return "return 4" } }], getter(() => { log.push("prototype"); return p })); row(log.join() === "body,prototype" && Object.getPrototypeOf(f) === p && f.call() === 4); }
// js_source.rs derived_classes_builtin_parents (bbp-309).
{ class D extends Array {} row(Object.getPrototypeOf(D) === Array && Object.getPrototypeOf(D.prototype) === Array.prototype); }
{ class D extends Array {} let d = new D(); row(d instanceof D && d instanceof Array); }
{ class D extends Error {} row(Object.getPrototypeOf(D) === Error && Object.getPrototypeOf(D.prototype) === Error.prototype); }
{ class D extends Error {} let d = new D(); row(d instanceof D && d instanceof Error); }
// builtin_subclasses.rs (bbp-309).
{ class A extends Array {} let a = new A(1,2,3); row(a.length === 3 && a[2] === 3 && a instanceof A && a instanceof Array && Array.isArray(a) && Object.getPrototypeOf(a) === A.prototype); }
{ class A extends Array { constructor(n) { super(n); this.tag = 7; } } class B extends A {} let b = new B(4); row(b.length === 4 && !(0 in b) && b.tag === 7 && b instanceof B && b instanceof A && b.push(8) === 5 && b[4] === 8); }
{ class A extends Array {} let ok; try { new A(-1); } catch(e) { ok = e instanceof RangeError; } row(ok); }
{ class E extends Error {} E.prototype.name = 'E'; let cause = {}; let e = new E('x',{cause}); row(e instanceof E && e instanceof Error && e.message === 'x' && e.cause === cause && e.toString() === 'E: x'); }
{ let ok = true; for (let C of [TypeError,RangeError,ReferenceError,SyntaxError,EvalError,URIError,InternalError]) { class E extends C {} let e = new E('x'); ok = ok && e instanceof E && e instanceof C && e instanceof Error && e.message === 'x' && e.name === C.name; } row(ok); }
{ class B extends Boolean {} class N extends Number {} class S extends String {} let b = new B(0), n = new N({valueOf(){return 12;}}), s = new S({toString(){return 'abc';}}); row(b instanceof B && b.valueOf() === false && n instanceof N && n.valueOf() === 12 && s instanceof S && s.valueOf() === 'abc' && s.length === 3 && s[1] === 'b'); }
{ let n = 0; for (let C of [Symbol]) { try { class X extends C {} new X(1); } catch(e) { if(e instanceof TypeError) n++; } } row(n === 1); }
{ class F extends Function {} let f = new F('x','return x + 1'); row(f instanceof F && f instanceof Function && f(2) === 3); }
{ let ok; try { class P extends Proxy {} } catch(e) { ok = e instanceof TypeError; } row(ok && Proxy.prototype === undefined); }
{ class A extends Array {} let B = A.bind(null,1); let a = new B(2); row(a instanceof A && a.length === 2 && a[0] === 1 && a[1] === 2); }
{ let B = Array.bind(null,1); B.prototype = Array.prototype; class A extends B {} let a = new A(2); row(a instanceof A && a.length === 2 && a[0] === 1 && a[1] === 2); }
{ class E extends Error {} row(new E({toString(){return 'x';}}).message === 'x'); }
// collection_subclasses_use_derived_set_and_add (builtin_subclasses.rs line 61): a
// subclass's own `set` and `add` store the iterable's entries (light-cljs-943).
{ let calls = 0; class M extends Map { set(k,v) { calls++; return super.set(k,v+1); } } let m = new M([[1,2],[3,4]]); row(m instanceof M && m instanceof Map && m.get(1) === 3 && calls === 2 && m.size === 2); }
{ let calls = 0; class S extends Set { add(k) { calls++; return super.add(k+1); } } let s = new S([1,2]); row(s instanceof S && s instanceof Set && s.has(2) && s.has(3) && calls === 2); }
{ let reads = 0; let G = getter(() => { reads++; return Map.prototype; }); let m = Reflect.construct(Map, [[[1, 2]]], G); row(reads === 1 && Object.getPrototypeOf(m) === Map.prototype && m.get(1) === 2); }
failed === 0 ? rows : -failed;
