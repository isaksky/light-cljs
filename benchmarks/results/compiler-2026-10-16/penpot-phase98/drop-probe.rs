// Diagnostic only: after analysis has completed, measure disjoint release groups.
impl Drop for CompilationSession {
    fn drop(&mut self) {
        use crate::allocation_probe::report;
        let catalog = self.catalog.get_mut();
        let mut counts = [0; 3];
        let mut sizes = [0; 2];
        for unit in &catalog.units {
            let unit = unit.borrow();
            if let AnalysisState::Analyzed(program, _) = &unit.state {
                let group = usize::from(unit.emitted);
                counts[group] += 1;
                sizes[group] += program.exprs.len();
                counts[2] += usize::from(Rc::strong_count(program) == 1);
            }
        }
        eprintln!("retained-programs un-emitted={} emitted={} unique={} logical-exprs-un-emitted={} logical-exprs-emitted={}",
            counts[0], counts[1], counts[2], sizes[0], sizes[1]);
        report("session-before-program-release");
        for unit in &catalog.units {
            let mut unit = unit.borrow_mut();
            if unit.emitted && matches!(unit.state, AnalysisState::Analyzed(..)) {
                unit.state = AnalysisState::Unstarted;
            }
        }
        report("session-emitted-programs-released");
        for unit in &catalog.units {
            let mut unit = unit.borrow_mut();
            if matches!(unit.state, AnalysisState::Analyzed(..)) {
                unit.state = AnalysisState::Unstarted;
            }
        }
        report("session-all-programs-released");
        catalog.units.clear();
        catalog.registrations.clear();
        catalog.bindings.clear();
        catalog.active.clear();
        catalog.multifn_targets.clear();
        report("session-catalog-released");
        catalog.bootstrap = None;
        report("session-bootstrap-released");
    }
}
