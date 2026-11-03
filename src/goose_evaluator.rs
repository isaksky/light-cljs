//! Explicitly gated source-macro execution in the native Goose prototype.
//!
//! The compiler still owns reading, declaration resolution and helper emission.
//! This adapter never opens ScriptHost or installs the canonical JS runtime.
use crate::compiler_data::{Datum, ExpansionOrigin, Form};
use crate::evaluator::{EvalError, EvaluatorOptions, ExecutionControl, MacroEvaluator, ValueId};
use crate::form_bridge::ConversionLimits;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
#[path = "../crates/big-boy-pants-js/prototypes/goose/compiler_bridge.rs"]
mod bridge;
static NEXT: AtomicU64 = AtomicU64::new(1);
fn error(e: impl std::fmt::Display) -> EvalError {
    EvalError(format!("Goose: {e}"))
}
#[derive(Clone, Debug)]
enum Event {
    Ready,
    Start,
    Produced,
    Resumed,
    Done,
    Need(u64, String),
    Failure(String),
}
struct Process {
    child: Child,
    input: ChildStdin,
}
/// One initialized subprocess per compilation; clones share only transport.
/// Each outstanding call has its own correlated mailbox. The writer lock is
/// held for one command write, never while waiting or invoking the compiler.
pub struct GooseSession {
    directory: PathBuf,
    process: Mutex<Option<Process>>,
    pending: Arc<Mutex<HashMap<u64, mpsc::Sender<Event>>>>,
    next: AtomicU64,
    readers: Mutex<Vec<thread::JoinHandle<()>>>,
    starts: AtomicU64,
    exports: AtomicU64,
    export_ns: AtomicU64,
    bytes: AtomicU64,
    started: Instant,
    spawn_ns: u64,
    bridge_ns: AtomicU64,
    control: Mutex<Option<ExecutionControl>>,
    trace: Arc<Mutex<Vec<String>>>,
    costs: Arc<Mutex<[u64; 5]>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    terminal: Mutex<Option<EvalError>>,
    claimed: AtomicBool,
    compiling: AtomicBool,
}
#[derive(Clone, Copy, Debug)]
pub struct SessionStats {
    pub subprocesses: u64,
    pub initializations: u64,
    pub jobs: u64,
    pub exports: u64,
    pub export_ns: u64,
    pub bridge_bytes: u64,
    pub elapsed_ns: u64,
    pub spawn_ns: u64,
    pub rust_bridge_ns: u64,
    pub goose_base_load_ns: u64,
    pub goose_boot_ns: u64,
    pub goose_snapshot_ns: u64,
    pub goose_job_load_ns: u64,
    pub goose_bridge_ns: u64,
}
impl GooseSession {
    pub fn start(executable: &Path, workers: usize) -> Result<Arc<Self>, EvalError> {
        let started = Instant::now();
        if !(1..=8).contains(&workers) {
            return Err(error("worker count must be 1..8"));
        }
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("light-cljs-goose-{}-{id}", std::process::id()));
        std::fs::create_dir(&directory).map_err(error)?;
        let base = big_boy_pants_js::frontend::goose::export_source("goose-base.js", "rt => null")
            .map_err(error)?;
        std::fs::write(directory.join("base.gsp"), base.bytes).map_err(error)?;
        let mut command = Command::new(executable);
        command.arg(&directory).arg(workers.to_string());
        #[cfg(test)]
        {
            let serial = std::env::var("GOOSE_BENCH_CONTROL").is_ok_and(|v| v == "serial");
            if cfg!(feature = "count-allocations") {
                command.arg(if serial {
                    "serial-instrumented"
                } else {
                    "instrumented"
                });
            } else if serial {
                command.arg("serial");
            }
        }
        let mut child = match command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&directory);
                return Err(error(e));
            }
        };
        let input = child.stdin.take().unwrap();
        let spawn_ns = started.elapsed().as_nanos() as u64;
        let output = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (tx, rx) = mpsc::channel();
        let pending = Arc::new(Mutex::new(HashMap::from([(0, tx)])));
        let mailboxes = pending.clone();
        let trace = Arc::new(Mutex::new(Vec::new()));
        let traces = trace.clone();
        let costs = Arc::new(Mutex::new([0u64; 5]));
        let measured = costs.clone();
        let reader = thread::spawn(move || {
            let mut output = BufReader::new(output);
            let mut line = String::new();
            let mut terminal_reason = "subprocess exited";
            loop {
                line.clear();
                match (&mut output).take(65537).read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) if line.len() <= 65536 => {}
                    _ => {
                        terminal_reason = "malformed subprocess reply";
                        break;
                    }
                }
                let parts = line.split_whitespace().collect::<Vec<_>>();
                let parsed = (|| {
                    let kind = *parts.first()?;
                    let id = parts.get(1)?.parse::<u64>().ok()?;
                    let event = match kind {
                        "READY"
                            if (parts.len() == 3 || parts.len() == 6)
                                && parts[1..3] == ["1", "1"]
                                && parts[3..].iter().all(|s| s.parse::<u64>().is_ok()) =>
                        {
                            return Some((0, Event::Ready));
                        }
                        "START" if parts.len() == 2 => Event::Start,
                        "CLOSED" if parts == ["CLOSED", "1"] => return Some((0, Event::Resumed)),
                        "RESUME" if parts.len() == 2 => Event::Resumed,
                        "PRODUCE" if parts.len() == 3 && parts[2].parse::<u64>().is_ok() => {
                            Event::Resumed
                        }
                        "COST" | "FINISH"
                            if parts.len() == 4
                                && parts[2..].iter().all(|s| s.parse::<u64>().is_ok()) =>
                        {
                            Event::Resumed
                        }
                        "METRIC"
                            if matches!(parts.len(), 13 | 15)
                                && parts[2..].iter().all(|s| s.parse::<u64>().is_ok()) =>
                        {
                            Event::Resumed
                        }
                        "DONE" if parts.len() == 2 => Event::Done,
                        "NEED"
                            if parts.len() == 6
                                && parts[4..].iter().all(|s| s.parse::<u64>().is_ok()) =>
                        {
                            Event::Need(parts[2].parse().ok()?, parts[3].to_owned())
                        }
                        "PRODUCED" if parts.len() == 3 && parts[2].parse::<u64>().is_ok() => {
                            Event::Produced
                        }
                        "ERROR" | "FAULT" => Event::Failure(line.trim().to_owned()),
                        _ => return None,
                    };
                    Some((id, event))
                })();
                match parsed {
                    Some((id, event)) => {
                        let mut costs = measured.lock().unwrap();
                        if parts[0] == "READY" && parts.len() == 6 {
                            for i in 0..3 {
                                costs[i] = parts[i + 3].parse().unwrap();
                            }
                        }
                        if parts[0] == "COST" {
                            for i in 0..2 {
                                costs[i + 3] = costs[i + 3]
                                    .saturating_add(parts[i + 2].parse::<u64>().unwrap());
                            }
                        }
                        drop(costs);
                        let mut trace = traces.lock().unwrap();
                        if trace.len() < 4096 {
                            trace.push(line.trim().to_owned());
                        }
                        let boxes = mailboxes.lock().unwrap();
                        if let Some(tx) = boxes.get(&id) {
                            let _ = tx.send(event);
                        } else if parts[0] != "CLOSED" {
                            for tx in boxes.values() {
                                let _ =
                                    tx.send(Event::Failure("uncorrelated subprocess reply".into()));
                            }
                            break;
                        }
                    }
                    None => {
                        for tx in mailboxes.lock().unwrap().values() {
                            let _ = tx.send(Event::Failure("malformed subprocess reply".into()));
                        }
                        break;
                    }
                }
            }
            for tx in mailboxes.lock().unwrap().values() {
                let _ = tx.send(Event::Failure(terminal_reason.into()));
            }
        });
        // Drain stderr so a failed native worker cannot block on a full pipe.
        // Error context uses bounded protocol messages, never unbounded logs.
        let errors = Arc::new(Mutex::new(Vec::new()));
        let captured = errors.clone();
        let drain = thread::spawn(move || {
            let mut stderr = stderr;
            let mut bytes = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut bytes) {
                if n == 0 {
                    break;
                }
                let mut captured = captured.lock().unwrap();
                let count = n.min(65536 - captured.len());
                captured.extend_from_slice(&bytes[..count]);
            }
        });
        let session = Arc::new(Self {
            directory,
            process: Mutex::new(Some(Process { child, input })),
            pending,
            next: AtomicU64::new(1),
            readers: Mutex::new(vec![reader, drain]),
            starts: AtomicU64::new(0),
            exports: AtomicU64::new(0),
            export_ns: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            started,
            spawn_ns,
            bridge_ns: AtomicU64::new(0),
            control: Mutex::new(None),
            trace,
            costs,
            stderr: errors,
            terminal: Mutex::new(None),
            claimed: AtomicBool::new(false),
            compiling: AtomicBool::new(false),
        });
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Event::Ready) => {
                session.pending.lock().unwrap().remove(&0);
                Ok(session)
            }
            other => {
                session.close();
                Err(error(format!("startup failed: {other:?}")))
            }
        }
    }
    pub fn stats(&self) -> SessionStats {
        let costs = *self.costs.lock().unwrap();
        SessionStats {
            subprocesses: 1,
            initializations: 1,
            jobs: self.starts.load(Ordering::Relaxed),
            exports: self.exports.load(Ordering::Relaxed),
            export_ns: self.export_ns.load(Ordering::Relaxed),
            bridge_bytes: self.bytes.load(Ordering::Relaxed),
            elapsed_ns: self.started.elapsed().as_nanos() as u64,
            spawn_ns: self.spawn_ns,
            rust_bridge_ns: self.bridge_ns.load(Ordering::Relaxed),
            goose_base_load_ns: costs[0],
            goose_boot_ns: costs[1],
            goose_snapshot_ns: costs[2],
            goose_job_load_ns: costs[3],
            goose_bridge_ns: costs[4],
        }
    }
    pub fn trace(&self) -> Vec<String> {
        self.trace.lock().unwrap().clone()
    }
    pub(crate) fn claim_compilation(&self) -> Result<(), EvalError> {
        self.claimed
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .map(|_| ())
            .map_err(|_| error("session already belongs to a compilation"))
    }
    pub(crate) fn begin_compilation(&self) {
        if self.compiling.swap(true, Ordering::Relaxed) {
            *self.terminal.lock().unwrap() =
                Some(error("a fresh compilation requires a fresh Goose session"));
            self.close();
        }
    }
    /// Terminal failures retire the entire compilation, including every worker.
    pub fn close(&self) {
        if let Some(mut process) = self.process.lock().unwrap().take() {
            let _ = writeln!(process.input, "0");
            let _ = process.input.flush();
            let until = Instant::now() + Duration::from_millis(100);
            while process.child.try_wait().ok().flatten().is_none() && Instant::now() < until {
                thread::sleep(Duration::from_millis(1));
            }
            let _ = process.child.kill();
            let _ = process.child.wait();
        }
        for reader in self.readers.lock().unwrap().drain(..) {
            let _ = reader.join();
        }
        self.pending.lock().unwrap().clear();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
    fn export(&self, path: &Path, source: &str) -> Result<(), EvalError> {
        let started = Instant::now();
        let export = big_boy_pants_js::frontend::goose::export_source("compiler-macro.js", source)
            .map_err(error)?;
        std::fs::write(path, &export.bytes).map_err(error)?;
        self.exports.fetch_add(1, Ordering::Relaxed);
        self.export_ns
            .fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        Ok(())
    }
    fn answer_key(&self, id: u64, sequence: u64, key: &str) -> Result<(), EvalError> {
        let temporary = self.directory.join(format!("{id}-{sequence}.key.tmp"));
        std::fs::write(&temporary, key).map_err(error)?;
        std::fs::rename(
            temporary,
            self.directory.join(format!("{id}-{sequence}.key")),
        )
        .map_err(error)
    }
    fn execute(
        &self,
        source: &str,
        graph: &bridge::Graph,
        c: &ExecutionControl,
        mut helper: impl FnMut(&str) -> Result<(usize, String), EvalError>,
    ) -> Result<Vec<u8>, EvalError> {
        let result = (|| {
            if let Some(error) = self.terminal.lock().unwrap().clone() {
                return Err(error);
            }
            c.check()?;
            let id = self.next.fetch_add(1, Ordering::Relaxed);
            let stem = self.directory.join(id.to_string());
            self.export(&stem.with_extension("gsp"), source)?;
            let bridged = Instant::now();
            let input = graph.encode()?;
            self.bytes.fetch_add(input.len() as u64, Ordering::Relaxed);
            std::fs::write(stem.with_extension("gfb"), input).map_err(error)?;
            self.bridge_ns
                .fetch_add(bridged.elapsed().as_nanos() as u64, Ordering::Relaxed);
            let (tx, rx) = mpsc::channel();
            let mut helpers = Vec::new();
            self.pending.lock().unwrap().insert(id, tx);
            {
                let mut process = self.process.lock().unwrap();
                let process = process.as_mut().ok_or_else(|| error("session is closed"))?;
                writeln!(process.input, "{id}").map_err(error)?;
                process.input.flush().map_err(error)?;
            }
            loop {
                c.poll()?;
                match rx.recv_timeout(Duration::from_millis(2)) {
                    Ok(Event::Start) => {
                        self.starts.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(Event::Done) => {
                        self.pending.lock().unwrap().remove(&id);
                        if let Some(error) = self.terminal.lock().unwrap().clone() {
                            return Err(error);
                        }
                        let bytes = std::fs::read(stem.with_extension("out")).map_err(error)?;
                        for extension in ["gsp", "gfb", "out"] {
                            let _ = std::fs::remove_file(stem.with_extension(extension));
                        }
                        for sequence in helpers {
                            for extension in ["helper", "key"] {
                                let _ = std::fs::remove_file(
                                    self.directory.join(format!("{id}-{sequence}.{extension}")),
                                );
                            }
                        }
                        self.bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                        return Ok(bytes);
                    }
                    Ok(Event::Need(sequence, name)) => {
                        helpers.push(sequence);
                        let failed = self.terminal.lock().unwrap().clone();
                        let reply = match failed {
                            Some(error) => Err(error),
                            None => helper(&name),
                        };
                        let (unit, source) = match reply {
                            Ok(reply) => reply,
                            Err(failure) => {
                                *self.terminal.lock().unwrap() = Some(failure);
                                self.answer_key(id, sequence, "-1")?;
                                continue;
                            }
                        };
                        c.poll()?;
                        let temporary = self.directory.join(format!("{id}-{sequence}.tmp"));
                        self.export(&temporary, &source)?;
                        std::fs::rename(
                            temporary,
                            self.directory.join(format!("{id}-{sequence}.helper")),
                        )
                        .map_err(error)?;
                        self.answer_key(id, sequence, &unit.to_string())?;
                    }
                    Ok(Event::Produced) => {}
                    Ok(Event::Resumed) => {}
                    Ok(Event::Failure(message)) => {
                        return Err(self
                            .terminal
                            .lock()
                            .unwrap()
                            .clone()
                            .unwrap_or_else(|| error(message)));
                    }
                    Ok(_) => return Err(error("unexpected protocol message")),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(e) => return Err(error(e)),
                }
            }
        })();
        // The outer evaluator owns terminal cleanup, after a caught helper
        // failure has returned through its saved VM continuation.
        result.map_err(|mut e: EvalError| {
            let stderr = self.stderr.lock().unwrap();
            if !stderr.is_empty() {
                e.0.push_str(": ");
                e.0.push_str(&String::from_utf8_lossy(&stderr));
            }
            e
        })
    }
}
impl Drop for GooseSession {
    fn drop(&mut self) {
        self.close();
    }
}
#[derive(Clone)]
enum Stored {
    Source(String),
    Form(Form, ConversionLimits),
    Data(Datum, ConversionLimits),
    Result(
        Vec<u8>,
        Arc<HashMap<i32, bridge::Input>>,
        Rc<bridge::Budget>,
    ),
}
type HelperService = Rc<dyn Fn(&dyn MacroEvaluator, usize) -> Result<String, String>>;
/// A thread-local compiler view with session/version-scoped, non-reused handles.
pub struct GooseEvaluator {
    session: Arc<GooseSession>,
    identity: u64,
    slots: RefCell<HashMap<u64, Stored>>,
    next: Cell<u64>,
    control: ExecutionControl,
    helper: RefCell<Option<HelperService>>,
    roots: RefCell<HashMap<String, usize>>,
    budget: RefCell<Option<Rc<bridge::Budget>>>,
    #[cfg(test)]
    pub(crate) helper_delay: Cell<Duration>,
}
impl GooseEvaluator {
    pub fn new(session: Arc<GooseSession>, options: &EvaluatorOptions) -> Result<Self, EvalError> {
        let control = {
            let mut control = session.control.lock().unwrap();
            if control.is_none() {
                *control = Some(ExecutionControl::new(options)?);
            }
            control.as_ref().unwrap().clone()
        };
        Ok(Self {
            session,
            identity: NEXT.fetch_add(1, Ordering::Relaxed),
            slots: RefCell::new(HashMap::new()),
            next: Cell::new(0),
            control,
            helper: RefCell::new(None),
            roots: RefCell::new(HashMap::new()),
            budget: RefCell::new(None),
            #[cfg(test)]
            helper_delay: Cell::new(Duration::ZERO),
        })
    }
    pub(crate) fn session(&self) -> Arc<GooseSession> {
        self.session.clone()
    }
    fn store(&self, value: Stored) -> ValueId {
        let index = self.next.get();
        self.next.set(index + 1);
        self.slots.borrow_mut().insert(index, value);
        ValueId {
            session: self.identity,
            index,
        }
    }
    fn available(&self) -> Result<(), EvalError> {
        if self.session.process.lock().unwrap().is_none() {
            return Err(error("session is closed"));
        }
        if let Some(error) = self.session.terminal.lock().unwrap().clone() {
            return Err(error);
        }
        let result = self.control.poll();
        if result.is_err() && self.budget.borrow().is_none() {
            self.session.close();
        }
        result
    }
    fn get(&self, id: ValueId) -> Result<Stored, EvalError> {
        if id.session != self.identity {
            return Err(error("foreign handle"));
        }
        self.slots
            .borrow()
            .get(&id.index)
            .cloned()
            .ok_or_else(|| error("released handle"))
    }
    pub(crate) fn install_helper(&self, helper: HelperService) {
        *self.helper.borrow_mut() = Some(helper);
    }
}
impl MacroEvaluator for GooseEvaluator {
    fn source_only(&self) -> bool {
        true
    }
    fn control(&self) -> ExecutionControl {
        self.control.clone()
    }
    fn heap(&self) -> usize {
        self.identity as usize
    }
    fn needs_reader_registration(&self) -> bool {
        false
    }
    fn has_native_bridge(&self) -> bool {
        true
    }
    fn compiler_frontier(&self, _: Option<usize>) -> Result<(), EvalError> {
        Ok(())
    }
    fn gensym_scope(&self, _: u64, _: &str, _: &str, _: bool) -> Result<(), EvalError> {
        Ok(())
    }
    fn register_compiler_root(
        &self,
        name: &str,
        unit: Option<usize>,
        _: bool,
        _: &str,
        _: Option<usize>,
    ) -> Result<(), EvalError> {
        if let Some(unit) = unit {
            self.roots.borrow_mut().insert(name.to_owned(), unit);
        }
        Ok(())
    }
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {
        self.available()?;
        Ok(self.store(Stored::Source(source.to_owned())))
    }
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError> {
        self.available()?;
        let Stored::Source(source) = self.get(function)? else {
            return Err(error("handle is not a function"));
        };
        let values = arguments
            .iter()
            .map(|&id| self.get(id))
            .collect::<Result<Vec<_>, _>>()?;
        let mut limits = ConversionLimits::default();
        for value in &values {
            if let Stored::Form(_, l) | Stored::Data(_, l) = value {
                limits.nodes = limits.nodes.min(l.nodes);
                limits.depth = limits.depth.min(l.depth);
            }
        }
        let previous = self.budget.borrow().clone();
        let budget = previous
            .clone()
            .unwrap_or_else(|| Rc::new(bridge::Budget::new(limits.nodes, self.control.clone())));
        *self.budget.borrow_mut() = Some(budget.clone());
        let result = (|| {
            let bridged = Instant::now();
            let graph = bridge::Graph::arguments(&values, limits, &budget)?;
            self.session
                .bridge_ns
                .fetch_add(bridged.elapsed().as_nanos() as u64, Ordering::Relaxed);
            let bytes =
                self.session
                    .execute(&source, &graph, &self.control, |name| {
                        #[cfg(test)]
                        thread::sleep(self.helper_delay.get());
                        let unit = self.roots.borrow().get(name).copied().ok_or_else(|| {
                            error(format!("compiler root is not registered: {name}"))
                        })?;
                        let helper = self
                            .helper
                            .borrow()
                            .clone()
                            .ok_or_else(|| error("compiler service is not installed"))?;
                        helper(self, unit)
                            .map(|source| (unit, source))
                            .map_err(error)
                    })?;
            Ok(self.store(Stored::Result(bytes, Arc::new(graph.originals), budget)))
        })();
        *self.budget.borrow_mut() = previous.clone();
        if result.is_err() && previous.is_none() {
            self.control.cancel();
            self.session.close();
        }
        result
    }
    fn import_form(&self, f: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.available()?;
        Ok(self.store(Stored::Form(f.clone(), limits)))
    }
    fn import_data(&self, d: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.available()?;
        Ok(self.store(Stored::Data(d.clone(), limits)))
    }
    fn export_form(
        &self,
        id: ValueId,
        call: &Form,
        origin: ExpansionOrigin,
        limits: ConversionLimits,
    ) -> Result<Form, EvalError> {
        let Stored::Result(bytes, originals, budget) = self.get(id)? else {
            return Err(error("handle is not a result"));
        };
        let bridged = Instant::now();
        let result = bridge::Graph::decode(&bytes, limits, &budget)
            .and_then(|graph| graph.expansion(call, origin, &originals, limits, &budget));
        self.session
            .bridge_ns
            .fetch_add(bridged.elapsed().as_nanos() as u64, Ordering::Relaxed);
        if result.is_err() && self.budget.borrow().is_none() {
            self.control.cancel();
            self.session.close();
        }
        result
    }
    fn release(&self, id: ValueId) -> Result<(), EvalError> {
        self.get(id)?;
        self.slots.borrow_mut().remove(&id.index);
        Ok(())
    }
}

#[cfg(all(test, light_v8))]
mod tests {
    use super::*;
    fn runner() -> PathBuf {
        std::env::var_os("GOOSE_COMPILER_RUNNER")
            .expect("run compiler.bb")
            .into()
    }
    #[test]
    #[ignore = "build native runner with compiler.bb"]
    fn goose_compiler_concurrent_helper_once() {
        for workers in [1, 2, 4, 8] {
            let session = GooseSession::start(&runner(), workers).unwrap();
            let root = session.directory.join("sources");
            std::fs::create_dir(&root).unwrap();
            std::fs::write(root.join("macros.cljs"),"(ns macros) (def shared [11 22]) (defmacro work [n] (loop [i 0] (if (= i n) (list 'quote shared) (recur (+ i 1)))))").unwrap();
            std::fs::write(root.join("main.cljs"),"(ns main (:require [macros :refer [work]])) (def shared [99]) (def a (work 20000)) (def b (work 20000)) (def c (work 20000)) (def d (work 20000)) (js/console.log (pr-str [a b c d]))").unwrap();
            let mut builds = Vec::new();
            for goose in [false, true] {
                let mut host = crate::MacroHost::new();
                host.evaluator_options.memory_limit = 512 << 20;
                if goose {
                    host.use_goose_source_macros(session.clone()).unwrap();
                }
                let _names = crate::generated_names::enter_namespace(0);
                let (program, _) = crate::compiler::analyze_namespace_entry_recorded(
                    "main",
                    std::slice::from_ref(&root),
                    &mut host,
                    Default::default(),
                    &mut Vec::new(),
                    crate::compiler::EntryRecorders {
                        slots: crate::scheduler::Slots::new(4).unwrap(),
                        ..Default::default()
                    },
                )
                .unwrap_or_else(|e| {
                    panic!(
                        "workers={workers} Goose={goose}: {e} trace={:?}",
                        session.trace()
                    )
                });
                builds.push(crate::emit_js(&program));
            }
            assert_eq!(builds[0], builds[1], "deterministic compiler output");
            let trace = session.trace();
            eprintln!("workers={workers}: {trace:?}");
            assert_eq!(
                trace
                    .iter()
                    .filter(|line| line.starts_with("PRODUCED "))
                    .count(),
                1,
                "one actual helper producer"
            );
            let (mut active, mut peak) = (0i32, 0i32);
            for line in &trace {
                let kind = line.split_whitespace().next().unwrap();
                match kind {
                    "START" | "RESUME" | "PRODUCE" => {
                        active += 1;
                        peak = peak.max(active);
                    }
                    "NEED" | "DONE" | "PRODUCED" => active -= 1,
                    _ => {}
                }
                assert!(active >= 0, "{trace:?}");
            }
            assert_eq!(active, 0);
            assert!(peak <= workers as i32);
            if workers > 1 {
                assert!(
                    peak >= 2,
                    "two real compiler macro jobs must overlap: {trace:?}"
                );
            }
            session.close();
            assert!(!session.directory.exists());
        }
    }
    #[test]
    #[ignore = "build native runner with compiler.bb"]
    fn goose_compiler_caught_nested_conversion_limit_is_terminal() {
        let values = (0..1000)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let declaration = format!("(defmacro wide [] (list (quote quote) [{values}])) ");
        let nested = format!(
            "{declaration} (def helper (wide)) (defmacro caught [] (try helper (catch :default ignored 42))) (caught)"
        );
        for (source, limited, expect_error) in [
            (format!("{declaration} (wide)"), true, false),
            (nested.clone(), true, true),
            (nested, false, false),
        ] {
            let session = GooseSession::start(&runner(), 1).unwrap();
            let mut host = crate::MacroHost::new();
            if limited {
                host.limits.generated_nodes = 4200;
            }
            host.use_goose_source_macros(session.clone()).unwrap();
            let result = crate::compile_with_macros(&source, &mut host);
            let trace = session.trace();
            eprintln!("nested limited={limited} error={expect_error}: {trace:?}");
            if expect_error {
                let error = result.unwrap_err().to_string();
                assert!(
                    session.terminal.lock().unwrap().is_some(),
                    "failure must happen inside the nested compiler callback: {trace:?}"
                );
                assert!(error.contains("Goose conversion limit"), "{error}");
                assert!(
                    trace.iter().any(|s| s == "DONE 1"),
                    "JS catch must execute before terminal rejection: {trace:?}"
                );
                session.close();
                assert!(error.contains("caught"));
            } else {
                result.unwrap();
                session.close();
            }
        }
    }
}

#[cfg(all(test, light_v8))]
#[path = "../crates/big-boy-pants-js/prototypes/goose/compiler_tests.rs"]
mod lifecycle_tests;
