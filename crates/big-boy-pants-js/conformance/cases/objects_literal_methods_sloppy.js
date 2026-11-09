// bbp-authored conformance case (light-cljs-880), run as a sloppy script.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 14484-14571, 14597-14711:
// object_literal_methods_in_sloppy_scripts (line 14484, bbp-116), the
// enclosing-function row of object_literal_methods_inherit_strictness (14525)
// and the sloppy rows of method_calls_bind_this (14597, bbp-126): sloppy
// methods see the global object for an undefined receiver and accept legacy
// octal literals; a method-local or enclosing "use strict" makes them strict.
// Left out: the compiled-function strictness flags (Pants bytecode API).
// Each row is a `row(...)` check; the script completes with the number of
// rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// object_literal_methods_in_sloppy_scripts (line 14484, bbp-116).
{
  let f = ({m() { return this; }}).m;
  row(f() === globalThis);
}
row(({m() { return this; }}).m.call(undefined) === globalThis);
row(({m() { return 010; }}).m() === 8);
row(({m(a = 010) { return a; }}).m() === 8);
{
  let o = {m() { return this; }};
  row(o.m() === o && o.m.call(o) === o);
}
row(({m() { 'use strict'; return this; }}).m.call(undefined) === undefined);
row(({m() { 'use strict'; return this; }}).m.call(42) === 42);
{
  let o = {m() { 'use strict'; }, n(a = 010) { return a; }};
  row(o.n() === 8);
}
// object_literal_methods_inherit_strictness (line 14525): a strict enclosing function's methods are strict.
{
  function f() { 'use strict'; return {m() { return this; }}; }
  row((f().m.call(undefined)) === undefined);
}
// method_calls_bind_this (line 14597, bbp-126): sloppy receivers.
{
  let p = {me: function () { return this; }};
  row(p.me() === p);
}
{
  function g() { return this; }
  row(g() === globalThis);
}
{
  function g() { 'use strict'; return this; }
  row((g()) === undefined);
}
failed === 0 ? rows : -failed;
