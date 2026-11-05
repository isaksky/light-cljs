// Synthetic fixture, not light-cljs output (clj-quickjs-297).
let a = [1, 2, 3, 2];
let ok = a.indexOf(2) === 1 && a.lastIndexOf(2) === 3 && a.includes(3);
ok = ok && a.indexOf(2, -2) === 3 && a.lastIndexOf(2, -2) === 1;
ok = ok && a.indexOf(1, -99) === 0 && a.lastIndexOf(2, 99) === 3;
ok = ok && a.indexOf(2, Infinity) === -1 && a.lastIndexOf(2, -Infinity) === -1;
ok = ok && a.lastIndexOf(2, undefined) === -1 && a.lastIndexOf(2) === 3;
ok = ok && a.includes(1, -Infinity) && !a.includes(1, Infinity);
ok = ok && a.at(-1) === 2 && a.at(-4) === 1 && a.at(-5) === undefined;
ok = ok && a.at(4) === undefined && a.at(NaN) === 1 && a.at(1.9) === 2;
ok = ok && a.at(Infinity) === undefined && a.at(-Infinity) === undefined;
let holes = new Array(3);
ok = ok && holes.indexOf(undefined) === -1 && holes.lastIndexOf(undefined) === -1;
ok = ok && holes.includes(undefined) && [NaN].includes(NaN) && [NaN].indexOf(NaN) === -1;
ok = ok && [0].includes(-0) && [-0].indexOf(0) === 0;
ok = ok && [12345678901234567890n].includes(12345678901234567890n);
ok = ok && [12345678901234567890n].indexOf(12345678901234567890n) === 0;
ok = ok && [1].indexOf('1') === -1 && ![1].includes('1');
let seen = '', ctx = {x: 7};
let found = holes.find(function(v, i, o) { seen += i; return this === ctx && o === holes && i === 2 && v === undefined; }, ctx);
ok = ok && found === undefined && seen === '012';
seen = '';
ok = ok && holes.findLastIndex((v, i) => { seen += i; return i === 0; }) === 0 && seen === '210';
ok = ok && a.find(x => x > 1) === 2 && a.findIndex(x => x > 1) === 1;
ok = ok && a.findLast(x => x > 1) === 2 && a.findLastIndex(x => x > 1) === 3;
ok = ok && a.find(() => false) === undefined && a.findLast(() => false) === undefined;
ok = ok && a.findIndex(() => false) === -1 && a.findLastIndex(() => false) === -1;
let o = Object.create({1: 'inherited'}); o.length = 3; o[2] = 'last';
ok = ok && Array.prototype.indexOf.call(o, 'inherited') === 1;
ok = ok && Array.prototype.lastIndexOf.call(o, 'inherited') === 1;
ok = ok && Array.prototype.includes.call(o, undefined) && Array.prototype.at.call(o, -1) === 'last';
ok = ok && Array.prototype.find.call(o, v => v === 'inherited') === 'inherited';
ok = ok && Array.prototype.at.call('abc', -1) === 'c' && Array.prototype.indexOf.call('abc', 'b') === 1;
for (let name of ['indexOf', 'lastIndexOf', 'includes', 'find', 'findIndex', 'findLast', 'findLastIndex', 'at']) {
    let d = Object.getOwnPropertyDescriptor(Array.prototype, name);
    ok = ok && d.value.name === name && d.value.length === 1 && d.writable && d.configurable && !d.enumerable;
}
ok;
