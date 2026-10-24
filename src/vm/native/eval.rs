//! Runtime compilation has no heap/waiter lock or nested interpreter loop.
//! Source preparation is incremental (including ropes). The existing frontend
//! is synchronous, so its input is capped to bound a compilation step as well.

use super::{Action, Fault};
use crate::heap::gc::Trace;
use crate::value::Value;
use crate::value::{JsString, string::StringFlatten};
use crate::vm::executable::EvalBinding;

#[derive(Debug)]
pub(super) struct Caller {
    pub(super) strict: bool,
    pub(super) bindings: Vec<EvalBinding>,
    pub(super) captures: Vec<Value>,
    pub(super) receiver_strict: bool,
}

pub(crate) const MAX_SOURCE_UNITS: usize = 16_384;

#[derive(Debug)]
pub(crate) struct State {
    flatten: StringFlatten,
    text: Option<JsString>,
    next: usize,
    source: String,
    pub(super) caller: Option<Caller>,
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
            caller: None,
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
                let ch = char::from_u32(scalar).ok_or_else(|| {
                    Fault::SyntaxError(
                        "<eval>: unpaired surrogate in source text; use a Unicode escape".into(),
                    )
                })?;
                self.source.push(ch);
            }
            return Ok(Action::Continue);
        }
        // The frontend's weighted nesting bound is proven for a 2 MiB stack.
        // VM workers may have only 64 KiB. Keep the entire heap-independent
        // compilation (including AST teardown on errors) on a bounded helper
        // stack; no JavaScript or task continuation runs on this thread.
        let source = std::mem::take(&mut self.source);
        let scope = self
            .caller
            .as_ref()
            .map(|c| (c.strict, c.bindings.clone(), c.receiver_strict));
        let executable = std::thread::Builder::new()
            .name("eval-compiler".into())
            .stack_size(2 * 1024 * 1024)
            .spawn(move || match scope {
                Some((strict, bindings, receiver_strict)) => crate::frontend::compile_direct_eval(
                    "<eval>",
                    &source,
                    strict,
                    &bindings,
                    receiver_strict,
                ),
                None => crate::frontend::compile_eval("<eval>", &source),
            })
            .map_err(|_| Fault::OutOfMemory("eval compiler thread"))?
            .join()
            .map_err(|_| Fault::Internal("eval compiler panicked"))?
            .map_err(|error| Fault::SyntaxError(format!("<eval>:{error}")))?;
        let captures = self.caller.take().map_or_else(Vec::new, |c| c.captures);
        Ok(Action::Eval(executable, captures))
    }
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        if let Some(caller) = &self.caller {
            caller.captures.trace(visit);
        }
    }
}
