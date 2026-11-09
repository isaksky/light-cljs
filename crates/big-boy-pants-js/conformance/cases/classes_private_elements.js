// bbp-authored conformance case (light-cljs-889).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 8589-8598
// (private_class_names, bbp-291; its fixture is the case
// pants/private_classes) and 8638-8678 (private_class_edge_cases, every row
// but the two `with` rows, which are classes_private_elements_sloppy), with
// light-cljs-766's planner rows and rows of its own for the brand check
// (own elements only, never a prototype's, a proxy's target's or a fresh
// evaluation's), read-only private methods, getter-only and setter-only
// accessors, static elements, `#x in o` (a TypeError for a primitive),
// proxies whose traps never run, frozen, sealed and non-extensible objects
// that still take private fields through the constructor-return trick, and
// private elements hidden from own keys, for-in, descriptors, spread and
// Object.assign. Adapted where the oracles disagree or Goose's realm lacks a
// built-in: the setter-only row drops its `#x in this` (QuickJS answers
// false for a setter-only element there, where the Pants engine follows the
// specification and answers true), and the async row checks `typeof` instead of `instanceof
// Promise` (Goose's realm has no %Promise% constructor yet). Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-766).
{ class C { #x=1; get(){return this.#x} } row(new C().get() === 1); }
{ class C { static #p = 1; #m(){ return 2 } static t(o){ return #p in C && #m in o } test(){ return this.#m() } } row(C.t(new C()) + ',' + new C().test() === "true,2"); }
// private_class_names and private_class_edge_cases.
row((() => { class A { #x = 1; get x() { return this.#x; } } return new A().x; })() === 1);
{ class C { #x; get() { return this.#x; } } row(new C().get() === undefined); }
{ class C { #x = function() {}; #m() {} get #a() {} set #a(v) {} names() { return [this.#x.name,this.#m.name]; } } row(new C().names().join() === '#x,#m'); }
{ class C { #x=1; m() { let n=0; this.#x ||= n++; this.#x ??= n++; this.#x &&= 3; return this.#x === 3 && n === 0; } } row(new C().m()); }
{ let n=0; class C { #x=2; m() { let o=()=>{n++;return this;}; return o().#x++ === 2 && n === 1 && this.#x === 3; } } row(new C().m()); }
{ class C { #x=1; get #only() { return this.#x; } m() { try { this.#only=2; } catch(e) { return e instanceof TypeError; } } } row(new C().m()); }
{ class C { #m() {} m() { try { this.#m=2; } catch(e) { return e instanceof TypeError; } } } row(new C().m()); }
{ class C { set #x(v) {} m() {try {this.#x;} catch(e) {return e instanceof TypeError;} return false;} } row(new C().m()); }
{ class C { static set #x(v) {} static m() {try {this.#x;} catch(e) {return e instanceof TypeError;} return false;} } row(C.m()); }
{ let writes=0; class C { set #x(v) {writes++;} m() {let n=0;try {this.#x+=(writes++,1);} catch(e) {if(e instanceof TypeError)n++;} try {this.#x++;} catch(e) {if(e instanceof TypeError)n++;} try {this.#x??=2;} catch(e) {if(e instanceof TypeError)n++;} return n===3 && writes===0;} } row(new C().m()); }
{ class C { set #x(v) {this.y=v;} get #x() {return this.y;} m() {this.#x=4;return this.#x;} } row(new C().m() === 4); }
{ class C { #x=1; m(o) { return o?.#x; } } row(new C().m(null) === undefined && new C().m(new C()) === 1); }
{ class C { #m() {return this;} m(o) { return o?.#m() === o && this.#m?.() === this; } } let c=new C(); row(c.m(c)); }
{ class C { #x=0; m() { [this.#x] = [3]; for ([this.#x] of [[4],[5]]) {} return this.#x; } } row(new C().m() === 5); }
{ class C { #x=1; m() {return eval('eval("this.#x")');} } row(new C().m() === 1); }
{ class C { #x=1; m() {try {Function('return this.#x');} catch(e) {return e instanceof SyntaxError;}} } row(new C().m()); }
{ class C { #x=1; m() {try {(0,eval)('this.#x');} catch(e) {return e instanceof SyntaxError;}} } row(new C().m()); }
{ class C { #x=1; m() {try {eval('class D { #x; m(o) { return o.#x; } } new D().m(this)');} catch(e) {return e instanceof TypeError;}} } row(new C().m()); }
{ class C { #x=1; *#g() {yield this.#x;} m() {return this.#g().next().value;} } row(new C().m() === 1); }
{ class C { #x=1; async #m() {return this.#x;} m() {return typeof this.#m() === "object";} } row(new C().m()); }
{ class C { #if=1; #x=2; m() {return this.#if+this.#x;} } row(new C().m() === 3); }
{ class C { #x=1
  #y=2
  m() {return this.#x+this.#y;} } row(new C().m() === 3); }
{ class C { #x = (() => { try {return #x in this;} catch(e) {return 9;} })(); m() {return this.#x;} } row(new C().m() === false); }
{ class C { #x=1; #y=this.#later; #later=2; } let ok=false;try {new C();} catch(e) {ok=e instanceof TypeError;} row(ok); }
{ let n=0; class C { #x; has(o) {return #x in o;} m(o) {try {o.#x;} catch(e) {return e instanceof TypeError;}} } let c=new C(), p=new Proxy(c,{get(){n++;throw 1;},has(){n++;throw 2;}}); row(!c.has(p) && c.m(p) && n===0); }
{ class Base {constructor(o) {return o;}} class C extends Base { #x=1; static read(o){return o.#x;} } let o=Object.preventExtensions({});new C(o); row(C.read(o) === 1); }
{ class Base {constructor(o) {return o;}} class C extends Base { #x=1; static read(o){return o.#x;} } let o=[];new C(o); row(C.read(o) === 1 && o.length===0); }
{ class Base {constructor(o) {return o;}} class C extends Base { #x=1; static read(o){return o.#x;} } let o=function(){};new C(o); row(C.read(o) === 1); }
{ class Base {constructor(o) {return o;}} class C extends Base { #x=1; static read(o){return o.#x;} } let target={}, o=new Proxy(target,{defineProperty(){throw 1;}});new C(o); let ok=false;try {C.read(target);} catch(e){ok=e instanceof TypeError;} row(C.read(o)===1 && ok); }
{ class Base {constructor(o) {return o;}} class C extends Base { #x; } let o={};new C(o);let ok=false;try {new C(o);} catch(e){ok=e instanceof TypeError;} row(ok); }
{ class Base {constructor(o) {return o;}} class C extends Base { #m(){} } let o={};new C(o);let ok=false;try {new C(o);} catch(e){ok=e instanceof TypeError;} row(ok); }
{ class C { #x=1; static #s=2; static get #a(){return this.#s;} static set #a(v){this.#s=v;} static m(){Object.freeze(this);this.#a=4;return this.#a;} } row(C.m()===4); }
// Brand checks: own elements only, a TypeError otherwise.
{ class C { #x = 1; static read(o) { return o.#x; } static write(o) { o.#x = 2; } static call(o) { return o.#m(); } #m() { return 3; } } let c = new C();
  row(name(() => C.read(Object.create(c))) === "TypeError" && name(() => C.write(C.prototype)) === "TypeError" && name(() => C.call({})) === "TypeError" && C.call(c) === 3); }
{ class C { #x = 1; static has(o) { return #x in o; } } row(name(() => C.has(1)) === "TypeError" && name(() => C.has("s")) === "TypeError" && name(() => C.has(undefined)) === "TypeError" && name(() => C.has(Symbol())) === "TypeError" && C.has(new C()) && !C.has(Object.create(new C()))); }
{ class C { #x = 1; static read(o) { return o.#x; } } row(name(() => C.read(null)) === "TypeError" && name(() => C.read(1)) === "TypeError"); }
{ function make() { return class { #x = 3; static has(o) { return #x in o; } }; } let A = make(), B = make(); row(A.has(new A()) && !A.has(new B()) && !B.has(new A())); }
{ class A { #x = 1; static read(o) { return o.#x; } } class B extends A { #x = 2; static read(o) { return o.#x; } } let b = new B(); row(A.read(b) === 1 && B.read(b) === 2 && name(() => B.read(new A())) === "TypeError"); }
// Methods and accessors: read-only, getter-only and setter-only.
{ class C { #m() { return this; } m() { return this.#m === this.#m && this.#m() === this; } } row(new C().m()); }
{ class C { #m() {} static check(a, b) { return a.#m === b.#m; } } row(C.check(new C(), new C())); }
{ class C { get #g() { return 5; } m() { return this.#g + name(() => { this.#g = 1; }); } } row(new C().m() === "5TypeError"); }
{ let seen; class C { set #s(v) { seen = v; } m() { this.#s = 7; return seen + name(() => this.#s); } } row(new C().m() === "7TypeError"); }
{ class C { get #a() { return this.v; } set #a(v) { this.v = v * 2; } m() { this.#a = 3; this.#a += 1; return this.#a; } } row(new C().m() === 14); }
{ class C { static #s = 1; static get #t() { return C.#s; } static m() { C.#s++; return C.#t; } static has(o) { return #s in o; } } row(C.m() === 2 && C.has(C) && !C.has(new C())); }
{ class A { static #s = 1; static read() { return this.#s; } } class B extends A {} row(A.read() === 1 && name(() => B.read()) === "TypeError"); }
// Proxies: a proxy has its own private elements, and no trap runs.
{ let n = 0; class C { #x = 1; static read(o) { return o.#x; } static has(o) { return #x in o; } } let c = new C(), handler = {get() { n++; }, set() { n++; }, has() { n++; }, getOwnPropertyDescriptor() { n++; }, defineProperty() { n++; }}, p = new Proxy(c, handler);
  row(name(() => C.read(p)) === "TypeError" && !C.has(p) && n === 0); }
{ class Base { constructor(o) { return o; } } class C extends Base { #x = 1; static read(o) { return o.#x; } static has(o) { return #x in o; } } let target = {}, p = new Proxy(target, {}); new C(p); row(C.read(p) === 1 && C.has(p) && !C.has(target)); }
// Integrity levels do not stop private fields.
{ class Base { constructor(o) { return o; } } class C extends Base { #x = 1; static inc(o) { return ++o.#x; } } let a = Object.freeze({}), b = Object.seal({}), c = Object.freeze([1, 2]); new C(a); new C(b); new C(c); row(C.inc(a) === 2 && C.inc(b) === 2 && C.inc(c) === 2 && Object.isFrozen(a) && Object.isSealed(b) && Object.isFrozen(c)); }
{ class C { #x = 1; m() { Object.freeze(this); this.#x = 5; return this.#x; } } row(new C().m() === 5); }
// Private elements are not properties.
{ class C { #x = 1; #m() {} get #a() { return 1; } y = 2; } let c = new C(); row(Object.getOwnPropertyNames(c).join() === "y" && Reflect.ownKeys(c).join() === "y" && Object.keys(c).join() === "y"); }
{ class C { #x = 1; y = 2; } let c = new C(), keys = []; for (let k in c) keys.push(k); row(keys.join() === "y" && Object.getOwnPropertyDescriptor(c, '#x') === undefined && !('#x' in c)); }
{ class C { #x = 1; y = 2; static read(o) { return o.#x; } } let c = new C(), s = {...c}, t = Object.assign({}, c); row(Object.keys(s).join() === "y" && Object.keys(t).join() === "y" && name(() => C.read(s)) === "TypeError" && Object.entries(Object.getOwnPropertyDescriptors(c)).length === 1); }
{ class C { static #s = 1; static t = 2; } row(Reflect.ownKeys(C).join() === "length,name,prototype,t" && Object.getOwnPropertyNames(C.prototype).join() === "constructor"); }
{ class Base { constructor(o) { return o; } } class C extends Base { #x = 1; static read(o) { return o.#x; } } let f = function g(a, b) {}; new C(f); row(C.read(f) === 1 && f.length === 2 && f.name === "g" && Reflect.ownKeys(f).join() === Reflect.ownKeys(function g(a, b) {}).join()); }
{ class Base { constructor(o) { return o; } } class C extends Base { #x = 1; static read(o) { return o.#x; } } let f = function () {}.bind(null), e = new Error("m"); new C(f); new C(e); row(C.read(f) === 1 && C.read(e) === 1 && e.message === "m"); }
// Fresh names each evaluation; elements defined in order.
{ let classes = []; for (let i = 0; i < 3; i++) classes.push(class { #v = i; static read(o) { return o.#v; } }); row(classes[1].read(new classes[1]()) === 1 && name(() => classes[0].read(new classes[2]())) === "TypeError"); }
{ let log = []; class C { a = log.push('a'); #b = log.push('b'); c = log.push('c'); #m() {} } new C(); row(log.join() === "a,b,c"); }
{ class C { first = this.#m(); #m() { return 23; } static first = this.#sm(); static #sm() { return 29; } } row(new C().first === 23 && C.first === 29); }
{ class Outer { #x = 17; inner() { return new class { read(o) { return o.#x; } }(); } shadow() { return new class { #x = 19; read() { return this.#x; } }(); } } let o = new Outer(); row(o.inner().read(o) === 17 && o.shadow().read() === 19); }
failed === 0 ? rows : -failed;
