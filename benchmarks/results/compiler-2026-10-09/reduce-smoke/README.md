# Finite JavaScript reduction: resident sum smoke test

Measured 2026-10-09 on Apple M3 Ultra (Virtual), darwin 24.6.0
arm64, Node v24.18.0. Each source vector and callback is built before
timing. Only repeated scalar reductions over the same immutable vector are timed;
parsing, source generation, collection construction and runtime startup are excluded.
Compiler/startup measurements remain separate work. No speedup or performance
threshold is claimed. The run followed the completed reference checks.

Three warmups precede seven retained samples for each size/callback.
`samples.json` records all samples, iteration counts, environment, runtime bytes
and SHA-256; this identifies the accumulated uncommitted runtime precisely.
Values below are median nanoseconds per complete reduction.

| Elements | First-class core + | User callback |
| ---: | ---: | ---: |
| 1,024 | 12,535 | 12,583 |
| 16,384 | 219,816 | 221,628 |
| 150,000 | 2,246,990 | 2,239,199 |

The 150,000-element input returns 11,249,925,000. Independent runtime tests also
reduce 150,000-element lists, seq views and strings; the emitted-source test sums
150,000 ones. Traversal has bounded call depth and no per-step captured closure or
seq-view allocation. Map/set reduction may allocate one traversal snapshot.
Runtime source size is 24,090 bytes. Primitive `42` still emits
104 bytes and `(reduce + 0 [1 2 3])` executes as 6 through the compile CLI.

A separate Node `--expose-gc` probe created 256 ephemeral map inputs with host
payloads, threw the same sentinel object on each first reduction step, verified
unchanged map contents, then yielded and forced GC. All 256 payload WeakRefs
cleared (zero retained). Inspection also confirms that traversal storage is local
and branding uses weak maps; this observation is not a general GC timing promise.

Reproduce the retained timing artifact from the repository root:

```js
const fs=require('node:fs'),os=require('node:os'),crypto=require('node:crypto');
const source=fs.readFileSync('src/js_runtime.js','utf8'),rt=(0,eval)(source),c=rt.core;
const perf=require('node:perf_hooks').performance;
let sink=0;
function measure(f,n){for(let i=0;i<3;i++)for(let j=0;j<n;j++)sink+=f();const samples=[];for(let i=0;i<7;i++){const start=perf.now();for(let j=0;j<n;j++)sink+=f();samples.push((perf.now()-start)*1e6/n);}return {iterations:n,samples_ns:samples,median_ns:[...samples].sort((a,b)=>a-b)[3]};}
const rows=[];
for(const size of [1024,16384,150000]){
 const vector=rt.vectorFromArray(Array.from({length:size},(_,i)=>i));
 const expected=size*(size-1)/2, n=Math.max(10,Math.floor(2000000/size));
 if(c.reduce(c.add,0,vector)!==expected)throw Error('incorrect sum');
 const callback=(a,b)=>a+b;
 rows.push({size,core_add:measure(()=>c.reduce(c.add,0,vector),n),user_callback:measure(()=>c.reduce(callback,0,vector),n)});
 if(c.nth(vector,0)!==0||c.nth(vector,size-1)!==size-1)throw Error('input changed');
}
const output={date:'2026-10-09',node:process.version,cpu:os.cpus()[0].model,platform:os.platform(),arch:os.arch(),release:os.release(),runtime_bytes:Buffer.byteLength(source),runtime_sha256:crypto.createHash('sha256').update(source).digest('hex'),method:'Resident runtime; prebuilt immutable vector, scalar sum with explicit zero init. Three warmups and seven measured samples per size/callback; nanoseconds/reduction. Parsing, source generation, runtime startup and collection construction are excluded. No per-step closure creation.',rows,sink};
fs.writeFileSync('benchmarks/results/compiler-2026-10-09/reduce-smoke/samples.json',JSON.stringify(output,null,2)+'\n');
console.log(JSON.stringify(rows.map(r=>({size:r.size,core_add_ns:Math.round(r.core_add.median_ns),user_callback_ns:Math.round(r.user_callback.median_ns)})),null,2));
```
