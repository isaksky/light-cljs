// bbp-authored conformance case (light-cljs-882), run as a sloppy script.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 17343-17600:
// object_extensibility (line 17343, bbp-122), its sloppy rows: in sloppy
// code a write or delete that a non-extensible, sealed or frozen object
// refuses does nothing (objects_integrity_mapped_arguments has the
// mapped arguments objects). Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
{ var a = {x: 1}; Object.preventExtensions(a); a.y = 2; a[0] = 1; row(a.y === undefined && a[0] === undefined && a.x === 1); }
{ var f = Object.freeze({x: 1}); f.x = 2; f.y = 3; row(f.x === 1 && f.y === undefined && delete f.x === false && f.x === 1); }
{ var q = Object.freeze([1, 2]); q[0] = 9; q[5] = 9; q.length = 0; row(q[0] === 1 && q.length === 2 && q[5] === undefined && delete q[0] === false); }
{ var s = Object.seal({a: 1}); s.a = 2; s.b = 3; row(delete s.a === false && s.a === 2 && s.b === undefined); }
{ var p = Object.preventExtensions([1]); p[1] = 2; row(p.length === 1 && p[1] === undefined); }
{ var t = (function (x) { return x; })`a${0}`; t[0] = 'z'; t.extra = 1; row(t[0] === 'a' && t.extra === undefined && delete t[0] === false); }
failed === 0 ? rows : -failed;
