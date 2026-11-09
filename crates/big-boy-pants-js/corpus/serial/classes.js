function classes() {
  // Base classes (light-cljs-886) at every budget cut and across
  // collections inside the job (serial_tests.goose): each round makes a
  // class nothing keeps (its constructor, eager prototype, methods,
  // accessors, field initializer and static block), constructs instances
  // whose fields run before the constructor body, and keeps every tenth.
  const kept = [];
  let log = 0;
  for (let i = 0; i < 30; i++) {
    const C = class K {
      x = i;
      y = () => this.x + 1;
      static n = i * 2;
      static { log += K.n === i * 2 ? 1 : 100; }
      constructor(z) { this.z = z + this.y(); this.t = new.target === K; }
      m() { return this.x + this.z; }
      get g() { return this.x * 2; }
      set g(v) { this.x = v; }
      static s() { return K.n; }
      ['k' + (i % 3)]() { return i % 3; }
    };
    const c = new C(1);
    const junk = new C(2);
    const r = Reflect.construct(C, [3], Object);
    if (i % 10 === 0) { kept.push(c); kept.push(C); kept.push(r); }
    if (c.m() !== i + i + 2 || C.s() !== i * 2 || !c.t || r.t || junk.g !== i * 2) { return false; }
  }
  let ok = kept.length === 9 && log === 30;
  for (let j = 0; j < 3; j++) {
    const c = kept[j * 3]; const C = kept[j * 3 + 1]; const r = kept[j * 3 + 2];
    c.g = 1;
    if (!(c instanceof C) || c.x !== 1 || c.y() !== 2 || C.n !== j * 20 || c['k' + (j * 10 % 3)]() !== j * 10 % 3 || Object.getPrototypeOf(r) !== Object.prototype || r.x !== j * 10) { ok = false; }
  }
  let caught = false;
  try { kept[1](); } catch (e) { caught = e instanceof TypeError; }
  const d = Object.getOwnPropertyDescriptor(kept[1], 'prototype');
  // A newTarget whose `prototype` is a getter: the Construct native frame.
  const G = (function () {}).bind(null);
  Object.defineProperty(G, 'prototype', { get() { return kept[1].prototype; } });
  const g = Reflect.construct(kept[1], [5], G);
  return ok && caught && g.z === 6 && !g.t && g instanceof kept[1] && !d.writable && !d.enumerable && !d.configurable && Object.getOwnPropertyNames(kept[1]).join() === 'length,name,prototype,s,n';
}
