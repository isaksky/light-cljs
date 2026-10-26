//! String.raw (clj-quickjs-58). Each property read and coercion is a saved
//! stage, so getters and ToPrimitive callbacks can yield or wait once.
//! Written fresh with QuickJS `js_string_raw` as a semantic reference
//! (research/quickjs/quickjs.c, revision 7d1c13c5d8f8).

use super::*;

#[derive(Debug, PartialEq, Eq)]
enum Stage {
    Raw,
    RawValue,
    Length,
    LengthValue,
    Element,
    ElementValue,
    ElementString,
    Substitution,
    SubstitutionString,
    Done,
}

#[derive(Debug)]
pub(super) struct StringRaw {
    raw: Value,
    substitutions: Vec<Value>,
    stage: Stage,
    len: u64,
    next: u64,
    parts: Vec<(JsString, usize, usize)>,
    parts_reservation: crate::heap::budget::ByteReservation,
    units: usize,
}

impl Trace for StringRaw {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.raw.trace(visit);
        for value in &self.substitutions {
            value.trace(visit);
        }
    }
}

fn require_object_coercible(value: Value) -> Result<Value, Fault> {
    match value {
        Value::Null | Value::Undefined => Err(Fault::TypeError(format!(
            "cannot convert {} to object",
            value.type_name()
        ))),
        _ => Ok(value),
    }
}

impl StringRaw {
    pub(super) fn new(args: &[Value]) -> Result<Self, Fault> {
        Ok(Self {
            // Property reads on primitives provide the same behavior as
            // ToObject without retaining a transient wrapper.
            raw: require_object_coercible(args.first().cloned().unwrap_or(Value::Undefined))?,
            substitutions: args.get(1..).unwrap_or_default().to_vec(),
            stage: Stage::Raw,
            len: 0,
            next: 0,
            parts: vec![],
            parts_reservation: Default::default(),
            units: 0,
        })
    }

    pub(super) fn finish(
        &mut self,
        heap: &Heap,
    ) -> Result<Option<crate::value::string::StringBuild>, Fault> {
        if self.stage != Stage::Done {
            return Ok(None);
        }
        crate::value::string::StringBuild::with_length_accounted(
            heap,
            std::mem::take(&mut self.parts),
            self.units,
        )
        .map(|build| {
            Some(build.with_parts_reservation(std::mem::take(&mut self.parts_reservation)))
        })
    }

    fn append(&mut self, heap: &Heap, text: JsString) -> Result<Action, Fault> {
        let len = text.len();
        self.units = self
            .units
            .checked_add(len)
            .filter(|n| *n <= crate::value::string::MAX_STRING_LEN)
            .ok_or(Fault::StringTooLong)?;
        if len != 0 {
            let charge = heap.reserve_bytes(128).ok_or(Fault::HeapLimit)?;
            let text = text.import(heap).ok_or(Fault::HeapLimit)?;
            self.parts.push((text, 0, len));
            self.parts_reservation.merge(charge);
        }
        Ok(Action::Continue)
    }

    fn value(input: Input) -> Result<Value, Fault> {
        match input {
            Input::CallResult(value) => Ok(value),
            _ => Err(Fault::Internal("String.raw expected a call result")),
        }
    }

    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        mut input: Input,
    ) -> Result<Action, Fault> {
        // Fall through only from a property read to its value stage; all
        // iteration and string copying is driven by budgeted VM steps.
        let key = match self.stage {
            Stage::Raw => {
                self.stage = Stage::RawValue;
                Some(property::Key::Name(JsString::from("raw")))
            }
            Stage::Length => {
                self.stage = Stage::LengthValue;
                Some(property::Key::Name(JsString::from("length")))
            }
            Stage::Element => {
                self.stage = Stage::ElementValue;
                Some(index_key(self.next))
            }
            _ => None,
        };
        if let Some(key) = key {
            input = match property::read(heap, task, &self.raw, &key)? {
                Some(property::Read::Getter(callee)) => {
                    return Ok(Action::Call {
                        callee,
                        this: self.raw.clone(),
                        args: vec![],
                    });
                }
                Some(property::Read::Value(value)) => Input::CallResult(value),
                None => Input::CallResult(Value::Undefined),
            };
        }
        match self.stage {
            Stage::RawValue => {
                self.raw = require_object_coercible(Self::value(input)?)?;
                self.stage = Stage::Length;
                Ok(Action::Continue)
            }
            Stage::LengthValue => {
                let value = Self::value(input)?;
                if matches!(value, Value::Object(_)) {
                    return Ok(Action::ToNumber(value));
                }
                self.len = to_length(&value)?;
                self.stage = if self.len == 0 {
                    Stage::Done
                } else {
                    Stage::Element
                };
                Ok(Action::Continue)
            }
            Stage::ElementValue => {
                let value = Self::value(input)?;
                self.stage = Stage::ElementString;
                if matches!(value, Value::Object(_) | Value::BigInt(_)) {
                    return Ok(Action::ToString(value));
                }
                self.stage = Stage::Substitution;
                self.append(heap, dispatch::to_js_string_accounted(heap, &value)?)
            }
            Stage::ElementString => {
                let Value::String(text) = Self::value(input)? else {
                    return Err(Fault::Internal("String.raw expected string"));
                };
                self.stage = Stage::Substitution;
                self.append(heap, text)
            }
            Stage::Substitution => {
                self.next += 1;
                if self.next == self.len {
                    self.stage = Stage::Done;
                    return Ok(Action::Continue);
                }
                let value = usize::try_from(self.next - 1)
                    .ok()
                    .and_then(|index| self.substitutions.get(index))
                    .cloned();
                self.stage = Stage::Element;
                match value {
                    Some(value @ (Value::Object(_) | Value::BigInt(_))) => {
                        self.stage = Stage::SubstitutionString;
                        Ok(Action::ToString(value))
                    }
                    Some(value) => {
                        self.append(heap, dispatch::to_js_string_accounted(heap, &value)?)
                    }
                    None => Ok(Action::Continue),
                }
            }
            Stage::SubstitutionString => {
                let Value::String(text) = Self::value(input)? else {
                    return Err(Fault::Internal("String.raw expected substitution string"));
                };
                self.stage = Stage::Element;
                self.append(heap, text)
            }
            _ => Err(Fault::Internal("invalid String.raw stage")),
        }
    }
}
