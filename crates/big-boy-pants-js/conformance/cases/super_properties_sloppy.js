// bbp-authored conformance case (light-cljs-888), a sloppy script.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 852
// and 858 (super_property_access's sloppy rows, bbp-138) and 950-955
// (super_property_access_sloppy_immutable_receivers, its non-extensible
// receiver row), with light-cljs-766's planner row for `delete super.a` in
// sloppy code: a refused super assignment does nothing in sloppy code (an
// inherited read-only property, a non-extensible or primitive receiver)
// and the assignment's value is still its result. Left out: the Pants
// engine's TypeError for a realm object as receiver (Object, its
// prototype), which QuickJS does not raise, and js_source.rs line 864 (a
// super assignment in a class's computed key in sloppy code), where QuickJS
// and the Pants engine disagree. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
// The planner's row (light-cljs-766).
{ var a = { f() { delete super.a; } }; var r; try { a.f(); r = 'none' } catch (e) { r = e instanceof ReferenceError } row(r === true); }
// super_property_access's sloppy rows.
{ let k={[Symbol.toPrimitive](){return 'x';},toString(){return 'y';}}; let p={get x(){return this.a;},set x(v){this.a=v;}}; let o={__proto__:p,m(){super[k]=7;return super[k];}}; row(o.m()===7 && o.a===7); }
{ let p = {}; Object.defineProperty(p, 'x', {value: 1}); let o = {__proto__: p, m() { super.x = 2; return super.x; }}; row(o.m() === 1); }
// Refused writes with a separate receiver.
{ let o = {m() { return super.x = 1; }}; let receiver = {}; Object.preventExtensions(receiver); row(o.m.call(receiver) === 1 && receiver.x === undefined); }
{ let p = {x: 1}; let o = {__proto__: p, get x() { return 9 }, m() { return super.x = 5 }}; row(o.m() === 5 && o.x === 9 && p.x === 1); }
{ let o = {__proto__: {}, m() { return super.x = 5 }}; Object.freeze(o); row(o.m() === 5 && o.x === undefined); }
// A null base is a TypeError in sloppy code too, before the key converts.
{ let n = 0; let o = {__proto__: null, m() { super[{toString() { n++; throw 7 }}] = 1; }}; let r; try { o.m(); r = 'none' } catch (e) { r = e instanceof TypeError } row(r === true && n === 0); }
failed === 0 ? rows : -failed;
