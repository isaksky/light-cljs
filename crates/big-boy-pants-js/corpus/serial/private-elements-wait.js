function privateElementsWait(dep) {
  // Private class elements across dependency waits (light-cljs-889): `dep`
  // is a host effect the test answers with "D", in place or after the job's
  // whole stack was saved as a continuation and restored in another heap
  // (snapshot_tests.goose). A private getter runs as a frame above the
  // reading method (g), a private setter under the Setter native frame (s),
  // a private method (m) and a static private method (t) as calls; the
  // private names on the operand stacks, the instance's private elements
  // and the class's static ones survive every restore. 4 waits; completes
  // with "gDsDmDtD:1,4,5,5,true,true,true,true,false".
  let log = "";
  class C {
    #x = { n: 1 };
    #y = 2;
    static #s = 5;
    get #g() { log += "g" + dep(); return this.#x.n; }
    set #g(v) { log += "s" + dep(); this.#x = { n: v }; }
    #m(a) { log += "m" + dep(); return a + this.#y; }
    static #sm() { log += "t" + dep(); return C.#s; }
    run() { const a = this.#g; this.#g = 4; const b = this.#m(3); const c = C.#sm(); return [a, this.#x.n, b, c, #x in this, #g in this, #m in this].join(); }
    static has(o) { return #x in o; }
  }
  const c = new C();
  const r = c.run();
  return log + ":" + r + "," + C.has(c) + "," + C.has({});
}
