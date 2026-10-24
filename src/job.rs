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

#[derive(Debug, Default)]
pub(crate) struct JobInbox {
    inbox: Mutex<Inbox>,
    #[cfg(test)]
    pub(crate) boundaries: Arc<std::sync::atomic::AtomicUsize>,
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
        let mut inbox = self.inbox.lock().unwrap();
        let next = inbox.queue.pop_front();
        if next.is_none() {
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
/// cannot inspect or execute the callable. A sender retains its callable,
/// not the destination task. Future promise reaction records can own it.
#[derive(Clone, Debug)]
pub(crate) struct JobSender {
    inbox: Weak<JobInbox>,
    heap: Arc<Heap>,
    callable: Rooted,
}

impl JobSender {
    #[cfg(test)]
    pub(crate) fn callable_for_test(&self) -> Value {
        self.callable.value_in(&self.heap).unwrap()
    }
    pub(crate) fn new(inbox: &Arc<JobInbox>, heap: &Arc<Heap>, callable: Value) -> Self {
        Self {
            inbox: Arc::downgrade(inbox),
            heap: Arc::clone(heap),
            callable: Rooted::new(heap, callable),
        }
    }

    /// Transfer only immutable/published arguments. No JavaScript, wake,
    /// host effect or callback executes here. A whole-task dependency wait
    /// stays parked: delivery alone never wakes or re-enters the task.
    pub(crate) fn enqueue(&self, args: &[Rooted]) -> Result<(), JobError> {
        let _access = self.heap.enter_mutator(Access::Op);
        let args = args
            .iter()
            .map(|v| {
                let v = v
                    .value_in(&self.heap)
                    .map_err(|_| JobError::ForeignRuntime)?;
                share::check(&self.heap, &v).map_err(|_| JobError::NotShareable)?;
                Ok(v)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if args.len() >= usize::from(u16::MAX) {
            return Err(JobError::TooManyArguments);
        }
        self.inbox.upgrade().ok_or(JobError::Closed)?.push(Job {
            callable: self.callable.value_in(&self.heap).expect("sender heap"),
            args,
        })
    }
}
