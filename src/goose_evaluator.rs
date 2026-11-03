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
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
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
}
impl GooseSession {
    pub fn start(executable: &Path, workers: usize) -> Result<Arc<Self>, EvalError> {
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
        let mut child = match Command::new(executable)
            .arg(&directory)
            .arg(workers.to_string())
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
        let output = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (tx, rx) = mpsc::channel();
        let pending = Arc::new(Mutex::new(HashMap::from([(0, tx)])));
        let mailboxes = pending.clone();
        let reader = thread::spawn(move || {
            let mut output = BufReader::new(output);
            let mut line = String::new();
            loop {
                line.clear();
                match output.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) if line.len() <= 65536 => {}
                    _ => break,
                }
                let parts = line.split_whitespace().collect::<Vec<_>>();
                let parsed = (|| {
                    let kind = *parts.first()?;
                    let id = parts.get(1)?.parse::<u64>().ok()?;
                    let event = match kind {
                        "READY" if parts == ["READY", "1", "1"] => return Some((0, Event::Ready)),
                        "START" if parts.len() == 2 => Event::Start,
                        "DONE" if parts.len() == 2 => Event::Done,
                        "NEED" if parts.len() == 4 => {
                            Event::Need(parts[2].parse().ok()?, parts[3].to_owned())
                        }
                        "ERROR" | "FAULT" => Event::Failure(line.trim().to_owned()),
                        _ => return None,
                    };
                    Some((id, event))
                })();
                match parsed {
                    Some((id, event)) => {
                        if let Some(tx) = mailboxes.lock().unwrap().get(&id) {
                            let _ = tx.send(event);
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
                let _ = tx.send(Event::Failure("subprocess exited".into()));
            }
        });
        // Drain stderr so a failed native worker cannot block on a full pipe.
        // Error context uses bounded protocol messages, never unbounded logs.
        let drain = thread::spawn(move || {
            let _ = std::io::copy(&mut BufReader::new(stderr), &mut std::io::sink());
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
            started: Instant::now(),
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
        SessionStats {
            subprocesses: 1,
            initializations: 1,
            jobs: self.starts.load(Ordering::Relaxed),
            exports: self.exports.load(Ordering::Relaxed),
            export_ns: self.export_ns.load(Ordering::Relaxed),
            bridge_bytes: self.bytes.load(Ordering::Relaxed),
            elapsed_ns: self.started.elapsed().as_nanos() as u64,
        }
    }
    /// Terminal failures retire the entire compilation, including every worker.
    pub fn close(&self) {
        if let Some(mut process) = self.process.lock().unwrap().take() {
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
    fn execute(
        &self,
        source: &str,
        graph: &bridge::Graph,
        c: &ExecutionControl,
        mut helper: impl FnMut(&str) -> Result<String, EvalError>,
    ) -> Result<Vec<u8>, EvalError> {
        let result = (|| {
            c.check()?;
            let id = self.next.fetch_add(1, Ordering::Relaxed);
            let stem = self.directory.join(id.to_string());
            self.export(&stem.with_extension("gsp"), source)?;
            let input = graph.encode()?;
            self.bytes.fetch_add(input.len() as u64, Ordering::Relaxed);
            std::fs::write(stem.with_extension("gfb"), input).map_err(error)?;
            let (tx, rx) = mpsc::channel();
            self.pending.lock().unwrap().insert(id, tx);
            {
                let mut process = self.process.lock().unwrap();
                let process = process.as_mut().ok_or_else(|| error("session is closed"))?;
                writeln!(process.input, "{id}").map_err(error)?;
                process.input.flush().map_err(error)?;
            }
            loop {
                c.check()?;
                match rx.recv_timeout(Duration::from_millis(2)) {
                    Ok(Event::Start) => {
                        self.starts.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(Event::Done) => {
                        self.pending.lock().unwrap().remove(&id);
                        let bytes = std::fs::read(stem.with_extension("out")).map_err(error)?;
                        self.bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                        return Ok(bytes);
                    }
                    Ok(Event::Need(sequence, name)) => {
                        let source = helper(&name)?;
                        c.check()?;
                        let temporary = self.directory.join(format!("{id}-{sequence}.tmp"));
                        self.export(&temporary, &source)?;
                        std::fs::rename(
                            temporary,
                            self.directory.join(format!("{id}-{sequence}.helper")),
                        )
                        .map_err(error)?;
                    }
                    Ok(Event::Failure(message)) => return Err(error(message)),
                    Ok(_) => return Err(error("unexpected protocol message")),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(e) => return Err(error(e)),
                }
            }
        })();
        if result.is_err() {
            self.close();
        }
        result
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
    Form(Form),
    Data(Datum),
    Result(Vec<u8>, Arc<HashMap<i32, Form>>),
}
/// A thread-local compiler view with session/version-scoped, non-reused handles.
pub struct GooseEvaluator {
    session: Arc<GooseSession>,
    identity: u64,
    slots: RefCell<HashMap<u64, Stored>>,
    next: Cell<u64>,
    control: ExecutionControl,
    helper: RefCell<Option<Rc<dyn Fn(&dyn MacroEvaluator, usize) -> Result<String, String>>>>,
    roots: RefCell<HashMap<String, usize>>,
}
impl GooseEvaluator {
    pub fn new(session: Arc<GooseSession>, options: &EvaluatorOptions) -> Result<Self, EvalError> {
        Ok(Self {
            session,
            identity: NEXT.fetch_add(1, Ordering::Relaxed),
            slots: RefCell::new(HashMap::new()),
            next: Cell::new(0),
            control: ExecutionControl::new(options)?,
            helper: RefCell::new(None),
            roots: RefCell::new(HashMap::new()),
        })
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
    pub(crate) fn install_helper(
        &self,
        helper: Rc<dyn Fn(&dyn MacroEvaluator, usize) -> Result<String, String>>,
    ) {
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
        self.control.check()?;
        Ok(self.store(Stored::Source(source.to_owned())))
    }
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError> {
        let Stored::Source(source) = self.get(function)? else {
            return Err(error("handle is not a function"));
        };
        let values = arguments
            .iter()
            .map(|&id| self.get(id))
            .collect::<Result<Vec<_>, _>>()?;
        let graph = bridge::Graph::arguments(&values, ConversionLimits::default(), &self.control)?;
        let bytes =
            self.session
                .execute(&source, &graph, &self.control, |name| {
                    let unit =
                        self.roots.borrow().get(name).copied().ok_or_else(|| {
                            error(format!("compiler root is not registered: {name}"))
                        })?;
                    let helper = self
                        .helper
                        .borrow()
                        .clone()
                        .ok_or_else(|| error("compiler service is not installed"))?;
                    helper(self, unit).map_err(error)
                })?;
        Ok(self.store(Stored::Result(bytes, Arc::new(graph.originals))))
    }
    fn import_form(&self, f: &Form, _: ConversionLimits) -> Result<ValueId, EvalError> {
        self.control.check()?;
        Ok(self.store(Stored::Form(f.clone())))
    }
    fn import_data(&self, d: &Datum, _: ConversionLimits) -> Result<ValueId, EvalError> {
        self.control.check()?;
        Ok(self.store(Stored::Data(d.clone())))
    }
    fn export_form(
        &self,
        id: ValueId,
        call: &Form,
        origin: ExpansionOrigin,
        limits: ConversionLimits,
    ) -> Result<Form, EvalError> {
        let Stored::Result(bytes, originals) = self.get(id)? else {
            return Err(error("handle is not a result"));
        };
        let result = bridge::Graph::decode(&bytes, limits, &self.control)?.expansion(
            call,
            origin,
            &originals,
            limits,
            &self.control,
        );
        if result.is_err() {
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
