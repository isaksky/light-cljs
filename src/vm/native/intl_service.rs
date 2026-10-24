//! Shared GetOption stages: the caller stores its position before returning a
//! getter call or coercion to the trampoline. No Rust frame survives JavaScript.
use super::*;

pub(super) enum OptionKind {
    String,
    Boolean,
    Number,
}

pub(super) fn read(
    heap: &Heap,
    task: TaskId,
    options: &Value,
    name: &str,
) -> Result<Result<Value, Action>, Fault> {
    if matches!(options, Value::Undefined) {
        return Ok(Ok(Value::Undefined));
    }
    Ok(
        match property::read(
            heap,
            task,
            options,
            &property::Key::Name(JsString::from(name)),
        )? {
            Some(property::Read::Getter(callee)) => Err(Action::Call {
                callee,
                this: options.clone(),
                args: vec![],
            }),
            Some(property::Read::Value(value)) => Ok(value),
            None => Ok(Value::Undefined),
        },
    )
}
pub(super) fn convert(value: Value, kind: OptionKind) -> Result<Result<Value, Action>, Fault> {
    Ok(match kind {
        OptionKind::Boolean => Ok(Value::Bool(value.truthy())),
        OptionKind::String if matches!(value, Value::Object(_) | Value::BigInt(_)) => {
            Err(Action::ToString(value))
        }
        OptionKind::Number if matches!(value, Value::Object(_) | Value::BigInt(_)) => {
            Err(Action::ToNumber(value))
        }
        _ => Ok(value),
    })
}
