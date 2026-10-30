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
//! and then sees what the activities before it created, and what it runs
//! inside an earlier activity created ([`NamespaceView`]), so every thread
//! count gives the one-slot answer.
use crate::effect_lanes::{NamespaceTag, NamespaceView};
use std::collections::HashMap;

/// One creation of a namespace: where, and when among all creations.
struct Record {
    tag: NamespaceTag,
    order: u64,
}

impl Record {
    /// Whether this record shows its namespace to every reader one at
    /// `tag` would, and no later: one before every activity, or one of the
    /// same activity chain at the same or an earlier position.
    fn covers(&self, tag: &NamespaceTag) -> bool {
        self.tag.position <= tag.position && (self.tag.position == 0 || self.tag.chain == tag.chain)
    }
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
            log.record(name, &NamespaceTag::default());
        }
        log
    }
}

impl NamespaceLog {
    /// Note that the activity at `tag` created (or registered) `name`.
    pub(crate) fn record(&mut self, name: &str, tag: &NamespaceTag) {
        let records = match self.records.get_mut(name) {
            Some(records) => records,
            None => self.records.entry(name.to_owned()).or_default(),
        };
        if records.iter().any(|r| r.covers(tag)) {
            return;
        }
        let new = Record {
            tag: tag.clone(),
            order: self.next,
        };
        records.retain(|r| !new.covers(&r.tag));
        records.push(new);
        self.next += 1;
    }

    /// Whether `view` sees namespace `name`.
    pub(crate) fn has(&self, name: &str, view: &NamespaceView) -> bool {
        self.records
            .get(name)
            .is_some_and(|records| records.iter().any(|r| view.sees(&r.tag)))
    }

    /// The names `view` sees, in the order their first creation it sees
    /// happened with one slot: by position, then as recorded.
    pub(crate) fn names(&self, view: &NamespaceView) -> Vec<&str> {
        let mut seen: Vec<((u64, u64), &str)> = self
            .records
            .iter()
            .filter_map(|(name, records)| {
                (records.iter())
                    .filter(|r| view.sees(&r.tag))
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

    fn tag(position: u64, chain: &[u64]) -> NamespaceTag {
        NamespaceTag {
            position,
            chain: chain.to_vec(),
        }
    }

    #[test]
    fn readers_see_earlier_positions_and_what_they_run_for_an_earlier_one() {
        let mut log = NamespaceLog::default();
        // Unit 9 (token 90) runs ahead of unit 4 on another thread; its
        // initializer (token 91) creates `lent` after unit 9 created `own`.
        log.record("own", &tag(9, &[90, 1]));
        log.record("lent", &tag(9, &[91, 90, 1]));
        log.record("early", &tag(4, &[40, 2]));
        log.record("own", &tag(9, &[90, 1]));
        log.record("shared", &tag(9, &[90, 1]));
        log.record("shared", &tag(2, &[20, 3]));
        let at = |effective, lent| NamespaceView { effective, lent };
        let user = ["cljs.user", "cljs.core"];
        assert_eq!(
            log.names(&at(5, None)),
            [&user[..], &["shared", "early"]].concat()
        );
        assert_eq!(
            log.names(&at(9, None)),
            [&user[..], &["shared", "early", "own", "lent"]].concat()
        );
        // Unit 4 waits for the initializer, which then runs at position 4:
        // it sees its own creation, not its unit's.
        let lent = at(4, Some(91));
        assert_eq!(
            log.names(&lent),
            [&user[..], &["shared", "early", "lent"]].concat()
        );
        assert!(!log.has("own", &lent) && log.has("lent", &lent));
        assert!(!log.has("lent", &at(8, None)));
        assert_eq!(log.names(&NamespaceView::ALL).len(), 6);
        assert!(log.has("cljs.core", &at(0, None)));
    }
}
