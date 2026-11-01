//! Whole-tree reservations keep native allocation outside the shared bridge.
//! Only bounded, entirely fresh Datum DAGs use this path. Conversion itself is
//! unchanged, and successful prefixes are published even on ordinary failure.
use super::*;

#[derive(Clone, Default)]
pub(super) struct GeneratedNames(Arc<Mutex<HashSet<JsString>>>);

impl GeneratedNames {
    pub(super) fn insert(&self, name: JsString) -> bool {
        self.0.lock().unwrap().insert(name)
    }
    pub(super) fn remember(&self, scope: &Scope<'_>, key: &[u16]) -> Result<bool, EvalError> {
        let mut names = self.0.lock().unwrap();
        if names.contains(key) {
            return Ok(false);
        }
        // Serialize admission too, so racing misses cannot admit duplicates.
        Ok(names.insert(string(scope, key)?))
    }
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.0.lock().unwrap().is_empty()
    }
}

pub(super) fn reserve_generated_name(bridge: &Mutex<Bridge>, name: JsString) -> bool {
    loop {
        let claim = {
            let bridge = bridge.lock().unwrap();
            match bridge.inflight.values().next() {
                Some(claim) => claim.clone(),
                None => return bridge.generated_names.insert(name),
            }
        };
        // The old import guard ordered spelling admission before reservations.
        // Keep that ordering without holding Bridge while allocation finishes.
        claim.wait();
    }
}

pub(super) struct Claim {
    _root: Datum,
    keys: Vec<usize>,
    finished: Mutex<bool>,
    wake: Condvar,
}

impl Claim {
    pub(super) fn wait(&self) {
        let _wait = crate::wait_timeline::begin(crate::wait_timeline::Kind::BridgeImportWait, 0, 0);
        let mut finished = self.finished.lock().unwrap();
        while !*finished {
            finished = self.wake.wait(finished).unwrap();
        }
    }
}

/// Never drop this while holding Bridge. Unwind cleanup releases reservations;
/// normal errors first publish all successfully constructed descendants.
struct Lease {
    bridge: Arc<Mutex<Bridge>>,
    claim: Arc<Claim>,
}

impl Drop for Lease {
    fn drop(&mut self) {
        {
            let mut bridge = self
                .bridge
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for key in &self.claim.keys {
                if bridge
                    .inflight
                    .get(key)
                    .is_some_and(|c| Arc::ptr_eq(c, &self.claim))
                {
                    bridge.inflight.remove(key);
                }
            }
        }
        *self
            .claim
            .finished
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.claim.wake.notify_all();
    }
}

enum Plan {
    Serial,
    Wait(Arc<Claim>),
    Fresh(Vec<usize>),
}

impl Bridge {
    fn plan_import(&self, data: &Datum, enabled: bool) -> Plan {
        if !enabled && self.inflight.is_empty() {
            return Plan::Serial;
        }
        self.plan_imports(vec![data.clone()], enabled)
    }

    fn plan_imports(&self, mut pending: Vec<Datum>, enabled: bool) -> Plan {
        let mut seen = IdentityMap::default();
        let mut fresh = true;
        while let Some(data) = pending.pop() {
            let key = data.identity();
            if seen.insert(key, ()).is_some() {
                continue;
            }
            if let Some(claim) = self.inflight.get(&key) {
                return Plan::Wait(claim.clone());
            }
            if seen.len() > 4096 {
                // An unvisited suffix might overlap any active reservation.
                return self
                    .inflight
                    .values()
                    .next()
                    .cloned()
                    .map_or(Plan::Serial, Plan::Wait);
            }
            if let Some(imported) = self.imports.get(&key) {
                fresh = false;
                if imported.occurrence.is_live()
                    || imported.native.as_ref().is_some_and(WeakHandle::is_live)
                {
                    continue;
                }
                // Stale imports need the unchanged reimport path. Inspect its
                // descendants too before declaring it disjoint from claims.
            }
            pending.extend(Self::datum_children(&data));
        }
        if enabled && fresh && seen.len() >= 64 {
            Plan::Fresh(seen.into_keys().collect())
        } else {
            Plan::Serial
        }
    }

    pub(super) fn environment_claim(&self, data: &Datum) -> Option<Arc<Claim>> {
        if self.inflight.is_empty() {
            return None;
        }
        let mut pending = vec![data.clone()];
        // Incremental conversion may reimport removed keys from its previous
        // envelope. Their occurrence wrappers can have been reclaimed even
        // when the previous raw map survives. Inspect keys independently of
        // that map's cached frontier; the new input alone is not sufficient.
        if !matches!(data.kind(), D::Nil)
            && let Some((previous, _)) = &self.environment
        {
            let Ok(entries) = previous.entries() else {
                // Planning never reports a new conversion error or reorders
                // the original one. Unrecognized envelopes wait conservatively.
                return self.inflight.values().next().cloned();
            };
            pending.extend(entries.into_iter().map(|(key, _)| key));
        }
        match self.plan_imports(pending, false) {
            Plan::Wait(claim) => Some(claim),
            Plan::Serial => None,
            Plan::Fresh(_) => unreachable!("disabled allocation planner"),
        }
    }

    fn detached(&mut self, data: &Datum, keys: Vec<usize>) -> (Box<Bridge>, Arc<Claim>) {
        let claim = Arc::new(Claim {
            _root: data.clone(),
            keys,
            finished: Mutex::new(false),
            wake: Condvar::new(),
        });
        let mut local = Box::new(Bridge {
            control: self.control.clone(),
            next_token: self.next_token.clone(),
            generated_names: self.generated_names.clone(),
            pending_watches: Some(Vec::new()),
            detached_imports: Some(Vec::new()),
            ..Default::default()
        });
        for &key in &claim.keys {
            assert!(!self.imports.contains_key(&key));
            assert!(self.inflight.insert(key, claim.clone()).is_none());
            if let Some(form) = self.forms.get(&key) {
                local.forms.insert(key, form.clone());
            }
        }
        (local, claim)
    }

    fn publish_import(&mut self, local: &mut Bridge) {
        for key in local.detached_imports.take().unwrap() {
            let mut imported = local.imports.remove(&key).unwrap();
            let cost = local.retained_nodes[imported.retained]
                .as_ref()
                .unwrap()
                .cost
                .unwrap();
            assert!(
                !self.imports.contains_key(&key),
                "reserved input acquired a competing import"
            );
            imported.retained = self.retain_datum(&imported.data);
            let retained = self.retained_nodes[imported.retained].as_mut().unwrap();
            self.cost_entries += usize::from(retained.cost.replace(cost).is_none());
            self.imports.insert(key, imported);
        }
        self.native_objects.extend(local.native_objects.drain());
        self.stats.imported += local.stats.imported;
    }
}

enum Attempt<'s> {
    Done(Local<'s>),
    Wait(Arc<Claim>),
    Detached(Box<Bridge>, Arc<Claim>),
}

impl BbpEvaluator {
    pub(crate) fn enable_parallel_imports(&self) {
        self.parallel_imports.set(true);
    }

    pub(super) fn import_root(
        &self,
        data: &Datum,
        form: Option<&Form>,
        limits: ConversionLimits,
    ) -> Result<Rooted, EvalError> {
        self.host_scope(|scope| {
            let collected = self.runtime.collection_count();
            loop {
                let attempt = self.with_import_bridge(scope, 1, |bridge| {
                    bridge.prune(collected);
                    let plan = bridge.plan_import(data, self.parallel_imports.get());
                    if let Plan::Wait(claim) = plan {
                        return Ok::<_, EvalError>(Attempt::Wait(claim));
                    }
                    // Planning/waiting never consumes the original walk's fuel.
                    if let Some(form) = form {
                        bridge.remember(form, limits.depth, &mut limits.nodes.clone())?;
                    }
                    Ok(match plan {
                        Plan::Serial => Attempt::Done(bridge.import(
                            scope,
                            data,
                            limits.depth,
                            &mut limits.nodes.clone(),
                        )?),
                        Plan::Fresh(keys) => {
                            let (local, claim) = bridge.detached(data, keys);
                            Attempt::Detached(local, claim)
                        }
                        Plan::Wait(_) => unreachable!(),
                    })
                })?;
                match attempt {
                    Attempt::Done(value) => return Ok(scope.persist(value)),
                    Attempt::Wait(claim) => {
                        #[cfg(test)]
                        if let Some(hook) = &*self.import_waiting.borrow() {
                            hook();
                        }
                        claim.wait();
                    }
                    Attempt::Detached(mut local, claim) => {
                        let lease = Lease {
                            bridge: self.bridge.clone(),
                            claim,
                        };
                        #[cfg(test)]
                        if let Some(hook) = &*self.import_claimed.borrow() {
                            hook();
                        }
                        let allocating = crate::wait_timeline::begin(
                            crate::wait_timeline::Kind::BridgeDetachedImport,
                            lease.claim.keys.len() as u64,
                            0,
                        );
                        let result =
                            local.import(scope, data, limits.depth, &mut limits.nodes.clone());
                        drop(allocating);
                        // Includes partial success. All locals remain rooted in
                        // this same no-GC Scope through publication and watches.
                        let queue = self.with_bridge_operation(1, |bridge| {
                            bridge.publish_import(&mut local);
                            bridge.reclaimed.clone()
                        });
                        scope
                            .watch_reclamation_identities(
                                &queue,
                                local.pending_watches.take().unwrap(),
                            )
                            .expect("detached imports remain live in their original scope");
                        drop(lease);
                        return result.map(|value| scope.persist(value));
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    const WAIT: Duration = Duration::from_secs(10);

    fn vector(children: Vec<Datum>) -> Datum {
        Datum::new(D::Collection {
            kind: C::Vector,
            children: children.into(),
        })
    }

    fn tree() -> Datum {
        vector((0..80).map(|n| Datum::new(D::Integer(n))).collect())
    }

    fn paused_import(
        shared: BbpRuntime,
        input: Datum,
        claimed: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    ) -> Result<Rooted, EvalError> {
        let evaluator = BbpEvaluator::shell(shared, 0);
        evaluator.enable_parallel_imports();
        *evaluator.import_claimed.borrow_mut() = Some(Box::new(move || {
            claimed.send(()).unwrap();
            release.recv_timeout(WAIT).unwrap();
        }));
        evaluator.import_root(&input, None, ConversionLimits::default())
    }

    #[test]
    fn disjoint_imports_allocate_concurrently_and_release_all_retention_after_gc() {
        let shared = BbpRuntime::new(&Default::default()).unwrap();
        let (claimed_tx, claimed_rx) = mpsc::channel();
        let (release_a, wait_a) = mpsc::channel();
        let (release_b, wait_b) = mpsc::channel();
        let roots = std::thread::scope(|threads| {
            let (a, b) = (shared.clone(), shared.clone());
            let tx = claimed_tx.clone();
            let a = threads.spawn(move || paused_import(a, tree(), tx, wait_a));
            let b = threads.spawn(move || paused_import(b, tree(), claimed_tx, wait_b));
            let first = claimed_rx.recv_timeout(WAIT);
            let second = claimed_rx.recv_timeout(WAIT);
            // Always unblock both threads before reporting any failed assertion.
            let _ = release_a.send(());
            let _ = release_b.send(());
            assert!(
                first.is_ok() && second.is_ok(),
                "disjoint allocations serialized"
            );
            [a.join().unwrap().unwrap(), b.join().unwrap().unwrap()]
        });
        shared.0.runtime.collect().unwrap();
        shared.0.runtime.scope(|s| {
            let mut bridge = shared.0.bridge.lock().unwrap();
            bridge.prune(shared.0.runtime.collection_count());
            assert!(bridge.inflight.is_empty());
            assert_eq!(bridge.imports.len(), 162);
            for root in &roots {
                let exported = bridge
                    .export(s, s.local(root).unwrap(), 128, &mut 1000)
                    .unwrap();
                assert!(
                    matches!(exported.kind(), D::Collection {children, ..} if children.len() == 80)
                );
            }
        });
        drop(roots);
        shared.0.runtime.collect().unwrap();
        shared.0.runtime.scope(|_| {
            let mut bridge = shared.0.bridge.lock().unwrap();
            bridge.prune(shared.0.runtime.collection_count());
            assert!(bridge.imports.is_empty());
            assert!(bridge.native_objects.is_empty());
            assert!(bridge.retained.is_empty());
            assert_eq!(bridge.cost_entries, 0);
        });
    }

    #[test]
    fn overlapping_imports_wait_and_reuse_the_same_native_child() {
        // Both an enabled worker and a normal evaluator must respect claims.
        for enabled in [false, true] {
            let shared = BbpRuntime::new(&Default::default()).unwrap();
            let child = tree();
            let input = vector(vec![child.clone()]);
            let (claimed_tx, claimed_rx) = mpsc::channel();
            let (release, wait) = mpsc::channel();
            let (waiting_tx, waiting_rx) = mpsc::channel();
            let roots = std::thread::scope(|threads| {
                let runtime = shared.clone();
                let a = threads.spawn(move || paused_import(runtime, input, claimed_tx, wait));
                let claimed = claimed_rx.recv_timeout(WAIT);
                let runtime = shared.clone();
                let b = threads.spawn(move || {
                    let evaluator = BbpEvaluator::shell(runtime, 0);
                    evaluator.parallel_imports.set(enabled);
                    *evaluator.import_waiting.borrow_mut() = Some(Box::new(move || {
                        waiting_tx.send(()).unwrap();
                    }));
                    evaluator.import_root(&child, None, ConversionLimits::default())
                });
                let waiting = waiting_rx.recv_timeout(WAIT);
                let _ = release.send(());
                assert!(
                    claimed.is_ok() && waiting.is_ok(),
                    "overlap bypassed the reservation"
                );
                [a.join().unwrap().unwrap(), b.join().unwrap().unwrap()]
            });
            shared.0.runtime.scope(|s| {
                let child = s
                    .form_nth(s.form_value(s.local(&roots[0]).unwrap()), 0)
                    .unwrap();
                assert!(s.same_object(child, s.local(&roots[1]).unwrap()));
            });
            let bridge = shared.0.bridge.lock().unwrap();
            assert!(bridge.inflight.is_empty());
            assert_eq!(bridge.stats.imported, 82);
        }
    }

    #[test]
    fn failed_detached_import_publishes_successful_prefix_and_reclaims_it() {
        for limits in [
            ConversionLimits {
                nodes: 70,
                depth: 128,
            },
            ConversionLimits {
                nodes: 1000,
                depth: 2,
            },
        ] {
            let shared = BbpRuntime::new(&Default::default()).unwrap();
            let evaluator = BbpEvaluator::shell(shared.clone(), 0);
            evaluator.enable_parallel_imports();
            let children: Vec<_> = (0..80).map(|n| Datum::new(D::Integer(n))).collect();
            let mut input = children.clone();
            input.push(vector(vec![Datum::new(D::Nil)]));
            let input = vector(input);
            assert!(evaluator.import_root(&input, None, limits).is_err());
            {
                let bridge = shared.0.bridge.lock().unwrap();
                assert!(bridge.inflight.is_empty());
                assert!(bridge.pending_watches.is_none());
                assert!(!bridge.imports.contains_key(&input.identity()));
                let expected = if limits.nodes == 70 { 69 } else { 80 };
                assert_eq!(bridge.imports.len(), expected);
                for child in &children[..expected] {
                    assert!(bridge.imports.contains_key(&child.identity()));
                    assert_eq!(bridge.cost(child.identity()), Some((1, 1)));
                }
            }
            shared.0.runtime.collect().unwrap();
            shared.0.runtime.scope(|_| {
                let mut bridge = shared.0.bridge.lock().unwrap();
                bridge.prune(shared.0.runtime.collection_count());
                assert!(bridge.imports.is_empty());
                assert!(bridge.native_objects.is_empty());
                assert!(bridge.retained.is_empty());
                assert_eq!(bridge.cost_entries, 0);
            });
        }
    }

    #[test]
    fn environment_import_waits_before_entering_its_bridge_operation() {
        let shared = BbpRuntime::new(&Default::default()).unwrap();
        let input = tree();
        let (claimed_tx, claimed_rx) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let (waiting_tx, waiting_rx) = mpsc::channel();
        let roots = std::thread::scope(|threads| {
            let runtime = shared.clone();
            let data = input.clone();
            let a = threads.spawn(move || paused_import(runtime, data, claimed_tx, wait));
            let claimed = claimed_rx.recv_timeout(WAIT);
            let runtime = shared.clone();
            let b = threads.spawn(move || {
                let evaluator = BbpEvaluator::shell(runtime, 0);
                *evaluator.import_waiting.borrow_mut() = Some(Box::new(move || {
                    waiting_tx.send(()).unwrap();
                }));
                evaluator.host_scope(|s| {
                    evaluator.with_import_bridge(s, 2, |bridge| {
                        assert!(bridge.inflight.is_empty());
                        s.persist(bridge.import(s, &input, 128, &mut 1000).unwrap())
                    })
                })
            });
            let waiting = waiting_rx.recv_timeout(WAIT);
            let _ = release.send(());
            assert!(claimed.is_ok() && waiting.is_ok());
            [a.join().unwrap().unwrap(), b.join().unwrap()]
        });
        shared.0.runtime.scope(|s| {
            assert!(s.same_object(s.local(&roots[0]).unwrap(), s.local(&roots[1]).unwrap()))
        });
    }

    #[test]
    fn generated_names_observe_spellings_from_inflight_imports() {
        let shared = BbpRuntime::new(&Default::default()).unwrap();
        let mut children = vec![tree()];
        children.push(Datum::new(D::Symbol(Name::new(None, "g1"))));
        let input = vector(children);
        let (claimed_tx, claimed_rx) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let (waiting_tx, waiting_rx) = mpsc::channel();
        std::thread::scope(|threads| {
            let runtime = shared.clone();
            let a = threads.spawn(move || paused_import(runtime, input, claimed_tx, wait));
            let claimed = claimed_rx.recv_timeout(WAIT);
            let runtime = shared.clone();
            let b = threads.spawn(move || {
                let evaluator = BbpEvaluator::shell(runtime, 0);
                *evaluator.import_waiting.borrow_mut() =
                    Some(Box::new(move || waiting_tx.send(()).unwrap()));
                evaluator.unscoped_gensyms(&Datum::new(D::Nil), &["g"])
            });
            let runtime = shared.clone();
            let c = threads.spawn(move || {
                runtime.0.runtime.scope(|s| {
                    let name = string(s, &"ag1".encode_utf16().collect::<Vec<_>>()).unwrap();
                    reserve_generated_name(&runtime.0.bridge, name)
                })
            });
            let waiting = waiting_rx.recv_timeout(WAIT);
            let _ = release.send(());
            assert!(claimed.is_ok() && waiting.is_ok());
            a.join().unwrap().unwrap();
            assert_eq!(b.join().unwrap().unwrap(), ["g2"]);
            assert!(
                !c.join().unwrap(),
                "native gensym stole an imported spelling"
            );
        });
    }

    #[test]
    fn detached_import_keeps_metadata_literal_provenance_and_conversion_costs() {
        let shared = BbpRuntime::new(&Default::default()).unwrap();
        let evaluator = BbpEvaluator::shell(shared.clone(), 0);
        evaluator.enable_parallel_imports();
        let payload = Datum::new(D::String("9007199254740993".encode_utf16().collect()));
        let literal = Datum::new(D::CompilerLiteral {
            kind: L::BigInteger,
            payload: payload.clone(),
        });
        let metadata = Datum::new(D::Collection {
            kind: C::Map,
            children: (0..80).map(|n| Datum::new(D::Integer(n))).collect(),
        });
        let input = vector(vec![literal.clone(), literal.clone()]).with_metadata(metadata.clone());
        let form = Form::from_data(
            input.clone(),
            crate::compiler_data::Source::with_line_offset("detached.cljs", "\nα", 10)
                .span(1..3)
                .unwrap(),
            crate::compiler_data::Origin::Source,
        );
        let root = evaluator
            .import_root(&input, Some(&form), ConversionLimits::default())
            .unwrap();
        let raw = shared.0.runtime.scope(|s| {
            let value = s.local(&root).unwrap();
            let location = s.provenance(value).unwrap();
            assert_eq!((location.line, location.column), (12, 1));
            s.persist(s.form_value(value))
        });
        drop(root);
        shared.0.runtime.collect().unwrap();
        let restored = evaluator
            .import_root(&input, None, ConversionLimits::default())
            .unwrap();
        shared.0.runtime.scope(|s| {
            assert!(s.same_object(
                s.form_value(s.local(&restored).unwrap()),
                s.local(&raw).unwrap()
            ));
            let mut bridge = shared.0.bridge.lock().unwrap();
            assert_eq!(bridge.cost(input.identity()), Some((86, 3)));
            assert_eq!(bridge.cost(payload.identity()), Some((1, 1)));
            assert!(bridge.forms.contains_key(&payload.identity()));
            let output = bridge
                .export(s, s.local(&restored).unwrap(), 128, &mut 1000)
                .unwrap();
            assert_eq!(output.identity(), input.identity());
            assert_eq!(output.metadata().unwrap().identity(), metadata.identity());
        });
        assert!(
            evaluator
                .import_root(
                    &input,
                    None,
                    ConversionLimits {
                        nodes: 85,
                        depth: 128
                    }
                )
                .is_err()
        );
        assert!(
            evaluator
                .import_root(
                    &input,
                    None,
                    ConversionLimits {
                        nodes: 1000,
                        depth: 2
                    }
                )
                .is_err()
        );
        drop((raw, restored));
        shared.0.runtime.collect().unwrap();
        shared.0.runtime.scope(|_| {
            let mut bridge = shared.0.bridge.lock().unwrap();
            bridge.prune(shared.0.runtime.collection_count());
            assert!(bridge.imports.is_empty());
            assert!(bridge.forms.is_empty());
            assert!(bridge.retained.is_empty());
            assert_eq!(bridge.cost_entries, 0);
        });
    }

    #[test]
    fn detached_heap_failure_publishes_the_same_prefix_as_serial_import() {
        let input = vector(
            (0..80)
                .map(|n| Datum::new(D::Integer(n)))
                .chain([Datum::new(D::String(vec![97; 4 << 20].into()))])
                .collect(),
        );
        for enabled in [false, true] {
            let shared = BbpRuntime::new(&EvaluatorOptions {
                memory_limit: 4 << 20,
                ..Default::default()
            })
            .unwrap();
            let evaluator = BbpEvaluator::shell(shared.clone(), 0);
            evaluator.parallel_imports.set(enabled);
            let error = evaluator
                .import_root(&input, None, ConversionLimits::default())
                .unwrap_err();
            assert!(error.to_string().contains("heap byte or string limit"));
            let bridge = shared.0.bridge.lock().unwrap();
            assert!(bridge.inflight.is_empty());
            assert_eq!(bridge.stats.imported, 80);
            assert_eq!(bridge.imports.len(), 80);
        }
    }

    #[test]
    fn detached_conversion_preserves_poll_error_precedence_and_successful_prefix() {
        let input = tree();
        let form = Form::from_data(
            input.clone(),
            crate::compiler_data::Source::new("limits.cljs", "x")
                .span(0..1)
                .unwrap(),
            crate::compiler_data::Origin::Source,
        );
        for remember in [false, true] {
            for poll_budget in [0, 1, 60, 85, 200] {
                for limits in [
                    ConversionLimits { nodes: 0, depth: 0 },
                    ConversionLimits {
                        nodes: 64,
                        depth: 128,
                    },
                    ConversionLimits {
                        nodes: 1000,
                        depth: 1,
                    },
                    ConversionLimits {
                        nodes: 1000,
                        depth: 128,
                    },
                ] {
                    let mut outcomes = Vec::new();
                    for enabled in [false, true] {
                        let shared = BbpRuntime::new(&EvaluatorOptions {
                            poll_budget,
                            ..Default::default()
                        })
                        .unwrap();
                        let evaluator = BbpEvaluator::shell(shared.clone(), 0);
                        evaluator.parallel_imports.set(enabled);
                        let result =
                            evaluator.import_root(&input, remember.then_some(&form), limits);
                        let bridge = shared.0.bridge.lock().unwrap();
                        let mut imported: Vec<_> = bridge.imports.keys().copied().collect();
                        imported.sort_unstable();
                        let mut remembered: Vec<_> = bridge.forms.keys().copied().collect();
                        remembered.sort_unstable();
                        assert!(bridge.inflight.is_empty());
                        outcomes.push((
                            result.err().map(|e| e.to_string()),
                            evaluator.control.remaining_polls(),
                            imported,
                            remembered,
                        ));
                    }
                    assert_eq!(
                        outcomes[0], outcomes[1],
                        "remember={remember}, polls={poll_budget}, limits={limits:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn disjoint_environment_import_finishes_while_fresh_tree_is_paused() {
        let shared = BbpRuntime::new(&Default::default()).unwrap();
        let (claimed_tx, claimed_rx) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::scope(|threads| {
            let runtime = shared.clone();
            let a = threads.spawn(move || paused_import(runtime, tree(), claimed_tx, wait));
            let claimed = claimed_rx.recv_timeout(WAIT);
            let runtime = shared.clone();
            let b = threads.spawn(move || {
                let evaluator = BbpEvaluator::shell(runtime, 0);
                let input = Datum::new(D::Collection {
                    kind: C::Map,
                    children: vec![Datum::new(D::Keyword(Name::new(None, "value"))), tree()].into(),
                });
                let root = evaluator.host_scope(|s| {
                    evaluator.with_import_bridge_for(s, 2, Some(&input), |bridge| {
                        assert!(!bridge.inflight.is_empty(), "claim must still be paused");
                        s.persist(
                            bridge
                                .environment(s, &input, ConversionLimits::default())
                                .unwrap(),
                        )
                    })
                });
                done_tx.send(()).unwrap();
                root
            });
            let done = done_rx.recv_timeout(WAIT);
            let _ = release.send(());
            assert!(
                claimed.is_ok() && done.is_ok(),
                "disjoint environment waited for the claim"
            );
            a.join().unwrap().unwrap();
            b.join().unwrap();
        });
    }

    #[test]
    fn environment_claim_checks_new_values_metadata_and_removed_previous_keys() {
        let shared = BbpRuntime::new(&Default::default()).unwrap();
        let input = tree();
        let (claimed_tx, claimed_rx) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        std::thread::scope(|threads| {
            let runtime = shared.clone();
            let data = input.clone();
            let a = threads.spawn(move || paused_import(runtime, data, claimed_tx, wait));
            let claimed = claimed_rx.recv_timeout(WAIT);
            // Return the observations before any assertion, so a regression
            // always wakes the paused allocator and cannot strand the test.
            let observations = shared.0.runtime.scope(|s| {
                let mut bridge = shared.0.bridge.lock().unwrap();
                let empty = Datum::new(D::Collection {
                    kind: C::Map,
                    children: Vec::new().into(),
                });
                let wrapped = vector(vec![input.clone()]);
                let with_meta = Datum::new(D::Nil).with_metadata(input.clone());
                let literal = Datum::new(D::CompilerLiteral {
                    kind: L::JsArray,
                    payload: input.clone(),
                });
                let a = [wrapped, with_meta, literal]
                    .iter()
                    .all(|data| bridge.environment_claim(data).is_some());
                let previous = Datum::new(D::Collection {
                    kind: C::Map,
                    children: vec![input.clone(), Datum::new(D::Nil)].into(),
                });
                // Model a removed key whose old native occurrence has died:
                // the prior envelope still remembers its Datum identity.
                let raw = s.new_literal_map(&[]).unwrap();
                bridge.environment = Some((previous, s.persist(raw)));
                let b = bridge.environment_claim(&empty).is_some();
                let c = bridge.environment_claim(&Datum::new(D::Nil)).is_none();
                bridge.environment = None;
                (a, b, c)
            });
            let _ = release.send(());
            assert!(claimed.is_ok());
            assert_eq!(observations, (true, true, true));
            a.join().unwrap().unwrap();
        });
    }
}
