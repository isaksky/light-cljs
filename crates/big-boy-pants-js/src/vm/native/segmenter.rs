//! Segmenter option/coercion and boundary scans resume in explicit VM state.
use super::intl_service::{self, OptionKind};
use super::*;
use crate::{
    heap::HeapObject,
    intl::{
        segmenter::{self, Data, Scan},
        service,
    },
    object::OrdinaryObject,
    value::string::{StringBuild, StringFlatten},
};
const OPTIONS: &[&str] = &["localeMatcher", "granularity"];
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
    String,
    StringValue,
    Flatten,
    Index,
    IndexValue,
    Scan,
    Copy,
}
#[derive(Debug)]
pub(super) struct State {
    stage: Stage,
    locales_reader: Option<intl::Intl>,
    locales: Value,
    options: Value,
    source: Value,
    receiver: Value,
    segmenter: Value,
    locale: String,
    word: bool,
    option: usize,
    supported: bool,
    containing: bool,
    input: JsString,
    text: JsString,
    flatten: Option<StringFlatten>,
    scan: Option<Scan>,
    copy: Option<StringBuild>,
    target: usize,
}
impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for v in [
            &self.locales,
            &self.options,
            &self.source,
            &self.receiver,
            &self.segmenter,
        ] {
            v.trace(visit);
        }
        if let Some(reader) = &self.locales_reader {
            reader.trace(visit);
        }
        // Immutable UTF-16 storage and scanning/copy cursors have no heap edges.
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
        let source = args.first().cloned().unwrap_or(Value::Undefined);
        let constructor = matches!(which, IntlSegmenter | IntlSegmenterSupportedLocales);
        let mut result = Self {
            stage: Stage::Locales,
            locales_reader: if constructor {
                Some(intl::Intl::new(std::slice::from_ref(&source), false)?)
            } else {
                None
            },
            locales: Value::Undefined,
            options: args.get(1).cloned().unwrap_or(Value::Undefined),
            source,
            receiver: this.clone(),
            segmenter: Value::Undefined,
            locale: std::string::String::new(),
            word: false,
            option: 0,
            supported: which == IntlSegmenterSupportedLocales,
            containing: which == IntlSegmentsContaining,
            input: JsString::from(""),
            text: JsString::from(""),
            flatten: None,
            scan: None,
            copy: None,
            target: 0,
        };
        if which == IntlSegmenterSegment {
            result.word = segmenter::options(heap, task, this)?.1;
            result.segmenter = this.clone();
            result.stage = Stage::String;
        } else if !constructor {
            let (segmenter, input, text, index) = match segmenter::record(heap, task, this)? {
                Data::Segments {
                    segmenter,
                    input,
                    text,
                } if result.containing => (segmenter, input, text, 0),
                Data::Iterator {
                    segmenter,
                    input,
                    text,
                    index,
                } if !result.containing => (segmenter, input, text, index),
                _ => return Err(Fault::TypeError("incompatible Segments receiver".into())),
            };
            result.word = segmenter::options(heap, task, &segmenter)?.1;
            result.segmenter = segmenter;
            result.input = input;
            result.text = text;
            result.target = index;
            result.stage = if result.containing {
                Stage::Index
            } else {
                Stage::Scan
            };
        }
        Ok(result)
    }
    fn value(input: Input) -> Result<Value, Fault> {
        if let Input::CallResult(v) = input {
            Ok(v)
        } else {
            Err(Fault::Internal("Segmenter expected call result"))
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
    fn option_value(&mut self, value: Value) -> Result<Action, Fault> {
        if matches!(value, Value::Undefined) {
            return Ok(self.advance_option());
        }
        self.stage = Stage::Converted;
        match intl_service::convert(value, OptionKind::String)? {
            Ok(v) => self.apply(v),
            Err(a) => Ok(a),
        }
    }
    fn apply(&mut self, value: Value) -> Result<Action, Fault> {
        if matches!(&value,Value::String(s) if s.len()>8) {
            return Err(Fault::RangeError("invalid Segmenter option".into()));
        }
        if self.option == 0 {
            service::string_option(&value, &["lookup", "best fit"])?;
        } else {
            self.word = service::string_option(&value, &["grapheme", "word"])? == "word";
        }
        Ok(self.advance_option())
    }
    fn alloc(heap: &Heap, task: TaskId, proto: ObjectId, data: Data) -> Result<Value, Fault> {
        heap.alloc(HeapObject::Plain(
            OrdinaryObject::with_prototype(heap, task, proto).with_segmenter(data),
        ))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("Segmenter object"))
    }
    fn done(&self, heap: &Heap, task: TaskId) -> Result<Action, Fault> {
        if self.containing {
            return Ok(Action::Return(Value::Undefined));
        }
        service::resolved_options(
            heap,
            task,
            [("value", Value::Undefined), ("done", Value::Bool(true))],
        )
        .map(Action::Return)
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
                Action::Return(v) => {
                    self.locales = v;
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
                    self.locale = tag;
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
                self.locale = service::supported_locale(&default.to_string_lossy(), &[])
                    .map(|(tag, _)| tag)
                    .unwrap_or_else(|| "en-US".into());
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
                    Ok(v) => self.option_value(v),
                    Err(a) => Ok(a),
                }
            }
            Stage::OptionValue => self.option_value(Self::value(input)?),
            Stage::Converted => self.apply(Self::value(input)?),
            Stage::Construct => {
                if self.supported {
                    return service::supported_locales(heap, task, &self.locales)
                        .map(Action::Return);
                }
                let proto = crate::realm::intrinsics(heap)
                    .ok_or(Fault::OutOfMemory("realm"))?
                    .intl_segmenter_prototype;
                Self::alloc(
                    heap,
                    task,
                    proto,
                    Data::Segmenter {
                        locale: self.locale.clone(),
                        word: self.word,
                    },
                )
                .map(Action::Return)
            }
            Stage::String => {
                self.stage = Stage::StringValue;
                Ok(Action::ToString(self.source.clone()))
            }
            Stage::StringValue => {
                let Value::String(text) = Self::value(input)? else {
                    return Err(Fault::Internal("Segmenter string"));
                };
                self.input = text.clone();
                self.flatten = Some(StringFlatten::new_accounted(heap, text)?);
                self.stage = Stage::Flatten;
                Ok(Action::Continue)
            }
            Stage::Flatten => {
                let Some(text) = self.flatten.as_mut().unwrap().advance(128) else {
                    return Ok(Action::Continue);
                };
                self.flatten = None;
                let proto = crate::realm::intrinsics(heap)
                    .ok_or(Fault::OutOfMemory("realm"))?
                    .intl_segments_prototype;
                Self::alloc(
                    heap,
                    task,
                    proto,
                    Data::Segments {
                        segmenter: self.segmenter.clone(),
                        input: self.input.clone(),
                        text,
                    },
                )
                .map(Action::Return)
            }
            Stage::Index => {
                self.stage = Stage::IndexValue;
                Ok(Action::ToNumber(self.source.clone()))
            }
            Stage::IndexValue => {
                let Value::Number(index) = Self::value(input)? else {
                    return Err(Fault::Internal("Segments index"));
                };
                let index = if index.is_nan() { 0.0 } else { index.trunc() };
                if index < 0.0 || index >= self.text.len() as f64 {
                    return self.done(heap, task);
                }
                self.target = index as usize;
                self.stage = Stage::Scan;
                Ok(Action::Continue)
            }
            Stage::Scan => {
                if self.scan.is_none() {
                    if self.target >= self.text.len() {
                        return self.done(heap, task);
                    }
                    self.scan = Some(Scan::new(
                        &self.text,
                        if self.containing { 0 } else { self.target },
                        self.word,
                    ));
                }
                let scan = self.scan.as_mut().unwrap();
                if !scan.advance(&self.text) {
                    return Ok(Action::Continue);
                }
                if self.containing && scan.end <= self.target {
                    self.scan = Some(Scan::new(&self.text, scan.end, self.word));
                    return Ok(Action::Continue);
                }
                self.copy = Some(StringBuild::new_accounted(
                    heap,
                    vec![(self.text.clone(), scan.start, scan.end - scan.start)],
                )?);
                self.stage = Stage::Copy;
                Ok(Action::Continue)
            }
            Stage::Copy => {
                let Some(text) = self.copy.as_mut().unwrap().advance(128) else {
                    return Ok(Action::Continue);
                };
                let scan = self.scan.as_ref().unwrap();
                let mut fields = vec![
                    ("segment", Value::String(text)),
                    ("index", Value::Number(scan.start as f64)),
                    ("input", Value::String(self.input.clone())),
                ];
                if self.word {
                    fields.push(("isWordLike", Value::Bool(scan.word_like)));
                }
                let value = service::resolved_options(heap, task, fields)?;
                if self.containing {
                    return Ok(Action::Return(value));
                }
                segmenter::access(heap, task, &self.receiver, |data| match data {
                    Data::Iterator { index, .. } => {
                        *index = scan.end;
                        Ok(())
                    }
                    _ => Err(Fault::Internal("Segment iterator slot")),
                })?;
                service::resolved_options(
                    heap,
                    task,
                    [("value", value), ("done", Value::Bool(false))],
                )
                .map(Action::Return)
            }
        }
    }
}

pub(crate) fn iterator(heap: &Heap, task: TaskId, this: &Value) -> Result<Value, Fault> {
    let Data::Segments {
        segmenter,
        input,
        text,
    } = segmenter::record(heap, task, this)?
    else {
        return Err(Fault::TypeError("Segments receiver required".into()));
    };
    let proto = crate::realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("realm"))?
        .intl_segment_iterator_prototype;
    State::alloc(
        heap,
        task,
        proto,
        Data::Iterator {
            segmenter,
            input,
            text,
            index: 0,
        },
    )
}
