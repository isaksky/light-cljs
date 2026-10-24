//! Resumable RegExp coercions and generic property access. Matching itself is
//! a bounded leaf (MAX_MATCH_STEPS), without JavaScript calls or GC edges.
use super::{Action, Input};
use crate::heap::Heap;
use crate::heap::gc::Trace;
use crate::ids::TaskId;
use crate::regexp::object::{self, key};
use crate::value::{JsString, Value};
use crate::vm::{
    builtin::Builtin,
    dispatch::{self, Fault},
    property,
};

#[derive(Debug)]
pub(super) enum State {
    Constructor {
        pattern: Value,
        flags: Value,
        original: Value,
        identity: bool,
        stage: u8,
    },
    Exec {
        this: Value,
        input: Value,
        text: JsString,
        stage: u8,
    },
    Test {
        this: Value,
        input: Value,
        stage: u8,
    },
    ToString {
        this: Value,
        source: JsString,
        stage: u8,
    },
    Flags {
        this: Value,
        out: String,
        next: usize,
    },
}
impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        match self {
            Self::Constructor {
                pattern,
                flags,
                original,
                ..
            } => {
                pattern.trace(visit);
                flags.trace(visit);
                original.trace(visit);
            }
            Self::Exec { this, input, .. } | Self::Test { this, input, .. } => {
                this.trace(visit);
                input.trace(visit);
            }
            Self::ToString { this, .. } | Self::Flags { this, .. } => this.trace(visit),
        }
    }
}

pub(super) fn start(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    this: &Value,
    args: &[Value],
    construct: bool,
) -> Result<Option<State>, Fault> {
    let arg = |i| args.get(i).cloned().unwrap_or(Value::Undefined);
    Ok(Some(match which {
        Builtin::RegExp => {
            let original = arg(0);
            let mut pattern = original.clone();
            let mut flags = arg(1);
            let mut identity = false;
            if let Some(c) = object::data(heap, task, &original)? {
                identity = !construct && matches!(flags, Value::Undefined);
                pattern = Value::String(c.source.clone());
                if matches!(flags, Value::Undefined) {
                    flags = Value::String(object::flags(c.flags));
                }
            }
            State::Constructor {
                pattern,
                flags,
                original,
                identity,
                stage: 0,
            }
        }
        Builtin::RegExpExec if !construct => {
            object::receiver(heap, task, this)?;
            State::Exec {
                this: this.clone(),
                input: arg(0),
                text: JsString::from(""),
                stage: 0,
            }
        }
        Builtin::RegExpTest if !construct => {
            require_object(this)?;
            State::Test {
                this: this.clone(),
                input: arg(0),
                stage: 0,
            }
        }
        Builtin::RegExpToString if !construct => {
            require_object(this)?;
            State::ToString {
                this: this.clone(),
                source: JsString::from(""),
                stage: 0,
            }
        }
        Builtin::RegExpFlags if !construct => {
            require_object(this)?;
            State::Flags {
                this: this.clone(),
                out: String::new(),
                next: 0,
            }
        }
        _ => return Ok(None),
    }))
}

pub(crate) fn getter(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    this: &Value,
) -> Result<Value, Fault> {
    let c = object::data(heap, task, this)?;
    let intrinsic = crate::realm::intrinsics(heap).ok_or(Fault::OutOfMemory("RegExp realm"))?;
    let prototype = matches!(this, Value::Object(id) if *id == intrinsic.regexp_prototype);
    match (which, c) {
        (Builtin::RegExpSource, Some(c)) => Ok(Value::String(object::source(&c.source)?)),
        (Builtin::RegExpSource, None) if prototype => Ok(Value::String(JsString::from("(?:)"))),
        (Builtin::RegExpFlag(f), Some(c)) => Ok(Value::Bool(object::flag(c.flags, f))),
        (Builtin::RegExpFlag(_), None) if prototype => Ok(Value::Undefined),
        _ => Err(Fault::TypeError(
            "RegExp getter requires a RegExp receiver".into(),
        )),
    }
}

fn require_object(this: &Value) -> Result<(), Fault> {
    if matches!(this, Value::Object(_)) {
        Ok(())
    } else {
        Err(Fault::TypeError("RegExp method requires an object".into()))
    }
}
fn read(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    name: &str,
) -> Result<Result<Value, Action>, Fault> {
    match property::read(heap, task, this, &key(name))?
        .unwrap_or(property::Read::Value(Value::Undefined))
    {
        property::Read::Value(v) => Ok(Ok(v)),
        property::Read::Getter(callee) => Ok(Err(Action::Call {
            callee,
            this: this.clone(),
            args: vec![],
        })),
    }
}
fn string(v: Value) -> Result<JsString, Fault> {
    match v {
        Value::String(s) => Ok(s),
        _ => Err(Fault::Internal("RegExp string conversion missing")),
    }
}
impl State {
    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        let mut v = match input {
            Input::Start => Value::Undefined,
            Input::CallResult(v) => v,
            _ => return Err(Fault::Internal("RegExp unexpected dependency")),
        };
        match self {
            Self::Constructor {
                pattern,
                flags,
                original,
                identity,
                stage,
            } => loop {
                match *stage {
                    0 => {
                        *stage = 1;
                        if *identity {
                            match read(heap, task, original, "constructor")? {
                                Ok(value) => v = value,
                                Err(a) => return Ok(a),
                            }
                        }
                    }
                    1 => {
                        if *identity {
                            let constructor = crate::realm::intrinsics(heap)
                                .ok_or(Fault::OutOfMemory("RegExp realm"))?
                                .global(crate::realm::Global::RegExp);
                            if crate::atom::same_value(&v, &constructor) {
                                return Ok(Action::Return(original.clone()));
                            }
                        }
                        *stage = 2;
                        if matches!(pattern, Value::Undefined) {
                            v = Value::String(JsString::from(""));
                        } else {
                            return Ok(Action::ToString(pattern.clone()));
                        }
                    }
                    2 => {
                        *pattern = v;
                        *stage = 3;
                        if matches!(flags, Value::Undefined) {
                            v = Value::String(JsString::from(""));
                        } else {
                            return Ok(Action::ToString(flags.clone()));
                        }
                    }
                    3 => {
                        let source = string(pattern.clone())?;
                        let flags = string(v)?;
                        let c = object::compile(&source, &flags).map_err(|e| e.fault())?;
                        return object::allocate(heap, task, c).map(Action::Return);
                    }
                    _ => return Err(Fault::Internal("RegExp constructor stage")),
                }
            },
            Self::Exec {
                this,
                input,
                text,
                stage,
            } => loop {
                match *stage {
                    0 => {
                        *stage = 1;
                        return Ok(Action::ToString(input.clone()));
                    }
                    1 => {
                        *text = string(v)?;
                        *stage = 2;
                        match read(heap, task, this, "lastIndex")? {
                            Ok(value) => v = value,
                            Err(a) => return Ok(a),
                        }
                    }
                    2 => {
                        *stage = 3;
                        return Ok(Action::ToNumber(v));
                    }
                    3 => {
                        return object::exec(heap, task, this, text, super::to_length(&v)?)
                            .map(Action::Return);
                    }
                    _ => return Err(Fault::Internal("RegExp exec stage")),
                }
            },
            Self::Test { this, input, stage } => {
                loop {
                    match *stage {
                        0 => {
                            *stage = 1;
                            return Ok(Action::ToString(input.clone()));
                        }
                        1 => {
                            *input = v;
                            *stage = 2;
                            match read(heap, task, this, "exec")? {
                                Ok(value) => v = value,
                                Err(a) => return Ok(a),
                            }
                        }
                        2 => {
                            *stage = 3;
                            let callee = if dispatch::callee(heap, &v).is_ok() {
                                v
                            } else {
                                object::receiver(heap, task, this)?;
                                // Fall back to the intrinsic exec when an own exec isn't callable.
                                let proto = Value::Object(
                                    crate::realm::intrinsics(heap)
                                        .ok_or(Fault::OutOfMemory("RegExp realm"))?
                                        .regexp_prototype,
                                );
                                property::get(heap, task, &proto, &key("exec"))?
                            };
                            return Ok(Action::Call {
                                callee,
                                this: this.clone(),
                                args: vec![input.clone()],
                            });
                        }
                        3 => {
                            return match v {
                                Value::Null => Ok(Action::Return(Value::Bool(false))),
                                Value::Object(_) => Ok(Action::Return(Value::Bool(true))),
                                _ => Err(Fault::TypeError(
                                    "RegExp exec must return an object or null".into(),
                                )),
                            };
                        }
                        _ => return Err(Fault::Internal("RegExp test stage")),
                    }
                }
            }
            Self::ToString {
                this,
                source,
                stage,
            } => loop {
                match *stage {
                    0 | 3 => {
                        let name = if *stage == 0 { "source" } else { "flags" };
                        *stage += 1;
                        match read(heap, task, this, name)? {
                            Ok(value) => v = value,
                            Err(a) => return Ok(a),
                        }
                    }
                    1 | 4 => {
                        *stage += 1;
                        return Ok(Action::ToString(v));
                    }
                    2 => {
                        *source = string(v)?;
                        v = Value::Undefined;
                        *stage = 3;
                    }
                    5 => {
                        let flags = string(v)?;
                        let s = JsString::from("/")
                            .concat(source)
                            .and_then(|s| s.concat(&JsString::from("/")))
                            .and_then(|s| s.concat(&flags))
                            .ok_or(Fault::StringTooLong)?;
                        return Ok(Action::Return(Value::String(s)));
                    }
                    _ => return Err(Fault::Internal("RegExp toString stage")),
                }
            },
            Self::Flags { this, out, next } => loop {
                if *next > 0 && v.truthy() {
                    out.push(object::FLAG_NAMES[*next - 1].1);
                }
                if *next == object::FLAG_NAMES.len() {
                    return Ok(Action::Return(Value::String(JsString::from(out.as_str()))));
                }
                let name = object::FLAG_NAMES[*next].0;
                *next += 1;
                match read(heap, task, this, name)? {
                    Ok(value) => v = value,
                    Err(a) => return Ok(a),
                }
            },
        }
    }
}
