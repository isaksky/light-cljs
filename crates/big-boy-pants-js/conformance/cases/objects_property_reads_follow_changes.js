// bbp-authored conformance case (light-cljs-880).
// Derives from: crates/big-boy-pants-js/rust/tests/property_caches.rs lines 17-68:
// named_cache_tracks_shapes_deletion_and_order (line 17),
// named_cache_tracks_prototypes_and_method_replacement (27),
// function_member_reads_follow_current_records_and_prototypes (34) and the
// script of function_member_reads_rebind_after_every_instruction_collection
// (47): reads and writes through one function keep seeing the current
// properties after shape changes, deletion, growth past 80 keys, prototype
// property and method replacement, and changes to a function's own
// properties. Goose has no inline cache; the rows pin the semantics.
// `Object.keys` is for-in and `Object.create` a `__proto__` literal here; the
// prototype row replaces methods instead of calling Object.setPrototypeOf
// (light-cljs-883). Left out: the class row (light-cljs-886, -887), the
// `Symbol()` key (light-cljs-885), and named_cache_reads_current_descriptors_
// and_setters (85), which needs Object.defineProperty and freeze
// (light-cljs-881, -882). Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// named_cache_tracks_shapes_deletion_and_order (line 17).
{
  function read(o) { return o.x; } function write(o,v) { o.x = v; } let a = {x:1}, b = {y:2,x:3}; let ok = read(a) === 1 && read(b) === 3; a.y = 4; write(a,5); ok = ok && read(a) === 5; delete b.y; delete b.x; b.z = 6; b.x = 7; write(b,8); let keys = []; for (let k in b) keys.push(k);
  row(ok && read(b) === 8 && keys.join(',') === 'z,x');
}
{
  function read(o) { return o.x; } let a = {x:1}; let ok = read(a) === 1; for (let i=0;i<80;i++) a['k'+i]=i; a.x=9; let n = 0; for (let k in a) n++;
  row(ok && read(a) === 9 && n === 81);
}
// named_cache_tracks_prototypes_and_method_replacement (line 27): the rows before Object.setPrototypeOf.
{
  function read(o) { return o.x; } function call(o) { return o.m(); } let p = {x:1,m(){return this.x;}}, o = {__proto__: p}; let ok = read(o) === 1 && call(o) === 1; p.x=3; ok = ok && read(o) === 3; p.m=function(){return 99;}; ok = ok && call(o) === 99; o.x=7; o.m=function(){return this.x+10;};
  row(ok && read(o) === 7 && call(o) === 17 && p.x === 3);
}
// function_member_reads_follow_current_records_and_prototypes (line 34): its rows without defineProperty, classes or Symbol().
{
  function read(f) { return f.x; } function f(a,b) {} let ok=read(f)===undefined && typeof f.call==='function'; f.x=1; ok=ok && read(f)===1 && f.length===2 && f.name==='f'; delete f.x; f.y=9;
  row(ok && read(f)===undefined && f.y===9 && f.prototype.constructor===f);
}
{
  function f() {} const g=f.bind(null); g.x=8; function read(f){return f.x;}
  row(read(g)===8 && g.name==='bound f' && g.length===0);
}
// function_member_reads_rebind_after_every_instruction_collection (line 50): its script.
{
  function run() { function f(a) { return this.x+a; } f.x=4; function read(g){return g.x;} const o={x:3}; return read(f)===4 && f.call(o,2)===5 && f.prototype.constructor===f; }
  row(run());
}
failed === 0 ? rows : -failed;
