// bbp-authored conformance case (light-cljs-932).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 23041-23080 (regexp_flags_errors_and_internal_state, its rows without matching), 23085-23113 (regexp_generic_flags_reads_all_eight_in_order), 23228-23244 (regexp_object_tag_and_brand) and 23132-23140 (regexp_coercions_wait_yield_collect_and_migrate, its toString and flags rows without waits); QuickJS js_regexp_get_flag, js_regexp_get_flags, js_regexp_get_source and js_regexp_toString:
// RegExp.prototype's flag getters (undefined on RegExp.prototype itself, a
// TypeError on another object), `source` (EscapeRegExpPattern), the
// generic `flags` getter and toString, and Object.prototype.toString's
// RegExp tag by the internal slot. Each row is a `row(...)` check; the
// script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-770).
row([/a/dgimsy.flags, new RegExp('a', 'ysmigd').flags, /a/v.flags, String(/a\/b/g), new RegExp('/').source, RegExp.prototype.source, String(RegExp.prototype), typeof RegExp.prototype.global, Object.prototype.toString.call(/a/), RegExp.length, RegExp.name, String(new RegExp(''))].join('|') === 'dgimsy|dgimsy|v|/a\\/b/g|\\/|(?:)|/(?:)/|undefined|[object RegExp]|2|RegExp|/(?:)/');
{ let get = Object.getOwnPropertyDescriptor(RegExp.prototype, 'global').get; row(name(() => get.call({})) === "TypeError"); }
row(name(() => RegExp.prototype.toString.call(1)) === "TypeError");
// regexp_generic_flags_reads_all_eight_in_order (line 23085).
{
  let reads = [];
  let receiver = {
    get hasIndices() { reads.push('hasIndices'); return true; },
    get global() { reads.push('global'); return true; },
    get ignoreCase() { reads.push('ignoreCase'); return true; },
    get multiline() { reads.push('multiline'); return true; },
    get dotAll() { reads.push('dotAll'); return true; },
    get unicode() { reads.push('unicode'); return true; },
    get unicodeSets() { reads.push('unicodeSets'); return true; },
    get sticky() { reads.push('sticky'); return true; }
  };
  let get = Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get;
  row(get.call(receiver) === 'dgimsuvy' && reads.join(',') === 'hasIndices,global,ignoreCase,multiline,dotAll,unicode,unicodeSets,sticky');
}
// regexp_flags_errors_and_internal_state (line 23041).
row(RegExp.prototype.toString.call({source: 'S', flags: 'F'}) === '/S/F');
row(RegExp.prototype.toString.call({source: 'x', flags: 'y'}) === '/x/y');
row(RegExp.prototype.source === '(?:)' && RegExp.prototype.flags === '' && RegExp.prototype.global === undefined && RegExp.prototype.toString() === '/(?:)/');
{ let r = /./msgy; row(r.global && r.multiline && r.dotAll && r.sticky && !r.ignoreCase && !r.unicode && !r.unicodeSets && !r.hasIndices); }
{ let get = Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get; row(get.call({global: 1, ignoreCase: 1, multiline: 1, dotAll: 1, sticky: 1}) === 'gimsy'); }
// (QuickJS leaves U+2028 and U+2029 unescaped and escapes a backslash
// before a line terminator differently: those rows are left out.)
row(new RegExp('a/b\n\r').source === 'a\\/b\\n\\r');
row(new RegExp('[/]').source === '[/]' && /a\/b/.source === 'a\\/b');
row(new RegExp('\n').source === '\\n' && new RegExp('\\/').source === '\\/' && new RegExp('[\\]/]').source === '[\\]/]');
row(/\0a/.toString() === '/\\0a/');
row(/a/.source === 'a' && new RegExp('').source === '(?:)' && RegExp('a').toString() === '/a/' && String(/[/]/) === '/[/]/');
row(/a/dgimsy.hasIndices && /a/u.unicode && !(/a/u.unicodeSets) && /a/v.unicodeSets && !(/a/v.unicode) && new RegExp('a', 'd').hasIndices);
// Every flag getter on RegExp.prototype is undefined, on another object a TypeError.
{
  let names = ['hasIndices', 'global', 'ignoreCase', 'multiline', 'dotAll', 'unicode', 'unicodeSets', 'sticky'];
  let ok = true;
  for (let i = 0; i < names.length; i++) {
    let d = Object.getOwnPropertyDescriptor(RegExp.prototype, names[i]);
    ok = ok && typeof d.get === 'function' && d.set === undefined && !d.enumerable && d.configurable && d.get.name === 'get ' + names[i] && d.get.length === 0;
    ok = ok && RegExp.prototype[names[i]] === undefined && name(() => d.get.call(Object.create(RegExp.prototype))) === "TypeError" && name(() => d.get.call(1)) === "TypeError";
  }
  row(ok);
}
{ let d = Object.getOwnPropertyDescriptor(RegExp.prototype, 'source'); row(typeof d.get === 'function' && d.set === undefined && !d.enumerable && d.configurable && d.get.name === 'get source' && name(() => d.get.call({})) === "TypeError"); }
{ let d = Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags'); row(typeof d.get === 'function' && d.set === undefined && !d.enumerable && d.configurable && d.get.name === 'get flags' && name(() => d.get.call(1)) === "TypeError" && d.get.call({}) === ''); }
{ let d = Object.getOwnPropertyDescriptor(RegExp.prototype, 'toString'); row(d.value === RegExp.prototype.toString && d.writable && !d.enumerable && d.configurable && d.value.name === 'toString' && d.value.length === 0); }
row('exec' in RegExp.prototype && 'test' in RegExp.prototype && Symbol.replace in RegExp.prototype && Symbol.split in RegExp.prototype && Symbol.match in RegExp.prototype);
// Getters see a subclass instance's slot; toString and flags read generically.
{ class R extends RegExp {} let r = new R('x', 'gi'); row(r.flags === 'gi' && r.global && String(r) === '/x/gi'); }
{ let r = /a/g; Object.defineProperty(r, 'source', {value: 'Z'}); Object.defineProperty(r, 'flags', {value: 'Q'}); row(String(r) === '/Z/Q' && RegExp.prototype.toString.call(r) === '/Z/Q'); }
{ let r = /a/; Object.defineProperty(r, 'global', {value: true}); row(r.flags === 'g'); }
{ let log = ''; let s = RegExp.prototype.toString.call({get source() { log += 's'; return {toString() { log += 'S'; return 'a'; }}; }, get flags() { log += 'f'; return {toString() { log += 'F'; return 'g'; }}; }}); row(s === '/a/g' && log === 'sSfF'); }
row(RegExp.prototype.toString.call({}) === '/undefined/undefined');
row(Array.prototype.toString.call({join: RegExp.prototype.toString, source: 'S', flags: 'F'}) === '/S/F' && [/a/g, /b/].join() === '/a/g,/b/');
row(name(() => RegExp.prototype.toString.call({source: Symbol()})) === "TypeError");
{ let caught; try { RegExp.prototype.toString.call({get source() { throw 5; }}); } catch (e) { caught = e; } row(caught === 5); }
{ let get = Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get; let caught; try { get.call({get hasIndices() { throw 6; }}); } catch (e) { caught = e; } row(caught === 6); }
// regexp_object_tag_and_brand (line 23228).
{
  let literal = /a/; let constructed = new RegExp('a');
  let tag = Object.prototype.toString;
  let before = tag.call(literal) === '[object RegExp]' && tag.call(constructed) === '[object RegExp]';
  Object.setPrototypeOf(literal, null);
  row(before && tag.call(literal) === '[object RegExp]' && tag.call(RegExp.prototype) === '[object Object]' && tag.call(Object.create(RegExp.prototype)) === '[object Object]');
}
row(typeof /a/ === 'object' && /a/ instanceof RegExp && Object.getPrototypeOf(/a/) === RegExp.prototype && /a/.constructor === RegExp);
rows = failed === 0 ? rows : -failed;
rows;
