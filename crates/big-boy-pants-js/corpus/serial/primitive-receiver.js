// Accessors on realm prototypes seen from primitive bases (light-cljs-833):
// serial_tests.goose installs `probe` on Number.prototype and on
// String.prototype as a configurable accessor whose getter and setter are
// probeGet and probeSet below (never called by the script itself), as
// Object.defineProperty would, which the realm does not implement yet. In
// this strict script both run with the primitive itself as receiver, no
// wrapper; the setter's return value is not the assignment's, and a
// compound assignment reads through the getter, then writes through the
// setter. Completes with true; without the accessors it fails.
var enabled;
if (enabled) {
  globalThis.unused = [function probeGet() { return typeof this + ":" + this; },
                       function probeSet(v) { seen = seen + typeof this + ":" + this + "=" + v + ";"; return 99; }];
}
var seen = "";
var got = (7).probe + "," + "ab".probe;
var assigned = ((7).probe = 1);
"ab".probe = 2;
var n = 3;
n.probe += "!";
var k = "probe";
true[k] === undefined && got === "number:7,string:ab" && assigned === 1 && seen === "number:7=1;string:ab=2;number:3=number:3!;";
