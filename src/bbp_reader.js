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
  // JS reader literals use the same explicit descriptors as source forms.
  // The other EDN value constructors retain their separate service boundary.
  const uuid=unsupported('reader UUID literal'), PersistentQueueValue={get EMPTY(){return unsupported('reader queue literal')();}};
  const readerJsArray=items=>__rt.formCompilerLiteral('js-array',vector(items));
  const readerJsObject=pairs=>__rt.formCompilerLiteral('js-object',constantMap(literalObjectPairs(pairs)));
  // This entry can live in cljs.reader's shared tag-table atom: capture
  // pure constructors and inspection, never createReader's mutable adapter.
  const readerJsTag=value=>{
    const fail=message=>{
      const error=new Error(message+' at <tag-parser>:1:1');
      error.source='<tag-parser>';error.line=1;error.column=1;error.offset=0;error.category='reader';throw error;
    };
    const data=inspect(value);
    if(data?.kind==='vector')return readerJsArray(data.items);
    if(data?.kind==='map'){
      const pairs=[];
      for(const [key,value] of data.entries){
        const name=inspect(key);
        if(typeof val(key)!=='string'&&!(name?.kind==='keyword'&&name.namespace===null))fail('Invalid JS object literal key');
        pairs.push(typeof val(key)==='string'?key:name.name,value);
      }
      return readerJsObject(pairs);
    }
    return fail('Invalid JS literal');
  };
  const readerJsLiterals={array:readerJsArray,object:readerJsObject};
  // Publish one pure table, so cljs.reader can retain its handlers in a
  // shared atom and later tasks still recognize them as builtin readers.
  const candidateTags=__rt.formMap('inst',unsupported('reader inst literal'),
    'uuid',unsupported('reader UUID literal'),'queue',unsupported('reader queue literal'),'js',readerJsTag);
  const previousTags=__rt.atomCas(readerTagsCell,null,candidateTags);
  const sharedTags=previousTags===null?candidateTags:previousTags;
  const readerCoreNames=new Set();
  /* READER */
  const reader=createReader({keyword:core.keyword,symbol:core.symbol,vectorFromArray:vector,
    listFromArray:list,mapFromArray:readerMap,setFromArray:readerSet,inspect,jsLiterals:readerJsLiterals,
    unwrap:val,hasOwn:(object,key)=>Object.getOwnPropertyDescriptor(object,key)!==undefined,
    callable:handler=>['keyword','symbol','vector','map','set'].includes(kind(handler))||typeof val(handler).call==='function',
    callReader:(handler,args)=>invoke(handler,...args),
    ednKind:x=>{const k=kind(x);return k==='record'?'map':['list','cons','lazy-seq'].includes(k)?'seq':k??(val(x) instanceof Date?'instant':null);},
    builtinTags:new Map([
      ['inst',__rt.formGet(sharedTags,'inst',null)],
      ['uuid',__rt.formGet(sharedTags,'uuid',null)],
      ['queue',__rt.formGet(sharedTags,'queue',null)],
      ['js',__rt.formGet(sharedTags,'js',null)]
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
