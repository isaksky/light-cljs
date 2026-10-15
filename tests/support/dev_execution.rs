//! Runs dev builds (docs/dev-workflow.md, "The dev build") and their
//! single-file `--optimize none` oracles through
//! `tests/support/dev_execution.mjs`, the one Node driver for both.
//!
//! [`run_dev`] returns the driver's record in the shared outcome protocol
//! (`tests/support/execution.js`), so checking a dev build against its oracle
//! compares two strings.
use crate::temp_dir::TempDir;
use light_cljs::dev_build::emit_dev_build;
use light_cljs::source_map::SourceFile;
use light_cljs::{ApplicationOptions, OptimizationLevel, Program, prepare_application};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const DRIVER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/dev_execution.mjs"
);
/// Far above any program's run time, so a loaded machine does not fail it.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Writes the dev build of `program`, analyzed from `sources` (path and text
/// by source index), into the directory `dir`.
pub fn write_dev_build(program: &Program, sources: &[(PathBuf, String)], dir: &Path) {
    let urls = sources
        .iter()
        .map(|(path, _)| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let files = urls
        .iter()
        .zip(sources)
        .map(|(url, (_, text))| SourceFile { url, text })
        .collect::<Vec<_>>();
    fs::create_dir_all(dir).unwrap();
    for file in emit_dev_build(program, &files) {
        fs::write(dir.join(file.name), file.text).unwrap();
    }
}

/// Writes the `--optimize none` output of `program` to the file `path`.
pub fn write_none_build(program: &Program, path: &Path) {
    let options = ApplicationOptions::new(OptimizationLevel::None);
    fs::write(path, prepare_application(program, options).emit_js()).unwrap();
}

/// Runs the driver on `build`, a dev build directory or a single-file build,
/// calling a function program value with `start`, then applying the unit
/// files `units`; returns its record line without the newline. A run that
/// fails, times out or prints anything but one record is an error carrying
/// its stdout and stderr.
pub fn run_dev(build: &Path, start: bool, units: &[PathBuf]) -> Result<String, String> {
    let dir = TempDir::new("dev-run");
    let (stdout, stderr) = (dir.join("stdout.txt"), dir.join("stderr.txt"));
    let mut command = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()));
    command.arg(DRIVER).arg(build);
    if start {
        command.arg("--start");
    }
    let spawned = command
        .args(units)
        .stdin(Stdio::null())
        .stdout(fs::File::create(&stdout).unwrap())
        .stderr(fs::File::create(&stderr).unwrap())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => return Err(format!("dev execution requires Node: {error}")),
    };
    let begun = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if begun.elapsed() >= TIMEOUT {
            child.kill().unwrap();
            child.wait().unwrap();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let output = fs::read_to_string(&stdout).unwrap();
    let errors = fs::read_to_string(&stderr).unwrap();
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
                "dev execution of {}: {status}\nstdout:\n{output}\nstderr:\n{errors}",
                build.display()
            ))
        }
    }
}

/// The record of the `none` build of the program analyzed from `sources`,
/// and the record of its dev build, both written under `dir`.
pub fn none_and_dev(
    program: &Program,
    sources: &[(PathBuf, String)],
    dir: &Path,
    start: bool,
) -> [Result<String, String>; 2] {
    let (none, dev) = (dir.join("none.js"), dir.join("dev"));
    fs::create_dir_all(dir).unwrap();
    write_none_build(program, &none);
    write_dev_build(program, sources, &dev);
    [run_dev(&none, start, &[]), run_dev(&dev, start, &[])]
}
