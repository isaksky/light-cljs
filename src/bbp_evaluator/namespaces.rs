//! The compilation's namespace registry for native macros (light-cljs-589):
//! what `all-ns`, `find-ns` and `the-ns` see, shared by every task of the
//! runtime, in the order one slot creates the namespaces.
//!
//! The legacy runtimes keep one registry per engine: the namespaces each
//! reader registration names (a macro's or a unit's program, as the engine
//! compiles it) and those `create-ns` makes, which includes every macro
//! call's namespace. A native task's creation goes into this log tagged with
//! its activity's place in the effect lanes ([`NamespaceTag`]), without
//! waiting, so macro calls stay parallel. When an earlier activity later
//! waits for the work that made a creation (a unit's analysis, a cell's
//! production, a shared lazy realization), the creation counts at that
//! earlier position, after what the waiter created before it waited, as
//! with one slot, where the work runs inside the waiter
//! ([`crate::effect_lanes::Lanes::namespace_floors`]). A reader waits for
//! its lane first and then sees what counts before it ([`NamespaceView`]),
//! so every thread count gives the one-slot answer.
//!
//! Names are exact UTF-16 code units, as JavaScript strings and native
//! symbols keep them.
use crate::effect_lanes::{NamespaceTag, NamespaceView, Position, Target};
use std::collections::HashMap;

/// One creation of a namespace: where, and when among all creations
/// ([`crate::effect_lanes::next_order`]).
struct Record {
    tag: NamespaceTag,
    order: u64,
}

/// Where a record counts: its position and order, or the floor of the work
/// it is part of when that is earlier, then its own order there.
type Place = (Position, u64, u64);

impl Record {
    /// Whether this record counts for every reader one at `tag` would, and
    /// no later: one before every activity, or one of the same work at the
    /// same or an earlier position.
    fn covers(&self, tag: &NamespaceTag) -> bool {
        self.tag.position <= tag.position
            && (self.tag.position == 0 || self.tag.owners == tag.owners)
    }
    fn place(&self, floors: &HashMap<Target, (Position, u64)>) -> Place {
        let own = (self.tag.position, self.order, 0);
        (self.tag.owners.iter())
            .filter_map(|owner| floors.get(owner))
            .filter(|&&(position, _)| position < self.tag.position)
            .map(|&(position, order)| (position, order, self.order))
            .fold(own, std::cmp::min)
    }
}

pub(crate) struct NamespaceLog {
    records: HashMap<Vec<u16>, Vec<Record>>,
}

impl Default for NamespaceLog {
    /// `cljs.user`, then `cljs.core`, before anything, as in the legacy
    /// runtimes.
    fn default() -> Self {
        let mut log = Self {
            records: HashMap::new(),
        };
        for name in ["cljs.user", "cljs.core"] {
            log.record(
                &name.encode_utf16().collect::<Vec<_>>(),
                &NamespaceTag::default(),
            );
        }
        log
    }
}

impl NamespaceLog {
    /// Note that the activity at `tag` created (or registered) `name`.
    pub(crate) fn record(&mut self, name: &[u16], tag: &NamespaceTag) {
        let records = match self.records.get_mut(name) {
            Some(records) => records,
            None => self.records.entry(name.to_vec()).or_default(),
        };
        if records.iter().any(|r| r.covers(tag)) {
            return;
        }
        let new = Record {
            tag: tag.clone(),
            order: crate::effect_lanes::next_order(),
        };
        records.retain(|r| !new.covers(&r.tag));
        records.push(new);
    }

    /// Whether `view` sees namespace `name`, under `floors`.
    pub(crate) fn has(
        &self,
        name: &[u16],
        view: &NamespaceView,
        floors: &HashMap<Target, (Position, u64)>,
    ) -> bool {
        self.records
            .get(name)
            .is_some_and(|records| (records.iter()).any(|r| r.place(floors).0 <= view.effective))
    }

    /// The names `view` sees under `floors`, in the order they count in.
    pub(crate) fn names(
        &self,
        view: &NamespaceView,
        floors: &HashMap<Target, (Position, u64)>,
    ) -> Vec<&[u16]> {
        let mut seen: Vec<(Place, &[u16])> = self
            .records
            .iter()
            .filter_map(|(name, records)| {
                (records.iter())
                    .map(|r| r.place(floors))
                    .filter(|place| place.0 <= view.effective)
                    .min()
                    .map(|first| (first, name.as_slice()))
            })
            .collect();
        seen.sort_unstable();
        seen.into_iter().map(|(_, name)| name).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(position: u64, owners: &[Target]) -> NamespaceTag {
        NamespaceTag {
            position,
            owners: owners.to_vec(),
        }
    }

    fn text(names: Vec<&[u16]>) -> Vec<String> {
        names.into_iter().map(String::from_utf16_lossy).collect()
    }

    fn name(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn creations_count_where_an_earlier_waiter_runs_their_work() {
        let mut log = NamespaceLog::default();
        let unit = Target::Unit(9);
        let lazy = Target::Lazy(7);
        // Unit 9 runs ahead of unit 4 on another thread: its own creation,
        // then one inside the lazy realization it starts.
        log.record(&name("own"), &tag(9, &[unit]));
        log.record(&name("lent"), &tag(9, &[unit, lazy]));
        log.record(&name("early"), &tag(4, &[Target::Unit(4)]));
        log.record(&name("own"), &tag(9, &[unit]));
        log.record(&name("shared"), &tag(9, &[unit]));
        log.record(&name("shared"), &tag(2, &[Target::Unit(2)]));
        let at = |effective| NamespaceView { effective };
        let none = HashMap::new();
        let user = ["cljs.user", "cljs.core"];
        let expect = |names: &[&'static str]| -> Vec<&'static str> { [&user[..], names].concat() };
        assert_eq!(text(log.names(&at(5), &none)), expect(&["shared", "early"]));
        assert_eq!(
            text(log.names(&at(9), &none)),
            expect(&["shared", "early", "own", "lent"])
        );
        // Unit 4 waits for the realization after it created `early`: with
        // one slot the realization runs inside unit 4, then.
        let floors = HashMap::from([(lazy, (4, crate::effect_lanes::next_order()))]);
        assert_eq!(
            text(log.names(&at(5), &floors)),
            expect(&["shared", "early", "lent"])
        );
        assert!(log.has(&name("lent"), &at(4), &floors));
        assert!(!log.has(&name("lent"), &at(4), &none));
        assert!(!log.has(&name("own"), &at(8), &floors));
        assert_eq!(log.names(&NamespaceView::ALL, &none).len(), 6);
    }

    #[test]
    fn names_keep_their_exact_code_units() {
        let mut log = NamespaceLog::default();
        log.record(&[0xD800], &tag(1, &[]));
        log.record(&[0xD83D, 0xDE00], &tag(1, &[]));
        let none = HashMap::new();
        assert!(log.has(&[0xD800], &NamespaceView::ALL, &none));
        assert!(!log.has(&[0xD801], &NamespaceView::ALL, &none));
        assert!(!log.has(&[0xFFFD], &NamespaceView::ALL, &none));
        let names = log.names(&NamespaceView::ALL, &none);
        assert_eq!(&names[2..], [&[0xD800][..], &[0xD83D, 0xDE00][..]]);
    }
}
