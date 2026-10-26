// First-party synthetic ECMAScript Proxy [[Get]] regressions.
// Run unchanged in Node/V8 and big-boy-pants-js.
function assert(v, label) {if (!v) throw new Error(label);}
function typeError(f, label) {let e; try {f();} catch (x) {e=x;} assert(e instanceof TypeError, label);}
{
    let target={x:1}, receiver={}, handler={get(t,k,r) {
        assert(this===handler && t===target && arguments.length===3, 'trap arguments');
        assert(r===receiver, 'explicit receiver'); return k;
    }}, p=new Proxy(target,handler);
    assert(Reflect.get(p,'x',receiver)==='x', 'Reflect.get');
    let s=Symbol('key'); assert(Reflect.get(p,s,receiver)===s, 'symbol key');
    assert(Reflect.get(p,12,receiver)==='12', 'index key');
    assert(Reflect.get(p,'\ud800',receiver)==='\ud800', 'UTF-16 key');
    let child=Object.create(p); handler.get=(t,k,r)=>r;
    assert(p.x===p && child.x===child, 'inherited receiver');
    assert(Reflect.get(p,'x',null)===null && Reflect.get(p,'x',undefined)===undefined, 'primitive receivers');
    assert(Reflect.get(new Proxy(p,{}),'x',receiver)===receiver, 'nested forwarding receiver');
    let seen, base=new Proxy({get x(){seen=this; return 7;}},{}), obj=Object.create(base);
    assert(obj.x===7 && seen===obj, 'forwarded target getter');
    assert(new Proxy({x:3},{get:null}).x===3 && new Proxy({x:4},{get:undefined}).x===4, 'absent traps');
    let fn=function(){return this;};
    assert(new Proxy({m:fn},{get(t,k){return t[k];}}).m() instanceof Object, 'method call');
    let superBase=new Proxy({get x(){return this.n;}},{});
    let home={__proto__:superBase,m(){return super.x;}};
    assert(home.m.call({n:9})===9, 'super receiver');
}
{
    let log='', target={x:1}, handler={get get(){log+='g';return function(t,k,r){log+='t';return 4;};}};
    let p=new Proxy(target,handler), key={[Symbol.toPrimitive](hint){log+=hint;return 'x';}};
    assert(p[key]===4 && log==='stringgt', 'conversion then trap getter');
    log=''; assert(Reflect.get(p,key)===4 && log==='stringgt', 'Reflect key conversion');
    log=''; typeError(()=>Reflect.get(1,key),'invalid target'); assert(log==='', 'validate target first');
    let inherited=Object.create({get(t,k){return 5;}}); assert(new Proxy({},inherited).x===5,'inherited trap');
    let h=new Proxy({}, {get(t,k){assert(k==='get','handler trap key'); return ()=>6;}});
    assert(new Proxy({},h).x===6,'Proxy handler');
    for (let v of [1,false,'get',{}]) typeError(()=>new Proxy({},{get:v}).x,'noncallable trap');
    assert(new Proxy({},{get:new Proxy(function(){return 8;},{})}).x===8,'callable Proxy trap');
    for (let h of [{get get(){throw 42;}},{get(){throw 42;}}]) {
        let e; try {new Proxy({},h).x;} catch(x){e=x;} assert(e===42,'trap exception');
    }
}
{
    for (let value of [0,-0,NaN,null,undefined,{},Symbol('fixed')]) {
        let t={};Object.defineProperty(t,'x',{value});
        assert(Object.is(new Proxy(t,{get(){return value;}}).x,value),'SameValue permitted');
        typeError(()=>new Proxy(t,{get(){return value===0 ? (Object.is(value,-0)?0:-0) : 17;}}).x,'frozen data invariant');
    }
    let t={};Object.defineProperty(t,'x',{set(v){}});
    assert(new Proxy(t,{get(){return undefined;}}).x===undefined,'getterless target');
    typeError(()=>new Proxy(t,{get(){return 1;}}).x,'getterless invariant');
    let calls=0, accessor={};Object.defineProperty(accessor,'x',{get(){calls++;throw 99;}});
    assert(new Proxy(accessor,{get(){return 3;}}).x===3 && calls===0,'target getter never invoked');
    assert(new Proxy(Object.preventExtensions({}),{get(){return 4;}}).x===4,'nonextensible target allows virtual read');
    let mutable={x:1};
    typeError(()=>new Proxy(mutable,{get(t){Object.defineProperty(t,'x',{value:2,writable:false,configurable:false});return 1;}}).x,'invariants after mutation');
    let log='', inner=new Proxy({x:1},{getOwnPropertyDescriptor(t,k){log+='d';return Object.getOwnPropertyDescriptor(t,k);},isExtensible(){throw 99;}});
    assert(new Proxy(inner,{get(){log+='g';return 9;}}).x===9 && log==='gd','descriptor only invariant');
    let e;try{new Proxy(new Proxy({},{getOwnPropertyDescriptor(){throw 42;}}),{get(){return 1;}}).x;}catch(x){e=x;}
    assert(e===42,'target descriptor exception');
}
{
    let log='', p=new Proxy([1,2],{get(t,k,r){log+=String(k)+',';return Reflect.get(t,k,r);}});
    assert(Array.prototype.map.call(p,x=>x*2).join(',')==='2,4' && log==='length,constructor,0,1,','array internal reads');
    let d=new Proxy({value:7,configurable:true},{get(t,k,r){return Reflect.get(t,k,r);}});
    assert(Object.defineProperty({},'x',d).x===7,'descriptor internal reads');
    assert(new Error('x',new Proxy({cause:42},{get(t,k){return t[k];}})).cause===42,'Error cause read');
    assert(JSON.stringify(new Proxy({x:1},{get(t,k){return t[k];}}))==='{"x":1}','JSON reads');
    let a=[...new Proxy([1,2],{get(t,k,r){return Reflect.get(t,k,r);}})];assert(a.join(',')==='1,2','iterator reads');
    let rejected=Promise.resolve(new Proxy({},{get(){throw 42;}}));
    rejected.then(()=>{throw 'unexpected fulfillment';}, reason=>assert(reason===42,'promise rejection reason'));
}
true;
{
    let target=function(){throw 'target ran';}, seen=[], handler={apply(t,receiver,args){
        assert(this===handler && t===target && arguments.length===3,'apply arguments');
        assert(Array.isArray(args) && Object.getPrototypeOf(args)===Array.prototype,'argument array');
        seen.push(args);return [receiver,args.join(',')];
    }},p=new Proxy(target,handler),r={};
    let a=Reflect.apply(p,r,[1,2]),b=p.call(r,3),c=p.apply(r,[4,5]),d=p.bind(r,6)(7);
    assert(a[0]===r && a[1]==='1,2' && b[1]==='3' && c[1]==='4,5' && d[1]==='6,7','apply through forwarding');
    assert(seen.length===4 && seen[0]!==seen[1],'fresh argument arrays');
    let f=function(a){'use strict';return [this,a];};
    for(let h of [{},{apply:null},{apply:undefined}]) {
        let q=new Proxy(f,h),v=q(8);assert(v[0]===undefined && v[1]===8,'absent apply forwarding');
    }
    let n=0, q=new Proxy(new Proxy(f,{apply(t,r,args){n++;return Reflect.apply(t,r,args);}}),{});
    assert(q.call(r,9)[0]===r && n===1,'nested apply forwarding');
    let getter=0,g=new Proxy(f,{get apply(){getter++;return (t,r,args)=>args[0];}});
    assert(g(10)===10 && getter===1,'apply getter');
    let ph=new Proxy({}, {get(t,k){assert(k==='apply','apply Proxy handler');return (t,r,args)=>args[0];}});
    assert(new Proxy(f,ph)(11)===11,'apply Proxy handler lookup');
    assert(new Proxy(f,{apply:new Proxy(function(t,r,a){return a[0];},{})})(12)===12,'callable apply trap');
    for(let v of [false,1,{},'apply']) typeError(()=>new Proxy(f,{apply:v})(),'noncallable apply trap');
    for(let h of [{get apply(){throw 42;}},{apply(){throw 42;}}]) {
        let e;try{new Proxy(f,h)();}catch(x){e=x;}assert(e===42,'apply exception');
    }
    let touched=0, invalid=new Proxy({}, {get apply(){touched++;return ()=>1;}});
    typeError(()=>invalid(),'noncallable target');assert(touched===0,'classification never reads apply');
    let cls=class {};assert(new Proxy(cls,{apply(){return 13;}})()===13,'class Proxy apply trap');
    assert([1,2].map(new Proxy(x=>x,{apply(t,r,a){return a[0]*2;}})).join(',')==='2,4','native callback apply');
}
true;
