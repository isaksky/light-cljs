function classesWait(dep) {
  // A base class across a dependency wait (light-cljs-886): `dep` is a host
  // effect the test answers with "D", in place or after the job's whole
  // stack was saved as a continuation and restored in another heap
  // (snapshot_tests.goose). Reflect.construct gives the class a newTarget
  // whose `prototype` getter waits, under the Construct native frame that
  // then enters the constructor; the class (its static field and block
  // already run), its eager prototype and the arguments keep their
  // identity after it, and the instance fields run before the body.
  // Completes with "D:true".
  let seen = '';
  class C {
    a = 1;
    b = this.a + 1;
    static s = 7;
    static { this.t = this.s + 1; }
    constructor(x, y) { this.c = x + y + this.b; this.n = new.target === G; }
    m() { return this.c; }
  }
  const G = (function () {}).bind(null);
  Object.defineProperty(G, 'prototype', { get() { seen += dep(); return C.prototype; } });
  const o = Reflect.construct(C, [3, 4], G);
  return seen + ':' + (o.n && o.m() === 9 && Object.keys(o).join() === 'a,b,c,n' && Object.getPrototypeOf(o) === C.prototype && C.prototype.constructor === C && C.t === 8 && o instanceof C);
}
