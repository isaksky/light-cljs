//! Exact diagnostics shared by the analyzer/API and CLI. The last two fixture
//! columns record the upstream classification for each source.
use light_cljs::{
    AnalysisErrorKind, CompileError, LineIndex, analyze, compile, compile_namespace, parse,
};
use std::{fs, path::PathBuf, process::Command};

fn rows() -> impl Iterator<Item = [&'static str; 9]> {
    include_str!("../fixtures/analysis-boundaries.tsv")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split('\t').collect::<Vec<_>>().try_into().unwrap())
}

fn category(kind: &AnalysisErrorKind) -> &'static str {
    match kind {
        AnalysisErrorKind::InvalidForm(_) => "invalid-form",
        AnalysisErrorKind::InvalidBinding => "invalid-binding",
        AnalysisErrorKind::DuplicateParameter(_) => "duplicate-parameter",
        AnalysisErrorKind::UnresolvedSymbol(_) => "unresolved",
        AnalysisErrorKind::Unsupported(_) => "unsupported",
        AnalysisErrorKind::NumberOutOfRange => "number-range",
        AnalysisErrorKind::MacroExpansion(_) => "macro-expansion",
        AnalysisErrorKind::LazyTarget(_) => "lazy-target",
    }
}

#[test]
fn expression_boundaries_retain_exact_diagnostics_in_unselected_branches() {
    for [id, mode, before, rejected, after, expected, message, _, _] in rows() {
        if mode == "ns" {
            continue;
        }
        // Both a newline and multibyte text on the diagnostic line precede the
        // source. The nested form's branch is never selected at runtime.
        for (prefix, suffix) in [
            ("; λ\n(do \"é\" ", ")"),
            ("; λ\n(do \"é\" (if false ", " 0))"),
        ] {
            let source = format!("{prefix}{before}{rejected}{after}{suffix}");
            let start = prefix.len() + before.len();
            let span = start..start + rejected.len();
            let column = prefix.split_once('\n').unwrap().1.len() + before.len() + 1;
            let syntax = parse(&source).unwrap_or_else(|error| panic!("{id}: {error}"));
            if expected == "accepted" {
                analyze(&source, &syntax).unwrap_or_else(|error| panic!("{id}: {error}"));
                compile(&source).unwrap_or_else(|error| panic!("{id}: {error}"));
                continue;
            }
            let Err(error) = analyze(&source, &syntax) else {
                panic!("{id}: expected {expected}, but analysis accepted {source:?}");
            };
            assert_eq!(category(&error.kind), expected, "{id}: {error}");
            assert_eq!(error.span, span, "{id}: {error}");
            assert_eq!(
                LineIndex::new(&source).line_column(error.span.start),
                Some((2, column)),
                "{id}"
            );
            assert!(error.to_string().contains(message), "{id}: {error}");
            assert_eq!(compile(&source), Err(CompileError::Analysis(error)), "{id}");
            for flag in ["--compile", "--analyze"] {
                let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
                    .args([flag, "--text", &source])
                    .output()
                    .unwrap();
                assert!(!output.status.success(), "{id}: {flag}");
                assert!(output.stdout.is_empty(), "{id}: partial output from {flag}");
                let diagnostic = String::from_utf8(output.stderr).unwrap();
                assert!(
                    diagnostic.contains(&format!("2:{column}: ")),
                    "{id}: {diagnostic}"
                );
                assert!(
                    diagnostic.contains(&format!("(bytes {}..{})", span.start, span.end)),
                    "{id}: {diagnostic}"
                );
                assert!(diagnostic.contains(message), "{id}: {diagnostic}");
            }
        }
    }
}

struct Sources(PathBuf);
impl Drop for Sources {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn namespace_boundaries_retain_file_spans_and_emit_no_partial_javascript() {
    let sources =
        Sources(std::env::temp_dir().join(format!("light-cljs-boundaries-{}", std::process::id())));
    fs::create_dir_all(sources.0.join("boundary")).unwrap();
    // A real dependency ensures rejection isn't an accidental missing-file error.
    fs::write(
        sources.0.join("boundary/lib.cljs"),
        "(ns boundary.lib) (def value 7)",
    )
    .unwrap();
    fs::write(sources.0.join("other.cljs"), "(ns other) (def value 8)").unwrap();
    let loader_cases = [
        [
            "missing-dependency",
            "ns",
            "(ns boundary (:require boundary.lib ",
            "missing.ns",
            "))",
            "namespace",
            "namespace missing.ns not found",
            "loader-only",
            "loader-only",
        ],
        [
            "unknown-alias",
            "ns",
            "(ns boundary (:require [boundary.lib :as b])) (let* [λ 1] ",
            "nope/value",
            ")",
            "namespace",
            "unknown namespace or alias nope",
            "loader-only",
            "loader-only",
        ],
        [
            "missing-global",
            "ns",
            "(ns boundary (:require [boundary.lib :as b])) (let* [λ 1] ",
            "b/absent",
            ")",
            "namespace",
            "unresolved symbol: b/absent",
            "loader-only",
            "loader-only",
        ],
    ];
    for [id, mode, before, rejected, after, expected, message, _, _] in rows().chain(loader_cases) {
        if mode != "ns" {
            continue;
        }
        let prefix = "; λ\n";
        let source = format!("{prefix}{before}{rejected}{after}");
        let file = sources.0.join("boundary.cljs");
        fs::write(&file, &source).unwrap();
        let start = prefix.len() + before.len();
        let span = start..start + rejected.len();
        let column = before.len() + 1;
        if expected == "accepted" {
            compile_namespace("boundary", std::slice::from_ref(&sources.0))
                .unwrap_or_else(|error| panic!("{id}: {error}"));
            continue;
        }
        let error = compile_namespace("boundary", std::slice::from_ref(&sources.0)).unwrap_err();
        assert_eq!(error.span, Some(span.clone()), "{id}: {error}");
        assert_eq!(error.line_column, Some((2, column)), "{id}: {error}");
        assert_eq!(error.path, Some(fs::canonicalize(&file).unwrap()), "{id}");
        assert!(error.message.contains(message), "{id}: {error}");
        let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args(["--compile", "--entry", "boundary", "--source-root"])
            .arg(&sources.0)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{id}");
        assert!(output.stdout.is_empty(), "{id}: partial JavaScript");
        let diagnostic = String::from_utf8(output.stderr).unwrap();
        assert!(
            diagnostic.contains(&format!("boundary.cljs:2:{column}: ")),
            "{id}: {diagnostic}"
        );
        assert!(
            diagnostic.contains(&format!("(bytes {}..{})", span.start, span.end)),
            "{id}: {diagnostic}"
        );
        assert!(diagnostic.contains(message), "{id}: {diagnostic}");
    }
}
