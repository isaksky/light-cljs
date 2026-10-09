//! Cooperative accounting. A scope is owned by the outermost interpreter entry,
//! not by individual calls, sources, readers, or host callback reentries.
use super::*;
use std::sync::{Arc, atomic::AtomicBool};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceLimit {
    Fuel,
    Allocation,
    Output,
}

/// Terminal control is separate from all language exception values and data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceError {
    LimitExceeded(ResourceLimit),
    Cancelled,
    CleanupLimitExceeded,
    CallDepthExceeded,
    EvaluationDepthExceeded,
    Interrupted,
}
impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ResourceError {}

/// Allocation counts semantic storage units, not allocator bytes or live RSS.
/// Source bytes, form/expression records, collection members and runtime records
/// are charged cumulatively. GC never refunds a budget. Depths are capped by the
/// implementation's native-stack safety guards (32 calls / 128 expressions).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceLimits {
    pub fuel: Option<u64>,
    pub allocation: Option<u64>,
    pub output: Option<u64>,
    pub call_depth: usize,
    pub evaluation_depth: usize,
    pub cleanup_fuel: u64,
    pub cleanup_allocation: u64,
}
impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            fuel: None,
            allocation: None,
            output: None,
            call_depth: 32,
            evaluation_depth: 128,
            cleanup_fuel: 1024,
            cleanup_allocation: 1024,
        }
    }
}
/// May be signalled from another thread. Context and values remain single-threaded.
/// Replacing this token does not clear an active scope's latched cancellation.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);
impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResourceUsage {
    pub fuel: u64,
    pub allocation: u64,
    pub output: u64,
}

#[derive(Default)]
struct State {
    configured: ResourceLimits,
    limits: ResourceLimits,
    usage: ResourceUsage,
    entries: usize,
    terminal: Option<ResourceError>,
    interruption: Option<String>,
    cleanup_depth: usize,
    cleanup_started: bool,
    cleanup_exhausted: bool,
    cleanup_fuel: u64,
    cleanup_allocation: u64,
    token: Option<CancellationToken>,
    active_token: Option<CancellationToken>,
    poll: Option<ExecutionPoll>,
    active_poll: Option<ExecutionPoll>,
}
#[derive(Default)]
pub(super) struct Accounting(RefCell<State>);
pub(super) struct Scope(Rc<Accounting>);
impl Drop for Scope {
    fn drop(&mut self) {
        let mut state = self.0.0.borrow_mut();
        state.entries -= 1;
        if state.entries == 0 {
            state.terminal = None;
            state.interruption = None;
            state.active_token = None;
            state.active_poll = None;
        }
    }
}
impl Accounting {
    pub(super) fn is_active(&self) -> bool {
        self.0.borrow().entries != 0
    }
    pub(super) fn enter(self: &Rc<Self>) -> Scope {
        let mut s = self.0.borrow_mut();
        if s.entries == 0 {
            s.limits = s.configured;
            s.usage = ResourceUsage::default();
            s.terminal = None;
            s.interruption = None;
            s.cleanup_depth = 0;
            s.cleanup_started = false;
            s.cleanup_exhausted = false;
            s.active_token = s.token.clone();
            s.active_poll = s.poll;
        }
        s.entries += 1;
        Scope(self.clone())
    }
    pub(super) fn fail(&self, error: ResourceError) -> ResourceError {
        let mut s = self.0.borrow_mut();
        if s.cleanup_depth != 0 {
            s.cleanup_exhausted = true;
            return error;
        }
        *s.terminal.get_or_insert(error)
    }
    pub(super) fn check(&self) -> Result<(), ResourceError> {
        let mut s = self.0.borrow_mut();
        if s.entries == 0 {
            return Ok(());
        }
        if s.cleanup_depth != 0 {
            return if s.cleanup_exhausted {
                Err(ResourceError::CleanupLimitExceeded)
            } else {
                Ok(())
            };
        }
        if let Some(e) = s.terminal {
            return Err(e);
        }
        if s.active_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            s.terminal = Some(ResourceError::Cancelled);
            return Err(ResourceError::Cancelled);
        }
        Ok(())
    }
    pub(super) fn charge(&self, resource: ResourceLimit, amount: u64) -> Result<(), ResourceError> {
        self.check()?;
        let mut s = self.0.borrow_mut();
        if s.entries == 0 {
            return Ok(());
        }
        if s.cleanup_depth != 0 && resource != ResourceLimit::Output {
            let remaining = if resource == ResourceLimit::Fuel {
                &mut s.cleanup_fuel
            } else {
                &mut s.cleanup_allocation
            };
            if amount > *remaining {
                *remaining = 0;
                s.cleanup_exhausted = true;
                return Err(ResourceError::CleanupLimitExceeded);
            }
            *remaining -= amount;
            return Ok(());
        }
        let limit = match resource {
            ResourceLimit::Fuel => s.limits.fuel,
            ResourceLimit::Allocation => s.limits.allocation,
            ResourceLimit::Output => s.limits.output,
        };
        let counter = match resource {
            ResourceLimit::Fuel => &mut s.usage.fuel,
            ResourceLimit::Allocation => &mut s.usage.allocation,
            ResourceLimit::Output => &mut s.usage.output,
        };
        let next = counter.checked_add(amount);
        if next.is_none() || limit.is_some_and(|limit| next.unwrap() > limit) {
            let error = ResourceError::LimitExceeded(resource);
            if s.cleanup_depth != 0 {
                s.cleanup_exhausted = true;
            }
            return Err(*s.terminal.get_or_insert(error));
        }
        *counter = next.unwrap();
        Ok(())
    }
    pub(super) fn poll(&self, point: PollPoint) -> Result<(), ResourceError> {
        self.charge(ResourceLimit::Fuel, 1)?;
        let callback = {
            let s = self.0.borrow();
            if s.entries == 0
                || s.cleanup_depth != 0
                || !matches!(
                    point,
                    PollPoint::Function | PollPoint::Recur | PollPoint::Iteration
                )
            {
                None
            } else {
                s.active_poll
            }
        };
        if let Some(callback) = callback
            && let Err(message) = callback(point)
        {
            let mut s = self.0.borrow_mut();
            if s.terminal.is_none() {
                s.terminal = Some(ResourceError::Interrupted);
                s.interruption = Some(message);
            }
            return Err(s.terminal.unwrap());
        }
        self.check()
    }
    pub(super) fn work(&self, amount: u64) -> Result<(), ResourceError> {
        self.charge(ResourceLimit::Fuel, amount)
    }
    pub(super) fn kind(&self, error: ResourceError) -> EvalErrorKind {
        match error {
            ResourceError::CallDepthExceeded => EvalErrorKind::CallDepthExceeded,
            ResourceError::EvaluationDepthExceeded => EvalErrorKind::EvaluationDepthExceeded,
            ResourceError::CleanupLimitExceeded => EvalErrorKind::CleanupLimitExceeded,
            ResourceError::Interrupted => EvalErrorKind::ExecutionInterrupted(
                self.0
                    .borrow()
                    .interruption
                    .clone()
                    .unwrap_or_else(|| "interrupted".into()),
            ),
            error => EvalErrorKind::Resource(error),
        }
    }
    // Preserve per-node control polling and exact exhaustion diagnostics whenever
    // the host observes or limits analysis work. Cancellation remains checked at
    // the analysis boundary even when reuse is enabled.
    pub(super) fn allows_analysis_reuse(&self) -> bool {
        let s = self.0.borrow();
        s.limits.fuel.is_none()
            && s.limits.allocation.is_none()
            && s.limits.evaluation_depth >= 128
            && s.active_poll.is_none()
            && s.cleanup_depth == 0
    }
    pub(super) fn depths(&self) -> (usize, usize) {
        let s = self.0.borrow();
        (
            s.limits.call_depth.min(32),
            s.limits.evaluation_depth.min(128),
        )
    }
    pub(super) fn begin_cleanup(&self) {
        let mut s = self.0.borrow_mut();
        if !s.cleanup_started {
            s.cleanup_started = true;
            s.cleanup_fuel = s.limits.cleanup_fuel;
            s.cleanup_allocation = s.limits.cleanup_allocation;
        }
        s.cleanup_depth += 1;
    }
    pub(super) fn end_cleanup(&self) {
        self.0.borrow_mut().cleanup_depth -= 1;
    }
}
impl Context {
    /// Changes apply at the next outermost entry; reentry cannot reset a budget.
    pub fn set_resource_limits(&mut self, limits: ResourceLimits) {
        self.accounting.0.borrow_mut().configured = limits;
    }
    pub fn resource_limits(&self) -> ResourceLimits {
        self.accounting.0.borrow().configured
    }
    pub fn resource_usage(&self) -> ResourceUsage {
        self.accounting.0.borrow().usage
    }
    pub fn set_cancellation_token(&mut self, token: Option<CancellationToken>) {
        self.accounting.0.borrow_mut().token = token;
    }
    pub fn set_execution_poll(&mut self, poll: Option<ExecutionPoll>) {
        let mut s = self.accounting.0.borrow_mut();
        s.poll = poll;
        s.active_poll = poll;
    }
    /// Host work, future macro expansion and output hooks use the same scope.
    /// Poll before each bounded unit of work; arbitrary native code is not preempted.
    pub fn checkpoint(&self, point: PollPoint) -> Result<(), ResourceError> {
        self.accounting.poll(point)
    }
    /// Reserve semantic storage units BEFORE constructing a large host payload.
    pub fn charge_allocation(&self, units: u64) -> Result<(), ResourceError> {
        self.accounting.charge(ResourceLimit::Allocation, units)
    }
    /// Reserve output bytes BEFORE forwarding them to the host's output sink.
    pub fn charge_output(&self, bytes: u64) -> Result<(), ResourceError> {
        self.accounting.charge(ResourceLimit::Output, bytes)
    }
    pub(super) fn poll_execution(&self, point: PollPoint) -> Result<(), EvalErrorKind> {
        self.checkpoint(point).map_err(|e| self.accounting.kind(e))
    }
}
impl From<ResourceError> for NativeError {
    fn from(error: ResourceError) -> Self {
        InvokeError::Dispatch(EvalErrorKind::Resource(error)).into()
    }
}
