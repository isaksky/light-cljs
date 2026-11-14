//! Canonical paths without Windows' verbatim prefix.
//!
//! On Windows `fs::canonicalize` returns `\\?\C:\...`, which the compiler
//! would then print in diagnostics and messages and hand to tools that do
//! not accept it. [`canonicalize`] returns the plain `C:\...` form instead
//! when that form names the same file: a drive path short enough for the
//! ordinary Win32 limit. Elsewhere it is `fs::canonicalize`. Every path the
//! compiler canonicalizes goes through it, so canonical paths compare equal
//! with one another.

use std::io;
use std::path::{Path, PathBuf};

/// `fs::canonicalize`, without the verbatim prefix where Windows allows it.
pub fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    std::fs::canonicalize(path).map(simplify)
}

#[cfg(windows)]
fn simplify(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    let Some(plain) = text.strip_prefix(r"\\?\") else {
        return path;
    };
    // Only a drive path (`C:\...`) has a plain form, and only below the
    // 260-character limit that form is subject to.
    let bytes = plain.as_bytes();
    let drive =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    if drive && plain.len() < 260 {
        PathBuf::from(plain)
    } else {
        path
    }
}

#[cfg(not(windows))]
fn simplify(path: PathBuf) -> PathBuf {
    path
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_canonical_path_names_the_same_directory() {
        let here = std::env::current_dir().unwrap();
        let canonical = super::canonicalize(&here).unwrap();
        assert_eq!(
            std::fs::canonicalize(&canonical).unwrap(),
            std::fs::canonicalize(&here).unwrap()
        );
        assert!(!canonical.to_str().unwrap().starts_with(r"\\?\"));
    }
}
