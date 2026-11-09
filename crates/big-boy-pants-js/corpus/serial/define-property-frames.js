// Property descriptors (light-cljs-881): Object.defineProperty,
// Reflect.defineProperty and Object.defineProperties read descriptor
// fields and properties through getters that each run a loop above their
// Descriptor native frame; an object key converts in a Convert frame above
// it; built-in getters and setters run on the general call path (a Forward
// frame, Array.prototype.join's native frame); arrays take element
// attributes and a read-only `length`. serial_tests.goose stops and
// resumes those frames at every budget cut, and collects inside the job at
// every cut while they hold the target, the descriptor object, the
// descriptor being read and defineProperties's key list.
function definePropertyFrames() {
  "use strict";
  let order = "";
  const spin = (tag) => { let i = 0; while (i < 3) i++; order += tag; };
  const o = {};
  const d = { get enumerable() { spin("e"); return true; }, get value() { spin("v"); return { n: 1 }; }, get writable() { spin("w"); return false; } };
  Object.defineProperty(o, { toString() { spin("k"); return "a"; } }, d);
  const t = {};
  const props = { get x() { spin("x"); return { get get() { spin("g"); return function () { return this.y + 1; }; }, configurable: true }; }, y: { value: 41, writable: true } };
  Object.defineProperties(t, props);
  const r = {};
  const ok1 = Reflect.defineProperty(r, "p", { get value() { spin("p"); return 7; } }) && !Reflect.defineProperty(r, "p", { value: 8 });
  const b = {};
  Object.defineProperty(b, "j", { get: Array.prototype.join });
  b.length = 2; b[0] = "u"; b[1] = "w";
  const s = [];
  Object.defineProperty(s, "push", { set: Array.prototype.push, get: Array.prototype.join });
  s.push = "q";
  const a = [1, 2, 3, 4];
  Object.defineProperty(a, 1, { configurable: false });
  let refused = false;
  try { a.length = 0; } catch (e) { refused = e instanceof TypeError; }
  Object.defineProperty(a, "length", { writable: false });
  let grown = false;
  try { a.push(9); } catch (e) { grown = e instanceof TypeError; }
  let wrote = false;
  try { o.a = 2; } catch (e) { wrote = e instanceof TypeError; }
  const desc = Object.getOwnPropertyDescriptor(o, "a");
  return order === "kevwxgp" && o.a.n === 1 && desc.enumerable && !desc.writable && !desc.configurable && wrote
    && t.x === 42 && ok1 && r.p === 7 && b.j === "u,w" && s.push === "q" && s.length === 1
    && refused && a.length === 2 && grown && a.length === 2 && Reflect.deleteProperty(t, "x") && !("x" in t);
}
