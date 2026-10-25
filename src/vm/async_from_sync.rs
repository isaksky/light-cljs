//! Private CreateAsyncFromSyncIterator wrappers. Their methods are ordinary
//! async VM activations with captured protocol records, not Promise chains or
//! Rust stack continuations. GetAsyncIterator alone creates these wrappers.
use super::dispatch::{self, Fault};
use super::generator::ResumeKind;
use crate::heap::{Heap, HeapObject};
use crate::ids::TaskId;
use crate::object::{OrdinaryObject, PropertyFlags};
use crate::value::{Primitive, Value};
use crate::vm::{Assembler, Executable, Opcode, ThrowKind};

fn method_call(a: &mut Assembler, argc: u8) {
    a.op(Opcode::CallMethod).raw(argc).raw(0);
}
fn nullish(a: &mut Assembler, slot: u16, target: crate::vm::asm::Label) {
    for value in [Primitive::Null, Primitive::Undefined] {
        a.get_local(slot)
            .constant(value)
            .op(Opcode::StrictEq)
            .op(Opcode::Not)
            .jump_if_false(target);
    }
}
fn close(a: &mut Assembler) {
    let done = a.label();
    a.get_capture(0)
        .constant(Primitive::String("return".into()))
        .op(Opcode::GetElem)
        .set_local(1);
    nullish(a, 1, done);
    a.get_capture(0).get_local(1);
    method_call(a, 0);
    a.op(Opcode::RequireObject).op(Opcode::Pop).bind(done);
}
fn result(a: &mut Assembler, value: u16, done: u16) {
    a.op(Opcode::NewObjectLiteral)
        .constant(Primitive::String("value".into()))
        .get_local(value)
        .op(Opcode::DefineElem)
        .constant(Primitive::String("done".into()))
        .get_local(done)
        .op(Opcode::DefineElem)
        .op(Opcode::Return);
}
fn driver(kind: ResumeKind) -> Executable {
    let mut a = Assembler::new();
    a.captures(2).op(Opcode::AsyncStart);
    if kind == ResumeKind::Next {
        a.get_capture(1).set_local(1);
    } else {
        let found = a.label();
        let missing = a.label();
        a.get_capture(0)
            .constant(Primitive::String(
                if kind == ResumeKind::Return {
                    "return"
                } else {
                    "throw"
                }
                .into(),
            ))
            .op(Opcode::GetElem)
            .set_local(1);
        nullish(&mut a, 1, missing);
        a.jump(found).bind(missing);
        if kind == ResumeKind::Return {
            a.constant(Primitive::Bool(true)).set_local(3);
            result(&mut a, 0, 3);
        } else {
            close(&mut a);
            a.op(Opcode::ThrowError)
                .raw(ThrowKind::YieldStarMissingThrow as u8)
                .raw(0)
                .op(Opcode::Return);
        }
        a.bind(found);
    }
    a.get_capture(0).get_local(1).get_local(0);
    method_call(&mut a, 1);
    a.op(Opcode::RequireObject)
        .set_local(2)
        .get_local(2)
        .constant(Primitive::String("done".into()))
        .op(Opcode::GetElem)
        .op(Opcode::Not)
        .op(Opcode::Not)
        .set_local(3)
        .get_local(2)
        .constant(Primitive::String("value".into()))
        .op(Opcode::GetElem);
    if kind == ResumeKind::Return {
        a.op(Opcode::Await).set_local(4);
        result(&mut a, 4, 3);
    } else {
        let rejected = a.label();
        a.set_local(4)
            .push_catch(rejected)
            .get_local(4)
            .op(Opcode::Await)
            .op(Opcode::PopCatch)
            .set_local(4);
        result(&mut a, 4, 3);
        a.bind(rejected).set_local(4);
        let closing = a.label();
        let throwing = a.label();
        a.get_local(3)
            .jump_if_false(closing)
            .jump(throwing)
            .bind(closing);
        let ignore_close_error = a.label();
        a.push_catch(ignore_close_error);
        close(&mut a);
        a.op(Opcode::PopCatch)
            .jump(throwing)
            .bind(ignore_close_error)
            .op(Opcode::Pop);
        a.bind(throwing).get_local(4).op(Opcode::Throw);
    }
    a.finish("<AsyncFromSyncIterator>", 1, 5)
        .expect("async-from-sync driver")
}

pub(crate) fn create(
    heap: &Heap,
    task: TaskId,
    iterator: Value,
    next: Value,
) -> Result<(Value, Value), Fault> {
    static DRIVERS: std::sync::OnceLock<[Executable; 3]> = std::sync::OnceLock::new();
    let drivers = DRIVERS.get_or_init(|| {
        [
            driver(ResumeKind::Next),
            driver(ResumeKind::Return),
            driver(ResumeKind::Throw),
        ]
    });
    let realm = crate::realm::intrinsics(heap).ok_or(Fault::OutOfMemory("async iterator realm"))?;
    let mut object = OrdinaryObject::with_prototype(task, realm.async_iterator_prototype);
    let mut next_method = Value::Undefined;
    for (name, code) in ["next", "return", "throw"].into_iter().zip(drivers) {
        let method = dispatch::new_function(
            heap,
            code.0.clone(),
            Box::new([iterator.clone(), next.clone()]),
            task,
            None,
        )?;
        if name == "next" {
            next_method = method.clone();
        }
        let key = heap
            .keys()
            .intern(&name.into())
            .ok_or(Fault::OutOfMemory("async iterator method"))?;
        object.define_with(key, method, PropertyFlags::METHOD);
    }
    let id = heap
        .alloc(HeapObject::Plain(object))
        .ok_or(Fault::OutOfMemory("async-from-sync iterator"))?;
    Ok((Value::Object(id), next_method))
}
