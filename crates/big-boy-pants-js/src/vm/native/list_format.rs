//! ListFormat construction and StringListFromIterable: every user call and
//! output element resumes through the trampoline, without retaining Rust frames.
use super::intl_service::{self, OptionKind};
use super::*;
use crate::vm::exception::Exception;
use crate::{
    heap::HeapObject,
    intl::{
        list_format::{self, ListFormat},
        service,
    },
    object::OrdinaryObject,
};

const OPTIONS: &[&str] = &["localeMatcher", "type", "style"];
#[derive(Debug)]
enum Stage {
    Locales,
    Options,
    Resolve,
    Default,
    Read,
    OptionValue,
    Converted,
    Construct,
    Start,
    Iterator,
    Next,
    NextResult,
    Done,
    Value,
    Validate,
    Prepare,
    Format,
    Close,
    CloseMethod,
    CloseResult,
    Rethrow,
}

#[derive(Debug)]
pub(super) struct State {
    stage: Stage,
    locales_reader: Option<intl::Intl>,
    locales: Value,
    options: Value,
    source: Value,
    slots: Option<ListFormat>,
    option: usize,
    supported: bool,
    parts: bool,
    acquisition: Option<iterator::State>,
    iterator: Value,
    next: Value,
    result: Value,
    output: Value,
    items: Vec<JsString>,
    text: JsString,
    index: usize,
    pending: Option<Exception>,
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for value in [
            &self.locales,
            &self.options,
            &self.source,
            &self.iterator,
            &self.next,
            &self.result,
            &self.output,
        ] {
            value.trace(visit);
        }
        if let Some(reader) = &self.locales_reader {
            reader.trace(visit);
        }
        if let Some(acquisition) = &self.acquisition {
            acquisition.trace(visit);
        }
        if let Some(pending) = &self.pending {
            pending.value.trace(visit);
        }
        // JsString bodies and immutable slots have no heap edges.
    }
}

impl State {
    pub(super) fn new(
        which: builtin::Builtin,
        this: &Value,
        args: &[Value],
        heap: &Heap,
        task: TaskId,
    ) -> Result<Self, Fault> {
        use builtin::Builtin::*;
        let formatting = matches!(which, IntlListFormatFormat | IntlListFormatFormatToParts);
        let source = args.first().cloned().unwrap_or(Value::Undefined);
        let slots = if formatting {
            Some(list_format::record(heap, task, this)?)
        } else {
            None
        };
        Ok(Self {
            stage: if formatting {
                Stage::Start
            } else {
                Stage::Locales
            },
            locales_reader: if formatting {
                None
            } else {
                Some(intl::Intl::new(std::slice::from_ref(&source), false)?)
            },
            locales: Value::Undefined,
            options: args.get(1).cloned().unwrap_or(Value::Undefined),
            source,
            slots,
            option: 0,
            supported: which == IntlListFormatSupportedLocales,
            parts: which == IntlListFormatFormatToParts,
            acquisition: None,
            iterator: Value::Undefined,
            next: Value::Undefined,
            result: Value::Undefined,
            output: Value::Undefined,
            items: Vec::new(),
            text: JsString::from(""),
            index: 0,
            pending: None,
        })
    }

    pub(super) fn catches(&self) -> bool {
        matches!(
            self.stage,
            Stage::Validate | Stage::Close | Stage::CloseMethod | Stage::CloseResult
        )
    }
    pub(super) fn catches_step(&self) -> bool {
        matches!(self.stage, Stage::Validate | Stage::Close)
    }
    pub(super) fn failed(&mut self, exception: Exception) {
        if self.pending.is_none() {
            self.pending = Some(exception);
            self.stage = Stage::Close;
        } else {
            self.stage = Stage::Rethrow;
        }
    }
    fn value(input: Input) -> Result<Value, Fault> {
        if let Input::CallResult(value) = input {
            Ok(value)
        } else {
            Err(Fault::Internal("ListFormat expected call result"))
        }
    }
    fn read(
        &mut self,
        heap: &Heap,
        task: TaskId,
        base: Value,
        name: &str,
        stage: Stage,
    ) -> Result<Action, Fault> {
        self.stage = stage;
        match intl_service::read(heap, task, &base, name)? {
            Ok(value) => self.step(heap, task, Input::CallResult(value)),
            Err(action) => Ok(action),
        }
    }
    fn option_value(&mut self, value: Value) -> Result<Action, Fault> {
        if matches!(value, Value::Undefined) {
            return Ok(self.advance_option());
        }
        self.stage = Stage::Converted;
        match intl_service::convert(value, OptionKind::String)? {
            Ok(value) => self.apply(value),
            Err(action) => Ok(action),
        }
    }
    fn advance_option(&mut self) -> Action {
        self.option += 1;
        self.stage = if self.option == 1 && !self.supported {
            Stage::Resolve
        } else {
            Stage::Read
        };
        Action::Continue
    }
    fn apply(&mut self, value: Value) -> Result<Action, Fault> {
        // Every allowed spelling is short: reject long ropes by length before
        // the shared leaf option parser could flatten them without a budget.
        if matches!(&value, Value::String(text) if text.len() > 11) {
            return Err(Fault::RangeError("invalid Intl option".into()));
        }
        match self.option {
            0 => {
                service::string_option(&value, &["lookup", "best fit"])?;
            }
            1 => {
                self.slots.as_mut().unwrap().kind =
                    service::string_option(&value, &["conjunction", "disjunction", "unit"])?
            }
            2 => {
                self.slots.as_mut().unwrap().style =
                    service::string_option(&value, &["long", "short", "narrow"])?
            }
            _ => unreachable!(),
        }
        Ok(self.advance_option())
    }
    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        match self.stage {
            Stage::Locales => match self
                .locales_reader
                .as_mut()
                .unwrap()
                .step(heap, task, input)?
            {
                Action::Return(value) => {
                    self.locales = value;
                    self.locales_reader = None;
                    self.stage = Stage::Options;
                    Ok(Action::Continue)
                }
                action => Ok(action),
            },
            Stage::Options => {
                service::get_options_object(&self.options)?;
                self.stage = Stage::Read;
                Ok(Action::Continue)
            }
            Stage::Resolve => {
                if let Some((tag, _)) = service::resolve_locale(heap, task, &self.locales, &[])? {
                    self.slots = Some(ListFormat::new(tag));
                    self.stage = Stage::Read;
                    Ok(Action::Continue)
                } else {
                    self.stage = Stage::Default;
                    Ok(Action::DefaultLocale)
                }
            }
            Stage::Default => {
                let Value::String(default) = Self::value(input)? else {
                    return Err(Fault::Internal("default locale"));
                };
                let tag = service::supported_locale(&default.to_string_lossy(), &[])
                    .map(|(tag, _)| tag)
                    .unwrap_or_else(|| "en-US".into());
                self.slots = Some(ListFormat::new(tag));
                self.stage = Stage::Read;
                Ok(Action::Continue)
            }
            Stage::Read => {
                if self.option == if self.supported { 1 } else { OPTIONS.len() } {
                    self.stage = Stage::Construct;
                    return Ok(Action::Continue);
                }
                self.stage = Stage::OptionValue;
                match intl_service::read(heap, task, &self.options, OPTIONS[self.option])? {
                    Ok(value) => self.option_value(value),
                    Err(action) => Ok(action),
                }
            }
            Stage::OptionValue => self.option_value(Self::value(input)?),
            Stage::Converted => self.apply(Self::value(input)?),
            Stage::Construct => {
                if self.supported {
                    return service::supported_locales(heap, task, &self.locales)
                        .map(Action::Return);
                }
                let prototype = crate::realm::intrinsics(heap)
                    .ok_or(Fault::OutOfMemory("realm exhausted"))?
                    .intl_list_format_prototype;
                let object = OrdinaryObject::with_prototype(heap, task, prototype)
                    .with_list_format(self.slots.take().unwrap());
                heap.alloc(HeapObject::Plain(object))
                    .map(Value::Object)
                    .map(Action::Return)
                    .ok_or(Fault::OutOfMemory("heap exhausted"))
            }
            Stage::Start => {
                if matches!(self.source, Value::Undefined) {
                    self.stage = Stage::Prepare;
                } else {
                    self.acquisition = Some(iterator::State::get_iterator(self.source.clone()));
                    self.stage = Stage::Iterator;
                }
                Ok(Action::Continue)
            }
            Stage::Iterator => match self.acquisition.as_mut().unwrap().step(heap, task, input)? {
                Action::Return(record) => {
                    self.iterator =
                        property::get_field(heap, task, &record, &JsString::from("iterator"))?;
                    self.next = property::get_field(heap, task, &record, &JsString::from("next"))?;
                    self.acquisition = None;
                    self.stage = Stage::Next;
                    Ok(Action::Continue)
                }
                action => Ok(action),
            },
            Stage::Next => {
                self.stage = Stage::NextResult;
                Ok(Action::Call {
                    callee: self.next.clone(),
                    this: self.iterator.clone(),
                    args: vec![],
                })
            }
            Stage::NextResult => {
                self.result = Self::value(input)?;
                if !matches!(self.result, Value::Object(_)) {
                    return Err(Fault::TypeError("iterator result must be an object".into()));
                }
                self.read(heap, task, self.result.clone(), "done", Stage::Done)
            }
            Stage::Done => {
                if Self::value(input)?.truthy() {
                    self.stage = Stage::Prepare;
                    Ok(Action::Continue)
                } else {
                    self.read(heap, task, self.result.clone(), "value", Stage::Value)
                }
            }
            Stage::Value => {
                self.result = Self::value(input)?;
                self.stage = Stage::Validate;
                Ok(Action::Continue)
            }
            Stage::Validate => {
                let Value::String(text) = &self.result else {
                    return Err(Fault::TypeError("iterable yielded non-string value".into()));
                };
                self.items.push(text.clone());
                self.result = Value::Undefined;
                self.stage = Stage::Next;
                Ok(Action::Continue)
            }
            Stage::Prepare => {
                if self.parts {
                    self.output = property::new_array(heap, task)?;
                }
                self.stage = Stage::Format;
                Ok(Action::Continue)
            }
            Stage::Format => self.format(heap, task),
            Stage::Close => self.read(
                heap,
                task,
                self.iterator.clone(),
                "return",
                Stage::CloseMethod,
            ),
            Stage::CloseMethod => {
                let method = Self::value(input)?;
                if matches!(method, Value::Null | Value::Undefined) {
                    self.stage = Stage::Rethrow;
                    Ok(Action::Continue)
                } else {
                    self.stage = Stage::CloseResult;
                    Ok(Action::Call {
                        callee: method,
                        this: self.iterator.clone(),
                        args: vec![],
                    })
                }
            }
            Stage::CloseResult => {
                self.stage = Stage::Rethrow;
                Ok(Action::Continue)
            }
            Stage::Rethrow => Err(Fault::Rethrow(
                self.pending
                    .take()
                    .ok_or(Fault::Internal("missing ListFormat exception"))?,
            )),
        }
    }
    fn format(&mut self, heap: &Heap, task: TaskId) -> Result<Action, Fault> {
        if self.index == self.items.len() {
            return Ok(Action::Return(if self.parts {
                self.output.clone()
            } else {
                Value::String(self.text.clone())
            }));
        }
        let item = self.items[self.index].clone();
        if self.index > 0 {
            let separator = JsString::from(
                self.slots
                    .as_ref()
                    .unwrap()
                    .separator(self.items.len(), self.index),
            );
            self.append(heap, task, "literal", separator)?;
        }
        self.append(heap, task, "element", item)?;
        self.index += 1;
        Ok(Action::Continue)
    }
    fn append(
        &mut self,
        heap: &Heap,
        task: TaskId,
        kind: &str,
        text: JsString,
    ) -> Result<(), Fault> {
        if self.parts {
            let part = service::resolved_options(
                heap,
                task,
                [
                    ("type", Value::String(JsString::from(kind))),
                    ("value", Value::String(text)),
                ],
            )?;
            property::append(heap, task, &self.output, Some(part))
        } else {
            self.text = self.text.concat_accounted(heap, &text)?;
            Ok(())
        }
    }
}
