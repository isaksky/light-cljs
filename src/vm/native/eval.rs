//! Runtime compilation has no heap/waiter lock or nested interpreter loop.
//! Source preparation is incremental (including ropes). The existing frontend
//! is synchronous, so its input is capped to bound a compilation step as well.

use super::{Action, Fault};
use crate::value::{JsString, string::StringFlatten};

pub(crate) const MAX_SOURCE_UNITS: usize = 16_384;

#[derive(Debug)]
pub(crate) struct State {
    flatten: StringFlatten,
    text: Option<JsString>,
    next: usize,
    source: String,
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
        crate::frontend::compile_eval("<eval>", &self.source)
            .map(Action::Eval)
            .map_err(|error| Fault::SyntaxError(format!("<eval>:{error}")))
    }
}
