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
let stop = [1,2,3];
Object.defineProperty(stop,2,{get() { throw new Error('read after early exit'); }});
assert(stop.some(v => v === 2) && !stop.every(v => v < 2));
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
// Re-check own data after every callback: later visits can become holes,
// accessors or inherited properties, while appended indices stay unvisited.
for (let method of ['forEach','map','filter','reduce']) {
    let values = [1,2,3,4], log = '', getters = 0;
    let parent = Object.create(Array.prototype);
    Object.defineProperty(parent,3,{get() { getters++; return 40; }});
    Object.setPrototypeOf(values,parent);
    function visit(v,i,o) {
        assert(o === values);
        log += v + ':' + i + ';';
        if (i === 0) {
            delete o[1];
            Object.defineProperty(o,2,{get() { getters++; return 30; },configurable:true});
            delete o[3];
            o.push(99);
        }
        return v;
    }
    if (method === 'reduce') values.reduce((acc,v,i,o) => acc+visit(v,i,o),0);
    else values[method](visit);
    assert(log === '1:0;30:2;40:3;' && getters === 2);
    let calls = 0, cleaned = 0, thrown;
    function fail() { calls++; try { if (calls === 2) throw 42; } finally { cleaned++; } }
    try { [1,2,3][method](fail,0); } catch(e) { thrown = e; }
    assert(thrown === 42 && calls === 2 && cleaned === 2);
}
// Ordinary closure entry and the forwarding/Proxy fallback share the same
// argument and exception behavior, including changing traps between visits.
assert([2,3].map(function(prefix,v,i,o) {
    assert(arguments.length === 4 && o[i] === v);
    return prefix+v+i;
}.bind(null,10)).join() === '12,14');
assert(['2','3'].map(Number).join() === '2,3');
let proxyCalls = 0, handler = {apply(target,receiver,args) {
    proxyCalls++;
    handler.apply = () => { proxyCalls++; throw 42; };
    return Reflect.apply(target,receiver,args);
}};
let proxyError;
try { [1,2,3].map(new Proxy(v => v,handler)); } catch(e) { proxyError = e; }
assert(proxyError === 42 && proxyCalls === 2);
let classCalls = 0;
try { [1].forEach(class C {}); } catch(e) { if (e instanceof TypeError) classCalls++; }
assert(classCalls === 1);
true;
