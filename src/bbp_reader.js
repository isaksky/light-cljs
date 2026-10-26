// The canonical bounded reader executes in the task's VM, so a user tag
// handler can park on a compiler helper without reentering Rust or replaying.
// Its only language values are native forms; inspection never exports Datum
// or borrows a legacy engine. Error cursors count the original UTF-16 units.
(() => {
  const kind = x => __rt.formKind(val(x));
  const inspect = x => {
    const k=kind(x);
    if(k==='symbol'||k==='keyword') {
      const namespace=__rt.formNamespace(val(x)), name=__rt.formName(val(x));
      return {kind:k,namespace,name,spelling:namespace===null?name:namespace+'/'+name};
    }
    if(k==='map'||k==='record') {
      const entries=[];
      for(const pair of arr(__rt.formPrintItems(val(x)))) entries.push(arr(pair));
      return {kind:'map',entries};
    }
    if(k==='vector'||k==='set'||k==='list'||k==='cons'||k==='lazy-seq')
      return {kind:k==='cons'||k==='lazy-seq'?'seq':k,items:arr(x)};
    return null;
  };
  const coreCells=core;
  coreCells.readerCurrentInputValue=null;
  // Keep hash storage (even after shrinking) while exposing legacy seq order.
  const readerMap=a=>__rt.formReaderCollection(literalMap(a));
  const readerSet=a=>__rt.formReaderCollection(set(a));
  const hashMap=core.hashMap, hashSet=core.hashSet, conj=core.conj, contains=core.contains;
  const first=core.first, next=core.next, meta=core.meta;
  const withMeta=(x,m)=>{
    if(!['symbol','list','vector','map','set','cons','lazy-seq','record'].includes(kind(x)))
      throw new Error('Invalid metadata target');
    return __rt.cljWithMeta(x,m);
  };
  const merge=core.merge;
  const isRecord=core.isRecord, into=core.into, str=core.str, nsName=core.nsName, toArray=arr;
  // Compiler literals and queues have no native form representation yet.
  const uuid=unsupported('reader UUID literal'), PersistentQueueValue={get EMPTY(){return unsupported('reader queue literal')();}};
  const readerJsLiterals={array:unsupported('reader JS literal'),object:unsupported('reader JS literal')};
  const readerCoreNames=new Set();
  /* READER */
  const reader=createReader({keyword:core.keyword,symbol:core.symbol,vectorFromArray:vector,
    listFromArray:list,mapFromArray:readerMap,setFromArray:readerSet,inspect,jsLiterals:readerJsLiterals,
    unwrap:val,hasOwn:(object,key)=>Object.getOwnPropertyDescriptor(object,key)!==undefined,
    callable:handler=>['keyword','symbol','vector','map','set'].includes(kind(handler))||typeof val(handler).call==='function',
    callReader:(handler,args)=>invoke(handler,...args),
    ednKind:x=>{const k=kind(x);return k==='record'?'map':['list','cons','lazy-seq'].includes(k)?'seq':k??(val(x) instanceof Date?'instant':null);},
    builtinTags:new Map([
      ['inst',unsupported('reader inst literal')],
      ['uuid',unsupported('reader UUID literal')],
      ['queue',unsupported('reader queue literal')],
      ['js',unsupported('reader JS literal')]
    ]),
    readBuiltin:tag=>unsupported('reader '+(tag==='uuid'?'UUID':tag==='js'?'JS':tag)+' literal')()});
  // Code-profile read-string also needs the compiler's namespace resolution
  // view. Leave that existing unsupported boundary explicit.
  for(const name of ['ednReadString','ednRead','readerReadString','readerRead',
    'readerStringReader','readerInputProvider','readerReadLine','readerParseTimestampFields',
    'readerParseTimestamp','readerInitialTagTable','readerRegisterTagParser','readerDeregisterTagParser',
    'readerRegisterDefaultTagParser','readerDeregisterDefaultTagParser','readerWithInput',
    'readerCurrentInput','readerClose']) core[name]=reader[name];
})();
