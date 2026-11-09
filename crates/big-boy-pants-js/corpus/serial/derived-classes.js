function derivedClasses() {
  // Derived classes (light-cljs-887) at every budget cut and across
  // collections inside the job (serial_tests.goose): each round makes base
  // and derived classes nothing keeps and constructs through super() (the
  // derived frame's `this` cell read too early by an arrow, then bound by
  // a super() inside another arrow; a default derived constructor's
  // forwarded arguments; instance fields after super()), subclasses of
  // Array and Error, and keeps every tenth.
  const kept = [];
  for (let i = 0; i < 30; i++) {
    class A { constructor(x) { this.x = x; this.t = new.target; } static s() { return 'A' + i; } }
    class B extends A {
      y = this.x + 1;
      constructor(x) {
        const f = () => this;
        let early = false;
        try { f(); } catch (e) { early = e instanceof ReferenceError; }
        const g = () => super(x);
        g();
        this.early = early; this.same = f() === this;
      }
    }
    class C extends B {}
    class L extends Array { first() { return this[0]; } }
    class E extends Error { constructor(m) { super(m); this.k = i; } }
    const c = new C(i);
    const l = new L(i, i + 1);
    const e = new E('m' + i);
    const junk = new C(-1);
    if (i % 10 === 0) { kept.push(c); kept.push(C); kept.push(l); kept.push(e); }
    if (c.x !== i || c.y !== i + 1 || c.t !== C || !c.early || !c.same || C.s() !== 'A' + i || l.length !== 2 || l.first() !== i || !(e instanceof E) || e.message !== 'm' + i || junk.y !== 0) { return false; }
  }
  let ok = kept.length === 12;
  for (let j = 0; j < 3; j++) {
    const c = kept[j * 4]; const C = kept[j * 4 + 1]; const l = kept[j * 4 + 2]; const e = kept[j * 4 + 3];
    l.push(5);
    if (!(c instanceof C) || c.x !== j * 10 || l.length !== 3 || !Array.isArray(l) || String(e) !== 'Error: m' + j * 10 || e.k !== j * 10) { ok = false; }
  }
  // A built-in's newTarget `prototype` getter: the Construct native frame
  // above a built-in constructor.
  const L0 = kept[2].constructor;
  const G = (function () {}).bind(null);
  Object.defineProperty(G, 'prototype', { get() { return L0.prototype; } });
  const a = Reflect.construct(Array, [1, 2, 3], G);
  // The Function constructor reads it after compiling (its DynamicFunction
  // frame's pc 1); each round's function and getter are garbage.
  let reads = 0;
  for (let k = 0; k < 10; k++) {
    const p = Object.create(Function.prototype); p.k = k;
    const H = (function () {}).bind(null);
    Object.defineProperty(H, 'prototype', { get() { reads++; return p; } });
    const fn = Reflect.construct(Function, ['x', 'return x + ' + k], H);
    if (fn(1) !== k + 1 || fn.k !== k) { ok = false; }
  }
  return ok && reads === 10 && a.length === 3 && a instanceof L0 && a.first() === 1 && Array.isArray(a);
}
