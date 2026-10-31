//! A per-thread timeline of what each compiling thread does and waits for,
//! for measuring why a parallel build leaves cores idle (light-cljs-573,
//! docs/shared-heap-compilation.md, "Why the Pants build leaves cores idle").
//!
//! It records only in a `--stats` build run with `LIGHT_CLJS_WAIT_TIMELINE`
//! naming a file; the build writes the events there when it reports its
//! statistics, for `benchmarks/wait-timeline.bb`. Without it every hook loads
//! one relaxed atomic and returns.
//!
//! Each thread appends to a buffer of its own, which it registers once;
//! appending takes only that buffer's uncontended lock, so recording adds no
//! lock threads share. Events carry the logical identities a wait needs, the
//! unit, task or dependency awaited, since tasks migrate between threads.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Instant;

static ON: AtomicBool = AtomicBool::new(false);
static EPOCH: OnceLock<Instant> = OnceLock::new();
static BUFFERS: Mutex<Vec<Arc<Mutex<Buffer>>>> = Mutex::new(Vec::new());

/// The environment variable naming the file the timeline is written to.
pub const VARIABLE: &str = "LIGHT_CLJS_WAIT_TIMELINE";

/// What an event marks. Interval kinds come as a begin and an end event on
/// one thread; the rest are instants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
#[cfg_attr(not(feature = "bbp-macros"), allow(dead_code))]
pub(crate) enum Kind {
    /// A `compile_trace` stage frame: `a` the stage's index, `b` its owner
    /// (namespace or unit).
    Stage = 0,
    /// An outermost engine service call (V8, QuickJS or Pants task command).
    Engine = 1,
    /// A pool job: `a` the unit (or the task of a resumed macro step), `b`
    /// 0 for a unit, 1 for a macro-free unit, 2 for a macro step.
    Job = 2,
    /// A worker waits for runnable work: `a` runnable jobs, `b` backlog,
    /// both at the start (normally 0).
    Idle = 3,
    /// One wait for unit `a` in the catalog: `b` encodes its producer's
    /// state at the start (1 analyzing on another thread, 2 queued and
    /// unstarted). The end event's `a` says how the wait ended: 0 woken by
    /// notification, 1 its 20 ms timeout expired, 2 not slept (help ran or
    /// the state changed).
    UnitWait = 4,
    /// A help attempt found nothing it may run: `a` runnable jobs it was not
    /// allowed to take (instant).
    HelpIdle = 5,
    /// A macro task parked on dependency key `b` of kind `a` (`ResultKind`
    /// number); the end's `a` counts expired 20 ms timeouts, `b` is 1 when
    /// the outcome arrived as a notification.
    Park = 6,
    /// A macro task outside the pool waits for key `b` of kind `a`; the
    /// end's `a` counts expired timeouts.
    Demand = 7,
    /// A worker waits for the coordinator to answer a forwarded request.
    Forward = 8,
    /// Waiting for the form bridge lock.
    BridgeWait = 9,
    /// Holding the form bridge lock.
    BridgeHold = 10,
    /// Producing helper or initializer key `b` of kind `a` inline.
    Produce = 11,
    /// Unit `a` given to the pool by the coordinator (instant).
    Dispatch = 12,
    /// Unit `a`'s analysis ended (instant).
    Settle = 13,
    /// Queue depth (instant): `a` runnable, `b` backlog.
    Queue = 14,
    /// A parked task `a`'s next step was queued by a completion (instant).
    Wake = 15,
    /// Collection totals at the end (instant): `a` collections, `b` pause
    /// nanoseconds; followed by [`Kind::GcWaits`].
    Gc = 16,
    /// `a` rendezvous nanoseconds (request until every thread stopped),
    /// `b` nanoseconds threads spent stopped or blocked by collections.
    GcWaits = 17,
    /// Batch of units the pool thread submitted to the scheduler: `a` the
    /// first unit, `b` the number (instant).
    Submit = 18,
    /// The statistics are reported: the end of the build (instant).
    End = 19,
    /// Waiting for the compile-time namespace registry's lock.
    NamespacesWait = 20,
    /// Holding the compile-time namespace registry's lock.
    NamespacesHold = 21,
}

#[derive(Clone, Copy)]
struct Event {
    nanos: u64,
    phase: u8,
    kind: Kind,
    a: u64,
    b: u64,
}

struct Buffer {
    name: String,
    events: Vec<Event>,
}

thread_local! {
    static LOCAL: RefCell<Option<Arc<Mutex<Buffer>>>> = const { RefCell::new(None) };
}

/// Whether the timeline records.
#[inline]
pub(crate) fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Start recording if `LIGHT_CLJS_WAIT_TIMELINE` is set; `--stats` builds
/// call this.
pub(crate) fn start_from_environment() {
    if std::env::var_os(VARIABLE).is_some() {
        EPOCH.get_or_init(Instant::now);
        ON.store(true, Ordering::Relaxed);
    }
}

fn push(phase: u8, kind: Kind, a: u64, b: u64) {
    let nanos = EPOCH
        .get()
        .map_or(0, |epoch| epoch.elapsed().as_nanos() as u64);
    LOCAL.with(|local| {
        let mut local = local.borrow_mut();
        let buffer = local.get_or_insert_with(|| {
            let name = std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_owned();
            let buffer = Arc::new(Mutex::new(Buffer {
                name,
                events: Vec::with_capacity(4096),
            }));
            BUFFERS
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(buffer.clone());
            buffer
        });
        buffer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .events
            .push(Event {
                nanos,
                phase,
                kind,
                a,
                b,
            });
    });
}

/// Record an instant.
#[inline]
pub(crate) fn instant(kind: Kind, a: u64, b: u64) {
    if on() {
        push(b'I', kind, a, b);
    }
}

/// Begin an interval; dropping the span (or [`Span::end`]) ends it.
#[inline]
pub(crate) fn begin(kind: Kind, a: u64, b: u64) -> Span {
    if !on() {
        return Span { kind: None };
    }
    push(b'B', kind, a, b);
    Span { kind: Some(kind) }
}

/// An open interval.
pub(crate) struct Span {
    kind: Option<Kind>,
}

impl Span {
    /// End the interval with `a` and `b` describing how it ended.
    pub(crate) fn end(mut self, a: u64, b: u64) {
        if let Some(kind) = self.kind.take() {
            push(b'E', kind, a, b);
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(kind) = self.kind.take() {
            push(b'E', kind, 0, 0);
        }
    }
}

/// Begin an interval without a guard, for hooks whose end is elsewhere.
#[inline]
pub(crate) fn begin_raw(kind: Kind, a: u64, b: u64) {
    if on() {
        push(b'B', kind, a, b);
    }
}

/// End an interval begun with [`begin_raw`].
#[inline]
pub(crate) fn end_raw(kind: Kind, a: u64, b: u64) {
    if on() {
        push(b'E', kind, a, b);
    }
}

/// Write every thread's events to the file `LIGHT_CLJS_WAIT_TIMELINE`
/// names and stop recording, if recording.
pub fn finish() -> Result<(), String> {
    if !on() {
        return Ok(());
    }
    push(b'I', Kind::End, 0, 0);
    ON.store(false, Ordering::Relaxed);
    let Some(path) = std::env::var_os(VARIABLE) else {
        return Ok(());
    };
    let buffers = std::mem::take(&mut *BUFFERS.lock().unwrap_or_else(PoisonError::into_inner));
    let mut text = String::from(
        "# light-cljs-wait-timeline 1\n# thread\tname\n# thread\tnanos\tphase\tkind\ta\tb\n",
    );
    use std::fmt::Write;
    for (index, buffer) in buffers.iter().enumerate() {
        let buffer = buffer.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = writeln!(text, "T\t{index}\t{}", buffer.name);
        for event in &buffer.events {
            let _ = writeln!(
                text,
                "{index}\t{}\t{}\t{}\t{}\t{}",
                event.nanos, event.phase as char, event.kind as u8, event.a, event.b
            );
        }
    }
    std::fs::write(&path, text)
        .map_err(|error| format!("{VARIABLE} {}: {error}", path.to_string_lossy()))
}
