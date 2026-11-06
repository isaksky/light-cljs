// Accessors (light-cljs-787): an object literal's getters and setters, own
// and inherited through `__proto__`, run with the object the property was
// read or written on as `this`. A getter's frame is entered from the read; a
// setter runs in a native frame and the assignment yields its right-hand
// side, not the setter's return value. Getter and setter halves merge.
function accessors() {
  let log = 0;
  const base = {
    get v() { return this.k * 2; },
    set v(x) { log += x; this.k = x; return 99; },
  };
  const own = { k: 3, get g() { return this.k + 1; } };
  const child = { __proto__: base, k: 5 };
  const r1 = own.g === 4 && child.v === 10;
  const assigned = (child.v = 7);
  const r2 = assigned === 7 && child.k === 7 && log === 7 && child.v === 14 && base.k === undefined;
  const merged = { get m() { return this.n; }, n: 1, set m(x) { this.n = x + 1; } };
  merged.m = 4;
  const r3 = merged.m === 5;
  // A setter's exception unwinds through its native frame.
  const thrower = { set t(x) { throw x; } };
  let caught = "";
  try { thrower.t = 1; } catch (e) { caught = e; }
  return r1 && r2 && r3 && caught === 1;
}
