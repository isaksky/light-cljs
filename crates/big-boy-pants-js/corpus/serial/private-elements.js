function privateElements() {
  // Private class elements (light-cljs-889) at every budget cut and across
  // collections inside the job (serial_tests.goose): each round evaluates a
  // class body (fresh private names) whose constructor counts in a static
  // private field, constructs an instance with private fields, a method
  // and an accessor pair (and setter-only accessors, which `#x in` sees),
  // adds the same elements to an array, a frozen object and a garbage
  // object through the constructor-return trick, and keeps every tenth
  // class with its instance and array. Afterwards the
  // kept elements still read and write, a brand check across classes
  // throws, and a proxy holds its own elements.
  const kept = [];
  class Base { constructor(o) { if (o) { return o; } } }
  for (let i = 0; i < 30; i++) {
    const C = class K extends Base {
      #x = i;
      #o = { n: i };
      static #count = 0;
      #m() { return this.#x + 1; }
      get #g() { return this.#o.n; }
      set #g(v) { this.#o = { n: v }; }
      set #w(v) { this.#x = v; }
      static set #sw(v) {}
      constructor(o) { super(o); K.#count++; }
      static read(o) { return o.#m() + o.#g; }
      static bump(o, v) { o.#g = v; o.#x++; return o.#x; }
      static count() { return K.#count; }
      static has(o) { return #x in o && #m in o && #g in o && #w in o; }
      static sets(o) { return #sw in o; }
    };
    const c = new C();
    const arr = new C([1, 2]);
    const frozen = new C(Object.freeze({}));
    new C({ junk: [1, 2, 3, 4, 5, 6, 7, 8] });
    if (C.read(c) !== 2 * i + 1 || C.bump(arr, 7) !== i + 1 || C.read(arr) !== i + 9 || C.read(frozen) !== 2 * i + 1 || arr.length !== 2 || C.count() !== 4 || !C.has(c) || C.has({}) || !C.sets(C) || C.sets(c) || Object.keys(c).length !== 0) { return false; }
    if (i % 10 === 0) { kept.push(C); kept.push(c); kept.push(arr); }
  }
  let ok = kept.length === 9;
  for (let j = 0; j < 3; j++) {
    const C = kept[j * 3]; const c = kept[j * 3 + 1]; const arr = kept[j * 3 + 2];
    if (C.read(c) !== 20 * j + 1 || C.read(arr) !== 10 * j + 9 || !C.has(arr) || kept[((j + 1) % 3) * 3].has(c)) { ok = false; }
  }
  let caught = false;
  try { kept[0].read(kept[4]); } catch (e) { caught = e instanceof TypeError; }
  const target = {};
  const p = new kept[0](new Proxy(target, {}));
  return ok && caught && kept[0].has(p) && !kept[0].has(target) && kept[0].bump(p, 3) === 1 && kept[0].read(p) === 5;
}
