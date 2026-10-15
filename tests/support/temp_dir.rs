//! Temporary directories for tests and benchmarks.
//!
//! [`TempDir::new`] creates a fresh, empty `$TMPDIR/light-<label>-<pid>-<n>`, and
//! dropping the guard removes it, also when the test holding it fails or panics.
//! Set `LIGHT_CLJS_KEEP_TMP=1` to keep these directories for inspection; each kept
//! directory's path is printed to stderr when its guard drops. A directory a test
//! takes from its own environment variable ([`TempDir::named_by`]) is the caller's
//! and is never removed.
use std::{
    fmt, fs, io,
    ops::Deref,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Keeps every [`TempDir::new`] directory when set to anything but `""` or `0`.
pub const KEEP: &str = "LIGHT_CLJS_KEEP_TMP";

pub struct TempDir {
    path: PathBuf,
    owned: bool,
}

impl TempDir {
    /// Creates the fresh directory `$TMPDIR/light-<label>-<pid>-<n>`. `<n>` has six
    /// digits, so the paths of one label have the same length within a process:
    /// compiled output can quote a source path, and some tests compare sizes.
    pub fn new(label: &str) -> Self {
        // `cargo test` runs the tests of a binary as threads of one process.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "light-{label}-{}-{:06}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self { path, owned: true },
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create {}: {error}", path.display()),
            }
        }
    }

    /// The directory `$variable` names, created if missing and never removed, when
    /// that variable is set; otherwise a fresh [`TempDir::new`] directory.
    pub fn named_by(variable: &str, label: &str) -> Self {
        match std::env::var_os(variable) {
            Some(path) => Self::retained(PathBuf::from(path)),
            None => Self::new(label),
        }
    }

    /// As [`TempDir::named_by`], but a directory `$variable` names must not exist
    /// yet, so no earlier run's files mix with this one's.
    pub fn new_named_by(variable: &str, label: &str) -> Self {
        if let Some(path) = std::env::var_os(variable) {
            assert!(
                !Path::new(&path).exists(),
                "{variable} names an existing directory: {}",
                Path::new(&path).display()
            );
        }
        Self::named_by(variable, label)
    }

    /// `path`, created if missing, as a directory that is never removed.
    pub fn retained(path: PathBuf) -> Self {
        fs::create_dir_all(&path)
            .unwrap_or_else(|error| panic!("cannot create {}: {error}", path.display()));
        Self { path, owned: false }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}

impl fmt::Debug for TempDir {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        self.path.fmt(formatter)
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if !self.owned {
            return;
        }
        if keep_requested() {
            eprintln!("{KEEP}: kept {}", self.path.display());
            return;
        }
        match fs::remove_dir_all(&self.path) {
            Ok(()) if std::thread::panicking() => {
                eprintln!("removed {}; set {KEEP}=1 to keep it", self.path.display())
            }
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => eprintln!("cannot remove {}: {error}", self.path.display()),
        }
    }
}

/// Whether `LIGHT_CLJS_KEEP_TMP` asks to keep temporary directories.
pub fn keep_requested() -> bool {
    std::env::var_os(KEEP).is_some_and(|value| !value.is_empty() && value != "0")
}
