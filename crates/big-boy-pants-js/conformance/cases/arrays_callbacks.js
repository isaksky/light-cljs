// bbp-authored conformance case (light-cljs-908).
// Derives from: rust/tests/fixtures/js/array_callbacks.js lines 1-92 (without its Uint8Array row, light-cljs-769), rust/tests/js_source.rs lines 30002-30087 (array_callbacks_preserve_call_limits's results and array_callbacks_order_species_and_errors's rows without species), rust/tests/fixtures/js/proxy_has.js lines 108-121, and light-cljs-767's planner row
// (Array.prototype.filter, reduce, reduceRight, some, every and flatMap with map and forEach: thisArg and arguments, holes, inherited elements and getters re-read at every visit, array-likes and strings, early exit, reduce without a value, callbacks that are bound functions, built-ins, proxies and classes, exceptions, and a proxy receiver's has trap). Completes with "ok", or the number of the first failed check.
(function () {
  var failed = 0;
  function check(n, ok) { if (!ok && failed === 0) { failed = n; } }
  check(1, (function () {
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
    let nested = [4,[5]], sparse = new Array(3); sparse[1] = nested;
    let flat = [1,2].flatMap(function(v,i,o) { assert(this === ctx && o.length === 2); return i === 0 ? sparse : {0:7,length:1,[Symbol.isConcatSpreadable]:true}; },ctx);
    assert(flat.length === 2 && flat[0] === nested && flat[0][1][0] === 5 && flat[1][0] === 7);
    assert([1,2].flatMap(v => v).join() === '1,2');
    let keep = [{x:1}]; let filtered = keep.filter((v,i,o) => { delete o[i]; return true; });
    assert(filtered[0].x === 1);
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
    return true;
  })());
  check(10, (function () { let n=0; for (let method of ['forEach','map','filter','reduce','reduceRight','some','every','flatMap']) { let o={get length() { n++; return 0; }}; try { Array.prototype[method].call(o,null); } catch(e) { if (e instanceof TypeError) n++; } } return (n === 16); })());
  check(11, (function () { let n=0; for (let method of ['forEach','map','filter','reduce','reduceRight','some','every','flatMap']) { for (let receiver of [null,undefined]) { try { Array.prototype[method].call(receiver,()=>true); } catch(e) { if (e instanceof TypeError) n++; } } try { new Array.prototype[method](); } catch(e) { if (e instanceof TypeError) n++; } } return (n === 24); })());
  check(12, (function () { let n=0; for (let method of ['reduce','reduceRight']) { for (let a of [[],new Array(4)]) { try { a[method]((a,b)=>a+b); } catch(e) { if (e instanceof TypeError) n++; } } } return (n === 4); })());
  check(13, (function () { let n=0; for (let method of ['reduce','reduceRight']) { let a=new Array(3); a[1]=undefined; if (a[method](()=>{ n+=100; }) === undefined) n++; } return (n === 2); })());
  check(14, (function () { let n=0; let o={get length() { n++; return {valueOf() { n++; return 2; }}; },get 0() { n++; return 3; },1:4}; return (Array.prototype.reduce.call(o,(acc,v)=>acc+v,0) === 7 && n === 3); })());
  check(15, (function () { let seen='',a=[1,2,3]; let r=a.reduceRight((acc,v,i,o)=>{ seen+=i; if (i === 2) { delete o[1]; o[0]=8; o.push(9); } return acc+v; },0); return (r === 11 && seen === '20'); })());
  check(16, (function () { let n=0; for (let method of ['forEach','map','filter','reduce','reduceRight','some','every','flatMap']) { try { [1,2][method](()=>{ n++; throw 42; },0); } catch(e) { if(e === 42) n++; } } return (n === 16); })());
  check(17, (function () { let n=0; [1].reduce(function(a,v) { if (this === undefined && arguments.length === 4) n++; },0); [1].reduceRight(function(a,v) { if (this === undefined && arguments.length === 4) n++; },0); return (n === 2); })());
  check(18, (function () { let a=[1],inner=[2,3,4],seen=''; Object.defineProperty(inner,0,{get() { delete inner[1]; inner[2]=9; inner.push(8); return 2; }}); let r=a.flatMap(()=>inner); return (r.join() === '2,9'); })());
  check(19, (function () { let proto=Object.create(Array.prototype); proto[1]=7; let inner=new Array(3); inner[2]=[8]; Object.setPrototypeOf(inner,proto); let r=[0].flatMap(()=>inner); return (r.length === 2 && r[0] === 7 && r[1][0] === 8); })());
  check(20, (function () { let a=[1,2]; let r=a.map(function(v) { return v*this.x; }.bind({x:3}),{x:9}); return (r.join() === '3,6'); })());
  check(40, (function () { let locals = ''; let f = function () { let v39 = 39; return arguments.length + v39; }; return ([1,2].reduce(f,0) === 43 && [1,2].forEach(f,0) === undefined && [1,2].map(f,0).join() === '42,42' && [1,2].filter(f,0).length === 2); })());
  check(41, (function () {  return ([1,2,3,4].filter(x => x % 2).join() + ';' + [1,2,3].reduce((a,b)=>a+b) + ';' + [1,2,3].reduceRight((a,b)=>a+b, '') + ';' + [1,2].some(x => x > 1) + [1,2].every(x => x > 1) + ';' + [1,[2]].flatMap(x => x).length === '1,3;6;321;truefalse;2'); })());
  check(43, (function () { let n = 0; let o = {length: 2, get 1() { n++; return 8; }, 0: 7}; let r = [0,1].map(Array.prototype.at, o); let t; try { [0].map(Array.prototype.at, null); } catch (e) { t = e instanceof TypeError; } return (r.join() === '7,8' && n === 1 && t && [[3,4]].some(Array.prototype.includes, [0]) === false && [[1,2],[3]].map(Function.prototype.call.bind(Array.prototype.indexOf)).join() === '-1,-1'); })());
  check(42, (function () { function hidden(target, key) {return new Proxy(target, {has(t, k) {return k !== key && k in t;}});} let p = hidden([1, 2, 3], '1'); let visited = ''; Array.prototype.forEach.call(p, x => {visited += x;}); let mapped = Array.prototype.map.call(p, x => x * 2); return (mapped.length === 3 && mapped[0] === 2 && !(1 in mapped) && mapped[2] === 6 && Array.prototype.filter.call(p, () => true).join(',') === '1,3' && Array.prototype.reduce.call(p, (a, b) => a + b, 0) === 4 && Array.prototype.reduceRight.call(p, (a, b) => a + b, 0) === 4 && visited === '13' && Array.prototype.every.call(p, x => x !== 2) && !Array.prototype.some.call(p, x => x === 2) && Array.prototype.flatMap.call(p, x => [x]).join(',') === '1,3' && [1].flatMap(() => p).join(',') === '1,3'); })());
  return failed === 0 ? "ok" : failed;
})();
