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
//! production, a shared lazy realization), or reads a lazy realization that
//! has already settled, the creation counts at that earlier position, after
//! what the waiter created before it waited, as with one slot, where the
//! work runs inside the waiter
//! ([`crate::effect_lanes::Lanes::namespace_floors`],
//! [`crate::effect_lanes::Lanes::floor_consumed`]). A reader waits for
//! its lane first and then sees what counts before it ([`NamespaceView`]),
//! so every thread count gives the one-slot answer.
//!
//! Of a name's creations only some can decide what a reader sees: the
//! earliest one before every activity (position 0), which counts first for
//! every reader, and otherwise, for each chain of owning work, the earliest
//! at its lowest position, which counts for every reader a later one of
//! that work would, and no later. The log keeps just those. A registration
//! names every namespace of its program, so recording each name made every
//! activity cost the program's size under the registry's lock, and four
//! threads slower than two (light-cljs-702). The programs of one compilation
//! are snapshots of one growing table, so the log keeps that table once and
//! records a registration as one entry, its tag and how much of the table it
//! names, keeping only the entries of each chain that can still be some
//! name's earliest; names a table does not share with it are recorded one by
//! one. Readers combine both per name.
//!
//! Names are exact UTF-16 code units, as JavaScript strings and native
//! symbols keep them.
use crate::effect_lanes::{NamespaceTag, NamespaceView, Position, Target};
use std::collections::HashMap;

/// Where a creation counts: its position and order, or the floor of the
/// work it is part of when that is earlier, then its own order there.
type Place = (Position, u64, u64);

type Floors = HashMap<Target, (Position, u64)>;

/// The place of a creation at `position` and `order` by the work `owners`.
fn place(position: Position, order: u64, owners: &[Target], floors: &Floors) -> Place {
    let own = (position, order, 0);
    (owners.iter())
        .filter_map(|owner| floors.get(owner))
        .filter(|&&(floor, _)| floor < position)
        .map(|&(floor, floor_order)| (floor, floor_order, order))
        .fold(own, std::cmp::min)
}

/// An interned chain of owning work ([`NamespaceTag::owners`]).
type ChainId = u32;

/// The creations of one name recorded one by one: the earliest at position
/// 0, or else each chain's earliest at its lowest position.
#[derive(Default)]
struct Created {
    zero: Option<u64>,
    chains: HashMap<ChainId, (Position, u64)>,
}

/// A registration of the program table's first `len` names at `position`:
/// name `i` was created at order `first + i`.
#[derive(Clone, Copy, Debug)]
struct Registration {
    position: Position,
    first: u64,
    len: usize,
}

/// A chain of owning work and its registrations that can still be some
/// name's earliest: none at the same or a lower position names as much.
struct Chain {
    owners: Vec<Target>,
    registrations: Vec<Registration>,
    longest: usize,
}

impl Chain {
    /// The earliest registration at the lowest position that names the
    /// table's name `index`, as its position and its order for that name.
    fn earliest(&self, index: usize) -> Option<(Position, u64)> {
        (self.registrations.iter())
            .filter(|r| r.len > index)
            .map(|r| (r.position, r.first + index as u64))
            .min()
    }
}

pub(crate) struct NamespaceLog {
    created: HashMap<Vec<u16>, Created>,
    chain_ids: HashMap<Vec<Target>, ChainId>,
    chains: Vec<Chain>,
    /// The program's namespace table every compatible registration names a
    /// prefix of, and the first index of each name in it.
    table: Vec<Vec<u16>>,
    index: HashMap<Vec<u16>, usize>,
    /// Registrations at position 0, each naming more than the one before.
    zero: Vec<Registration>,
    #[cfg(test)]
    chain_visits: std::cell::Cell<usize>,
}

impl Default for NamespaceLog {
    /// `cljs.user`, then `cljs.core`, before anything, as in the legacy
    /// runtimes.
    fn default() -> Self {
        let mut log = Self {
            created: HashMap::new(),
            chain_ids: HashMap::new(),
            chains: Vec::new(),
            table: Vec::new(),
            index: HashMap::new(),
            zero: Vec::new(),
            #[cfg(test)]
            chain_visits: std::cell::Cell::new(0),
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
    #[cfg(test)]
    fn visit_chain(&self) {
        self.chain_visits.set(self.chain_visits.get() + 1);
    }

    fn chain(&mut self, owners: &[Target]) -> ChainId {
        if let Some(&id) = self.chain_ids.get(owners) {
            return id;
        }
        let id = self.chains.len() as ChainId;
        self.chains.push(Chain {
            owners: owners.to_vec(),
            registrations: Vec::new(),
            longest: 0,
        });
        self.chain_ids.insert(owners.to_vec(), id);
        id
    }

    /// Whether a registration at position 0 names the table's first `len`
    /// names, so nothing later decides where they count.
    fn zero_covers(&self, len: usize) -> bool {
        self.zero.last().is_some_and(|zero| zero.len >= len)
    }

    /// Note that the activity at `tag` created (or registered) `name`.
    pub(crate) fn record(&mut self, name: &[u16], tag: &NamespaceTag) {
        if let Some(&index) = self.index.get(name)
            && self.zero_covers(index + 1)
        {
            return;
        }
        let chain = (tag.position != 0).then(|| self.chain(&tag.owners));
        let created = match self.created.get_mut(name) {
            Some(created) => created,
            None => self.created.entry(name.to_vec()).or_default(),
        };
        if created.zero.is_some() {
            return;
        }
        let order = crate::effect_lanes::next_order();
        match chain {
            None => {
                created.zero = Some(order);
                created.chains = HashMap::new();
            }
            Some(chain) => {
                let earliest = created.chains.entry(chain).or_insert((tag.position, order));
                *earliest = (*earliest).min((tag.position, order));
            }
        }
    }

    /// Note that the activity at `tag` registered the namespaces of
    /// `table`, a program's in its order, whose first `known` names this
    /// log's table is already known to start with. Returns how many of its
    /// names this log's table starts with now.
    pub(crate) fn register(
        &mut self,
        table: &crate::arena::Arena<String>,
        known: usize,
        tag: &NamespaceTag,
    ) -> usize {
        let shared = table.len().min(self.table.len());
        let mut same = known.min(shared);
        for name in table.range(same..shared) {
            if !name.encode_utf16().eq(self.table[same].iter().copied()) {
                break;
            }
            same += 1;
        }
        if same == shared {
            for name in table.tail_from(shared) {
                let name: Vec<u16> = name.encode_utf16().collect();
                self.index.entry(name.clone()).or_insert(self.table.len());
                self.table.push(name);
            }
            same = table.len();
        }
        self.register_prefix(same, tag);
        for name in table.tail_from(same) {
            self.record(&name.encode_utf16().collect::<Vec<_>>(), tag);
        }
        same
    }

    /// Record that the activity at `tag` created the table's first `len`
    /// names, unless earlier registrations decide where all of them count.
    fn register_prefix(&mut self, len: usize, tag: &NamespaceTag) {
        if len == 0 || self.zero_covers(len) {
            return;
        }
        let position = tag.position;
        if position == 0 {
            let first = crate::effect_lanes::next_orders(len as u64);
            self.zero.push(Registration {
                position,
                first,
                len,
            });
            return;
        }
        let id = self.chain(&tag.owners);
        let chain = &mut self.chains[id as usize];
        if (chain.registrations.iter()).any(|r| r.position <= position && r.len >= len) {
            return;
        }
        chain
            .registrations
            .retain(|r| !(position < r.position && len >= r.len));
        chain.registrations.push(Registration {
            position,
            first: crate::effect_lanes::next_orders(len as u64),
            len,
        });
        chain.longest = chain.longest.max(len);
    }

    /// The least place of `name`'s creations among those that can decide
    /// it, of those `keep` accepts.
    fn first_place(
        &self,
        name: &[u16],
        floors: &Floors,
        keep: impl Fn(&Place) -> bool,
    ) -> Option<Place> {
        let created = self.created.get(name);
        let index = self.index.get(name).copied();
        let zero_registered = index.and_then(|index| {
            let at = self.zero.partition_point(|zero| zero.len <= index);
            self.zero.get(at).map(|zero| zero.first + index as u64)
        });
        let zero = match (created.and_then(|c| c.zero), zero_registered) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if let Some(order) = zero {
            return Some((0, order, 0)).filter(&keep);
        }
        let mut first: Option<Place> = None;
        let mut consider = |position, order, owners: &[Target]| {
            let place = place(position, order, owners, floors);
            if keep(&place) && first.is_none_or(|first| place < first) {
                first = Some(place);
            }
        };
        let none = HashMap::new();
        let singles = created.map_or(&none, |c| &c.chains);
        for (&id, &(position, order)) in singles {
            #[cfg(test)]
            self.visit_chain();
            let chain = &self.chains[id as usize];
            let registered = index.and_then(|index| chain.earliest(index));
            let (position, order) =
                registered.map_or((position, order), |r| r.min((position, order)));
            consider(position, order, &chain.owners);
        }
        if let Some(index) = index {
            for (id, chain) in self.chains.iter().enumerate() {
                #[cfg(test)]
                self.visit_chain();
                if chain.longest <= index || singles.contains_key(&(id as ChainId)) {
                    continue;
                }
                if let Some((position, order)) = chain.earliest(index) {
                    consider(position, order, &chain.owners);
                }
            }
        }
        first
    }

    /// Whether `view` sees namespace `name`, under `floors`.
    pub(crate) fn has(&self, name: &[u16], view: &NamespaceView, floors: &Floors) -> bool {
        let created = self.created.get(name);
        let index = self.index.get(name).copied();
        if created.is_some_and(|c| c.zero.is_some())
            || index.is_some_and(|index| self.zero_covers(index + 1))
        {
            return true;
        }
        // Existence needs no ordering within a position, nor the minimum
        // place across chains. A creation is visible at its own position
        // or at any earlier floor its owners lend it.
        let visible = |position, owners: &[Target]| {
            position <= view.effective
                || owners.iter().any(|owner| {
                    floors
                        .get(owner)
                        .is_some_and(|&(floor, _)| floor <= view.effective)
                })
        };
        if let Some(created) = created {
            for (&id, &(position, _)) in &created.chains {
                #[cfg(test)]
                self.visit_chain();
                if visible(position, &self.chains[id as usize].owners) {
                    return true;
                }
            }
        }
        index.is_some_and(|index| {
            self.chains.iter().any(|chain| {
                #[cfg(test)]
                self.visit_chain();
                chain.longest > index
                    && chain
                        .registrations
                        .iter()
                        .any(|r| r.len > index && visible(r.position, &chain.owners))
            })
        })
    }

    /// The names `view` sees under `floors`, in the order they count in.
    pub(crate) fn names(&self, view: &NamespaceView, floors: &Floors) -> Vec<&[u16]> {
        let keep = |place: &Place| place.0 <= view.effective;
        let registered = (self.index.keys()).filter(|name| !self.created.contains_key(*name));
        let mut seen: Vec<(Place, &[u16])> = (self.created.keys().chain(registered))
            .filter_map(|name| Some((self.first_place(name, floors, keep)?, name.as_slice())))
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

    #[test]
    fn visible_lookup_stops_after_first_matching_chain() {
        let mut log = NamespaceLog::default();
        let app = name("app");
        let table: crate::arena::Arena<String> = ["app".to_owned()].into_iter().collect();
        const CHAINS: usize = 4096;
        for unit in 1..=CHAINS {
            log.register(&table, 1, &tag(unit as u64, &[Target::Unit(unit)]));
        }
        assert_eq!(log.chains.len(), CHAINS);
        let none = Floors::new();
        for _ in 0..32 {
            log.chain_visits.set(0);
            assert!(log.has(&app, &NamespaceView::ALL, &none));
            assert_eq!(log.chain_visits.get(), 1);
        }

        // Skip an invisible first creation, then stop at the second one's
        // lent floor. A miss must still inspect all the registrations.
        let at_zero = NamespaceView { effective: 0 };
        let floors = Floors::from([(Target::Unit(2), (0, 0))]);
        log.chain_visits.set(0);
        assert!(log.has(&app, &at_zero, &floors));
        assert_eq!(log.chain_visits.get(), 2);
        log.chain_visits.set(0);
        assert!(!log.has(&app, &at_zero, &none));
        assert_eq!(log.chain_visits.get(), CHAINS);
        log.chain_visits.set(0);
        assert!(!log.has(&name("missing"), &NamespaceView::ALL, &none));
        assert_eq!(log.chain_visits.get(), 0);

        // Single creations also stop at the first visible chain, whatever
        // the hash table's iteration order. Position zero needs no walk.
        let made = name("made");
        for unit in 1..=CHAINS {
            log.record(&made, &tag(unit as u64, &[Target::Unit(unit)]));
        }
        log.chain_visits.set(0);
        assert!(log.has(&made, &NamespaceView::ALL, &none));
        assert_eq!(log.chain_visits.get(), 1);
        log.record(&made, &NamespaceTag::default());
        log.register(&table, 1, &NamespaceTag::default());
        log.chain_visits.set(0);
        assert!(log.has(&made, &at_zero, &none));
        assert!(log.has(&app, &at_zero, &none));
        assert_eq!(log.chain_visits.get(), 0);
    }

    /// The log before light-cljs-702, which recorded every name of every
    /// registration and kept each name's creations in a list.
    #[derive(Default)]
    struct Reference(HashMap<Vec<u16>, Vec<(NamespaceTag, u64)>>);

    impl Reference {
        fn new() -> Self {
            let mut log = Self::default();
            for name in ["cljs.user", "cljs.core"] {
                log.record(&self::name(name), &NamespaceTag::default());
            }
            log
        }
        fn covers(earlier: &NamespaceTag, tag: &NamespaceTag) -> bool {
            earlier.position <= tag.position
                && (earlier.position == 0 || earlier.owners == tag.owners)
        }
        fn record(&mut self, name: &[u16], tag: &NamespaceTag) {
            let records = self.0.entry(name.to_vec()).or_default();
            if records.iter().any(|(r, _)| Self::covers(r, tag)) {
                return;
            }
            records.retain(|(r, _)| !Self::covers(tag, r));
            records.push((tag.clone(), crate::effect_lanes::next_order()));
        }
        fn place(&self, name: &[u16], floors: &Floors) -> Vec<Place> {
            self.0.get(name).map_or_else(Vec::new, |records| {
                (records.iter())
                    .map(|(tag, order)| place(tag.position, *order, &tag.owners, floors))
                    .collect()
            })
        }
        fn has(&self, name: &[u16], view: &NamespaceView, floors: &Floors) -> bool {
            (self.place(name, floors).iter()).any(|place| place.0 <= view.effective)
        }
        fn names(&self, view: &NamespaceView, floors: &Floors) -> Vec<Vec<u16>> {
            let mut seen: Vec<(Place, Vec<u16>)> = (self.0.keys())
                .filter_map(|name| {
                    let first = (self.place(name, floors).into_iter())
                        .filter(|place| place.0 <= view.effective)
                        .min()?;
                    Some((first, name.clone()))
                })
                .collect();
            seen.sort_unstable();
            seen.into_iter().map(|(_, name)| name).collect()
        }
    }

    /// light-cljs-702: a registration is one entry per chain of work, not
    /// one per name, and readers see exactly what recording every name of
    /// every registration gave, for tables that grow, shrink, repeat a
    /// name or diverge, single creations at any position, and floors set
    /// between them.
    #[test]
    fn registrations_record_once_and_read_as_every_name_recorded() {
        let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = move |bound: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % bound
        };
        for round in 0..40 {
            let mut log = NamespaceLog::default();
            let mut reference = Reference::new();
            let mut floors = Floors::new();
            let program: Vec<String> = (0..24).map(|i| format!("ns{}", i % 21)).collect();
            // Each task's last table and how much of it the log's starts
            // with, as `record_namespaces` keeps them.
            let mut known: HashMap<usize, (crate::arena::Arena<String>, usize)> = HashMap::new();
            for _ in 0..60 {
                let owners: Vec<Target> = match next(4) {
                    0 => vec![],
                    1 => vec![Target::Unit(next(6) as usize)],
                    2 => vec![Target::Unit(next(6) as usize), Target::Lazy(next(3))],
                    _ => vec![Target::Cell(next(3)), Target::Unit(next(6) as usize)],
                };
                let position = if round % 2 == 1 && next(8) == 0 {
                    0
                } else {
                    1 + next(5)
                };
                let tag = tag(position, &owners);
                match next(6) {
                    0 => {
                        let n = name(&format!("ns{}", next(26)));
                        log.record(&n, &tag);
                        reference.record(&n, &tag);
                    }
                    1 => {
                        let target = match next(3) {
                            0 => Target::Lazy(next(3)),
                            1 => Target::Cell(next(3)),
                            _ => Target::Unit(next(6) as usize),
                        };
                        let floor = (next(6), crate::effect_lanes::next_order());
                        let entry = floors.entry(target).or_insert(floor);
                        *entry = (*entry).min(floor);
                    }
                    _ => {
                        let task = next(3) as usize;
                        let len = next(program.len() as u64 + 1) as usize;
                        let mut table: crate::arena::Arena<String> =
                            program[..len].iter().cloned().collect();
                        if round % 4 == 3 && next(4) == 0 && len > 2 {
                            table = program[..len - 2].iter().cloned().collect();
                            table.push("diverged".into());
                            table.push(program[len - 1].clone());
                        }
                        let from = known.get(&task).map_or(0, |(last, known)| {
                            let shared = last.len().min(table.len());
                            let same = (table.changed_entries(last, shared).first())
                                .copied()
                                .unwrap_or(shared);
                            same.min(*known)
                        });
                        let known_now = log.register(&table, from, &tag);
                        known.insert(task, (table.clone(), known_now));
                        for n in &table {
                            reference.record(&name(n), &tag);
                        }
                    }
                }
                for effective in [0, 1, 2, 3, 4, 5, Position::MAX] {
                    let view = NamespaceView { effective };
                    let names: Vec<Vec<u16>> = (log.names(&view, &floors).into_iter())
                        .map(<[u16]>::to_vec)
                        .collect();
                    assert_eq!(names, reference.names(&view, &floors), "round {round}");
                    for i in 0..27 {
                        let n = name(&format!("ns{i}"));
                        assert_eq!(
                            log.has(&n, &view, &floors),
                            reference.has(&n, &view, &floors),
                            "round {round}, ns{i} at {effective}"
                        );
                    }
                }
            }
        }
    }

    /// light-cljs-702: activities that register the same program keep one
    /// entry each, not one per name, and a repeated registration of the
    /// same work adds nothing.
    #[test]
    fn registrations_cost_their_new_names_not_the_program() {
        let mut log = NamespaceLog::default();
        let table: crate::arena::Arena<String> = (0..1000).map(|i| format!("ns{i}")).collect();
        for unit in 1..=50 {
            let tag = tag(unit as u64, &[Target::Unit(unit)]);
            assert_eq!(log.register(&table, 0, &tag), 1000);
            assert_eq!(log.register(&table, 1000, &tag), 1000);
        }
        assert_eq!(log.table.len(), 1000);
        assert_eq!(log.created.len(), 2);
        assert_eq!(log.chains.len(), 50);
        assert!(log.chains.iter().all(|c| c.registrations.len() == 1));
        let none = HashMap::new();
        let view = NamespaceView { effective: 1 };
        assert_eq!(log.names(&view, &none).len(), 1002);
        assert!(log.has(&name("ns999"), &view, &none));
        assert!(!log.has(&name("ns999"), &NamespaceView { effective: 0 }, &none));
    }
}
