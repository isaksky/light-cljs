//! The `--stats` trace of one compilation: exclusive stage times, the
//! JavaScript engine's share of each, every deferred unit's cost and the
//! demand edges between units (docs/compiler-concurrency.md, "Gates and
//! first steps", step 2). It is the one-worker baseline later scaling claims
//! are measured against.
//!
//! Work is recorded as a stack of frames on the compiling thread. A frame's
//! *exclusive* time is its duration less that of the frames opened inside it,
//! so nested work counts once: a unit a macro demands while a namespace's top
//! level is analyzed counts as that unit, not as the top-level pass, and the
//! pass records it as a demand edge and as time it waited. Engine time is
//! taken at the outermost V8 service call (`v8_evaluator::invoke`) and
//! charged to the innermost frame; a unit demanded from inside JavaScript
//! pauses its demander's engine time while it runs, so engine time is
//! exclusive too. The time at depth 0, which counts Rust called back from
//! JavaScript, is kept separately ([`Trace::engine_outer`]).
//!
//! Each frame has a serial ([`frame`]). The engine notes the compile-time
//! roots it forces in JavaScript, ready ones included, and the bridge check
//! that ends each engine entry hands it the serial, so that it can report
//! them in one call as read edges from that frame ([`root_reads`]).
//!
//! With `--jobs`, each pool worker records its unit analyses on its own
//! thread ([`record_worker`]) and hands their rows to the coordinator with
//! the unit's charges, and the coordinator records them where it takes the
//! unit's analysis, in its order, as a serial build would have
//! ([`replay`]). A unit counted on counters of its own, a worker's or a
//! macro-free one's, is not counted by the frames around it until the
//! coordinator charges it ([`charged`]).
//!
//! Without a recording ([`record`]) nothing here reads a clock or allocates:
//! each hook loads one relaxed atomic and returns.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// How many threads are recording; while none is, every hook returns after
/// one load.
static RECORDING: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static RECORDER: RefCell<Option<Recorder>> = const { RefCell::new(None) };
}

/// A kind of work. Every moment of a recorded compilation is in exactly one
/// stage, the innermost open frame's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    /// Whatever no other frame covers: cache lookups and publication,
    /// artifact capture and install, the analysis loop's own work.
    Other,
    /// Locating, reading and parsing one source and its `ns` form, without
    /// the sources it requires (`Loader::visit`).
    Load,
    /// Resolving a namespace's requires and macros before its analysis
    /// (`prepare_source`).
    PrepareSource,
    /// A namespace's top-level pass, without the units it demands.
    TopLevel,
    /// The drain of a namespace's deferred units, without the units' own
    /// analysis or the three steps below.
    Drain,
    /// Appending an analyzed unit's fragment to the program.
    Append,
    /// Indexing the globals a unit's fragment added.
    Index,
    /// Preparing a unit's compile-time JavaScript and releasing its IR
    /// (`release_application_ir`).
    Release,
    /// A namespace's expression warnings.
    Warnings,
    /// One deferred unit's analysis, without the units it demands.
    Unit,
    /// Closing the program (`NamespaceAnalysis::finish`).
    Finish,
}

impl Stage {
    pub const ALL: [Stage; 11] = [
        Stage::Other,
        Stage::Load,
        Stage::PrepareSource,
        Stage::TopLevel,
        Stage::Drain,
        Stage::Append,
        Stage::Index,
        Stage::Release,
        Stage::Warnings,
        Stage::Unit,
        Stage::Finish,
    ];

    /// The stage's name in the report and the trace file.
    pub fn name(self) -> &'static str {
        match self {
            Stage::Other => "other",
            Stage::Load => "load",
            Stage::PrepareSource => "prepare-source",
            Stage::TopLevel => "top-level",
            Stage::Drain => "drain",
            Stage::Append => "append",
            Stage::Index => "index",
            Stage::Release => "release",
            Stage::Warnings => "warnings",
            Stage::Unit => "unit",
            Stage::Finish => "finish",
        }
    }
}

/// The exclusive totals of one stage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StageTotals {
    pub time: Duration,
    /// Engine time charged to the stage.
    pub engine: Duration,
    /// Macro host entries made in the stage: engine service calls other
    /// than frontier transitions and bridge checks ([`engine_exit`]).
    pub engine_calls: u64,
}

/// One namespace, by source index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NamespaceTrace {
    pub name: String,
    /// Its source text, in bytes.
    pub bytes: usize,
    /// The namespaces it requires, in order.
    pub requires: Vec<String>,
    /// Exclusive [`Stage::Load`] time.
    pub load: Duration,
    pub prepare_source: Duration,
    /// Exclusive [`Stage::TopLevel`] time and its engine share.
    pub top_level: Duration,
    pub top_level_engine: Duration,
    /// The time of the units the top-level pass demanded, nested demand
    /// included: what the pass waited for.
    pub top_level_wait: Duration,
    /// Drain bookkeeping: [`Stage::Drain`], [`Stage::Append`],
    /// [`Stage::Index`] and [`Stage::Release`].
    pub drain: Duration,
    pub drain_engine: Duration,
    pub warnings: Duration,
    /// Macro calls executed in the top-level pass itself.
    pub top_level_expansions: usize,
    /// Whether the namespace was installed from an artifact, with no
    /// top-level pass.
    pub installed: bool,
}

/// Who demanded a unit's analysis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Demander {
    /// The drain of the namespace with this source index: ready work, not a
    /// demand.
    Drain(u32),
    /// The top-level pass of the namespace with this source index.
    TopLevel(u32),
    /// The unit with this ID.
    Unit(u32),
    /// Another stage, outside any namespace's pass.
    Other(Stage),
}

impl Demander {
    /// The demander a frame of `stage` for `owner` stands for.
    fn of(stage: Stage, owner: u32) -> Self {
        match stage {
            Stage::Drain => Demander::Drain(owner),
            Stage::TopLevel => Demander::TopLevel(owner),
            Stage::Unit => Demander::Unit(owner),
            stage => Demander::Other(stage),
        }
    }

    /// The `KIND ID` columns of the trace file.
    pub fn columns(self) -> (&'static str, u32) {
        match self {
            Demander::Drain(index) => ("drain", index),
            Demander::TopLevel(index) => ("top-level", index),
            Demander::Unit(id) => ("unit", id),
            Demander::Other(stage) => (stage.name(), 0),
        }
    }
}

/// One deferred unit's analysis, in the order analyses finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitTrace {
    /// Its compilation unit ID.
    pub id: u32,
    /// The source index of the namespace that defined it.
    pub source: u32,
    /// The Var it initializes.
    pub name: String,
    /// The 1-based line and column of its form.
    pub line: u32,
    pub column: u32,
    /// Exclusive time and its engine share.
    pub time: Duration,
    pub engine: Duration,
    /// Macro host entries made by the unit itself ([`engine_exit`]).
    pub engine_calls: u64,
    /// The time of the units it demanded, nested demand included.
    pub wait: Duration,
    /// Macro calls executed and form nodes generated by the unit itself.
    pub expansions: usize,
    pub nodes: usize,
    /// Whether its analysis entered the macro host's engine, itself or
    /// through a unit it demanded.
    pub host: bool,
    pub demander: Demander,
    /// How many unit analyses were open below it.
    pub depth: u32,
    /// The thread that analyzed it: 0 is the coordinator, and a pool
    /// worker is its slot, from 1 ([`replay`]).
    pub thread: u32,
}

impl UnitTrace {
    /// Its duration with the units it demanded.
    pub fn inclusive(&self) -> Duration {
        self.time + self.wait
    }
}

/// The trace of one compilation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trace {
    /// By [`Stage::ALL`] position.
    pub stages: [StageTotals; Stage::ALL.len()],
    /// The time spent at engine depth 0, Rust called back from JavaScript
    /// included, and the number of such entries.
    pub engine_outer: Duration,
    pub engine_entries: u64,
    /// By source index.
    pub namespaces: Vec<NamespaceTrace>,
    pub units: Vec<UnitTrace>,
    /// Each time a unit's compile-time root was first evaluated in the
    /// engine, in order: an execution demand edge.
    pub executions: Vec<Execution>,
    /// Each distinct pair of a frame and a unit whose compile-time root or
    /// bundle the engine forced in it, ready ones included, in the order
    /// first noted: a read demand edge ([`root_reads`]).
    pub reads: Vec<Read>,
    /// From [`record`] to [`Recording::finish`].
    pub wall: Duration,
}

/// A unit whose compile-time root the engine evaluated for the first time
/// (`__lightCompileUnit`). Later reads of the ready root are [`Read`]s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Execution {
    pub unit: u32,
    /// The innermost frame when it was demanded.
    pub demander: Demander,
    /// Whether its JavaScript was already prepared; if not, the demand
    /// analyzed it first (a [`UnitTrace`] row with the same demander).
    pub prepared: bool,
}

/// A unit whose compile-time root or bundle the engine forced while
/// `demander` was the innermost frame: its first evaluation, which an
/// [`Execution`] records too, or any later read of the ready root.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Read {
    pub unit: u32,
    pub demander: Demander,
}

impl Trace {
    pub fn stage(&self, stage: Stage) -> &StageTotals {
        &self.stages[stage as usize]
    }

    /// The sum of every stage's exclusive time, which is [`Self::wall`].
    pub fn exclusive_total(&self) -> Duration {
        self.stages.iter().map(|stage| stage.time).sum()
    }

    /// The ready work of each namespace's drain, by source index: the units
    /// the drain itself started that did not enter the macro host, and those
    /// that did. Units already analyzed on demand are not in it.
    pub fn drain_widths(&self) -> Vec<(usize, usize)> {
        let mut widths = vec![(0, 0); self.namespaces.len()];
        for unit in &self.units {
            if let Demander::Drain(index) = unit.demander
                && let Some(width) = widths.get_mut(index as usize)
            {
                if unit.host {
                    width.1 += 1;
                } else {
                    width.0 += 1;
                }
            }
        }
        widths
    }

    /// How many read edges cross a namespace: their demander belongs to
    /// another namespace than the unit read, by the source indexes of the
    /// traced units. A demander outside any namespace's pass, or a unit this
    /// compilation did not analyze, counts as crossing.
    pub fn cross_namespace_reads(&self) -> usize {
        let mut sources = Vec::new();
        for unit in &self.units {
            let id = unit.id as usize;
            if sources.len() <= id {
                sources.resize(id + 1, None);
            }
            sources[id] = Some(unit.source);
        }
        let source = |id: u32| sources.get(id as usize).copied().flatten();
        self.reads
            .iter()
            .filter(|read| {
                let demander = match read.demander {
                    Demander::Drain(index) | Demander::TopLevel(index) => Some(index),
                    Demander::Unit(id) => source(id),
                    Demander::Other(_) => None,
                };
                demander.is_none() || demander != source(read.unit)
            })
            .count()
    }

    /// The `key value` lines of the `--stats` report.
    pub(crate) fn lines(&self, line: &mut dyn FnMut(&str, &dyn std::fmt::Display)) {
        let ms = |duration: Duration| format!("{:.1}", duration.as_secs_f64() * 1000.0);
        line("trace.wall-ms", &ms(self.wall));
        for stage in Stage::ALL {
            line(
                &format!("trace.{}-ms", stage.name()),
                &ms(self.stage(stage).time),
            );
        }
        let engine: Duration = self.stages.iter().map(|stage| stage.engine).sum();
        line("trace.engine-ms", &ms(engine));
        line(
            "trace.top-level.engine-ms",
            &ms(self.stage(Stage::TopLevel).engine),
        );
        line("trace.unit.engine-ms", &ms(self.stage(Stage::Unit).engine));
        line("trace.engine.outer-ms", &ms(self.engine_outer));
        line("trace.engine.entries", &self.engine_entries);
        let wait: Duration = self.namespaces.iter().map(|ns| ns.top_level_wait).sum();
        line("trace.top-level.wait-ms", &ms(wait));
        let bookkeeping = [Stage::Drain, Stage::Append, Stage::Index, Stage::Release]
            .into_iter()
            .map(|stage| self.stage(stage).time)
            .sum();
        line("trace.drain.bookkeeping-ms", &ms(bookkeeping));
        line("trace.units", &self.units.len());
        let count =
            |keep: &dyn Fn(&UnitTrace) -> bool| self.units.iter().filter(|u| keep(u)).count();
        line(
            "trace.units.drained",
            &count(&|unit| matches!(unit.demander, Demander::Drain(_))),
        );
        line(
            "trace.units.demanded-by-top-level",
            &count(&|unit| matches!(unit.demander, Demander::TopLevel(_))),
        );
        line(
            "trace.units.demanded-by-unit",
            &count(&|unit| matches!(unit.demander, Demander::Unit(_))),
        );
        line("trace.units.host", &count(&|unit| unit.host));
        line(
            "trace.units.without-macro-calls",
            &count(&|unit| !unit.host && unit.expansions == 0),
        );
        line("trace.executions", &self.executions.len());
        line(
            "trace.executions.unprepared",
            &self.executions.iter().filter(|e| !e.prepared).count(),
        );
        line("trace.reads", &self.reads.len());
        line("trace.reads.cross-namespace", &self.cross_namespace_reads());
        let host_time: Duration = self.units.iter().filter(|u| u.host).map(|u| u.time).sum();
        line("trace.units.host-ms", &ms(host_time));
        let longest = self.units.iter().map(UnitTrace::inclusive).max();
        line("trace.unit.longest-ms", &ms(longest.unwrap_or_default()));
        let widths = self.drain_widths();
        let drains = widths.iter().filter(|(free, host)| free + host > 0);
        line("trace.drains", &drains.clone().count());
        line(
            "trace.drain.width.max",
            &drains
                .clone()
                .map(|(free, host)| free + host)
                .max()
                .unwrap_or(0),
        );
        line(
            "trace.drain.macro-free.max",
            &drains.map(|(free, _)| *free).max().unwrap_or(0),
        );
    }

    /// The trace as the tab-separated file `--stats-file` writes, which
    /// `benchmarks/concurrency/dag.bb` reads: a version line, then `stage`,
    /// `namespace`, `unit`, `execute` and `read` rows, each kind described
    /// by a `#` header row.
    /// Times are microseconds. `stages` adds the phases measured outside
    /// the analysis, such as emission.
    pub fn tsv(&self, stages: &[(&str, Duration)]) -> String {
        use std::fmt::Write as _;
        let us = |duration: Duration| duration.as_micros();
        let clean = |text: &str| text.replace(['\t', '\n', '\r'], " ");
        let mut out = String::new();
        let _ = writeln!(out, "light-cljs-trace\t{TRACE_VERSION}");
        out.push_str("#stage\tname\tus\tengine-us\tengine-calls\n");
        for stage in Stage::ALL {
            let totals = self.stage(stage);
            let _ = writeln!(
                out,
                "stage\t{}\t{}\t{}\t{}",
                stage.name(),
                us(totals.time),
                us(totals.engine),
                totals.engine_calls
            );
        }
        let _ = writeln!(
            out,
            "stage\tengine-outer\t{}\t0\t{}",
            us(self.engine_outer),
            self.engine_entries
        );
        let _ = writeln!(out, "stage\ttrace-wall\t{}\t0\t0", us(self.wall));
        for (name, time) in stages {
            let _ = writeln!(out, "stage\t{name}\t{}\t0\t0", us(*time));
        }
        out.push_str(
            "#namespace\tindex\tname\tbytes\tload-us\tprepare-us\ttop-level-us\ttop-level-engine-us\ttop-level-wait-us\tdrain-us\tdrain-engine-us\twarnings-us\texpansions\tinstalled\trequires\n",
        );
        for (index, ns) in self.namespaces.iter().enumerate() {
            let _ = writeln!(
                out,
                "namespace\t{index}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                clean(&ns.name),
                ns.bytes,
                us(ns.load),
                us(ns.prepare_source),
                us(ns.top_level),
                us(ns.top_level_engine),
                us(ns.top_level_wait),
                us(ns.drain),
                us(ns.drain_engine),
                us(ns.warnings),
                ns.top_level_expansions,
                u8::from(ns.installed),
                clean(&ns.requires.join(",")),
            );
        }
        out.push_str(
            "#unit\tid\tnamespace\tname\tline\tcolumn\tus\tengine-us\tengine-calls\twait-us\texpansions\tnodes\thost\tdemander-kind\tdemander\tdepth\tthread\n",
        );
        for unit in &self.units {
            let (kind, demander) = unit.demander.columns();
            let _ = writeln!(
                out,
                "unit\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{kind}\t{demander}\t{}\t{}",
                unit.id,
                unit.source,
                clean(&unit.name),
                unit.line,
                unit.column,
                us(unit.time),
                us(unit.engine),
                unit.engine_calls,
                us(unit.wait),
                unit.expansions,
                unit.nodes,
                u8::from(unit.host),
                unit.depth,
                unit.thread,
            );
        }
        out.push_str("#execute\tunit\tdemander-kind\tdemander\tprepared\n");
        for execution in &self.executions {
            let (kind, demander) = execution.demander.columns();
            let _ = writeln!(
                out,
                "execute\t{}\t{kind}\t{demander}\t{}",
                execution.unit,
                u8::from(execution.prepared)
            );
        }
        out.push_str("#read\tunit\tdemander-kind\tdemander\n");
        for read in &self.reads {
            let (kind, demander) = read.demander.columns();
            let _ = writeln!(out, "read\t{}\t{kind}\t{demander}", read.unit);
        }
        out
    }
}

/// The version of the `--stats-file` format, its first line: 2 added the
/// `read` rows.
pub const TRACE_VERSION: u32 = 2;

/// The macro host's shared counters a frame reads at its ends.
pub(crate) struct Counters {
    expansions: Rc<Cell<usize>>,
    nodes: Rc<Cell<usize>>,
}

impl Counters {
    pub(crate) fn of(host: &crate::MacroHost) -> Self {
        Self {
            expansions: host.engine.expansions.clone(),
            nodes: host.engine.generated_nodes.clone(),
        }
    }

    fn read(&self) -> (usize, usize) {
        (self.expansions.get(), self.nodes.get())
    }

    /// Which counters these are: a frame counts the work of the frames
    /// inside it only where they count the same ones ([`charged`]).
    fn key(&self) -> usize {
        counter_key(&self.expansions)
    }
}

fn counter_key(expansions: &Rc<Cell<usize>>) -> usize {
    Rc::as_ptr(expansions) as usize
}

/// What a unit frame records beyond its times.
struct UnitStart {
    id: u32,
    source: u32,
    name: String,
    line: u32,
    column: u32,
    depth: u32,
}

/// The identity of a unit about to be analyzed, computed only when traced.
pub(crate) struct UnitInfo {
    pub id: usize,
    pub source: u32,
    pub name: String,
    pub span: crate::compiler_data::SourceSpan,
    pub depth: usize,
}

struct Frame {
    stage: Stage,
    /// The source index of the namespace, or the unit's ID.
    owner: u32,
    started: Instant,
    children: Duration,
    engine: Duration,
    engine_calls: u64,
    counted: Option<(usize, usize)>,
    /// The [`Counters::key`] of the counters it reads, or 0.
    counter: usize,
    child_counts: (usize, usize),
    /// The engine depth when the frame opened, restored when it closes.
    saved_depth: u32,
    host: bool,
    unit: Option<Box<UnitStart>>,
    /// Time of units demanded directly by this frame (the top-level pass's
    /// wait).
    demanded: Duration,
    /// Its index in [`Recorder::demanders`], by which the engine names the
    /// frame its root reads were made in.
    serial: u32,
}

impl Frame {
    fn new(stage: Stage, owner: u32, now: Instant) -> Self {
        Self {
            stage,
            owner,
            started: now,
            children: Duration::ZERO,
            engine: Duration::ZERO,
            engine_calls: 0,
            counted: None,
            counter: 0,
            child_counts: (0, 0),
            saved_depth: 0,
            host: false,
            unit: None,
            demanded: Duration::ZERO,
            serial: 0,
        }
    }
}

struct Recorder {
    started: Instant,
    frames: Vec<Frame>,
    /// Engine depth inside the innermost frame, and since when it is above 0.
    depth: u32,
    engine_started: Option<Instant>,
    /// Engine depth across frames, for [`Trace::engine_outer`].
    #[cfg_attr(not(feature = "v8-macros"), allow(dead_code))]
    outer_depth: u32,
    #[cfg_attr(not(feature = "v8-macros"), allow(dead_code))]
    outer_started: Option<Instant>,
    trace: Trace,
    /// Exclusive load times, in the order sources were appended.
    loads: Vec<Duration>,
    /// What each frame opened stands for, by [`Frame::serial`]; the root
    /// frame's is 0.
    demanders: Vec<Demander>,
    /// The edges in [`Trace::reads`].
    read_edges: std::collections::HashSet<Read>,
    /// The thread's [`UnitTrace::thread`].
    thread: u32,
}

impl Recorder {
    fn namespace(&mut self, index: u32) -> &mut NamespaceTrace {
        let index = index as usize;
        if self.trace.namespaces.len() <= index {
            self.trace
                .namespaces
                .resize_with(index + 1, NamespaceTrace::default);
        }
        &mut self.trace.namespaces[index]
    }

    fn push(&mut self, mut frame: Frame, counts: Option<(usize, usize)>) {
        let now = frame.started;
        if self.depth > 0
            && let Some(started) = self.engine_started.take()
        {
            self.frames.last_mut().expect("root frame").engine += now - started;
        }
        frame.saved_depth = self.depth;
        frame.counted = counts;
        frame.serial = self.demanders.len() as u32;
        self.demanders.push(Demander::of(frame.stage, frame.owner));
        self.depth = 0;
        self.frames.push(frame);
    }

    fn pop(&mut self, counts: Option<(usize, usize)>) {
        let now = Instant::now();
        if self.frames.len() <= 1 {
            return;
        }
        let mut frame = self.frames.pop().expect("open frame");
        // A frame closed inside an engine call made in it keeps that time.
        if self.depth > 0
            && let Some(started) = self.engine_started.take()
        {
            frame.engine += now - started;
        }
        self.depth = frame.saved_depth;
        if self.depth > 0 {
            self.engine_started = Some(now);
        }
        let inclusive = now - frame.started;
        let time = inclusive.saturating_sub(frame.children);
        let own_counts = match (frame.counted, counts) {
            (Some((expansions, nodes)), Some((now_expansions, now_nodes))) => {
                let inclusive = (
                    now_expansions.saturating_sub(expansions),
                    now_nodes.saturating_sub(nodes),
                );
                Some((
                    inclusive,
                    (
                        inclusive.0.saturating_sub(frame.child_counts.0),
                        inclusive.1.saturating_sub(frame.child_counts.1),
                    ),
                ))
            }
            _ => None,
        };
        let parent = self.frames.last_mut().expect("root frame");
        parent.children += inclusive;
        // What a frame counts on counters of its own reaches its parent's
        // where the unit is charged, if at all ([`charged`]).
        if let Some((inclusive, _)) = own_counts
            && parent.counter == frame.counter
        {
            parent.child_counts.0 += inclusive.0;
            parent.child_counts.1 += inclusive.1;
        }
        let parent_stage = parent.stage;
        let parent_owner = parent.owner;
        let demander = Demander::of(parent_stage, parent_owner);
        if frame.stage == Stage::Unit {
            parent.host |= frame.host;
            if matches!(parent_stage, Stage::TopLevel | Stage::Unit) {
                parent.demanded += inclusive;
            }
        }
        let totals = &mut self.trace.stages[frame.stage as usize];
        totals.time += time;
        totals.engine += frame.engine;
        totals.engine_calls += frame.engine_calls;
        let (_, own) = own_counts.unwrap_or_default();
        match frame.stage {
            Stage::Load => self.loads.push(time),
            Stage::PrepareSource => self.namespace(frame.owner).prepare_source += time,
            Stage::TopLevel => {
                let namespace = self.namespace(frame.owner);
                namespace.top_level += time;
                namespace.top_level_engine += frame.engine;
                namespace.top_level_wait += frame.demanded;
                namespace.top_level_expansions += own.0;
            }
            Stage::Drain | Stage::Append | Stage::Index | Stage::Release => {
                let namespace = self.namespace(frame.owner);
                namespace.drain += time;
                namespace.drain_engine += frame.engine;
            }
            Stage::Warnings => self.namespace(frame.owner).warnings += time,
            Stage::Unit => {
                let start = frame.unit.expect("unit frame");
                self.trace.units.push(UnitTrace {
                    id: start.id,
                    source: start.source,
                    name: start.name,
                    line: start.line,
                    column: start.column,
                    time,
                    engine: frame.engine,
                    engine_calls: frame.engine_calls,
                    wait: frame.children,
                    expansions: own.0,
                    nodes: own.1,
                    host: frame.host,
                    demander,
                    depth: start.depth,
                    thread: self.thread,
                });
            }
            Stage::Other | Stage::Finish => {}
        }
    }
}

/// Whether this thread records, checked with one load while no thread does.
#[inline]
fn active() -> bool {
    RECORDING.load(Ordering::Relaxed) != 0
        && RECORDER.with(|recorder| recorder.try_borrow().is_ok_and(|r| r.is_some()))
}

/// Run `f` on this thread's recorder, checked with one load while no thread
/// records.
#[inline]
fn with_recorder<T>(f: impl FnOnce(&mut Recorder) -> T) -> Option<T> {
    if RECORDING.load(Ordering::Relaxed) == 0 {
        return None;
    }
    RECORDER.with(|recorder| {
        let mut recorder = recorder.try_borrow_mut().ok()?;
        recorder.as_mut().map(f)
    })
}

/// A recording on this thread, from [`record`] until [`Self::finish`] or
/// drop.
pub(crate) struct Recording {
    on: bool,
}

/// Start recording this thread's compilation. A thread already recording
/// keeps its recording, and the inner one records nothing of its own.
pub(crate) fn record() -> Recording {
    record_thread(0)
}

/// Start recording the unit analyses of the build's pool worker in slot
/// `slot`, which the coordinator records where it charges each unit
/// ([`take_worker_rows`], [`replay`]). On a thread already recording, such
/// as the coordinator running the pool's jobs inline, it records nothing.
pub(crate) fn record_worker(slot: usize) -> Recording {
    record_thread(slot as u32)
}

fn record_thread(thread: u32) -> Recording {
    let on = RECORDER.with(|recorder| {
        let mut recorder = recorder.borrow_mut();
        if recorder.is_some() {
            return false;
        }
        let now = Instant::now();
        *recorder = Some(Recorder {
            started: now,
            frames: vec![Frame::new(Stage::Other, 0, now)],
            depth: 0,
            engine_started: None,
            outer_depth: 0,
            outer_started: None,
            trace: Trace::default(),
            loads: Vec::new(),
            demanders: vec![Demander::Other(Stage::Other)],
            read_edges: Default::default(),
            thread,
        });
        true
    });
    if on {
        RECORDING.fetch_add(1, Ordering::Relaxed);
    }
    Recording { on }
}

impl Recording {
    /// Stop recording and return the trace.
    pub(crate) fn finish(mut self) -> Option<Trace> {
        let recorder = self.take()?;
        let Recorder {
            started,
            mut frames,
            mut trace,
            loads,
            ..
        } = recorder;
        // The time of frames an error left open counts as other.
        let root = frames.swap_remove(0);
        let wall = started.elapsed();
        let staged: Duration = trace.stages.iter().map(|stage| stage.time).sum();
        let other = &mut trace.stages[Stage::Other as usize];
        other.time = wall.saturating_sub(staged);
        other.engine += root.engine;
        other.engine_calls += root.engine_calls;
        for (index, load) in loads.into_iter().enumerate() {
            if index < trace.namespaces.len() {
                trace.namespaces[index].load = load;
            } else {
                trace.namespaces.push(NamespaceTrace {
                    load,
                    ..Default::default()
                });
            }
        }
        trace.wall = wall;
        Some(trace)
    }

    fn take(&mut self) -> Option<Recorder> {
        if !std::mem::take(&mut self.on) {
            return None;
        }
        RECORDING.fetch_sub(1, Ordering::Relaxed);
        RECORDER.with(|recorder| recorder.borrow_mut().take())
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.take();
    }
}

/// An open frame; dropping it closes it.
pub(crate) struct Span {
    on: bool,
    counters: Option<Counters>,
}

impl Span {
    const OFF: Span = Span {
        on: false,
        counters: None,
    };
}

impl Drop for Span {
    fn drop(&mut self) {
        if self.on {
            let counts = self.counters.as_ref().map(Counters::read);
            with_recorder(|recorder| recorder.pop(counts));
        }
    }
}

/// Open a frame of `stage` for the namespace or unit `owner`, reading the
/// macro host's counters at both ends when given.
#[inline]
pub(crate) fn enter(stage: Stage, owner: usize, host: Option<&crate::MacroHost>) -> Span {
    if !active() {
        return Span::OFF;
    }
    let counters = host.map(Counters::of);
    let counts = counters.as_ref().map(Counters::read);
    let on = with_recorder(|recorder| {
        let mut frame = Frame::new(stage, owner as u32, Instant::now());
        frame.counter = counters.as_ref().map_or(0, Counters::key);
        recorder.push(frame, counts);
    })
    .is_some();
    Span { on, counters }
}

/// Open the frame of a unit's analysis, with `info` computed only when
/// traced.
#[inline]
pub(crate) fn enter_unit(info: impl FnOnce() -> UnitInfo, host: &crate::MacroHost) -> Span {
    // Another thread's recording must not make this one compute `info`.
    if !active() {
        return Span::OFF;
    }
    let info = info();
    let counters = Counters::of(host);
    let counts = Some(counters.read());
    let (line, column) = info
        .span
        .source()
        .line_column(info.span.bytes().start)
        .unwrap_or((0, 0));
    let on = with_recorder(|recorder| {
        let mut frame = Frame::new(Stage::Unit, info.id as u32, Instant::now());
        frame.counter = counters.key();
        frame.unit = Some(Box::new(UnitStart {
            id: info.id as u32,
            source: info.source,
            name: info.name,
            line: line as u32,
            column: column as u32,
            depth: info.depth as u32,
        }));
        recorder.push(frame, counts);
    })
    .is_some();
    Span {
        on,
        counters: Some(counters),
    }
}

/// Describe the namespace with source index `index`.
pub(crate) fn namespace(index: usize, name: &str, bytes: usize, requires: &[String]) {
    with_recorder(|recorder| {
        let namespace = recorder.namespace(index as u32);
        namespace.name = name.to_owned();
        namespace.bytes = bytes;
        namespace.requires = requires.to_vec();
    });
}

/// The engine evaluates `unit`'s compile-time root for the first time;
/// `prepared` tells whether its JavaScript existed or must be analyzed now.
pub(crate) fn executed(unit: usize, prepared: bool) {
    with_recorder(|recorder| {
        let frame = recorder.frames.last().expect("root frame");
        let demander = Demander::of(frame.stage, frame.owner);
        recorder.trace.executions.push(Execution {
            unit: unit as u32,
            demander,
            prepared,
        });
    });
}

/// The innermost frame's serial while this thread records. The bridge check
/// that ends each engine entry hands it to the engine, which names the
/// entry's root reads by it ([`root_reads`]): frames opened inside an entry
/// close before it returns, so the entry's own code ran in this frame.
#[inline]
pub(crate) fn frame() -> Option<u32> {
    with_recorder(|recorder| recorder.frames.last().expect("root frame").serial)
}

/// Record the root reads the engine noted, `FRAME:UNIT,UNIT...`: units whose
/// compile-time roots or bundles it forced, ready ones included, in entries
/// made while the frame with serial FRAME ([`frame`]) was innermost. Pairs
/// already recorded, and a malformed note, record nothing.
pub(crate) fn root_reads(note: &str) {
    with_recorder(|recorder| {
        let Some((frame, units)) = note.split_once(':') else {
            return;
        };
        let Some(&demander) = frame
            .parse::<usize>()
            .ok()
            .and_then(|frame| recorder.demanders.get(frame))
        else {
            return;
        };
        for unit in units.split(',').filter_map(|unit| unit.parse().ok()) {
            let read = Read { unit, demander };
            if recorder.read_edges.insert(read) {
                recorder.trace.reads.push(read);
            }
        }
    });
}

/// Whether this thread records a trace now: a pooled phase asks its jobs,
/// which run where the pool puts them, to time themselves.
#[inline]
pub(crate) fn recording() -> bool {
    active()
}

/// A load phase run on the pool (light-cljs-337): the coordinator's wall
/// time in it is charged to [`Stage::Load`], as a frame with no inner frames
/// would be, and the time of each source's parse job is its namespace's
/// load, in `times`'s order, which is source order. The two differ once
/// jobs run on workers: the stage is the wall time the phase took, the
/// namespaces' loads are the work.
pub(crate) fn pooled_loads(wall: Duration, times: impl IntoIterator<Item = Duration>) {
    with_recorder(|recorder| {
        recorder.trace.stages[Stage::Load as usize].time += wall;
        recorder.frames.last_mut().expect("root frame").children += wall;
        recorder.loads.extend(times);
    });
}

/// Mark the namespace with source index `index` as installed from an
/// artifact.
pub(crate) fn installed(index: usize) {
    with_recorder(|recorder| recorder.namespace(index as u32).installed = true);
}

/// What a pool worker recorded of one unit's analysis, the units it
/// demanded included, for the coordinator to record where it charges the
/// unit ([`replay`]).
#[derive(Debug, Default)]
pub(crate) struct WorkerRows {
    units: Vec<UnitTrace>,
    executions: Vec<Execution>,
    reads: Vec<Read>,
}

/// On a pool worker that records ([`record_worker`]), the rows recorded
/// since it last took them: the unit just analyzed, after the units it
/// demanded. None on the coordinator, which records its own rows in place.
pub(crate) fn take_worker_rows() -> Option<Box<WorkerRows>> {
    with_recorder(|recorder| {
        if recorder.thread == 0 {
            return None;
        }
        // The coordinator keeps each pair once.
        recorder.read_edges.clear();
        let trace = &mut recorder.trace;
        let rows = WorkerRows {
            units: std::mem::take(&mut trace.units),
            executions: std::mem::take(&mut trace.executions),
            reads: std::mem::take(&mut trace.reads),
        };
        let empty = rows.units.is_empty() && rows.executions.is_empty() && rows.reads.is_empty();
        (!empty).then(|| Box::new(rows))
    })
    .flatten()
}

/// Record on the coordinator the rows a pool worker recorded of a unit it
/// analyzed, where the coordinator takes the analysis, with `depth` unit
/// analyses open on its stack. What the worker's own root frame demanded,
/// its job's unit, the innermost frame here demands, as it would have in
/// a serial build, and every unit's depth counts from `depth`. Their times
/// are the workers' work, not the coordinator's wall time, so they are in
/// no stage total or wait, as the parse jobs' are not ([`pooled_loads`]).
pub(crate) fn replay(rows: WorkerRows, depth: usize) {
    with_recorder(|recorder| {
        let worker_root = Demander::Other(Stage::Other);
        let frame = recorder.frames.last_mut().expect("root frame");
        let here = Demander::of(frame.stage, frame.owner);
        let place = |demander| {
            if demander == worker_root {
                here
            } else {
                demander
            }
        };
        let trace = &mut recorder.trace;
        for mut unit in rows.units {
            if unit.demander == worker_root && frame.stage == Stage::Unit {
                frame.host |= unit.host;
            }
            unit.demander = place(unit.demander);
            unit.depth += depth as u32;
            trace.units.push(unit);
        }
        trace
            .executions
            .extend(rows.executions.into_iter().map(|execution| Execution {
                demander: place(execution.demander),
                ..execution
            }));
        for read in rows.reads {
            let read = Read {
                demander: place(read.demander),
                ..read
            };
            if recorder.read_edges.insert(read) {
                trace.reads.push(read);
            }
        }
    });
}

/// A unit counted on counters of its own, on a worker or as a macro-free
/// unit, charged `expansions` and `nodes` to the counters `counter` stands
/// for: the work of a unit inside the innermost frame that counts these,
/// which that frame does not count as its own.
pub(crate) fn charged(counter: &Rc<Cell<usize>>, expansions: usize, nodes: usize) {
    with_recorder(|recorder| {
        let key = counter_key(counter);
        if let Some(frame) = recorder.frames.iter_mut().rev().find(|f| f.counter == key) {
            frame.child_counts.0 += expansions;
            frame.child_counts.1 += nodes;
        }
    });
}

/// An engine service call begins; the result goes to [`engine_exit`].
#[inline]
#[cfg_attr(not(feature = "v8-macros"), allow(dead_code))]
pub(crate) fn engine_enter() -> bool {
    with_recorder(|recorder| {
        let now = Instant::now();
        if recorder.depth == 0 {
            recorder.engine_started = Some(now);
        }
        if recorder.outer_depth == 0 {
            recorder.outer_started = Some(now);
            recorder.trace.engine_entries += 1;
        }
        recorder.depth += 1;
        recorder.outer_depth += 1;
    })
    .is_some()
}

/// The service call [`engine_enter`] began ends. A call is a macro host
/// entry unless it is bookkeeping every unit makes: the compiler frontier's
/// transitions and the bridge check that follows each engine entry.
#[inline]
#[cfg_attr(not(feature = "v8-macros"), allow(dead_code))]
pub(crate) fn engine_exit(entered: bool, service: &str) {
    if !entered {
        return;
    }
    let host = !matches!(service, "compilerFrontier" | "checkBridge");
    with_recorder(|recorder| {
        let now = Instant::now();
        recorder.depth = recorder.depth.saturating_sub(1);
        recorder.outer_depth = recorder.outer_depth.saturating_sub(1);
        let frame = recorder.frames.last_mut().expect("root frame");
        if host {
            frame.engine_calls += 1;
            frame.host = true;
        }
        if recorder.depth == 0
            && let Some(started) = recorder.engine_started.take()
        {
            frame.engine += now - started;
        }
        if recorder.outer_depth == 0
            && let Some(started) = recorder.outer_started.take()
        {
            recorder.trace.engine_outer += now - started;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spin(duration: Duration) {
        let started = Instant::now();
        while started.elapsed() < duration {}
    }

    #[test]
    fn nested_frames_count_once_and_record_demand_edges() {
        let recording = record();
        namespace(0, "a", 10, &["b".into()]);
        {
            let _top = enter(Stage::TopLevel, 0, None);
            spin(Duration::from_millis(2));
            {
                let _unit = enter_unit(
                    || UnitInfo {
                        id: 3,
                        source: 0,
                        name: "a/x".into(),
                        span: crate::compiler_data::Source::new("a.cljs", "(def x 1)")
                            .span(0..9)
                            .unwrap(),
                        depth: 0,
                    },
                    &crate::MacroHost::new(),
                );
                spin(Duration::from_millis(3));
            }
        }
        {
            let _drain = enter(Stage::Drain, 0, None);
            spin(Duration::from_millis(1));
        }
        let trace = recording.finish().unwrap();
        let unit = &trace.units[0];
        assert_eq!(unit.demander, Demander::TopLevel(0));
        assert_eq!((unit.line, unit.column), (1, 1));
        assert!(unit.time >= Duration::from_millis(3));
        assert!(!unit.host);
        let namespace = &trace.namespaces[0];
        assert_eq!(namespace.requires, ["b"]);
        assert_eq!(namespace.top_level_wait, unit.time);
        assert!(namespace.top_level >= Duration::from_millis(2));
        assert!(namespace.top_level < Duration::from_millis(2) + unit.time);
        assert!(namespace.drain >= Duration::from_millis(1));
        assert_eq!(trace.exclusive_total(), trace.wall);
        // A finished recording records nothing more.
        let span = enter(Stage::TopLevel, 0, None);
        assert!(!span.on);
    }

    #[test]
    fn engine_time_is_charged_to_the_innermost_frame() {
        let recording = record();
        {
            let _top = enter(Stage::TopLevel, 0, None);
            let entered = engine_enter();
            spin(Duration::from_millis(2));
            {
                // A unit demanded from inside JavaScript.
                let _unit = enter_unit(
                    || UnitInfo {
                        id: 0,
                        source: 0,
                        name: "a/helper".into(),
                        span: crate::compiler_data::Source::new("a.cljs", "x")
                            .span(0..1)
                            .unwrap(),
                        depth: 0,
                    },
                    &crate::MacroHost::new(),
                );
                spin(Duration::from_millis(2));
                let frontier = engine_enter();
                engine_exit(frontier, "compilerFrontier");
            }
            engine_exit(entered, "call");
        }
        let trace = recording.finish().unwrap();
        let unit = &trace.units[0];
        assert!(!unit.host);
        assert_eq!(unit.engine_calls, 0);
        assert!(unit.engine < Duration::from_millis(1));
        let namespace = &trace.namespaces[0];
        assert!(namespace.top_level_engine >= Duration::from_millis(2));
        assert!(namespace.top_level_engine < Duration::from_millis(2) + unit.time);
        assert!(trace.engine_outer >= namespace.top_level_engine + unit.time);
        assert_eq!(trace.engine_entries, 1);
        assert_eq!(trace.stage(Stage::TopLevel).engine_calls, 1);
    }

    fn unit_info(id: usize, depth: usize) -> UnitInfo {
        UnitInfo {
            id,
            source: 0,
            name: format!("a/u{id}"),
            span: crate::compiler_data::Source::new("a.cljs", "x")
                .span(0..1)
                .unwrap(),
            depth,
        }
    }

    #[test]
    fn macro_calls_and_nodes_count_in_the_innermost_frame() {
        let host = crate::MacroHost::new();
        let charge = |expansions: usize, nodes: usize| {
            host.engine
                .expansions
                .set(host.engine.expansions.get() + expansions);
            host.engine
                .generated_nodes
                .set(host.engine.generated_nodes.get() + nodes);
        };
        let recording = record();
        {
            let _top = enter(Stage::TopLevel, 0, Some(&host));
            charge(1, 10);
            {
                let _outer = enter_unit(|| unit_info(1, 0), &host);
                charge(2, 20);
                {
                    let _inner = enter_unit(|| unit_info(0, 1), &host);
                    charge(4, 40);
                    executed(0, true);
                }
                charge(8, 80);
            }
            charge(16, 160);
            executed(1, false);
        }
        let trace = recording.finish().unwrap();
        let counts: Vec<_> = trace
            .units
            .iter()
            .map(|unit| (unit.id, unit.expansions, unit.nodes, unit.demander))
            .collect();
        assert_eq!(
            counts,
            [
                (0, 4, 40, Demander::Unit(1)),
                (1, 10, 100, Demander::TopLevel(0))
            ]
        );
        assert_eq!(trace.namespaces[0].top_level_expansions, 17);
        assert_eq!(
            trace.executions,
            [
                Execution {
                    unit: 0,
                    demander: Demander::Unit(0),
                    prepared: true
                },
                Execution {
                    unit: 1,
                    demander: Demander::TopLevel(0),
                    prepared: false
                }
            ]
        );
        let tsv = trace.tsv(&[]);
        assert!(tsv.contains("\nunit\t0\t0\ta/u0\t1\t1\t"), "{tsv}");
        assert!(tsv.contains("\t4\t40\t0\tunit\t1\t1\t0\n"), "{tsv}");
        assert!(tsv.ends_with(
            "execute\t0\tunit\t0\t1\nexecute\t1\ttop-level\t0\t0\n#read\tunit\tdemander-kind\tdemander\n"
        ));
    }

    #[test]
    fn root_reads_name_the_frame_they_were_made_in_once_per_pair() {
        let recording = record();
        let outer = frame().unwrap();
        let (top, unit) = {
            let _top = enter(Stage::TopLevel, 2, None);
            let top = frame().unwrap();
            let unit = {
                let _unit = enter_unit(|| unit_info(5, 0), &crate::MacroHost::new());
                frame().unwrap()
            };
            assert_eq!(frame(), Some(top));
            (top, unit)
        };
        assert_eq!(frame(), Some(outer));
        root_reads(&format!("{top}:7,8"));
        root_reads(&format!("{unit}:7,5"));
        // Pairs already recorded, unknown frames and malformed notes add none.
        root_reads(&format!("{top}:8,7"));
        root_reads("99:1");
        root_reads("x");
        let trace = recording.finish().unwrap();
        let read = |unit, demander| Read { unit, demander };
        assert_eq!(
            trace.reads,
            [
                read(7, Demander::TopLevel(2)),
                read(8, Demander::TopLevel(2)),
                read(7, Demander::Unit(5)),
                read(5, Demander::Unit(5)),
            ]
        );
        // Units 7 and 8 are not traced, so their reads cross; unit 5 reads
        // its own namespace.
        assert_eq!(trace.cross_namespace_reads(), 3);
        assert!(trace.tsv(&[]).ends_with(
            "read\t7\ttop-level\t2\nread\t8\ttop-level\t2\nread\t7\tunit\t5\nread\t5\tunit\t5\n"
        ));
        assert_eq!(frame(), None);
    }

    #[test]
    fn another_threads_recording_leaves_this_thread_untraced() {
        let started = std::sync::Barrier::new(2);
        let checked = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let recording = record();
                started.wait();
                checked.wait();
                assert!(recording.finish().unwrap().units.is_empty());
            });
            started.wait();
            let host = crate::MacroHost::new();
            let span = enter_unit(|| panic!("computed an untraced unit"), &host);
            assert!(!span.on && span.counters.is_none());
            let span = enter(Stage::TopLevel, 0, Some(&host));
            assert!(!span.on && span.counters.is_none());
            assert!(!engine_enter());
            checked.wait();
        });
    }

    fn charge(host: &crate::MacroHost, expansions: usize, nodes: usize) {
        let engine = &host.engine;
        engine.expansions.set(engine.expansions.get() + expansions);
        engine
            .generated_nodes
            .set(engine.generated_nodes.get() + nodes);
    }

    #[test]
    fn worker_rows_are_recorded_where_the_coordinator_charges_their_unit() {
        let recording = record();
        let host = crate::MacroHost::new();
        {
            let _top = enter(Stage::TopLevel, 0, Some(&host));
            charge(&host, 1, 10);
            // The coordinator records its own rows in place.
            assert!(take_worker_rows().is_none());
            let rows = std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        let _worker = record_worker(3);
                        // The unit counts on counters of its own; a unit it
                        // demands counts on its host's.
                        let own = crate::MacroHost::new();
                        {
                            let _unit = enter_unit(|| unit_info(5, 0), &own);
                            charge(&own, 2, 20);
                            {
                                let inner = crate::MacroHost::new();
                                let _inner = enter_unit(|| unit_info(6, 1), &inner);
                                charge(&inner, 4, 40);
                            }
                            executed(6, false);
                            root_reads(&format!("{}:6", frame().unwrap()));
                        }
                        let rows = take_worker_rows().unwrap();
                        assert!(take_worker_rows().is_none());
                        rows
                    })
                    .join()
                    .unwrap()
            });
            replay(*rows, 2);
            // Charging the unit adds what the worker counted.
            charge(&host, 6, 60);
            charged(&host.engine.expansions, 6, 60);
        }
        let trace = recording.finish().unwrap();
        let units: Vec<_> = trace
            .units
            .iter()
            .map(|u| (u.id, u.demander, u.depth, u.thread, u.expansions, u.nodes))
            .collect();
        assert_eq!(
            units,
            [
                (6, Demander::Unit(5), 3, 3, 4, 40),
                (5, Demander::TopLevel(0), 2, 3, 2, 20)
            ]
        );
        assert_eq!(trace.namespaces[0].top_level_expansions, 1);
        assert_eq!(
            trace.executions,
            [Execution {
                unit: 6,
                demander: Demander::Unit(5),
                prepared: false
            }]
        );
        assert_eq!(
            trace.reads,
            [Read {
                unit: 6,
                demander: Demander::Unit(5)
            }]
        );
        // The workers' work is in no stage total of the coordinator's.
        assert_eq!(trace.exclusive_total(), trace.wall);
    }
}
