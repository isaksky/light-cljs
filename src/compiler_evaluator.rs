//! Engine-independent services used by compiler macro sessions.
use crate::evaluator::{EvalError, Evaluator, MacroEvaluator, ValueId};
use std::rc::Rc;

pub(crate) type UnitService = Rc<dyn Fn(&dyn MacroEvaluator, u64) -> Result<String, String>>;
/// A string-to-string service: its answers, errors included, are ordinary
/// strings, so only a panic poisons the session.
pub(crate) type StringService = Rc<dyn Fn(&str) -> String>;
pub(crate) trait CompilerEvaluator: MacroEvaluator {
    fn evaluate_js(&self, expression: &str) -> Result<ValueId, EvalError>;
    fn install_compiler_service(
        self: Rc<Self>,
        name: &str,
        callback: UnitService,
    ) -> Result<(), EvalError>;
    /// Install the global function `name`, which takes one string.
    fn install_string_service(&self, name: &str, callback: StringService) -> Result<(), EvalError>;
    #[cfg(all(test, light_v8))]
    fn number(&self, id: ValueId) -> Result<f64, EvalError>;
    /// Whether the engine holds memory it took from its build's shared
    /// budget beyond its base ([`crate::evaluator::MemoryPool`]), which it
    /// gives back only when it is destroyed.
    fn holds_headroom(&self) -> bool {
        false
    }
    /// Collect the engine's garbage in full, between operations, so that
    /// what it still holds is what is reachable (`--watch --stats`).
    fn collect_garbage(&self) {}
    /// The values the engine's private registry roots for their `ValueId`s
    /// until they are released: bridge roots the process-wide
    /// [`crate::compile_stats::LiveCounts`] does not see, since they live in
    /// the engine and go with it.
    fn value_roots(&self) -> Result<usize, EvalError> {
        Ok(0)
    }
}
#[cfg(not(light_quickjs))]
impl CompilerEvaluator for Evaluator {
    fn evaluate_js(&self, _: &str) -> Result<ValueId, EvalError> {
        match *self {}
    }
    fn install_compiler_service(self: Rc<Self>, _: &str, _: UnitService) -> Result<(), EvalError> {
        match *self {}
    }
    fn install_string_service(&self, _: &str, _: StringService) -> Result<(), EvalError> {
        match *self {}
    }
    #[cfg(all(test, light_v8))]
    fn number(&self, _: ValueId) -> Result<f64, EvalError> {
        match *self {}
    }
}
#[cfg(light_quickjs)]
impl CompilerEvaluator for Evaluator {
    fn evaluate_js(&self, expression: &str) -> Result<ValueId, EvalError> {
        Evaluator::evaluate_js(self, expression)
    }
    fn collect_garbage(&self) {
        Evaluator::collect_garbage(self);
    }
    fn value_roots(&self) -> Result<usize, EvalError> {
        Evaluator::value_roots(self)
    }
    #[cfg(all(test, light_v8))]
    fn number(&self, id: ValueId) -> Result<f64, EvalError> {
        Evaluator::number(self, id)
    }
    fn install_string_service(&self, name: &str, callback: StringService) -> Result<(), EvalError> {
        self.install_callback(name, move |_, request| Ok(callback(&request)))
    }
    fn install_compiler_service(
        self: Rc<Self>,
        name: &str,
        callback: UnitService,
    ) -> Result<(), EvalError> {
        let weak = Rc::downgrade(&self);
        self.install_unit_service(name, move |ctx, id| {
            let evaluator = weak.upgrade().ok_or("evaluator expired")?;
            let active = evaluator.active(ctx).map_err(|e| e.to_string())?;
            callback(&active, id)
        })
    }
}
