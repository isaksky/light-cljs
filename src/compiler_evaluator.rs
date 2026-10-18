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
}
impl CompilerEvaluator for Evaluator {
    fn evaluate_js(&self, expression: &str) -> Result<ValueId, EvalError> {
        Evaluator::evaluate_js(self, expression)
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
