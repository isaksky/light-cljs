// bbp-authored conformance case (light-cljs-890).
// Derives from: crates/big-boy-pants-js/rust/tests/fixtures/js/proxy_builtin_reads.js lines 5-41
// (run by rust/tests/proxy_get.rs builtin_reads_follow_ecmascript and
// proxy_array_length_is_observable): its `cases` rows that the Pants engine
// and QuickJS agree on and both of its loops over them (each read through
// the proxy's `get` trap, in order, and a throwing read propagated),
// without the pause() hook (a no-op there), with light-cljs-766's planner
// row of a trapless proxy of an array. The Object.assign and object spread
// rows (the latter light-cljs-895's) compare their results by
// Object.entries instead of JSON.stringify. Rows left out, each with its
// reason:
//   'JSON' and the JSON tail (lines 8, 42-74): JSON is the core built-ins
//     area's (light-cljs-767), as are Array.from, concat, sort and flat
//     ('Array.from iterable', 'concat', 'sort', 'flat').
//   'Array.from arraylike': it also writes the proxy's @@iterator without
//     a `set` trap, defining on the proxy through its [[DefineOwnProperty]]
//     (light-cljs-891).
//   'array spread' and 'call spread': QuickJS reads @@iterator twice
//     ("@@iterator,@@iterator,length,number,0,...") where the Pants engine
//     and ECMAScript read it once (a disagreement).
function assert(v, label) {if (!v) throw new Error(label);}
function entries(o) { return Object.entries(o).map(e => e.join(':')).join(','); }
{
    let p = new Proxy([1,2], {});
    assert(Array.isArray(p) + ',' + p.length === 'true,2', 'planner row');
}
const cases = [
    ['join', p=>Array.prototype.join.call(p), '3,1', 'length,number,0,1'],
    ['Object.assign', p=>entries(Object.assign({},p)), '0:3,1:1,2:2', '0,1,2'],
    ['object spread', p=>entries({...p}), '0:3,1:1,2:2', '0,1,2'],
    ['String', p=>String(p), '3,1', '@@toPrimitive,toString,join,length,number,0,1'],
];
for (let row of cases) {
    let reads=[], target=[3,1,2];
    let length = {[Symbol.toPrimitive](hint) {reads.push(hint);return '2.9';}};
    let p = new Proxy(target, {get(t,k,r) {
        reads.push(k===Symbol.iterator ? '@@iterator' : k===Symbol.isConcatSpreadable ? '@@isConcatSpreadable' : k===Symbol.toPrimitive ? '@@toPrimitive' : k);
        return k==='length' ? length : Reflect.get(t,k,r);
    }});
    assert(row[1](p,target)===row[2], row[0]+' result');
    assert(reads.join(',')===row[3], row[0]+' reads: '+reads.join(','));
}
// Every consumer must propagate a throwing read, rather than using storage or
// swallowing the failure. Assign/spread read elements, not the array's length.
for (let row of cases) {
    let property = row[0]==='Object.assign' || row[0]==='object spread' ? '0' : 'length';
    let n=0, target=[3,1,2], p=new Proxy(target, {get(t,k,r) {
        if(k===property) {n++;throw 42;} return Reflect.get(t,k,r);
    }}), reason;
    try {row[1](p,target);} catch(e) {reason=e;}
    assert(reason===42 && n===1, row[0]+' throwing read');
}
true;
