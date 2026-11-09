// bbp-authored conformance case (light-cljs-886).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 29125-29158
// (class_fields, bbp-139: the rows without `extends`, `super` or `#x`) and
// rust/tests/class_static_blocks.rs lines 27-85
// (class_static_blocks_execute_in_order, class_static_blocks_scope_and_class_context,
// class_static_blocks_restore_enclosing_context; bbp-292: the rows without
// `extends`, `super` or `#x`), with light-cljs-766's planner rows: public
// instance fields defined (CreateDataPropertyOrThrow, not [[Set]]) on the
// new object before the constructor body, in order, their computed keys
// evaluated once with the class; arrow-function fields capturing `this`;
// static fields and static blocks run in order with `this` the class; the
// inner class-name binding immutable and visible in fields and blocks.
// Left out: `extends`, `super` and `#x` (light-cljs-887 to -889) and the
// typed-array rows. Each row is a `row(...)` check; the script completes
// with the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-766).
{ class C { x = 1; static y = 2 } row(new C().x + C.y === 3); }
{ class C { static { C.z = 5 } } row(C.z === 5); }
{ class C { static { this.v = 1 } static { this.w = this.v + 1 } } row(C.w === 2); }
{ class P { get = () => '123'; static() { return 42; } } row(new P().get() + new P().static() === '12342'); }
{ class C { static x = 42; static y = C.x; static z = this.x; } row(C.y + C.z === 84); }
// class_fields (js_source.rs line 29125).
{ class S { static x = 42; static y = S.x; static z = this.x; } row(S.x === 42 && S.y === 42 && S.z === 42); }
{ class P { get = () => '123'; static() { return 42; } } row(new P().get() === '123' && new P().static() === 42); }
{ let n = 0; class C { a = ++n; b = ++n; static s = ++n; } let a = new C(); let b = new C(); row(C.s === 1 && a.a === 2 && a.b === 3 && b.a === 4 && b.b === 5); }
{ class T { a; } let t = new T(); row(Object.getOwnPropertyNames(t).join(',') === 'a' && t.a === undefined); }
{ let n = 0; let key = {toString() { n++; return 'x'; }}; class C { [key] = ++n; static [++n] = 9; } let a = new C(); let b = new C(); row(n === 4 && a.x === 3 && b.x === 4 && C[2] === 9); }
{ class C { x = eval('this'); y = eval('new.target'); } let c = new C(); row(c.x === c && c.y === undefined); }
{ class C { x = eval('arguments'); } let ok = false; try { new C(); } catch (e) { ok = e instanceof SyntaxError; } row(ok); }
{ class C { x = eval('eval("arguments")'); } let ok = false; try { new C(); } catch (e) { ok = e instanceof SyntaxError; } row(ok); }
{ class C { x = 1; constructor(x = this.x) { this.y = x; } } row(new C().y === 1); }
{ class C { static x = this.f(); static f() { return 7; } } row(C.x === 7); }
{ class C { get; set = 1; static; async
x; } let c = new C(); row(c.get === undefined && c.set === 1 && c.static === undefined && c.async === undefined && c.x === undefined); }
{ class C { ['constructor'] = 1; static ['prototype2'] = 2; } row(new C().constructor === 1 && C.prototype2 === 2); }
{ class C { x = function() {}; y = () => this; } let c = new C(); row(c.x.name === 'x' && c.y() === c && c.y.name === 'y'); }
{ class C { x = 1; constructor() { return {}; } } row(new C().x === undefined); }
{ let n = 0; class C { static x = new C().x; x = ++n; } row(C.x === 1 && n === 1); }
{ let n = 0; class C { x = eval('var n = 8; this'); y = n; } let c = new C(); row(c.x === c && c.y === 0 && n === 0); }
{ class C { x = (function(a) { return arguments[0]; })(7); y = {m() { return arguments.length; }}; } let c = new C(); row(c.x === 7 && c.y.m() === 0); }
{ function f(C = class { x = eval('1'); }) { return new C().x; } row(f() === 1); }
{ function f(C = class { static x = eval('1'); }) { return C.x; } row(f() === 1); }
{ class C { get
x() { return 7; } set
x(v) { this.y = v; } static get
z() { return 8; } static set
z(v) { this.y = v; } } let c = new C(); c.x = 9; C.z = 10; row(c.x === 7 && c.y === 9 && C.z === 8 && C.y === 10); }
{ class C { a = 1
 if = 2; b
 while = 3; } let c = new C(); row(c.a === 1 && c.if === 2 && c.b === undefined && c.while === 3); }
// Fields are defined, not assigned: an inherited setter does not run, and
// a non-extensible newTarget prototype does not stop the definition; the
// field is an enumerable, writable, configurable own property.
{ let n = 0; let p = { set x(v) { n++; } }; class C { x = 7; } function G() {} G.prototype = p; let c = Reflect.construct(C, [], G); let d = Object.getOwnPropertyDescriptor(c, 'x'); row(c.x === 7 && n === 0 && d.enumerable && d.writable && d.configurable); }
{ class C { x = 1; constructor() { this.y = Object.keys(this).join(); } } row(new C().y === 'x'); }
{ let log = []; class C { [(log.push('a'), 'a')] = log.push('x'); static [(log.push('b'), 'b')] = log.push('s'); } row(log.join() === 'a,b,s' && new C().a === 4 && log.join() === 'a,b,s,x'); }
{ class C { static f = 1 } C.f = 2; row(C.f === 2 && Object.getOwnPropertyDescriptor(C, 'f').enumerable); }
{ class C { static s = 1; static { this.t = this.s + 1 } static u = this.t + 1 } row(C.u === 3); }
{ class C { x = this.constructor.name; y = () => this.x } row(new C().y() === 'C'); }
{ class C { static x = () => this; static { this.y = (() => this)(); } } row(C.x() === C && C.y === C); }
{ class C { static self = C; } row(C.self === C); }
{ class C { static f() { return 1; } static g = C.f() + 1; } row(C.g === 2); }
{ class C { m() { return 1; } x = this.m() + 1; } row(new C().x === 2); }
{ class C { static x = (() => { try { C = 1; } catch (e) { return e instanceof TypeError; } })(); } row(C.x === true); }
// class_static_blocks_execute_in_order (class_static_blocks.rs line 27).
{ class A { static { A.y = 2; } } row(A.y === 2); }
{ let log = ''; class A { static a = (log += 'a'); static { log += 'b'; this.x = this.f(); } static c = (log += 'c'); static { log += 'd'; } static f() { return 7; } } new A(); new A(); row(log === 'abcd' && A.x === 7); }
{ let log = ''; class A { static { log += 'b'; } static [(log += 'k', 'x')] = (log += 'f'); [(log += 'j', 'm')]() {} } row(log === 'kjbf'); }
{ const C = class Inner { static { this.self = Inner; } }; row(C.self === C); }
{ let n = 0; for (let i = 0; i < 3; i++) { class C { static { n++; } } } row(n === 3); }
{ class C { static x = 1
static { this.x++; } static y = this.x; } row(C.y === 2); }
{ class C { static {} static() { return 1; } static x = 2; } row(new C().static() === 1 && C.x === 2 && Object.getOwnPropertyNames(C).join(',') === 'length,name,prototype,x'); }
{ let escaped; let log = ''; try { class C { static { escaped = this; log += 'a'; throw 17; } static x = (log += 'x'); static { log += 'b'; } } } catch (e) { log += e; } row(log === 'a17' && escaped.x === undefined); }
{ let caught = false; try { class C { static { C = 1; } } } catch (e) { caught = e instanceof TypeError; } row(caught); }
{ let caught = false; try { let C = class { static { C; } }; } catch (e) { caught = e instanceof ReferenceError; } row(caught); }
// class_static_blocks_scope_and_class_context (class_static_blocks.rs line 45).
{ let x = 9; class C { static { this.before = x; var x = 1; this.f = () => ++x; } static { var x = 3; this.other = () => x; } static y = x; } row(C.before === undefined && C.f() === 2 && C.f() === 3 && C.other() === 3 && C.y === 9 && x === 9); }
{ class C { static { this.x = f(); function f() { return 7; } } static { this.y = typeof f; } } row(C.x === 7 && C.y === 'undefined'); }
{ class C { static { this.a = this; this.b = () => this; this.c = new.target; this.d = eval('new.target'); } } row(C.a === C && C.b() === C && C.c === undefined && C.d === undefined); }
{ class C { static { this.x = eval('var v = 7; v'); this.y = typeof v; } } row(C.x === 7 && C.y === 'undefined'); }
{ class C { static { this.x = (function(x) { return arguments[0]; })(7); this.y = (() => { return 8; })(); } } row(C.x === 7 && C.y === 8); }
{ function f() { class C { static {} } return arguments[0]; } row(f(7) === 7); }
{ class C { static { this.x = {arguments: 7}.arguments; } } row(C.x === 7); }
{ class C { static {} if = 7; } row(new C().if === 7); }
{ class C { static { class Inner { static { this.x = 7; } } this.x = Inner.x; } } row(C.x === 7); }
{ class C { static { this.f = async () => await 7; this.g = function*() { yield 8; }; } } row(C.g().next().value === 8); }
{ class C { static { let caught = false; try { eval('arguments'); } catch (e) { caught = e instanceof SyntaxError; } this.ok = caught; } } row(C.ok); }
{ class C { static { let caught = false; try { eval('eval("arguments")'); } catch (e) { caught = e instanceof SyntaxError; } this.ok = caught; } } row(C.ok); }
{ class C { static { for (let i = 0; i < 2; i++) { if (i === 0) continue; break; } this.ok = true; } } row(C.ok); }
// class_static_blocks_restore_enclosing_context (class_static_blocks.rs line 69).
{ function* f() { class C { static {} } yield 7; } row(f().next().value === 7); }
{ let n = 0; outer: while (n < 2) { n++; class C { static {} } continue outer; } row(n === 2); }
{ let n = 0; switch (7) { case 7: class C { static {} } n++; break; default: n = 9; } row(n === 1); }
// Static blocks and field initializers are not constructors.
{ let f; class C { static { f = () => 1; } static g = function () {}; } row(name(() => new f()) === 'TypeError' && typeof new C.g() === 'object'); }
failed === 0 ? rows : -failed;
