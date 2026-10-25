//! Runtime compilation has no heap/waiter lock or nested interpreter loop.
//! Source preparation is incremental (including ropes). The existing frontend
//! is synchronous, so its input is capped to bound a compilation step as well.

use super::{Action, Fault, Input};
use crate::heap::gc::Trace;
use crate::value::Value;
use crate::value::{JsString, string::StringFlatten};
use crate::vm::executable::EvalSite;

#[derive(Debug)]
pub(super) struct Caller {
    pub(super) site: EvalSite,
    pub(super) captures: Vec<Value>,
}

pub(crate) const MAX_SOURCE_UNITS: usize = 16_384;

#[derive(Debug)]
pub(crate) struct State {
    flatten: StringFlatten,
    text: Option<JsString>,
    next: usize,
    source: String,
    surrogates: Vec<(usize, u16)>,
    pub(super) caller: Option<Caller>,
    dynamic_parameters: Option<usize>,
}

impl State {
    pub(crate) fn new(text: JsString) -> Result<Self, Fault> {
        if text.len() > MAX_SOURCE_UNITS {
            return Err(Fault::RangeError(
                "eval source exceeds 16,384 UTF-16 code units".into(),
            ));
        }
        Ok(Self {
            flatten: StringFlatten::new(text)
                .ok_or(Fault::OutOfMemory("eval source allocation"))?,
            text: None,
            next: 0,
            source: String::new(),
            surrogates: Vec::new(),
            caller: None,
            dynamic_parameters: None,
        })
    }

    pub(crate) fn step(&mut self) -> Result<Action, Fault> {
        if self.text.is_none() {
            self.text = self.flatten.advance(256);
            return Ok(Action::Continue);
        }
        let units = self.text.as_ref().unwrap().code_units();
        if self.next < units.len() {
            for _ in 0..256 {
                if self.next == units.len() {
                    break;
                }
                let unit = units[self.next];
                self.next += 1;
                let scalar = if (0xd800..=0xdbff).contains(&unit)
                    && units
                        .get(self.next)
                        .is_some_and(|u| (0xdc00..=0xdfff).contains(u))
                {
                    let low = units[self.next];
                    self.next += 1;
                    0x10000 + ((u32::from(unit) - 0xd800) << 10) + u32::from(low) - 0xdc00
                } else {
                    u32::from(unit)
                };
                // One non-token scalar carries each lone unit through the
                // UTF-8 frontend. Its byte provenance restores the exact unit
                // only in literal contents; genuine U+FFFD stays distinct.
                // Both occupy one column and one UTF-16 unit, unlike escapes.
                let ch = char::from_u32(scalar).unwrap_or_else(|| {
                    self.surrogates.push((self.source.len(), unit));
                    '\u{fffd}'
                });
                self.source.push(ch);
            }
            return Ok(Action::Continue);
        }
        // The frontend's weighted nesting bound is proven for a 2 MiB stack.
        // VM workers may have only 64 KiB. Keep the entire heap-independent
        // compilation (including AST teardown on errors) on a bounded helper
        // stack; no JavaScript or task continuation runs on this thread.
        let source = std::mem::take(&mut self.source);
        let surrogates = std::mem::take(&mut self.surrogates);
        let scope = self.caller.as_ref().map(|c| c.site.clone());
        let dynamic_parameters = self.dynamic_parameters;
        let name = if dynamic_parameters.is_some() {
            "<Function>"
        } else {
            "<eval>"
        };
        let executable = std::thread::Builder::new()
            .name("eval-compiler".into())
            .stack_size(2 * 1024 * 1024)
            .spawn(move || match (dynamic_parameters, scope) {
                (Some(end), _) => {
                    crate::frontend::compile_dynamic_function(&source, &surrogates, end)
                }
                (_, Some(site)) => {
                    crate::frontend::compile_direct_eval(name, &source, &surrogates, &site)
                }
                _ => crate::frontend::compile_eval(name, &source, &surrogates),
            })
            .map_err(|_| Fault::OutOfMemory("eval compiler thread"))?
            .join()
            .map_err(|_| Fault::Internal("eval compiler panicked"))?
            .map_err(|error| Fault::CompileSyntaxError {
                message: error.message,
                name,
                line: error.line as usize,
                column: error.column as usize,
            })?;
        if dynamic_parameters.is_some() {
            return Ok(Action::DynamicFunction(executable));
        }
        let captures = self.caller.take().map_or_else(Vec::new, |c| c.captures);
        Ok(Action::Eval(executable, captures))
    }
}

/// Coercion is observable and may run JavaScript. Save every argument until
/// its turn, and use the native trampoline for ToString in left-to-right order.
#[derive(Debug)]
pub(super) struct DynamicFunction {
    args: Vec<Value>,
    next: usize,
    source: JsString,
    parameter_end: usize,
    compiler: Option<State>,
}

impl DynamicFunction {
    pub(super) fn new(args: &[Value]) -> Self {
        Self {
            args: args.to_vec(),
            next: 0,
            source: JsString::from("function anonymous("),
            parameter_end: 0,
            compiler: None,
        }
    }

    fn append(&mut self, text: &JsString) -> Result<(), Fault> {
        if self.source.len().saturating_add(text.len()) > MAX_SOURCE_UNITS {
            return Err(Fault::RangeError(
                "Function source exceeds 16,384 UTF-16 code units".into(),
            ));
        }
        self.source = self
            .source
            .concat(text)
            .ok_or(Fault::OutOfMemory("Function source allocation"))?;
        Ok(())
    }

    pub(super) fn step(&mut self, input: Input) -> Result<Action, Fault> {
        if let Some(compiler) = &mut self.compiler {
            return compiler.step();
        }
        if let Input::CallResult(Value::String(text)) = input {
            // The last argument is the body; all preceding arguments are
            // comma-separated parameter source fragments.
            if self.next == self.args.len() {
                self.append(&JsString::from("\n) {\n"))?;
                self.parameter_end = self.source.len() - 3;
            } else if self.next > 1 {
                self.append(&JsString::from(","))?;
            }
            self.append(&text)?;
        } else if !matches!(input, Input::Start) {
            return Err(Fault::Internal("Function expected a converted string"));
        }
        if self.next < self.args.len() {
            let arg = std::mem::replace(&mut self.args[self.next], Value::Undefined);
            self.next += 1;
            return Ok(Action::ToString(arg));
        }
        if self.args.is_empty() {
            self.append(&JsString::from("\n) {\n"))?;
            self.parameter_end = self.source.len() - 3;
        }
        self.append(&JsString::from("\n}"))?;
        let mut compiler = State::new(self.source.clone())?;
        compiler.dynamic_parameters = Some(self.parameter_end);
        self.compiler = Some(compiler);
        self.args.clear();
        Ok(Action::Continue)
    }
}

impl Trace for DynamicFunction {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.args.trace(visit);
    }
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        if let Some(caller) = &self.caller {
            caller.captures.trace(visit);
        }
    }
}
