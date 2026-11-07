// bbp-authored conformance case.
// Derives from: none (light-cljs-832; ECMAScript 10.4.3, String exotic
// objects, the Pants engine's rust/src/vm/property.rs String object
// properties): a String object's own `length` (not writable, enumerable or
// configurable) and indices (enumerable only), `in`, for-in order (string
// indices, then other index keys, then names), Object.getOwnPropertyDescriptor
// of a String object and of a string (ToObject), strict-mode writes and
// deletes that the exotic properties refuse, and an object spread of a
// String object. Each row is a `row(...)` check; the script completes with
// the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function throwsType(f) { try { f(); } catch (e) { return e.name === 'TypeError'; } return false; }
const s = new String('ab');
row(s.length === 2 && s[0] === 'a' && s[1] === 'b' && s[2] === undefined && s['1'] === 'b');
row('length' in s && '0' in s && 1 in s && !(2 in s) && !('-0' in s) && !('01' in s));
{ let keys = ''; for (const k in s) keys += k + ','; row(keys === '0,1,'); }
{ const t = new String('ab'); t.x = 1; t[5] = 2; t[3] = 3; let keys = ''; for (const k in t) keys += k + ','; row(keys === '0,1,3,5,x,'); }
{ const d = Object.getOwnPropertyDescriptor(s, 'length'); row(d.value === 2 && !d.writable && !d.enumerable && !d.configurable); }
{ const d = Object.getOwnPropertyDescriptor(s, '0'); row(d.value === 'a' && !d.writable && d.enumerable && !d.configurable); }
row(Object.getOwnPropertyDescriptor(s, '2') === undefined && Object.getOwnPropertyDescriptor(s, 'x') === undefined);
{ const d = Object.getOwnPropertyDescriptor('xyz', 2); row(d.value === 'z' && d.enumerable && !d.writable && Object.getOwnPropertyDescriptor('xyz', 'length').value === 3); }
row(throwsType(() => { s[0] = 'z'; }) && throwsType(() => { s.length = 5; }) && s[0] === 'a' && s.length === 2);
row(throwsType(() => { delete s[0]; }) && throwsType(() => { delete s.length; }) && s[0] === 'a');
{ const t = new String('ab'); t[5] = 1; t.y = 2; row(delete t[5] && delete t.y && t[5] === undefined && !('y' in t) && delete t[7]); }
row(Object.getOwnPropertyDescriptor('ab', '0').value === 'a' && Object.getOwnPropertyDescriptor(new String(''), '0') === undefined && Object.getOwnPropertyDescriptor(s, 1).value === 'b');
// String.prototype is a String object of "".
row(String.prototype.length === 0 && String.prototype[0] === undefined && Object.prototype.toString.call(String.prototype) === '[object String]');
// Units, not code points: a wide string's indices are its UTF-16 units.
{ const w = new String('é€😀'); row(w.length === 4 && w[0] === 'é' && w[1] === '€' && w[2] === '\ud83d' && w[3] === '\ude00'); }
// An object spread copies the indices, not `length`.
{ const o = {...new String('ab')}; row(o[0] === 'a' && o[1] === 'b' && !('length' in o)); }
// A String object on a prototype chain: its indices are read-only for the
// object inheriting them.
{ const o = {__proto__: new String('q')}; row(o[0] === 'q' && o.length === 1 && 0 in o && Object.getOwnPropertyDescriptor(o, '0') === undefined && throwsType(() => { o[0] = 'z'; }) && o[0] === 'q'); }
failed === 0 ? rows : -failed;
