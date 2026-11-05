// Synthetic fixture, not light-cljs output.
function assert(ok) { if (!ok) throw new Error('private class assertion'); }
function typeError(f) {
    let caught = false;
    try { f(); } catch (e) { caught = e instanceof TypeError; }
    assert(caught);
}
class A {
    #x = 1;
    #m(v) { return this.#x + v; }
    get #value() { return this.#x; }
    set #value(v) { this.#x = v; }
    get x() { return this.#value; }
    set x(v) { this.#value = v; }
    call(v) { return this.#m(v); }
    has(o) { return #x in o && #m in o && #value in o; }
    read(o) { return o.#x; }
    method(o) { return o.#m; }
    accessor(o) { return o.#value; }
    write(o) { o.#x = 7; }
    update() { this.#x++; this.#x += 2; this.#x &&= 8; return this.#x; }
    arrow() { return () => this.#x; }
    evalRead(o) { return eval('o.#x'); }
    static #s = 10;
    static #sm() { return this.#s; }
    static get #v() { return this.#s; }
    static set #v(v) { this.#s = v; }
    static read() { return this.#sm(); }
    static change(v) { this.#v = v; return this.#v; }
    static has(o) { return #s in o && #sm in o && #v in o; }
}
let a = new A();
assert(a.x === 1 && a.call(2) === 3 && a.has(a));
a.x = 4;
assert(a.x === 4 && a.evalRead(a) === 4 && a.update() === 8 && a.arrow()() === 8);
assert(!a.has({}) && !a.has(A.prototype));
for (let o of [{}, A.prototype, new Proxy(a, {})]) {
    typeError(() => a.read(o));
    typeError(() => a.method(o));
    typeError(() => a.accessor(o));
    typeError(() => a.write(o));
}
typeError(() => a.has(1));
assert(A.read() === 10 && A.change(12) === 12 && A.has(A));
class B extends A {}
let b = new B();
assert(b.has(b) && b.call(1) === 2 && !A.has(B));
typeError(() => B.read());
assert(Object.getOwnPropertyNames(a).length === 0);
assert(Object.getOwnPropertyDescriptor(A, '#s') === undefined);
Object.freeze(a);
a.x = 9;
assert(a.x === 9);
function make() { return class { #x = 3; has(o) { return #x in o; } }; }
let C = make(), D = make();
assert(new C().has(new C()) && !new C().has(new D()));
class Outer {
    #x = 17;
    inner() { return new class { read(o) { return o.#x; } }(); }
    shadow() { return new class { #x = 19; read() { return this.#x; } }(); }
}
let outer = new Outer();
assert(outer.inner().read(outer) === 17 && outer.shadow().read() === 19);
class Order {
    first = this.#m();
    #m() { return 23; }
    static first = this.#sm();
    static #sm() { return 29; }
}
assert(new Order().first === 23 && Order.first === 29);
true;
