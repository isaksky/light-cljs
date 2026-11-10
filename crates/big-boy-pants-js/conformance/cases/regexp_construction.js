// bbp-authored conformance case (light-cljs-932).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 23015-23037 (regexp_constructor_copies_pattern_and_resets_last_index), 23041-23080 (regexp_flags_errors_and_internal_state, its rows without matching), 22943-22955 (regexp_property_literals_execute_when_evaluated, without `.test`) and 23132-23140 (regexp_coercions_wait_yield_collect_and_migrate, its constructor rows without waits); rust/tests/builtin_subclasses.rs lines 78-84 (date_regexp_and_wrapper_subclasses_keep_internal_slots, its RegExp row without `test`) and 552-600 (regexp_new_target_protocol_order_and_waits, without waits):
// RegExp(pattern, flags) and new RegExp: IsRegExp's @@match read, the
// identity rule (`constructor`), a RegExp pattern's source and flags, an
// IsRegExp pattern's `source` and `flags` reads, newTarget's `prototype`
// read after them and before the ToString conversions, flag and pattern
// SyntaxErrors, lastIndex, subclasses, and literals evaluated as new
// objects. Left out: matching (exec, test, the symbol methods; later
// children of light-cljs-770) and the Pants engine's step-limit rows,
// which QuickJS does not share. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-770).
{ let re = /a/g; re.lastIndex = 3; row([RegExp(re) === re, new RegExp(re) === re, new RegExp(re, 'i').flags, new RegExp(re).lastIndex, new RegExp(re).flags, RegExp(re, undefined) === re].join() === "true,false,i,0,g,true"); }
row(name(() => new RegExp('[')) === "SyntaxError");
row(name(() => new RegExp('a', 'gg')) === "SyntaxError");
row(name(() => new RegExp('a', 'z')) === "SyntaxError");
row(name(() => new RegExp('a', 'uv')) === "SyntaxError");
row(name(() => RegExp('(')) === "SyntaxError");
{ let p = {[Symbol.match]: true, source: 'b', flags: 'g', constructor: RegExp}; row([RegExp(p) === p, new RegExp(p).source, new RegExp(p).flags].join() === "true,b,g"); }
{
  let log = [];
  let p = { get [Symbol.match]() { log.push('match'); return true; }, get source() { log.push('source'); return 'a'; }, get flags() { log.push('flags'); return 'g'; }, get constructor() { log.push('ctor'); return RegExp; } };
  let r = new RegExp(p);
  row(log.join() === "match,source,flags" && r.source === 'a' && r.flags === 'g');
  log = [];
  row(RegExp(p) === p && log.join() === "match,ctor");
}
{ class R extends RegExp {} let r = new R('a', 'g'); row(Object.getPrototypeOf(r) === R.prototype && r instanceof R && r instanceof RegExp && r.flags === 'g' && r.source === 'a' && r.lastIndex === 0); }
{ function f() { return /a/g; } let x = f(), y = f(); x.lastIndex = 5; row(x !== y && y.lastIndex === 0 && x.lastIndex === 5 && x.source === y.source); }
// regexp_constructor_copies_pattern_and_resets_last_index (line 23015).
{ let r = /b/g; r.lastIndex = 7; let copy = new RegExp(r); row(copy !== r && copy.source === 'b' && copy.source === r.source && copy.flags === 'g' && copy.lastIndex === 0 && r.lastIndex === 7); }
{ let r = /b/g; r.lastIndex = 7; let copy = RegExp(r, 'y'); row(copy !== r && copy.source === 'b' && copy.flags === 'y' && copy.lastIndex === 0 && r.lastIndex === 7); }
{ let r = /b/g; let copy = new RegExp(r, 'y'); row(copy.flags === 'y' && copy.source === 'b'); }
// regexp_flags_errors_and_internal_state (line 23041).
{ let r = /a/i; row(r.ignoreCase && r.flags === 'i'); }
{ let r = RegExp('a', 'i'); row(r.ignoreCase && r.flags === 'i'); }
{ let r = /a/g; row(RegExp(r) === r && new RegExp(r) !== r && new RegExp(r).flags === 'g' && RegExp(r, 'm').flags === 'm'); }
{ let r = /a/g; let d = Object.getOwnPropertyDescriptor(r, 'lastIndex'); row(d.value === 0 && d.writable && !d.enumerable && !d.configurable && r instanceof RegExp && r.constructor === RegExp); }
row(Object.getOwnPropertyNames(/a/).join() === "lastIndex" && Object.getOwnPropertyNames(new RegExp('a', 'g')).join() === "lastIndex" && Object.keys(/a/).length === 0);
{ let r = /a/; r.lastIndex = 'x'; row(r.lastIndex === 'x'); }
{ let r = /a/; row(Reflect.deleteProperty(r, 'lastIndex') === false && r.lastIndex === 0 && Reflect.defineProperty(r, 'lastIndex', {value: 4}) && r.lastIndex === 4); }
// The constructor object.
row(typeof RegExp === 'function' && RegExp.length === 2 && RegExp.name === 'RegExp' && RegExp.prototype.constructor === RegExp);
{ let d = Object.getOwnPropertyDescriptor(RegExp, 'prototype'); row(d.value === RegExp.prototype && !d.writable && !d.enumerable && !d.configurable); }
{ let d = Object.getOwnPropertyDescriptor(RegExp, Symbol.species); row(RegExp[Symbol.species] === RegExp && typeof d.get === 'function' && d.set === undefined && !d.enumerable && d.configurable && d.get.name === 'get [Symbol.species]'); }
row(Object.getPrototypeOf(RegExp.prototype) === Object.prototype && !(RegExp.prototype instanceof RegExp));
row(new RegExp().source === '(?:)' && new RegExp(undefined, undefined).flags === '' && String(RegExp()) === '/(?:)/' && new RegExp(null).source === 'null' && new RegExp(12).source === '12');
// Conversions: the pattern, then the flags; a Symbol is a TypeError.
{ let log = ''; let r = new RegExp({toString() { log += 'p'; return 'x'; }}, {toString() { log += 'f'; return 'g'; }}); row(log === 'pf' && r.source === 'x' && r.flags === 'g'); }
row(name(() => new RegExp(Symbol())) === "TypeError" && name(() => new RegExp('a', Symbol())) === "TypeError");
{ let caught; try { RegExp({toString() { throw 42; }}); } catch (e) { caught = e; } row(caught === 42); }
{ let caught; try { new RegExp('a', {toString() { throw 43; }}); } catch (e) { caught = e; } row(caught === 43); }
row(new RegExp({toString() { return 'a'; }}, 'g').flags === 'g' && new RegExp('a', {toString() { return 'g'; }}).flags === 'g');
// A RegExp pattern with @@match false is still copied as a RegExp.
{ let r = /a/g; Object.defineProperty(r, Symbol.match, {value: false}); let copy = RegExp(r); row(copy !== r && copy.source === 'a' && copy.flags === 'g'); }
// regexp_coercions_wait_yield_collect_and_migrate's constructor rows (line 23132), without waits.
{ let log = ''; let x = {get [Symbol.match]() { log += 'm'; return true; }, get source() { log += 's'; return 'a'; }, get flags() { log += 'f'; return 'g'; }}; let r = RegExp(x); row(log === 'msf' && r.source === 'a' && r.flags === 'g'); }
{ let r = /a/; let n = 0; Object.defineProperty(r, 'constructor', {get() { n++; return RegExp; }}); row(RegExp(r) === r && n === 1); }
// regexp_new_target_protocol_order_and_waits (builtin_subclasses.rs line 552).
// QuickJS reads newTarget's `prototype` after the ToString conversions, so
// its rows that convert an object pattern or flags after that read are
// left out.
{ let log = ''; let B = RegExp.bind(null); Object.defineProperty(B, 'prototype', {get() { log += 'p'; return RegExp.prototype; }}); let pattern = {get [Symbol.match]() { log += 'm'; return true; }, get source() { log += 's'; return 'a'; }, get flags() { log += 'f'; return 'g'; }}; Reflect.construct(RegExp, [pattern], B); row(log === 'msfp'); }
{ let log = ''; let B = RegExp.bind(null); Object.defineProperty(B, 'prototype', {get() { log += 'p'; return null; }}); let pattern = {get [Symbol.match]() { log += 'm'; return true; }, get source() { log += 's'; return 'a'; }, get flags() { throw 99; }}; let r = Reflect.construct(RegExp, [pattern, {toString() { log += 'y'; return 'i'; }}], B); row(log === 'mspy' && Object.getPrototypeOf(r) === RegExp.prototype && r.flags === 'i'); }
{ let log = ''; let B = RegExp.bind(null); Object.defineProperty(B, 'prototype', {get() { log += 'p'; return RegExp.prototype; }}); let pattern = /a/g; Object.defineProperty(pattern, Symbol.match, {get() { log += 'm'; return false; }}); Object.defineProperty(pattern, 'source', {get() { throw 99; }}); Object.defineProperty(pattern, 'flags', {get() { throw 99; }}); let r = Reflect.construct(RegExp, [pattern], B); row(log === 'mp' && r !== pattern && r.source === 'a' && r.flags === 'g'); }
{ let log = ''; let B = RegExp.bind(null); Object.defineProperty(B, 'prototype', {get() { log += 'p'; throw 17; }}); let caught; try { Reflect.construct(RegExp, ['['], B); } catch (e) { caught = e; } row(caught === 17 && log === 'p'); }
{ let log = ''; let pattern = new Proxy({get [Symbol.match]() { log += 'm'; return true; }, get constructor() { log += 'c'; return RegExp; }, get source() { throw 99; }}, {}); row(RegExp(pattern) === pattern && log === 'mc'); }
{ let log = ''; let pattern = {get [Symbol.match]() { log += 'm'; return true; }, get constructor() { log += 'c'; return null; }, get source() { log += 's'; return 'a'; }, get flags() { log += 'f'; return 'g'; }}; let r = RegExp(pattern); row(log === 'mcsf' && r.source === 'a' && r.flags === 'g'); }
// regexp_property_literals_execute_when_evaluated (line 22943): a literal
// is compiled with its image and only fails when evaluated.
row((false && /\p{Script=Greek}/u) === false);
row((false && /\p{Script_Extensions=Grek}/u) === false);
row((false && /\p{RGI_Emoji}/v) === false);
row(/\p{Script=Greek}/u.source === '\\p{Script=Greek}' && /[\p{L}--\p{Lu}]/v.unicodeSets);
row((false && /(?<=a)b/) === false);
rows = failed === 0 ? rows : -failed;
rows;
