// A global accessor (light-cljs-824): serial_tests.goose installs `probe`
// on the global object as a configurable accessor whose getter and setter
// are probeGet and probeSet below (never called by the script itself), as
// Object.defineProperty would, which the realm does not implement yet.
// Reading `probe` runs the getter as a frame, assigning it runs the setter
// in a Setter native frame, an exception from either reaches the script's
// catch, and the setter's return value is not the assignment's. Completes
// with true; without the accessor it fails.
var enabled;
if (enabled) {
  globalThis.unused = [function probeGet() { if (this.saved === 13) { throw "get"; } return this.saved === undefined ? 1 : this.saved; },
                       function probeSet(v) { if (v === 13) { this.saved = 13; throw "set"; } this.saved = v * 2; return 99; }];
}
let before = probe;
var assigned = (probe = 5);
var after = probe;
function viaFunction() { probe = 6; return probe; }
var inner = viaFunction();
var caught = "";
try { probe = 13; } catch (e) { caught = caught + e; }
try { probe; } catch (e) { caught = caught + e; }
before === 1 && assigned === 5 && after === 10 && inner === 12 && caught === "setget" && globalThis.saved === 13;
