// Synthetic fixture, not light-cljs output.
function assert(ok) { if (!ok) throw new Error('array callback assertion'); }
let methods = ['forEach','map','filter','reduce','reduceRight','some','every','flatMap'];
for (let name of methods) {
    let f = Array.prototype[name], d = Object.getOwnPropertyDescriptor(Array.prototype,name);
    assert(typeof f === 'function' && f.name === name && f.length === 1);
    assert(d.writable && d.configurable && !d.enumerable);
}
let a = [1,2,3], sum = 0, ctx = {factor:2};
assert(a.forEach(function(v,i,o) { assert(this === ctx && o === a && v === i+1); sum += v; },ctx) === undefined);
assert(sum === 6);
assert(a.map(function(v) { return v*this.factor; },ctx).join() === '2,4,6');
assert(a.filter(function(v) { return v >= this.factor; },ctx).join() === '2,3');
assert(a.reduce((acc,v,i,o) => { assert(o === a && i === v-1); return acc+v; },0) === 6);
assert(a.reduce((acc,v) => acc+v) === 6);
assert(a.reduceRight((acc,v) => acc+String(v),'') === '321');
assert(a.reduceRight((acc,v) => acc-v) === 0);
assert([].reduce((acc,v) => acc,undefined) === undefined);
assert([].every(() => false) && ![].some(() => true));
let calls = 0;
assert(a.some(function(v) { assert(this === ctx); calls++; return v === 2; },ctx) && calls === 2);
calls = 0;
assert(!a.every(function(v) { assert(this === ctx); calls++; return v < 2; },ctx) && calls === 2);
let holes = new Array(4); holes[1] = 2;
let proto = Object.create(Array.prototype); proto[2] = 7; Object.setPrototypeOf(holes,proto);
let seen = '';
let mapped = holes.map((v,i) => { seen += i; return v*2; });
assert(seen === '12' && mapped.length === 4 && !(0 in mapped) && mapped[1] === 4 && mapped[2] === 14 && !(3 in mapped));
assert(holes.filter(() => true).join() === '2,7');
assert(holes.reduce((acc,v) => acc+v) === 9 && holes.reduceRight((acc,v) => acc-v) === 5);
assert(holes.flatMap((v) => [v,v]).join() === '2,2,7,7');
let changed = [1,2,3], visited = '';
let changedResult = changed.map((v,i,o) => { visited += v; if (i === 0) { delete o[1]; o[2] = 8; o.push(9); } return v; });
assert(visited === '18' && changedResult.length === 3 && !(1 in changedResult) && changedResult[2] === 8);
let reads = 0, generic = {0:3,2:5,get length() { reads++; return {valueOf() { return 3.9; }}; }};
assert(Array.prototype.map.call(generic,(v,i,o) => { assert(o === generic); return v+i; }).join() === '3,,7' && reads === 1);
assert(Array.prototype.map.call('ab',(v,i,o) => { assert(typeof o === 'object'); return v+i; }).join() === 'a0,b1');
assert(Array.prototype.filter.call(new Uint8Array([1,2,3]),v => v > 1).join() === '2,3');
let nested = [4,[5]], sparse = new Array(3); sparse[1] = nested;
let flat = [1,2].flatMap(function(v,i,o) { assert(this === ctx && o.length === 2); return i === 0 ? sparse : {0:7,length:1,[Symbol.isConcatSpreadable]:true}; },ctx);
assert(flat.length === 2 && flat[0] === nested && flat[0][1][0] === 5 && flat[1][0] === 7);
assert([1,2].flatMap(v => v).join() === '1,2');
let keep = [{x:1}]; let filtered = keep.filter((v,i,o) => { delete o[i]; return true; });
assert(filtered[0].x === 1);
true;
