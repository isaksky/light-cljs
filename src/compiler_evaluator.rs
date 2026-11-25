//! Engine-independent services used by compiler macro sessions.
#[cfg(not(any(light_v8, light_quickjs)))]
use crate::evaluator::Evaluator;
#[cfg(light_quickjs)]
use crate::evaluator::QuickJsEvaluator;
use crate::evaluator::{EvalError, MacroEvaluator, ValueId};
use std::rc::Rc;

pub(crate) type UnitService = Rc<dyn Fn(&dyn MacroEvaluator, u64) -> Result<String, String>>;
/// A string-to-string service: its answers, errors included, are ordinary
/// strings, so only a panic poisons the session.
pub(crate) type StringService = Rc<dyn Fn(&str) -> String>;
pub(crate) trait CompilerEvaluator: MacroEvaluator {
    /// A data reader exports finite data without a macro call's provenance.
    fn export_data(
        &self,
        _id: ValueId,
        _limits: crate::form_bridge::ConversionLimits,
    ) -> Result<crate::compiler_data::Datum, EvalError> {
        Err(EvalError("this engine does not export reader data".into()))
    }
    fn evaluate_js(&self, expression: &str) -> Result<ValueId, EvalError>;
    fn install_compiler_service(
        self: Rc<Self>,
        name: &str,
        callback: UnitService,
    ) -> Result<(), EvalError>;
    /// Install the global function `name`, which takes one string.
    fn install_string_service(&self, name: &str, callback: StringService) -> Result<(), EvalError>;
    /// Install the shared build object with this heap's caller description.
    fn install_build_object(&self, caller: &str) -> Result<(), EvalError> {
        let value = self.evaluate_js(&format!(
            "(globalThis.lightCljsBuild = ({})(globalThis.__lightBuildService, {caller}), null)",
            crate::build_services::BUILD_OBJECT
        ))?;
        self.release(value)
    }
    #[cfg(all(test, light_v8))]
    fn number(&self, id: ValueId) -> Result<f64, EvalError>;
    /// The memory the engine took from its build's shared budget beyond
    /// its base ([`crate::evaluator::MemoryPool`]), which it gives back
    /// only when it is destroyed.
    fn headroom_held(&self) -> usize {
        0
    }
    /// The reserved share a parallel V8 macro isolate can share with its reader.
    #[cfg(light_v8)]
    fn v8_memory_slot(&self) -> Option<usize> {
        None
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
#[cfg(not(any(light_v8, light_quickjs)))]
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
}
#[cfg(light_quickjs)]
impl CompilerEvaluator for QuickJsEvaluator {
    fn export_data(
        &self,
        id: ValueId,
        limits: crate::form_bridge::ConversionLimits,
    ) -> Result<crate::compiler_data::Datum, EvalError> {
        QuickJsEvaluator::export_data(self, id, limits)
    }
    fn evaluate_js(&self, expression: &str) -> Result<ValueId, EvalError> {
        QuickJsEvaluator::evaluate_js(self, expression)
    }
    fn headroom_held(&self) -> usize {
        QuickJsEvaluator::headroom_held(self)
    }
    fn collect_garbage(&self) {
        QuickJsEvaluator::collect_garbage(self);
    }
    fn value_roots(&self) -> Result<usize, EvalError> {
        QuickJsEvaluator::value_roots(self)
    }
    #[cfg(all(test, light_v8))]
    fn number(&self, id: ValueId) -> Result<f64, EvalError> {
        QuickJsEvaluator::number(self, id)
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
