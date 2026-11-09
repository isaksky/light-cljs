// An array's `length` defined with an object value (light-cljs-898):
// ArraySetLength's two conversions, each a valueOf that runs a loop, in a
// Convert frame above the Descriptor frame of Object.defineProperty,
// Reflect.defineProperty and Object.defineProperties, and above a
// trapless proxy's [[DefineOwnProperty]] frame, reached from
// Reflect.defineProperty and from Reflect.set with the proxy as receiver.
// serial_tests.goose stops and resumes those frames at every budget cut
// and limit, and collects inside the job at every cut while the two copies
// of the value wait on the operand stack. The Pants engine converts an
// object length only in Reflect's methods, so Goose alone runs this; the
// conformance slice bbp/array_length_definition_conversion holds QuickJS's
// expectations.
function arrayLengthDefinition() {
  "use strict";
  let order = "";
  const spin = (tag) => { let i = 0; while (i < 3) i++; order += tag; };
  const length = (tag, n) => ({ valueOf() { spin(tag); return n; } });
  const a = [1, 2, 3, 4];
  Object.defineProperty(a, "length", { value: length("a", 3) });
  const b = [1, 2, 3, 4];
  const okb = Reflect.defineProperty(b, "length", { value: length("b", 2), writable: false });
  const c = [1, 2, 3, 4];
  Object.defineProperties(c, { x: { value: 5 }, length: { value: length("c", 1) }, y: { value: 6 } });
  const d = [1, 2, 3, 4], h = {};
  const okd = Reflect.defineProperty(new Proxy(d, h), "length", { value: { valueOf() { spin("d"); h.defineProperty = () => false; return 2; } } });
  const e = [1, 2, 3, 4];
  const oke = Reflect.set({}, "length", length("e", 0), new Proxy(e, {}));
  let range = false;
  try { Reflect.defineProperty([1], "length", { value: length("r", -1) }); } catch (x) { range = x instanceof RangeError; }
  return order === "aabbccddeerr" && a.length === 3 && okb && b.length === 2 && !Reflect.defineProperty(b, "length", { value: 1 })
    && c.length === 1 && c.x === 5 && c.y === 6 && okd && d.length === 2 && oke && e.length === 0 && range;
}
