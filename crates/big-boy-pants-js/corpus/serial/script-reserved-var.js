// A var declaration over a global property serial_tests.goose installs
// (light-cljs-824): non-configurable, non-writable `reserved` = 1. The
// declaration keeps the property; a strict assignment to it is a TypeError
// and changes nothing. Completes with true only with that property.
var reserved;
var before = reserved;
var caught = "none";
try { reserved = 5; } catch (e) { caught = e.name; }
before === 1 && caught === "TypeError" && reserved === 1;
