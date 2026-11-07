// A Symbol.toPrimitive getter (light-cljs-828): serial_tests.goose installs
// `hooked` on the global object, an ordinary object whose Symbol.toPrimitive
// is an accessor with hookGet below as its getter (never called by the
// script itself), as Object.defineProperty or a computed accessor name
// would, which Goose does not run yet. Each conversion runs the getter once
// with the object as receiver; a function result is called with the hint,
// null falls back to valueOf, another value is a TypeError, and the
// getter's exception propagates. Completes with true; without the accessor
// it fails.
var enabled;
if (enabled) {
  globalThis.unused = [function hookGet() {
    log += this === hooked ? "g" : "?";
    var m = this.mode;
    if (m === 0) { return function (h) { log += h; return 1; }; }
    if (m === 1) { return null; }
    if (m === 2) { return 5; }
    throw "thrown";
  }];
}
var log = "";
hooked.valueOf = function () { log += "v"; return 2; };
hooked.mode = 0;
var a = hooked + 1;
var b = `${hooked}`;
hooked.mode = 1;
var c = hooked * 3;
hooked.mode = 2;
var d = "";
try { +hooked; } catch (e) { d = e.name; }
hooked.mode = 3;
var f = "";
try { hooked - 1; } catch (e) { f = e; }
a === 2 && b === "1" && c === 6 && d === "TypeError" && f === "thrown" && log === "gdefaultgstringgvgg";
