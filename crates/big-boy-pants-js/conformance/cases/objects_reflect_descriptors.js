// bbp-authored conformance case (light-cljs-881).
// Derives from: crates/big-boy-pants-js/rust/tests/reflect.rs lines 38-220:
// ordinary_methods_and_failure_results (line 38, bbp-318),
// non_object_targets_and_bad_descriptors_throw (line 63),
// validation_precedes_callbacks_and_proxy_invariants_throw (line 193), its
// rows without Proxy, and rejection_results_do_not_depend_on_property_names
// (line 209), its rows on job objects; and the planner's rows
// (light-cljs-766). Reflect.defineProperty answers a boolean,
// Reflect.getOwnPropertyDescriptor and Reflect.deleteProperty require an
// object target, and deleteProperty is false for a non-configurable
// property. Left out: Reflect's other methods (light-cljs-882 to -891),
// Object.preventExtensions (light-cljs-882), and rows on realm objects
// (which QuickJS changes and the Pants engine and Goose refuse). Errors are
// compared by name. Each row is a `row(...)` check; the script completes
// with the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-766).
{ let o = {}; row(Reflect.defineProperty(o, 'x', {value: 1}) && Object.getOwnPropertyDescriptor(o, 'x').writable === false); }
row(Reflect.deleteProperty({a: 1}, 'a') === true);
// ordinary_methods_and_failure_results (line 38).
{ let o = {}; row(Reflect.defineProperty(o, 'a', {value: 4}) && Reflect.getOwnPropertyDescriptor(o, 'a').value === 4 && !Reflect.defineProperty(o, 'a', {value: 5}) && !Reflect.deleteProperty(o, 'a') && Reflect.deleteProperty(o, 'missing') && Reflect.getOwnPropertyDescriptor(o, 'missing') === undefined); }
// non_object_targets_and_bad_descriptors_throw (line 63).
{ let n = 0; for (let f of [Reflect.defineProperty, Reflect.deleteProperty, Reflect.getOwnPropertyDescriptor]) { try { f(1, 'x', {}); } catch (e) { if (e instanceof TypeError) n++; } } row(n === 3); }
{ let n = 0; for (let d of [1, {get: 2}, {get() {}, value: 1}]) { try { Reflect.defineProperty({}, 'x', d); } catch (e) { if (e instanceof TypeError) n++; } } row(n === 3); }
// validation_precedes_callbacks_and_proxy_invariants_throw (line 193): the
// target is checked before the key converts.
{ let n = 0, key = {toString() { throw 'key ran'; }}; for (let f of [Reflect.defineProperty, Reflect.deleteProperty, Reflect.getOwnPropertyDescriptor]) { try { f(null, key, {}); } catch (e) { if (e instanceof TypeError) n++; } } row(n === 3); }
// rejection_results_do_not_depend_on_property_names (line 209).
{ let ok = true; for (let key of ['immutable', 'built-in objects are shared by all tasks and immutable', 'functions shared between tasks are immutable']) { let o = {}; Object.defineProperty(o, key, {value: 1}); ok = ok && !Reflect.defineProperty(o, key, {value: 2}) && o[key] === 1; } row(ok); }
// Lengths, names and descriptors of the methods.
row(Reflect.defineProperty.length === 3 && Reflect.deleteProperty.length === 2 && Reflect.getOwnPropertyDescriptor.length === 2 && Reflect.getOwnPropertyDescriptor.name === 'getOwnPropertyDescriptor');
{ let d = Object.getOwnPropertyDescriptor(Reflect, 'defineProperty'); row(d.writable && !d.enumerable && d.configurable && typeof d.value === 'function'); }
// Arrays and functions through Reflect.
{ let a = [1, 2, 3]; row(Reflect.defineProperty(a, 'length', {value: 1}) && a.length === 1 && !Reflect.deleteProperty(a, 'length')); }
{ let a = [1, 2]; Reflect.defineProperty(a, 'length', {writable: false}); row(!Reflect.defineProperty(a, 2, {value: 3}) && !Reflect.defineProperty(a, 'length', {value: 5}) && a.length === 2); }
{ let f = function () {}; row(!Reflect.deleteProperty(f, 'prototype') && Reflect.deleteProperty(f, 'name') && f.name === '' && Reflect.defineProperty(f, 'prototype', {writable: false}) && !Reflect.getOwnPropertyDescriptor(f, 'prototype').writable); }
{ let o = {}; let r = Reflect.defineProperty(o, 'g', {get: () => 5, enumerable: true}); let d = Reflect.getOwnPropertyDescriptor(o, 'g'); row(r && o.g === 5 && typeof d.get === 'function' && d.set === undefined && d.enumerable && !d.configurable && !Reflect.deleteProperty(o, 'g')); }
failed === 0 ? rows : -failed;
