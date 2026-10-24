//! Task-owned ECMAScript jobs. The inbox is scheduling metadata, never a
//! shared mutable JavaScript object. No user code runs under its leaf lock.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Weak};

use crate::heap::gc::{Access, Trace};
use crate::heap::{Heap, ObjectId, Rooted};
use crate::value::{Value, share};

#[derive(Debug)]
pub(crate) struct Job {
    pub(crate) callable: Value,
    pub(crate) args: Vec<Value>,
}

impl Trace for Job {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.callable.trace(visit);
        self.args.iter().for_each(|v| v.trace(visit));
    }
}

#[derive(Debug, Default)]
struct Inbox {
    queue: VecDeque<Job>,
    closed: bool,
}

#[derive(Default)]
pub(crate) struct JobInbox {
    inbox: Mutex<Inbox>,
    #[cfg(test)]
    pub(crate) boundaries: Arc<std::sync::atomic::AtomicUsize>,
    #[cfg(test)]
    pub(crate) before_push: TestHook,
    #[cfg(test)]
    pub(crate) before_next: TestHook,
    #[cfg(test)]
    pub(crate) before_close: TestHook,
}

impl std::fmt::Debug for JobInbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobInbox")
            .field("inbox", &self.inbox)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
pub(crate) type TestHook = Mutex<Option<Box<dyn FnOnce() + Send>>>;

#[cfg(test)]
fn run_hook(slot: &TestHook) {
    let hook = slot.lock().unwrap().take();
    if let Some(hook) = hook {
        hook();
    }
}

impl JobInbox {
    #[cfg(test)]
    pub(crate) fn next_for_test(&self) -> Value {
        self.inbox
            .lock()
            .unwrap()
            .queue
            .front()
            .expect("queued job")
            .callable
            .clone()
    }
    pub(crate) fn push(&self, job: Job) -> Result<(), JobError> {
        #[cfg(test)]
        run_hook(&self.before_push);
        let mut inbox = self.inbox.lock().unwrap();
        if inbox.closed {
            return Err(JobError::Closed);
        }
        inbox.queue.push_back(job);
        Ok(())
    }

    /// Empty-check and close share the enqueue linearization point. Once
    /// this returns None, no later publication can append a stranded job.
    pub(crate) fn next_or_close(&self) -> Option<Job> {
        #[cfg(test)]
        run_hook(&self.before_next);
        let mut inbox = self.inbox.lock().unwrap();
        let next = inbox.queue.pop_front();
        if next.is_none() {
            #[cfg(test)]
            run_hook(&self.before_close);
            inbox.closed = true;
        }
        next
    }

    pub(crate) fn close(&self) {
        let mut inbox = self.inbox.lock().unwrap();
        inbox.closed = true;
        inbox.queue.clear();
    }

    /// Extension point for ClearKeptObjects: after the script and after
    /// each completed job, before the next job or reporting an outcome.
    pub(crate) fn boundary(&self) {
        #[cfg(test)]
        self.boundaries
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Trace for JobInbox {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.inbox
            .lock()
            .unwrap()
            .queue
            .iter()
            .for_each(|j| j.trace(visit));
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum JobError {
    Closed,
    ForeignRuntime,
    NotShareable,
    TooManyArguments,
}

/// An opaque, owner-bound delivery capability. Only the callable's task
/// creates it; a settling task may send shareable arguments through it but
/// cannot inspect or execute the callable. The owning reaction record must
/// trace this sender (or root its callable while held outside the heap).
/// There is no persistent root or strong heap/task reference here, so an
/// unreachable reaction/callable cycle can be reclaimed normally.
#[derive(Clone, Debug)]
pub(crate) struct JobSender {
    inbox: Weak<JobInbox>,
    heap: Weak<Heap>,
    callable: Value,
}

impl JobSender {
    #[cfg(test)]
    pub(crate) fn callable_for_test(&self) -> Value {
        self.callable.clone()
    }
    pub(crate) fn new(inbox: &Arc<JobInbox>, heap: &Arc<Heap>, callable: Value) -> Self {
        Self {
            inbox: Arc::downgrade(inbox),
            heap: Arc::downgrade(heap),
            callable,
        }
    }

    /// Transfer only immutable/published arguments. No JavaScript, wake,
    /// host effect or callback executes here. A whole-task dependency wait
    /// stays parked: delivery alone never wakes or re-enters the task.
    /// The caller keeps the sender's owning traced record live until this
    /// returns, as for other internal APIs taking raw VM values.
    pub(crate) fn enqueue(&self, args: &[Rooted]) -> Result<(), JobError> {
        if args.len() >= usize::from(u16::MAX) {
            return Err(JobError::TooManyArguments);
        }
        let heap = self.heap.upgrade().ok_or(JobError::Closed)?;
        let _access = heap.enter_mutator(Access::Op);
        let args = args
            .iter()
            .map(|v| {
                let v = v.value_in(&heap).map_err(|_| JobError::ForeignRuntime)?;
                share::check(&heap, &v).map_err(|_| JobError::NotShareable)?;
                Ok(v)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.inbox.upgrade().ok_or(JobError::Closed)?.push(Job {
            callable: self.callable.clone(),
            args,
        })
    }
}

impl Trace for JobSender {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.callable.trace(visit);
    }
}
