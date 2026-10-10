# Immutable JavaScript sets: resident scaling smoke test

Measured 2026-10-09 on Apple M3 Ultra (Virtual), macOS Darwin 24.6.0 arm64,
Node v24.18.0. These are local resident operation observations with no speedup
claim or performance threshold. Compiler, output and startup measurements remain
separate work. Final retained samples ran after compiler and reference checks
completed, without those checks competing for CPU.

Each operation discards three full warmups and retains seven measured samples.
The table reports median nanoseconds/operation; `samples.json` retains all samples,
iteration counts, environment, source size and SHA-256. Members are immutable
single-element vectors. A prebuilt equivalent list query hits the last member;
missing/new queries are prebuilt vectors. Construction, updates and initial seq
materialization allocate; lookups include structural hashing/equality without
query allocation. Each size verifies the original set and retained member identity.

| Members | Build | Get last | Get missing | Conj new | Disj last | Seq materialize |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 8 | 3,050 | 738 | 430 | 823 | 915 | 342 |
| 64 | 33,478 | 1,819 | 1,584 | 2,666 | 2,583 | 581 |
| 512 | 903,371 | 10,806 | 10,817 | 15,710 | 15,492 | 3,016 |

The implementation deliberately scans/copies member arrays: lookup and updates
are O(n), distinct bulk construction and naive unordered equality O(n²), initial
seq materialization O(n), and count/tails O(1), before nested structural costs.
The measurements are consistent with those choices, but GC, weak branding and
allocation costs prevent interpreting these few samples as a proof of asymptotic
behavior or a stable performance ratio. Map smoke queries used scalar keys;
these structural-member set measurements are not a like-for-like speed comparison.

The runtime source is 21,992 bytes; existing primitive `42` still emits 104 bytes
without the factory. The exact runtime SHA-256 is recorded because accumulated
uncommitted changes mean Git HEAD alone does not identify this implementation.

Reproduce from the repository root (writes this sample artifact):

```js
const fs=require('node:fs'),os=require('node:os'),crypto=require('node:crypto');
const source=fs.readFileSync('src/js_runtime.js','utf8'),rt=(0,eval)(source),c=rt.core;
const perf=require('node:perf_hooks').performance;
let sink=0;
function measure(f,n){for(let i=0;i<3;i++)for(let j=0;j<n;j++)sink+=f(j);const samples=[];for(let i=0;i<7;i++){const start=perf.now();for(let j=0;j<n;j++)sink+=f(j);samples.push((perf.now()-start)*1e6/n);}return {iterations:n,samples_ns:samples,median_ns:[...samples].sort((a,b)=>a-b)[3]};}
const rows=[];
for(const size of [8,64,512]){
 const items=Array.from({length:size},(_,i)=>c.vector(i));
 const s=rt.setFromArray(items), last=c.list(size-1), miss=c.vector(-1), added=c.vector(size);
 const row={size};
 row.build=measure(()=>c.count(rt.setFromArray(items)),1000);
 row.get_last=measure(()=>c.nth(c.get(s,last),0),30000);
 row.get_miss=measure(()=>c.get(s,miss,0),30000);
 row.conj_new=measure(()=>c.count(c.conj(s,added)),5000);
 row.disj_last=measure(()=>c.count(c.disj(s,last)),5000);
 row.seq_materialize=measure(()=>c.count(c.seq(s)),1000);
 if(c.count(s)!==size||c.get(s,last)!==items[size-1]||c.contains(s,added))throw Error('mutation');
 rows.push(row);
}
const output={date:'2026-10-09',node:process.version,cpu:os.cpus()[0].model,platform:os.platform(),arch:os.arch(),release:os.release(),runtime_bytes:Buffer.byteLength(source),runtime_sha256:crypto.createHash('sha256').update(source).digest('hex'),method:'Resident runtime; three full warmups and seven measured samples per operation; nanoseconds/op. Construction reuses an input array of immutable vector members; equivalent-list lookup hits last; missing/new queries are immutable vectors; updates start from a retained set. Queries are prebuilt. Includes structural hashing/equality.',rows,sink};
fs.writeFileSync('benchmarks/results/compiler-2026-10-09/sets-smoke/samples.json',JSON.stringify(output,null,2)+'\n');
console.log(JSON.stringify(rows.map(r=>({size:r.size,...Object.fromEntries(Object.entries(r).filter(([k])=>k!=='size').map(([k,v])=>[k,Math.round(v.median_ns)]))})),null,2));
```
