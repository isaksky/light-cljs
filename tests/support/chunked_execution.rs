//! Runs ES module output through `tests/support/chunked_execution.mjs`, the one
//! Node driver for single-file `--esm` and chunked builds.
//!
//! [`run_modules`] writes the files into a fresh temporary directory and returns
//! the driver's record in the shared outcome protocol
//! (`tests/support/execution.js`), so checking a chunked run against its
//! single-file oracle compares two strings. The startup file name is the
//! record's id: give it the same name in both runs.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const DRIVER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/chunked_execution.mjs"
);
/// Far above any example's run time, so a loaded machine does not fail it.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Writes each `(name, text)` of `files` (names may contain `/`) into a fresh
/// temporary directory, runs the driver on `startup` there, and returns its
/// record line without the newline. A run that fails, times out or prints
/// anything but one record is an error carrying its stdout and stderr. The
/// directory is removed either way.
pub fn run_modules(files: &[(&str, &str)], startup: &str) -> Result<String, String> {
    let dir = fresh_dir();
    let modules = dir.join("modules");
    for (name, text) in files {
        let path = modules.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let (stdout, stderr) = (dir.join("stdout.txt"), dir.join("stderr.txt"));
    let spawned = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg(DRIVER)
        .arg(&modules)
        .arg(startup)
        .stdin(Stdio::null())
        .stdout(fs::File::create(&stdout).unwrap())
        .stderr(fs::File::create(&stderr).unwrap())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_dir_all(&dir);
            return Err(format!("chunked execution requires Node: {error}"));
        }
    };
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if start.elapsed() >= TIMEOUT {
            child.kill().unwrap();
            child.wait().unwrap();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let output = fs::read_to_string(&stdout).unwrap();
    let errors = fs::read_to_string(&stderr).unwrap();
    let _ = fs::remove_dir_all(&dir);
    match (status, output.strip_suffix('\n')) {
        (Some(status), Some(record)) if status.success() && !record.contains('\n') => {
            Ok(record.into())
        }
        (status, _) => {
            let status = match status {
                Some(status) => status.to_string(),
                None => format!("timed out after {TIMEOUT:?}"),
            };
            Err(format!(
                "chunked execution of {startup}: {status}\nstdout:\n{output}\nstderr:\n{errors}"
            ))
        }
    }
}

fn fresh_dir() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let path = std::env::temp_dir().join(format!(
            "light-cljs-chunks-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&path) {
            Ok(()) => return path,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("cannot create {}: {error}", path.display()),
        }
    }
}
