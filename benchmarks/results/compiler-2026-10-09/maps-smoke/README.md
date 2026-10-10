# Immutable JavaScript maps: resident scaling smoke test

Measured 2026-10-09 on Apple M3 Ultra (Virtual), macOS Darwin 24.6.0 arm64,
Node v24.18.0. These are local resident operation observations, with no
baseline speedup claim or performance threshold. Compiler, output and startup
benchmarking remain separate work.

Each operation discards three full warmups and retains seven samples. The table
reports median nanoseconds per operation; `samples.json` retains all samples,
iteration counts, environment, source size and SHA-256. Construction, seq
materialization and updates allocate new immutable values; lookups reuse a map
and either hit the last inserted key or miss. Values are shallow shared vectors.
Every size verifies that the original map/count/value survived all updates.

| Entries | Build | Get last | Get missing | Assoc replace | Dissoc last | Seq materialize |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 8 | 398 | 196 | 182 | 705 | 558 | 1,193 |
| 64 | 12,717 | 1,253 | 1,274 | 2,100 | 2,207 | 16,735 |
| 512 | 563,802 | 9,957 | 10,223 | 15,146 | 14,409 | 354,413 |

The measurements are consistent with linear scanning/copying rather than a HAMT;
building distinct entries repeatedly scans the growing entry array. Garbage
collection and weak-brand bookkeeping contribute to allocating-operation costs,
so these samples do not prove an asymptotic bound or stable ratio. The implementation
has O(n) lookup/update, O(n²) construction, and O(n) initial seq materialization,
before nested structural key costs. Map count itself is O(1).

At measurement time `src/js_runtime.js` was 19,391 bytes. Existing scalar source
`42` still emits 104 bytes with no runtime factory. Source SHA-256 is retained in
`samples.json`; accumulated uncommitted changes mean HEAD alone is not an
implementation identity.

Reproduce from the repository root (writes only this sample artifact):

```js
const fs=require('node:fs'),os=require('node:os'),crypto=require('node:crypto');
const source=fs.readFileSync('src/js_runtime.js','utf8'),rt=(0,eval)(source),c=rt.core;
const perf=require('node:perf_hooks').performance;
let sink=0;
function measure(f,n){for(let i=0;i<3;i++)for(let j=0;j<n;j++)sink+=f(j);const samples=[];for(let i=0;i<7;i++){const start=perf.now();for(let j=0;j<n;j++)sink+=f(j);samples.push((perf.now()-start)*1e6/n);}return {iterations:n,samples_ns:samples,median_ns:[...samples].sort((a,b)=>a-b)[3]};}
const rows=[];
for(const size of [8,64,512]){
 const pairs=Array.from({length:size},(_,i)=>[i,c.vector(i)]).flat();
 const m=rt.mapFromArray(pairs), value=c.vector('changed');
 const row={size};
 row.build=measure(()=>c.count(rt.mapFromArray(pairs)),1000);
 row.get_last=measure(()=>c.nth(c.get(m,size-1),0),30000);
 row.get_miss=measure(()=>c.get(m,-1,0),30000);
 row.assoc_replace=measure(()=>c.count(c.assoc(m,size-1,value)),5000);
 row.dissoc_last=measure(()=>c.count(c.dissoc(m,size-1)),5000);
 row.seq_materialize=measure(()=>c.count(c.seq(m)),1000);
 if(c.count(m)!==size||c.nth(c.get(m,size-1),0)!==size-1)throw Error('mutation');
 rows.push(row);
}
const output={date:'2026-10-09',node:process.version,cpu:os.cpus()[0].model,platform:os.platform(),arch:os.arch(),release:os.release(),runtime_bytes:Buffer.byteLength(source),runtime_sha256:crypto.createHash('sha256').update(source).digest('hex'),method:'Resident runtime; three full warmups and seven measured samples per operation; nanoseconds/op. Construction creates fresh map entries from a reused input-pair array; lookups hit last or absent key; updates start from the same retained immutable map.',rows,sink};
fs.writeFileSync('benchmarks/results/compiler-2026-10-09/maps-smoke/samples.json',JSON.stringify(output,null,2)+'\n');
console.log(JSON.stringify(rows.map(r=>({size:r.size,...Object.fromEntries(Object.entries(r).filter(([k])=>k!=='size').map(([k,v])=>[k,Math.round(v.median_ns)]))})),null,2));
```
