// First-party observable Proxy reads. For the Node/V8 read-order oracle, reduce
// the 2^32 virtual length to 4: V8 rejects huge JSON lengths before index 0 with
// an output-size RangeError. The engine tests keep the full virtual length.
// The Rust test replaces pause with a dependency wait at every read/conversion.
function pause() {}
function assert(v, label) {if (!v) throw new Error(label);}
const cases = [
    ['JSON', p=>JSON.stringify(p), '[3,1]', 'toJSON,length,number,0,1'],
    ['Array.from iterable', p=>Array.from(p).join(','), '3,1', '@@iterator,length,number,0,length,number,1,length,number'],
    ['Array.from arraylike', p=>{p[Symbol.iterator]=undefined;return Array.from(p).join(',');}, '3,1', '@@iterator,length,number,0,1'],
    ['concat', p=>[].concat(p).join(','), '3,1', '@@isConcatSpreadable,length,number,0,1'],
    ['join', p=>Array.prototype.join.call(p), '3,1', 'length,number,0,1'],
    ['sort', (p,t)=>{Array.prototype.sort.call(p);return t.join(',');}, '1,3,2', 'length,number,0,1'],
    ['flat', p=>[p].flat().join(','), '3,1', 'length,number,0,1'],
    ['array spread', p=>[...p].join(','), '3,1', '@@iterator,length,number,0,length,number,1,length,number'],
    ['call spread', p=>((...args)=>args.join(','))(...p), '3,1', '@@iterator,length,number,0,length,number,1,length,number'],
    ['Object.assign', p=>JSON.stringify(Object.assign({},p)), '{"0":3,"1":1,"2":2}', '0,1,2'],
    ['object spread', p=>JSON.stringify({...p}), '{"0":3,"1":1,"2":2}', '0,1,2'],
    ['String', p=>String(p), '3,1', '@@toPrimitive,toString,join,length,number,0,1'],
];
for (let row of cases) {
    let reads=[], target=[3,1,2];
    let length = {[Symbol.toPrimitive](hint) {pause();reads.push(hint);return '2.9';}};
    let p = new Proxy(target, {get(t,k,r) {
        pause();
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
        pause(); if(k===property) {n++;throw 42;} return Reflect.get(t,k,r);
    }}), reason;
    try {row[1](p,target);} catch(e) {reason=e;}
    assert(reason===42 && n===1, row[0]+' throwing read');
}
// SerializeJSONArray applies ToLength rather than narrowing to an array length.
for (let row of [[-1,'[]'],[NaN,'[]'],[undefined,'[]'],['1.9','[3]'],[4,'[3,1,2,null]']]) {
    let p=new Proxy([3,1,2],{get(t,k,r){pause();return k==='length' ? row[0] : Reflect.get(t,k,r);}});
    assert(JSON.stringify(p)===row[1], 'JSON ToLength '+row[0]);
}
// A virtual length of 2^32 must reach index 0, without allocating that length.
// Narrowing it to u32 would instead serialize an empty array and skip the throw.
let largeReads=[], largeReason;
try {
    JSON.stringify(new Proxy([],{get(t,k,r){
        pause();largeReads.push(k);
        if(k==='length') return 4294967296;
        if(k==='0') throw 42;
        return Reflect.get(t,k,r);
    }}));
} catch(e) {largeReason=e;}
assert(largeReason===42, 'JSON length above u32 propagates element exception');
assert(largeReads.join(',')==='toJSON,length,0', 'JSON length above u32 reads: '+largeReads.join(','));
let nested=new Proxy(new Proxy([1,2],{get(t,k,r){pause();return k==='length' ? 1 : Reflect.get(t,k,r);}}),{});
assert(JSON.stringify(nested)==='[1]', 'nested JSON array');
assert(JSON.stringify(new Proxy(Array.prototype,{}))==='[]', 'intrinsic array brand');
for (let length of [1n, Symbol('length')]) {
    let reason;
    try {
        JSON.stringify(new Proxy([],{get(t,k,r){pause();return k==='length' ? length : Reflect.get(t,k,r);}}));
    } catch(e) {reason=e;}
    assert(reason instanceof TypeError, 'JSON rejects nonnumeric length');
}
let coercionReason;
try {
    JSON.stringify(new Proxy([],{get(t,k,r){pause();return k==='length' ? {valueOf(){pause();throw 42;}} : Reflect.get(t,k,r);}}));
} catch(e) {coercionReason=e;}
assert(coercionReason===42, 'JSON length conversion exception');
true;
