//! Bounded serial macro execution. The mutable runtime and function registry
//! belong to one task; only native forms cross its dependency boundary.
use crate::{CollectionKind as C, compiler_data::{Datum, DatumKind as D, ExpansionOrigin, Form, Name}, compiler_evaluator::{CompilerEvaluator, StringService, UnitService}, evaluator::{EvalError, EvaluatorOptions, ExecutionControl, MacroEvaluator, NEXT_SESSION, ValueId}, form_bridge::ConversionLimits};
use big_boy_pants_js::{Budget, Completion, FormKind, JsString, Local, Primitive, Provenance, Rooted, RunOutcome, Runtime, Scope, Task, ValueView};
use std::{cell::{Cell, RefCell}, collections::HashMap, rc::Rc, sync::atomic::Ordering};

fn error(e: impl std::fmt::Display) -> EvalError { EvalError(format!("big-boy-pants-js: {e}")) }
fn string(s: &[u16]) -> Result<JsString, EvalError> { JsString::from_code_units(s.to_vec()).ok_or_else(|| error("string limit exceeded")) }

#[derive(Clone, Copy, Debug, Default)]
pub struct BridgeStats { pub imported: usize, pub exported: usize, pub reused: usize, pub nanos: u128 }
#[derive(Default)]
struct Bridge {
    // Retaining the Datum prevents allocator address reuse from aliasing a key.
    imports: HashMap<usize, (Datum, Rooted)>,
    forms: HashMap<usize, Form>,
    stats: BridgeStats,
    tokens: Vec<usize>,
}
impl Bridge {
    fn import<'s>(&mut self, s: &Scope<'s>, d: &Datum, depth: usize, remaining: &mut usize) -> Result<Local<'s>, EvalError> {
        if let Some((_, value)) = self.imports.get(&d.identity()) { return s.local(value).map_err(error); }
        if depth == 0 || *remaining == 0 { return Err(error("form conversion limit exceeded")); }
        *remaining -= 1;
        let mut value = match d.kind() {
            D::Nil => s.primitive(Primitive::Null),
            D::Boolean(v) => s.primitive(Primitive::Bool(*v)),
            D::Integer(v) => s.primitive(Primitive::Number(*v as f64)),
            D::Double(v) => s.primitive(Primitive::Number(*v)),
            D::Ratio{numerator,denominator} => s.primitive(Primitive::Number(*numerator as f64 / *denominator as f64)),
            D::Character(v) => s.primitive(Primitive::String(string(&[*v])?)),
            D::String(v) => s.primitive(Primitive::String(string(v)?)),
            D::Symbol(n) | D::Keyword(n) => {
                let ns = n.namespace().map(string).transpose()?;
                let name = string(n.name())?;
                if matches!(d.kind(), D::Symbol(_)) { s.new_symbol(ns,name) } else { s.new_keyword(ns,name) }.map_err(error)?
            },
            D::Collection{kind,children} => {
                let values = children.iter().map(|v| self.import(s,v,depth-1,remaining)).collect::<Result<Vec<_>,_>>()?;
                match kind {
                    C::List | C::AnonymousFn => s.new_list(&values),
                    C::Vector => s.new_form_vector(&values),
                    C::Map => s.new_form_map(&values.chunks_exact(2).map(|p|(p[0],p[1])).collect::<Vec<_>>()),
                    C::Set => return Err(error("unsupported operation: native set forms")),
                }.map_err(error)?
            },
            D::CompilerLiteral{..} => return Err(error("unsupported operation: compiler literal")),
        };
        if let Some(meta) = d.metadata() { let m = self.import(s,&meta,depth-1,remaining)?; value = s.form_with_meta(value,Some(m)).map_err(error)?; }
        // Every input node is an occurrence, including numbers and strings.
        // Returning it preserves exact integer/ratio/character representation.
        let token=u32::try_from(self.tokens.len()).map_err(error)?;
        self.tokens.push(d.identity());
        value = s.new_occurrence(value, Provenance{source:token,line:0,column:0}).map_err(error)?;
        self.imports.insert(d.identity(),(d.clone(),s.persist(value)));
        self.stats.imported += 1;
        Ok(value)
    }
    fn export<'s>(&mut self,s:&Scope<'s>,v:Local<'s>,depth:usize,remaining:&mut usize)->Result<Datum,EvalError>{
        if let Ok(p) = s.provenance(v) {
            if let Some((data,root)) = self.tokens.get(p.source as usize).and_then(|key|self.imports.get(key)) {
                if s.same_object(v,s.local(root).map_err(error)?) { self.stats.reused += 1; return Ok(data.clone()); }
            }
        }
        if depth==0 || *remaining==0 { return Err(error("form conversion limit exceeded")); }
        *remaining-=1;
        let v=s.form_value(v);
        let kind=match s.form_kind(v) {
            Some(FormKind::Keyword|FormKind::Symbol) => {
                let name=s.form_name(v).map_err(error)?;
                let n=Name::from_utf16(name.namespace.as_ref().map(JsString::code_units),name.name.code_units());
                if s.form_kind(v)==Some(FormKind::Keyword){D::Keyword(n)}else{D::Symbol(n)}
            },
            Some(k) => {
                let values=if k==FormKind::Map { s.form_entries(v).map_err(error)?.into_iter().flat_map(|(k,v)|[k,v]).collect() } else {s.form_items(v).map_err(error)?};
                let children=values.into_iter().map(|x|self.export(s,x,depth-1,remaining)).collect::<Result<Vec<_>,_>>()?;
                D::Collection{kind:match k{FormKind::List=>C::List,FormKind::Vector=>C::Vector,_=>C::Map},children:children.into()}
            },
            None=>match s.view(v){
                ValueView::Null|ValueView::Undefined=>D::Nil,
                ValueView::Bool(b)=>D::Boolean(b), ValueView::Number(n)=>D::Double(n),
                ValueView::String=>D::String(s.string(v).unwrap().code_units().into()),
                _=>return Err(error("unsupported operation: macro returned a non-form value")),
            }
        };
        let mut data=Datum::new(kind);
        if let Some(m)=s.form_meta(v){data=data.with_metadata(self.export(s,m,depth-1,remaining)?);}
        self.stats.exported+=1;
        Ok(data)
    }
    fn remember(&mut self,form:&Form){self.forms.insert(form.data().identity(),form.clone());for f in form.children(){self.remember(&f);}if let Some(f)=form.metadata(){self.remember(&f);}}
}

pub struct BbpEvaluator {
    runtime: Runtime, task: Task, control: ExecutionControl, session:u64,
    next:Cell<u64>, bridge:RefCell<Bridge>, values:RefCell<HashMap<u64,Rooted>>,
}
impl BbpEvaluator {
    pub fn new(options:EvaluatorOptions)->Result<Self,EvalError>{
        let runtime=Runtime::new();
        let executable=big_boy_pants_js::frontend::compile_script("native macro runtime",include_str!("bbp_runtime.js")).map_err(error)?;
        let task=runtime.spawn(&executable,&[]).map_err(error)?;
        let this=Self{runtime,task,control:ExecutionControl::new(&options)?,session:NEXT_SESSION.fetch_add(1,Ordering::Relaxed),next:Cell::new(1),bridge:RefCell::new(Bridge::default()),values:RefCell::new(HashMap::new())};
        this.run()?;
        Ok(this)
    }
    pub fn bridge_stats(&self)->BridgeStats{self.bridge.borrow().stats}
    fn run(&self)->Result<(),EvalError>{
        loop {
            self.control.poll()?;
            let outcome=self.task.run_with(Budget::instructions(10_000),&mut RejectHost).map_err(error)?;
            match outcome {
                RunOutcome::Waiting(_)=>return Ok(()), RunOutcome::Yielded=>{},
                RunOutcome::Failed(e)=>return Err(error(e)), other=>return Err(error(format!("unexpected session outcome: {other:?}"))),
            }
        }
    }
    fn id(&self)->ValueId{let index=self.next.get();self.next.set(index+1);ValueId{session:self.session,index}}
    fn check(&self,id:ValueId)->Result<(),EvalError>{if id.session!=self.session||!self.values.borrow().contains_key(&id.index){Err(error("invalid value handle"))}else{Ok(())}}
    fn request(&self,op:u32,id:ValueId,input:Rooted)->Result<(),EvalError>{
        let request=self.task.take_producer_requests().pop().ok_or_else(||error("macro task has no request boundary"))?;
        let message=self.runtime.scope(|s|{let op=s.primitive(Primitive::Number(op as f64));let id=s.primitive(Primitive::Number(id.index as f64));s.persist(s.new_vector(&[op,id,s.local(&input).unwrap()]).unwrap())});
        let _wakeups = self.runtime.complete(request.key(),request.producer(),Completion::Ready(&message)).map_err(error)?;
        self.run()?;
        let failed=self.task.global_value("__lightFailure").map_err(error)?.ok_or_else(||error("missing result boundary"))?;
        if let Some(message)=failed.string(){return Err(error(message.to_string_lossy()));}
        if op!=3 {
            let value=self.task.global_value("__lightResult").map_err(error)?.ok_or_else(||error("missing macro result"))?;
            self.values.borrow_mut().insert(id.index,value);
        }
        Ok(())
    }
    fn evaluate(&self,source:&str)->Result<ValueId,EvalError>{let id=self.id();self.request(0,id,self.runtime.root(Primitive::string(source)))?;Ok(id)}
    fn import(&self,data:&Datum,limits:ConversionLimits)->Result<ValueId,EvalError>{
        let started=std::time::Instant::now();
        let value=self.runtime.scope(|s|{let mut b=self.bridge.borrow_mut();let v=b.import(s,data,limits.depth,&mut limits.nodes.clone())?;Ok::<_,EvalError>(s.persist(v))})?;
        self.bridge.borrow_mut().stats.nanos+=started.elapsed().as_nanos();
        let id=self.id();self.request(1,id,value)?;Ok(id)
    }
}
struct RejectHost;
impl big_boy_pants_js::Host for RejectHost{fn effect(&mut self,call:big_boy_pants_js::HostCall)->Result<Primitive,String>{Err(format!("unsupported operation: host effect {}",call.op))}}
impl MacroEvaluator for BbpEvaluator {
    fn control(&self)->ExecutionControl{self.control.clone()}
    fn heap(&self)->usize{self.session as usize}
    fn evaluate_unit(&self,source:&str)->Result<ValueId,EvalError>{self.evaluate(&format!("({source})(globalThis.__lightNativeRuntime)"))}
    fn call(&self,function:ValueId,arguments:&[ValueId])->Result<ValueId,EvalError>{
        self.check(function)?;for a in arguments{self.check(*a)?;}
        let input=self.runtime.scope(|s|{let values=std::iter::once(function).chain(arguments.iter().copied()).map(|v|s.primitive(Primitive::Number(v.index as f64))).collect::<Vec<_>>();s.persist(s.new_vector(&values).unwrap())});
        let id=self.id();self.request(2,id,input)?;Ok(id)
    }
    fn import_form(&self,form:&Form,limits:ConversionLimits)->Result<ValueId,EvalError>{self.bridge.borrow_mut().remember(form);self.import(&form.data(),limits)}
    fn import_data(&self,data:&Datum,limits:ConversionLimits)->Result<ValueId,EvalError>{self.import(data,limits)}
    fn export_form(&self,id:ValueId,call:&Form,origin:ExpansionOrigin,limits:ConversionLimits)->Result<Form,EvalError>{
        self.check(id)?;let started=std::time::Instant::now();
        let data=self.runtime.scope(|s|{let v=self.values.borrow().get(&id.index).unwrap().clone();self.bridge.borrow_mut().export(s,s.local(&v).map_err(error)?,limits.depth,&mut limits.nodes.clone())})?;
        let mut bridge=self.bridge.borrow_mut();bridge.stats.nanos+=started.elapsed().as_nanos();
        Ok(Form::from_bridge_expansion(data,call,origin,&bridge.forms))
    }
    fn release(&self,id:ValueId)->Result<(),EvalError>{self.check(id)?;self.request(3,id,self.runtime.root(Primitive::Null))?;self.values.borrow_mut().remove(&id.index);Ok(())}
}
impl CompilerEvaluator for BbpEvaluator {
    fn native_forms(&self)->bool{true}
    fn evaluate_js(&self,expression:&str)->Result<ValueId,EvalError>{self.evaluate(expression)}
    fn install_compiler_service(self:Rc<Self>,name:&str,_callback:UnitService)->Result<(),EvalError>{self.install_unsupported(name)}
    fn install_string_service(&self,name:&str,_callback:StringService)->Result<(),EvalError>{self.install_unsupported(name)}
    fn value_roots(&self)->Result<usize,EvalError>{Ok(self.values.borrow().len()+self.bridge.borrow().imports.len())}
    fn collect_garbage(&self){let _=self.runtime.collect();}
    #[cfg(all(test,light_v8))]
    fn number(&self,id:ValueId)->Result<f64,EvalError>{self.check(id)?;match self.values.borrow()[&id.index].view(){ValueView::Number(n)=>Ok(n),_=>Err(error("not a number"))}}
}
impl BbpEvaluator {
    fn install_unsupported(&self,name:&str)->Result<(),EvalError>{
        // Compiler-owned names only, no user source is interpolated here.
        if !name.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'){return Err(error("invalid service name"));}
        let value=self.evaluate(&format!("globalThis.{name}=function(){{throw new Error('unsupported operation: {name} requires deferred compiler services');}}"))?;self.release(value)
    }
}
