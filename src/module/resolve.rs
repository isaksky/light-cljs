//! ECMA-262 ResolveExport with a saved resolve set and explicit call frames.
//! A repeated (module, export name) pair returns null to its caller; another
//! star branch may still resolve it. Only an unresolved explicit export fails.

use super::ModuleKey;
use super::link::{Exports, Origin, PreparedModule};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Resolution {
    Missing,
    Ambiguous,
    Found(Origin),
}

#[derive(Debug)]
enum State {
    Start,
    Indirect,
    Stars { next: usize, found: Resolution },
}

#[derive(Debug)]
struct Frame {
    key: ModuleKey,
    name: String,
    state: State,
}

#[derive(Debug)]
pub(super) struct Resolver {
    root: ModuleKey,
    frames: Vec<Frame>,
    seen: HashSet<Origin>,
    result: Option<Resolution>,
    pub(super) circular: bool,
}

impl Resolver {
    pub(super) fn new(key: ModuleKey, name: String) -> Self {
        Self {
            root: key,
            frames: vec![Frame {
                key,
                name,
                state: State::Start,
            }],
            seen: HashSet::new(),
            result: None,
            circular: false,
        }
    }

    fn finish(&mut self, result: Resolution) {
        self.frames.pop();
        self.result = Some(result);
    }

    /// One lookup, star edge or saved return per budget tick.
    pub(super) fn step(
        &mut self,
        modules: &HashMap<ModuleKey, PreparedModule>,
        exports: &Exports,
        components: &HashMap<ModuleKey, ModuleKey>,
    ) -> Option<Resolution> {
        let Some(frame) = self.frames.last_mut() else {
            return self.result.take();
        };
        let node = &modules[&frame.key];
        match &mut frame.state {
            State::Start => {
                if !self.seen.insert((frame.key, frame.name.clone())) {
                    self.circular = true;
                    self.finish(Resolution::Missing);
                } else if components[&frame.key] != components[&self.root] {
                    // Dependency SCCs are complete in post-order. Reusing
                    // their table is safe: they cannot reach this resolve set.
                    let result = match exports[&frame.key].get(&frame.name) {
                        Some(Some(origin)) => Resolution::Found(origin.clone()),
                        Some(None) => Resolution::Ambiguous,
                        None => Resolution::Missing,
                    };
                    self.finish(result);
                } else if let Some(local) = node.local_exports.get(&frame.name) {
                    let origin = (frame.key, local.clone());
                    self.finish(Resolution::Found(origin));
                } else if let Some((dep, name)) = node.indirect_exports.get(&frame.name) {
                    frame.state = State::Indirect;
                    self.frames.push(Frame {
                        key: *dep,
                        name: name.clone(),
                        state: State::Start,
                    });
                } else if frame.name == "default" {
                    self.finish(Resolution::Missing);
                } else {
                    frame.state = State::Stars {
                        next: 0,
                        found: Resolution::Missing,
                    };
                }
            }
            State::Indirect => {
                let result = self.result.take().expect("indirect export returned");
                self.finish(result);
            }
            State::Stars { next, found } => {
                if let Some(result) = self.result.take() {
                    match (&*found, result) {
                        (_, Resolution::Ambiguous) => *found = Resolution::Ambiguous,
                        (Resolution::Missing, result) => *found = result,
                        (Resolution::Found(a), Resolution::Found(b)) if *a != b => {
                            *found = Resolution::Ambiguous
                        }
                        _ => {}
                    }
                    if *found == Resolution::Ambiguous {
                        self.finish(Resolution::Ambiguous);
                    }
                } else if let Some(specifier) = node.record.star_export_entries().get(*next) {
                    *next += 1;
                    let child = Frame {
                        key: node.request_keys[specifier.code_units()],
                        name: frame.name.clone(),
                        state: State::Start,
                    };
                    self.frames.push(child);
                } else {
                    let result = found.clone();
                    self.finish(result);
                }
            }
        }
        None
    }
}
