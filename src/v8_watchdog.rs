//! One interrupt scheduler per process. Idle compiler sessions do not poll or
//! own operating-system threads. It holds only weak tasks and thread-safe V8
//! handles; JavaScript and compiler data stay on each isolate's owning thread.
use crate::evaluator::EvalError;
use std::sync::{
    Arc, Mutex, OnceLock, Weak,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc::{self, Receiver, SyncSender},
};
use std::time::Duration;

pub(super) struct Task {
    handle: v8::IsolateHandle,
    active: AtomicUsize,
    pending: AtomicBool,
    scheduler: Arc<Scheduler>,
}

impl Task {
    pub(super) fn new(handle: v8::IsolateHandle) -> Result<Arc<Self>, EvalError> {
        let scheduler = scheduler()?.clone();
        let task = Arc::new(Self {
            handle,
            active: AtomicUsize::new(0),
            pending: AtomicBool::new(false),
            scheduler: scheduler.clone(),
        });
        scheduler.tasks.lock().unwrap().push(Arc::downgrade(&task));
        Ok(task)
    }

    pub(super) fn enter(&self) -> Execution<'_> {
        if self.active.fetch_add(1, Ordering::SeqCst) == 0 {
            self.scheduler.wake();
        }
        Execution(self)
    }

    pub(super) fn completed(&self) {
        self.pending.store(false, Ordering::Release);
    }

    pub(super) fn running(&self) -> bool {
        self.active.load(Ordering::SeqCst) != 0
    }
}

pub(super) struct Execution<'a>(&'a Task);

impl Drop for Execution<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

struct Scheduler {
    tasks: Mutex<Vec<Weak<Task>>>,
    // SeqCst pairs entry with the scheduler's second check before sleeping.
    awake: AtomicBool,
    notify: SyncSender<()>,
}

fn scheduler() -> Result<&'static Arc<Scheduler>, EvalError> {
    static SCHEDULER: OnceLock<Arc<Scheduler>> = OnceLock::new();
    static INITIALIZE: Mutex<()> = Mutex::new(());
    if let Some(scheduler) = SCHEDULER.get() {
        return Ok(scheduler);
    }
    let _initialize = INITIALIZE.lock().unwrap();
    if let Some(scheduler) = SCHEDULER.get() {
        return Ok(scheduler);
    }
    // A failed thread allocation belongs to this constructor. It neither
    // panics nor permanently poisons later session creation.
    let scheduler = Scheduler::start_with(|run| {
        std::thread::Builder::new()
            .name("light-cljs-v8-interrupts".into())
            .spawn(run)
            .map(|_| ())
    })?;
    Ok(SCHEDULER.get_or_init(|| scheduler))
}

impl Scheduler {
    fn start_with(
        spawn: impl FnOnce(Box<dyn FnOnce() + Send>) -> std::io::Result<()>,
    ) -> Result<Arc<Self>, EvalError> {
        let (notify, receiver) = mpsc::sync_channel(1);
        let scheduler = Arc::new(Scheduler {
            tasks: Mutex::new(Vec::new()),
            awake: AtomicBool::new(true),
            notify,
        });
        let worker = scheduler.clone();
        spawn(Box::new(move || worker.run(receiver)))
            .map_err(|error| EvalError(format!("start V8 interrupt scheduler: {error}")))?;
        Ok(scheduler)
    }
    fn wake(&self) {
        if !self.awake.swap(true, Ordering::SeqCst) {
            // A queued wake already suffices, including when it arrived just
            // before recv. The scheduler owns the receiver for process life.
            let _ = self.notify.try_send(());
        }
    }

    fn scan(&self, interrupt: bool) -> bool {
        let mut tasks = Vec::new();
        self.tasks.lock().unwrap().retain(|weak| {
            if let Some(task) = weak.upgrade() {
                tasks.push(task);
                true
            } else {
                false
            }
        });
        let mut active = false;
        // Do not hold the registry lock while entering V8's handle locks.
        for task in tasks {
            if task.running() {
                active = true;
                if interrupt
                    && !task.pending.swap(true, Ordering::AcqRel)
                    && !task
                        .handle
                        .request_interrupt(super::interrupt, std::ptr::null_mut())
                {
                    task.pending.store(false, Ordering::Release);
                }
            }
        }
        active
    }

    fn run(&self, receiver: Receiver<()>) {
        loop {
            if self.scan(true) {
                let _ = receiver.recv_timeout(Duration::from_micros(100));
            } else {
                self.awake.store(false, Ordering::SeqCst);
                // Revisit the registry as well as the activity flags: a new
                // session may have registered since the first scan. Its entry
                // either appears here or queues a wake after observing asleep.
                if !self.scan(false) && receiver.recv().is_err() {
                    return;
                }
                self.awake.store(true, Ordering::SeqCst);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_thread_allocation_failure_is_owned() {
        let result = Scheduler::start_with(|_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "thread quota",
            ))
        });
        let Err(error) = result else {
            panic!("thread creation must fail")
        };
        assert_eq!(error.0, "start V8 interrupt scheduler: thread quota");
    }
}
