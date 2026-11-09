// bbp-authored conformance case (light-cljs-881).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 7166-18600:
// define_property_data_descriptors (line 16421, bbp-119),
// define_property_validation_and_targets (line 16552, bbp-119, bbp-154),
// define_property_array_data_attributes (line 16780, bbp-120),
// accessor_descriptors_and_integrity (line 7166, bbp-125) and
// accessors_in_native_operations, their descriptor rows,
// array_length_define_property (line 18486, bbp-154),
// array_length_assignment (line 18392) and delete_property_references
// (line 17876, bbp-124), their non-configurable rows, and
// function_own_properties (line 15720), its definition rows; rust/tests/
// object_statics.rs object_statics_define_properties_conversion_before_writes
// (line 118); and the planner's rows (light-cljs-766). Left out: rows using
// Object.create, Object.keys, Object.freeze or Object.preventExtensions
// (light-cljs-882, -883), Symbol() and Proxy traps outside for-in
// (light-cljs-889, -891), definitions on promises (which the Pants engine
// refuses), writes into the realm (which QuickJS allows and the
// Pants engine and Goose refuse), `__rt.vector`, a length assignment of a
// non-number (a named gap of the core built-ins area, light-cljs-767), and
// that test's three rows where every descriptor must be read before the
// first definition: QuickJS defines each property as it reads it, so the
// oracles disagree there (Goose follows the specification and the Pants
// engine).
// Errors are compared by name. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
function desc(o, k) { let d = Object.getOwnPropertyDescriptor(o, k); return d === undefined ? "absent" : '' + d.value + ':' + d.writable + d.enumerable + d.configurable; }
// The planner's rows (light-cljs-766).
{ let a = {}; Object.defineProperty(a, 'y', {value: 3, writable: true, configurable: true, enumerable: true}); row(a.y === 3); }
{ let o = {}; Object.defineProperty(o, 'k', {value: 1}); let r; try { o.k = 2; r = 'none'; } catch (e) { r = (e instanceof TypeError) + ':' + o.k; } row(r === "true:1"); }
{ let o = {}; Object.defineProperty(o, 'k', {value: 1}); row(name(() => { delete o.k; }) === "TypeError" && o.k === 1); }
{ let o = {}; Object.defineProperty(o, 'k', {value: 1, enumerable: true}); row(name(() => Object.defineProperty(o, 'k', {value: 2})) === "TypeError" && o.k === 1); }
{ let log = ''; let d = { get enumerable() { log += 'e'; return true; }, get value() { log += 'v'; return 1; } }; let o = {}; Object.defineProperty(o, 'k', d); row(log + o.k === "ev1"); }
{ let a = [1, 2, 3]; Object.defineProperty(a, 'length', {value: 1}); row(a.length === 1 && a[1] === undefined); }
{ let o = {}; Object.defineProperty(o, 'x', { get: Array.prototype.join }); row(typeof o.x === "string"); }
{ let o = {}; Object.defineProperties(o, {a: {value: 1, enumerable: true}}); row(o.a === 1); }
// define_property_data_descriptors (line 16421).
{ let a = Object(); a.x = 1; Object.defineProperty(a, 'y', { value: 3, writable: true, configurable: true, enumerable: true }); row(a.y === 3); }
{ let o = {}; row(Object.defineProperty(o, 'k', {value: 1}) === o && desc(o, 'k') === "1:falsefalsefalse"); }
{ let o = {}; Object.defineProperty(o, 'p', {value: 1, configurable: true}); Object.defineProperty(o, 'p', {value: 2, writable: true}); o.p = 3; row(desc(o, 'p') === "3:truefalsetrue"); }
{ let f = () => 1; let o = {}; row(name(() => Object.defineProperty(o, 'x', {value: 1, get: f})) === "TypeError" && desc(o, 'x') === "absent"); }
row(name(() => Object.defineProperty(1, 'x', {})) === "TypeError");
{ let o = {}; Object.defineProperty(o, 'k', {value: 1}); row(Object.getOwnPropertyDescriptor(o, 'missing') === undefined); }
row(desc({x: 1}, 'x') === "1:truetruetrue");
// define_property_validation_and_targets (line 16552).
{
  const fixed = () => { let o = {}; Object.defineProperty(o, 'k', {value: 1, writable: true}); return o; };
  let o1 = fixed(); Object.defineProperty(o1, 'k', {value: 1});
  let o2 = fixed(); Object.defineProperty(o2, 'k', {value: 2});
  let o3 = fixed(); Object.defineProperty(o3, 'k', {writable: false});
  let o4 = fixed(); Object.defineProperty(o4, 'k', {enumerable: false, configurable: false});
  let o5 = fixed(); Object.defineProperty(o5, 'k', {});
  let o6 = fixed(); Object.defineProperty(o6, 'k', {writable: false}); Object.defineProperty(o6, 'k', {value: 1, writable: false});
  row(desc(o1, 'k') === "1:truefalsefalse" && desc(o2, 'k') === "2:truefalsefalse" && desc(o3, 'k') === "1:falsefalsefalse" && desc(o4, 'k') === "1:truefalsefalse" && desc(o5, 'k') === "1:truefalsefalse" && desc(o6, 'k') === "1:falsefalsefalse");
  for (let bad of [{configurable: true}, {enumerable: true}, {get: () => 1}]) { let o = fixed(); row(name(() => Object.defineProperty(o, 'k', bad)) === "TypeError" && desc(o, 'k') === "1:truefalsefalse"); }
}
{ let o = {k: 1}; Object.defineProperty(o, 'k', {enumerable: 0, writable: ''}); Object.defineProperty(o, 'k', {configurable: 'yes'}); row(desc(o, 'k') === "1:falsefalsetrue"); }
{ let o = {k: 1}; Object.defineProperty(o, 'k', {value: undefined}); row(desc(o, 'k') === "undefined:truetruetrue"); }
{ let o = {}; Object.defineProperty(o, 'k', {writable: true}); row(desc(o, 'k') === "undefined:truefalsefalse"); }
{ let o = {}; Object.defineProperty(o, 1.5, {value: 2}); row(Object.getOwnPropertyDescriptor(o, '1.5').value === 2); }
{ let o = {}; Object.defineProperty(o, 'k', {value: 1}); row(name(() => Object.defineProperty(o, 'k', {writable: true})) === "TypeError"); }
{ let o = {}; Object.defineProperty(o, 'k', {value: NaN}); Object.defineProperty(o, 'k', {value: NaN}); row(name(() => Object.defineProperty(o, 'k', {value: 0})) === "TypeError"); }
{ let o = {}; Object.defineProperty(o, 'k', {value: 0}); row(name(() => Object.defineProperty(o, 'k', {value: -0})) === "TypeError" && 1 / o.k === Infinity); }
row(name(() => Object.defineProperty({}, 'k', 1)) === "TypeError" && name(() => Object.defineProperty({}, 'k')) === "TypeError");
row(name(() => Object.defineProperty({}, 'k', {get: 1})) === "TypeError" && name(() => Object.defineProperty({}, 'k', {set: null})) === "TypeError");
row(name(() => Object.defineProperty({}, 'k', {writable: true, set: undefined})) === "TypeError");
row(name(() => Object.defineProperty(undefined, 'k', {})) === "TypeError" && name(() => Object.defineProperty('s', 'k', {})) === "TypeError");
row(name(() => Object.getOwnPropertyDescriptor(null, 'k')) === "TypeError" && name(() => Object.getOwnPropertyDescriptor()) === "TypeError");
{ let f = () => 1; Object.defineProperty(f, 'length', {value: 1}); row(f.length === 1); }
row(Object.getOwnPropertyDescriptor(() => 1, 'name').value === "");
{ let o = {}; Object.defineProperty(o, 'k', {get: () => 1}); row(o.k === 1); }
{ let o = {}; Object.defineProperty(o, 'k', {set: undefined}); row(o.k === undefined); }
{ let o = {k: 1}; Object.defineProperty(o, 'k', {get: undefined}); row(o.k === undefined); }
{ let o = {}; Object.defineProperty(o, {}, {value: 1}); row(o['[object Object]'] === 1); }
row(Object.defineProperty.length === 3 && Object.defineProperty.name === "defineProperty" && Object.defineProperties.length === 2);
// Arrays: elements and `length` keep attributes too; named properties are ordinary.
{ let a = [1, 2]; Object.defineProperty(a, 0, {value: 5}); row('' + a[0] + a.length === "52"); }
{ let a = [1]; Object.defineProperty(a, '3', {value: 4, writable: true, enumerable: true, configurable: true}); row('' + a[3] + a.length + a[2] === "44undefined"); }
{ let a = [1]; Object.defineProperty(a, 'length', {value: 1, writable: true}); row(a.length === 1); }
{ let a = [1]; Object.defineProperty(a, 'x', {value: 2}); let d = Object.getOwnPropertyDescriptor(a, 'x'); row('' + d.value + d.writable + a.length === "2false1"); }
row(desc([7], 0) === "7:truetruetrue" && desc([7, , 9], 'length') === "3:truefalsefalse" && desc([7, , 9], 1) === "absent");
row(name(() => Object.defineProperty([1], 'length', {configurable: true})) === "TypeError");
row(Object.defineProperty([1], 'length', {value: 0}).length === 0 && Object.defineProperty([1], 'length', {value: '1'}).length === 1);
row(Object.defineProperty([1], 0, {get: () => 2})[0] === 2);
// define_property_array_data_attributes (line 16780).
{ let a = []; Object.defineProperty(a, 0, {value: 1}); row(desc(a, 0) + ':' + a.length === "1:falsefalsefalse:1"); }
for (let write of [a => { a[0] = 2; }, a => { a['0'] = 2; }, a => { Object.defineProperty(a, 0, {value: 2}); }]) {
  let a = []; Object.defineProperty(a, 0, {value: 1}); row(name(() => write(a)) === "TypeError" && a[0] === 1);
}
{ let a = [1, 2]; Object.defineProperty(a, 1, {writable: false}); row(desc(a, 1) === "2:falsetruetrue"); }
{ let a = [1]; Object.defineProperty(a, 0, {writable: false}); Object.defineProperty(a, 0, {writable: true}); a[0] = 3; row(a[0] === 3); }
{
  const frozen = () => { let a = [1, 2]; Object.defineProperty(a, 'length', {writable: false}); return a; };
  row(desc(frozen(), 'length') === "2:falsefalsefalse");
  for (let write of [a => { a[2] = 3; }, a => { a[5] = 3; }, a => { a.length = 2; }, a => { Object.defineProperty(a, 2, {value: 3}); }, a => { Object.defineProperty(a, 'length', {writable: true}); }, a => { Object.defineProperty(a, 'length', {value: 3}); }, a => { a.push(3); }]) {
    let a = frozen(); row(name(() => write(a)) === "TypeError" && a.length === 2 && a[2] === undefined);
  }
  let a = frozen(); a[1] = 5; Object.defineProperty(a, 'length', {value: 2}); row('' + a[1] + a.length === "52");
}
{ let o = {}; Object.defineProperty(o, '0', {value: 1}); Object.defineProperty(o, 'k', {value: 1}); row(name(() => { o[0] = 2; }) === "TypeError" && name(() => { o['0'] = 2; }) === "TypeError" && name(() => { o['k'] = 2; }) === "TypeError" && o[0] === 1 && o.k === 1); }
// array_length_define_property (line 18486) and array_length_assignment (line 18392).
{ let a = [1, 2, 3]; Object.defineProperty(a, 'length', {value: '2'}); row(a.length === 2 && a[2] === undefined); }
{ let a = []; a[4000] = 4; a[5000] = 5; a[6000] = 6; Object.defineProperty(a, 'length', {value: 5000}); row(a.length === 5000 && a[4000] === 4 && Object.getOwnPropertyDescriptor(a, 5000) === undefined && Object.getOwnPropertyDescriptor(a, 6000) === undefined); }
{ let a = [1]; Object.defineProperty(a, 'length', {value: 5, writable: false}); let d = Object.getOwnPropertyDescriptor(a, 'length'); row(d.value === 5 && !d.writable && !d.enumerable && !d.configurable && a[4] === undefined); }
{ let a = [1, 2, 3, 4, 5]; Object.defineProperty(a, 3, {configurable: false}); let caught = name(() => Object.defineProperty(a, 'length', {value: 1, writable: false})); let d = Object.getOwnPropertyDescriptor(a, 'length'); row(caught === "TypeError" && a.length === 4 && a[4] === undefined && a[3] === 4 && !d.writable); }
{ let a = []; Object.defineProperty(a, 4000, {value: 2, configurable: false}); a[5000] = 3; let caught = name(() => Object.defineProperty(a, 'length', {value: 0})); row(caught === "TypeError" && a.length === 4001 && a[4000] === 2 && a[5000] === undefined && Object.getOwnPropertyDescriptor(a, 'length').writable); }
{ let a = [1]; Object.defineProperty(a, 'length', {writable: false}); Object.defineProperty(a, 'length', {value: '1'}); row(a.length === 1); }
{ let a = [1]; row(name(() => Object.defineProperty(a, 'length', {value: 0, enumerable: true})) === "TypeError" && a.length === 1 && a[0] === 1); }
{ let a = [1, 2, 3, 4, 5]; Object.defineProperty(a, 1, {configurable: false}); Object.defineProperty(a, 3, {configurable: false}); row(name(() => { a.length = 0; }) === "TypeError" && a.length === 4 && a[2] === 3 && a[4] === undefined); }
{ let a = [1, 2]; Object.defineProperty(a, 1, {writable: false, enumerable: false, configurable: true}); a.length = 1; a[1] = 3; let d = Object.getOwnPropertyDescriptor(a, 1); row(a.length === 2 && d.value === 3 && d.writable && d.enumerable && d.configurable); }
{ let a = []; Object.defineProperty(a, 5000, {value: 1, writable: false, configurable: true}); Object.defineProperty(a, 'length', {value: 0}); a[5000] = 3; row(Object.getOwnPropertyDescriptor(a, 5000).writable && a[5000] === 3); }
for (let invalid of [-1, 1.5, 4294967296, 0 / 0, undefined]) {
  let a = [1, 2]; row(name(() => Object.defineProperty(a, 'length', {value: invalid, writable: false})) === "RangeError" && a.length === 2 && Object.getOwnPropertyDescriptor(a, 'length').writable);
}
{ let a = [1, 2, 3, 4, 5]; Object.defineProperty(a, '3', {configurable: false}); row(name(() => { a.length = 2; }) === "TypeError" && a.length === 4 && a[0] === 1 && a[1] === 2 && a[2] === 3 && a[3] === 4 && a[4] === undefined); }
for (let write of [a => { a.length = 5; }, a => { a.length = 0; }, a => { a.length = 2; }, a => { a[a.length] = 1; }]) {
  let a = [1, 2]; Object.defineProperty(a, 'length', {writable: false}); row(name(() => write(a)) === "TypeError" && a.length === 2 && a[0] === 1 && a[2] === undefined);
}
// delete_property_references (line 17876): non-configurable properties stay.
{ let o = {}; Object.defineProperty(o, 'x', {value: 1}); row(name(() => { delete o.x; }) === "TypeError" && o.x === 1); }
{ let a = [1, 2]; Object.defineProperty(a, 0, {configurable: false}); row(name(() => { delete a[0]; }) === "TypeError" && a[0] === 1 && delete a[1] && a.length === 2); }
// accessor_descriptors_and_integrity (line 7166) and accessors_in_native_operations.
{ let get = () => 1; let a = {}; Object.defineProperty(a, 'z', {get: get}); Object.defineProperty(a, 'z', {get: get}); row(name(() => Object.defineProperty(a, 'z', {get: () => 2})) === "TypeError" && a.z === 1); }
{ let a = {get z() { return 1; }}; Object.defineProperty(a, 'z', {value: 3}); let d = Object.getOwnPropertyDescriptor(a, 'z'); row(a.z === 3 && !d.writable && d.enumerable && d.configurable && !('get' in d)); }
{ let a = {z: 1}; Object.defineProperty(a, 'z', {get: () => 3}); let d = Object.getOwnPropertyDescriptor(a, 'z'); row(a.z === 3 && d.set === undefined && d.enumerable && d.configurable && !('value' in d)); }
{ let n = 0; let a = {}; Object.defineProperty(a, 'z', {get: () => { n++; return 1; }}); let listed = ''; for (let k in a) { listed += k; } row(listed === '' && ('z' in a) && n === 0); }
{ let a = []; Object.defineProperty(a, 3, {get: () => 8, set: function (v) { this.x = v; }, configurable: true, enumerable: true}); let names = ''; for (let k in a) { names += k; } a[3] = 7; let ok = a.length === 4 && a[3] === 8 && a.x === 7 && names === '3'; delete a[3]; row(ok && a.length === 4 && !(3 in a)); }
{ let a = []; Object.defineProperty(a, 2, {get: () => 7, configurable: true}); a.length = 0; row(a.length === 0 && !(2 in a)); }
{ let a = []; Object.defineProperty(a, 2, {get: () => 7}); row(name(() => { a.length = 0; }) === "TypeError" && a.length === 3 && a[2] === 7); }
{ let order = ''; let d = {get enumerable() { order += 'e'; return 1; }, get configurable() { order += 'c'; return 1; }, get value() { order += 'v'; return 7; }, get writable() { order += 'w'; return 1; }}; let o = {}; Object.defineProperty(o, 'z', d); row(order === 'ecvw' && o.z === 7); }
{ let order = ''; let d = {get get() { order += 'g'; return 1; }, get set() { order += 's'; return undefined; }}; row(name(() => Object.defineProperty({}, 'z', d)) === "TypeError" && order === 'g'); }
{ let p = {get value() { return this.x; }}; let d = {__proto__: p}; d.x = 9; let a = {}; Object.defineProperty(a, 'z', d); row(a.z === 9); }
{ let f = () => 0; Object.defineProperty(f, 'name', {get: function () { return this.x; }}); f.x = 'F'; row(f.name === 'F' && Object.getOwnPropertyDescriptor(f, 'name').get !== undefined && ('name' in f)); }
{ let o = {x: 9}; let f = function () { return this.x; }; Object.defineProperty(o, 'z', {get: f.bind({x: 7})}); row(o.z === 7); }
{ let o = {}; Object.defineProperty(o, 'z', {get: Object.prototype.valueOf}); row(o.z === o); }
{ let o = {}; Object.defineProperty(o, 'x', {set: Array.prototype.push}); o.x = 5; row(o.length === 1 && o[0] === 5); }
{ let p = {}; let f = () => 0; Object.defineProperty(f, 'prototype', {get: () => p}); let o = {__proto__: p}; row(o instanceof f && !(1 instanceof f)); }
{ let f = () => 0; Object.defineProperty(f, 'prototype', {get: () => { throw 7; }}); let caught = 0; try { ({}) instanceof f; } catch (e) { caught = e; } row(caught === 7 && !(1 instanceof f)); }
{ let f = (a, b) => a + b; Object.defineProperty(f, 'length', {get: () => 4}); Object.defineProperty(f, 'name', {get: () => 'sum'}); let b = f.bind(null, 2); row(b.length === 3 && b.name === 'bound sum' && b(3) === 5); }
{ let o = {}; Object.defineProperty(o, 'valueOf', {get: () => () => 7}); row(+o === 7); }
// function_own_properties (line 15720): definitions on functions.
{ var g = function g() { }; Object.defineProperty(g, "prototype", { writable: false }); row(g.prototype.constructor === g && name(() => { g.prototype = {}; }) === "TypeError"); }
{ let f = function (a, b) {}; Object.defineProperty(f, 'length', {value: 7}); Object.defineProperty(f, 'name', {value: 'n'}); row(f.length === 7 && f.name === 'n' && desc(f, 'length') === "7:falsefalsetrue"); }
{ let f = function () {}; Object.defineProperty(f, 'x', {value: 1, enumerable: true}); let names = ''; for (let k in f) names += k; row(names === 'x' && name(() => { f.x = 2; }) === "TypeError"); }
// object_statics_define_properties_conversion_before_writes (object_statics.rs line 118).
{ let t = {}, d = {a: {value: 7, enumerable: true}}; Object.defineProperty(d, 'hide', {value: {value: 9}}); row(Object.defineProperties(t, d) === t && t.a === 7 && !('hide' in t) && !Object.getOwnPropertyDescriptor(t, 'a').writable); }
{ let t = {}; Object.defineProperty(t, 'b', {value: 0}); let ok = false; try { Object.defineProperties(t, {a: {value: 1}, b: {value: 2}, c: {value: 3}}); } catch (e) { ok = e instanceof TypeError; } row(ok && t.a === 1 && !('c' in t)); }
{ let n = 0; for (let p of [null, undefined]) { try { Object.defineProperties({}, p); } catch (e) { if (e instanceof TypeError) n++; } } let o = Object.defineProperties({}, 7); let k = 0; for (let x in o) k++; row(n === 2 && k === 0); }
{ let p = {set a(v) { throw 99; }}; let t = {__proto__: p}; Object.defineProperties(t, {a: {value: 7}}); row(t.a === 7); }
// Object spread and rest read built-in and bound getters (light-cljs-881).
{ let o = {}; Object.defineProperty(o, 'x', {get: Object.prototype.valueOf, enumerable: true}); Object.defineProperty(o, 'y', {get: function () { return this.k; }.bind({k: 3}), enumerable: true}); let s = {...o}; let {...r} = o; row(s.x === o && s.y === 3 && r.x === o && r.y === 3); }
// Every other object kind as a target: bound functions, errors,
// generators, iterators, String wrappers (whose exotic `length`
// and indices change by nothing) and frozen template arrays.
{
  let targets = [function () {}.bind(null), new Error('e'), (function* () {})(), [1][Symbol.iterator](), new String('ab')];
  let ok = true;
  for (let t of targets) {
    Object.defineProperty(t, 'k', {value: 1, enumerable: true});
    let d = Object.getOwnPropertyDescriptor(t, 'k');
    ok = ok && t.k === 1 && d.enumerable && !d.writable && !d.configurable && name(() => { t.k = 2; }) === "TypeError" && name(() => { delete t.k; }) === "TypeError" && !Reflect.defineProperty(t, 'k', {value: 2}) && Reflect.defineProperty(t, 'k', {value: 1});
  }
  row(ok);
}
{ let s = new String('ab'); row(Reflect.defineProperty(s, '0', {value: 'a'}) && !Reflect.defineProperty(s, '0', {value: 'z'}) && Reflect.defineProperty(s, 'length', {value: 2}) && !Reflect.defineProperty(s, 'length', {value: 3}) && !Reflect.defineProperty(s, 'length', {writable: true}) && s[0] === 'a' && s.length === 2 && Reflect.defineProperty(s, '5', {value: 'q'}) && s[5] === 'q'); }
{ let t = (s => s)`a${0}b`; row(Reflect.defineProperty(t, 0, {value: 'a'}) && !Reflect.defineProperty(t, 0, {value: 'z'}) && !Reflect.defineProperty(t, 'length', {value: 1}) && !Reflect.defineProperty(t, 'x', {value: 1}) && !Reflect.defineProperty(t.raw, 1, {writable: true}) && t[0] === 'a' && t.length === 2 && desc(t, 0) === "a:falsetruefalse"); }
failed === 0 ? rows : -failed;
