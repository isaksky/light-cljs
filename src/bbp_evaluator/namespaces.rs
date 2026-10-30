//! The compilation's namespace registry for native macros (light-cljs-589):
//! what `all-ns`, `find-ns` and `the-ns` see, shared by every task of the
//! runtime, in the order one slot creates the namespaces.
//!
//! The legacy runtimes keep one registry per engine: the namespaces each
//! reader registration names (a macro's or a unit's program, as the engine
//! compiles it) and those `create-ns` makes, which includes every macro
//! call's namespace. A native task's creation goes into this log tagged with
//! its activity's place in the effect lanes ([`NamespaceTag`]), without
//! waiting, so macro calls stay parallel. A reader waits for its lane first
//! and then sees what the activities before it created, and its own
//! ([`NamespaceView`]), so every thread count gives the one-slot answer.
use crate::effect_lanes::{NamespaceTag, NamespaceView};
use std::collections::HashMap;

/// One creation of a namespace: where, and when among all creations.
#[derive(Clone, Copy)]
struct Record {
    tag: NamespaceTag,
    order: u64,
}

pub(crate) struct NamespaceLog {
    records: HashMap<String, Vec<Record>>,
    next: u64,
}

impl Default for NamespaceLog {
    /// `cljs.user`, then `cljs.core`, before anything, as in the legacy
    /// runtimes.
    fn default() -> Self {
        let mut log = Self {
            records: HashMap::new(),
            next: 0,
        };
        for name in ["cljs.user", "cljs.core"] {
            log.record(name, NamespaceTag::default());
        }
        log
    }
}

impl NamespaceLog {
    /// Note that the activity at `tag` created (or registered) `name`. A
    /// record of the same anchor at the same or an earlier position, or one
    /// before every activity, already shows it to every reader this one
    /// would, and earlier.
    pub(crate) fn record(&mut self, name: &str, tag: NamespaceTag) {
        let records = match self.records.get_mut(name) {
            Some(records) => records,
            None => self.records.entry(name.to_owned()).or_default(),
        };
        let covers = |r: &Record| {
            r.tag.position <= tag.position && (r.tag.anchor == tag.anchor || r.tag.position == 0)
        };
        if records.iter().any(covers) {
            return;
        }
        records.retain(|r| {
            !(tag.position <= r.tag.position && (tag.anchor == r.tag.anchor || tag.position == 0))
        });
        records.push(Record {
            tag,
            order: self.next,
        });
        self.next += 1;
    }

    /// Whether `view` sees namespace `name`.
    pub(crate) fn has(&self, name: &str, view: &NamespaceView) -> bool {
        self.records
            .get(name)
            .is_some_and(|records| records.iter().any(|r| view.sees(r.tag)))
    }

    /// The names `view` sees, in the order their first creation it sees
    /// happened with one slot.
    pub(crate) fn names(&self, view: &NamespaceView) -> Vec<&str> {
        let mut seen: Vec<((u64, u64), &str)> = self
            .records
            .iter()
            .filter_map(|(name, records)| {
                (records.iter())
                    .filter(|r| view.sees(r.tag))
                    .map(|r| (r.tag.position, r.order))
                    .min()
                    .map(|first| (first, name.as_str()))
            })
            .collect();
        seen.sort_unstable();
        seen.into_iter().map(|(_, name)| name).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(position: u64, anchor: u64) -> NamespaceTag {
        NamespaceTag { position, anchor }
    }

    #[test]
    fn readers_see_earlier_positions_and_their_own_anchor_in_one_slot_order() {
        let mut log = NamespaceLog::default();
        // Unit 9 runs ahead of unit 4 on another thread.
        log.record("late", tag(9, 90));
        log.record("early", tag(4, 40));
        log.record("late", tag(9, 90));
        log.record("shared", tag(9, 90));
        log.record("shared", tag(2, 20));
        let at = |effective, own, anchor| NamespaceView {
            effective,
            own,
            anchor,
        };
        assert_eq!(
            log.names(&at(5, 5, 50)),
            ["cljs.user", "cljs.core", "shared", "early"]
        );
        assert_eq!(
            log.names(&at(9, 9, 90)),
            ["cljs.user", "cljs.core", "shared", "early", "late"]
        );
        // Unit 9 lent position 4 by a waiting unit 4: its own records
        // follow what came before position 4.
        let lent = at(4, 9, 90);
        assert_eq!(
            log.names(&lent),
            ["cljs.user", "cljs.core", "shared", "early", "late"]
        );
        assert!(!log.has("late", &at(8, 8, 80)) && log.has("late", &lent));
        assert_eq!(log.names(&NamespaceView::ALL).len(), 5);
        assert!(log.has("cljs.core", &at(0, 0, 0)));
    }
}
