function derivedClassesWait(dep) {
  // Derived classes across a dependency wait (light-cljs-887): `dep` is a
  // host effect the test answers with "D", in place or after the job's whole
  // stack was saved as a continuation and restored in another heap
  // (snapshot_tests.goose). Reflect.construct(Array, ...) reads its
  // newTarget's `prototype` getter above a Construct native frame whose
  // constructor is the built-in; the getter constructs a derived class
  // whose parent waits inside super(), above the derived frame whose `this`
  // cell is not bound yet (an arrow captured it). Completes with "D:true".
  let seen = '';
  class A { constructor(x) { seen += dep(); this.x = x; } }
  class B extends A {
    y = 2;
    constructor() {
      const f = () => this;
      let early = false;
      try { f(); } catch (e) { early = e instanceof ReferenceError; }
      super(1);
      this.ok = early && f() === this;
    }
  }
  class L extends Array {}
  let b;
  const G = (function () {}).bind(null);
  Object.defineProperty(G, 'prototype', { get() { b = new B(); return L.prototype; } });
  const a = Reflect.construct(Array, [3, 4], G);
  return seen + ':' + (b.ok && b.x === 1 && b.y === 2 && b instanceof B && a instanceof L && Array.isArray(a) && a.length === 2 && a[1] === 4);
}
